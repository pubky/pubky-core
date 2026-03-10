//! Native HTTP request handling with Pubky TLS and ICANN fallback for unreachable endpoints.

use std::time::Duration;

use futures_util::StreamExt;
use pkarr::extra::endpoints::Endpoint;
use reqwest::{IntoUrl, Method, RequestBuilder};
use tokio::net::TcpStream;
use url::Url;

use crate::errors::RequestError;
use crate::{PubkyHttpClient, PublicKey, Result, cross_log};

const PUBKY_TLS_PROBE_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HostKind {
    ResolvedPubky,
    Icann,
    Pubky,
}

fn classify_host(host: &str) -> HostKind {
    if let Some(pk_host) = host.strip_prefix("_pubky.") {
        if PublicKey::is_pubky_prefixed(pk_host) {
            return HostKind::Icann;
        }
        if PublicKey::try_from_z32(pk_host).is_ok() {
            return HostKind::ResolvedPubky;
        }
        return HostKind::Icann;
    } else if PublicKey::is_pubky_prefixed(host) || PublicKey::try_from_z32(host).is_err() {
        return HostKind::Icann;
    }
    HostKind::Pubky
}

impl PubkyHttpClient {
    /// Constructs a [`reqwest::RequestBuilder`] for the given HTTP `method` and `url`,
    /// routing through the client's unified request path.
    ///
    /// For Pubky z32 hosts, this method first checks whether the native Pubky TLS
    /// endpoint is reachable via a TCP probe. If the probe fails (e.g. the
    /// homeserver is behind a Cloudflare Tunnel, NAT, or firewall) and an ICANN
    /// endpoint is published in the PKARR record, the request transparently falls
    /// back to the ICANN endpoint with a `pubky-host` header — the same mechanism
    /// WASM clients already use.
    ///
    /// Returns a [`Result`] containing the prepared `RequestBuilder`, or a
    /// URL/transport parsing error if the supplied `url` is invalid.
    pub(crate) async fn cross_request(
        &self,
        method: Method,
        mut url: Url,
    ) -> Result<RequestBuilder> {
        let pubky_host = self.prepare_request(&mut url).await?;

        if let Some(ref z32_key) = pubky_host
            && let Some(icann_url) = self.try_icann_fallback(z32_key, &url).await
        {
            cross_log!(
                info,
                "Pubky TLS unreachable for {}; falling back to ICANN endpoint {}",
                z32_key,
                icann_url
            );
            let builder = self.icann_http.request(method, icann_url.as_str());
            return Ok(builder.header("pubky-host", z32_key.as_str()));
        }

        Ok(self.request(method, &url))
    }

    /// Resolve PKARR endpoints and attempt ICANN fallback when Pubky TLS is
    /// unreachable. Returns `Some(rewritten_url)` when the caller should use
    /// the ICANN path, or `None` to proceed with Pubky TLS as usual.
    async fn try_icann_fallback(&self, z32_key: &str, original_url: &Url) -> Option<Url> {
        let stream = self.pkarr.resolve_https_endpoints(z32_key);
        let mut stream = std::pin::pin!(stream);

        let mut pubky_tls_endpoint: Option<Endpoint> = None;
        let mut icann_endpoint: Option<Endpoint> = None;

        while let Some(endpoint) = stream.next().await {
            if endpoint.domain().is_some() {
                if icann_endpoint.is_none() {
                    icann_endpoint = Some(endpoint);
                }
            } else if pubky_tls_endpoint.is_none() {
                pubky_tls_endpoint = Some(endpoint);
            }
            if pubky_tls_endpoint.is_some() && icann_endpoint.is_some() {
                break;
            }
        }

        let icann = icann_endpoint?;

        if let Some(ref pubky_tls) = pubky_tls_endpoint
            && Self::probe_reachable(pubky_tls).await
        {
            cross_log!(
                debug,
                "Pubky TLS endpoint reachable for {}, using direct connection",
                z32_key
            );
            return None;
        }

        let mut icann_url = original_url.clone();
        if let Some(domain) = icann.domain() {
            icann_url.set_host(Some(domain)).ok()?;
        }
        if let Some(port) = icann.port() {
            icann_url.set_port(Some(port)).ok()?;
        }

        Some(icann_url)
    }

    /// TCP connect probe to check if at least one of the endpoint's resolved
    /// socket addresses is reachable within [`PUBKY_TLS_PROBE_TIMEOUT`].
    async fn probe_reachable(endpoint: &Endpoint) -> bool {
        let addrs = endpoint.to_socket_addrs();
        if addrs.is_empty() {
            return false;
        }

        for addr in addrs {
            match tokio::time::timeout(PUBKY_TLS_PROBE_TIMEOUT, TcpStream::connect(addr)).await {
                Ok(Ok(_)) => return true,
                Ok(Err(e)) => {
                    cross_log!(debug, "TCP probe to {} failed: {}", addr, e);
                }
                Err(_) => {
                    cross_log!(debug, "TCP probe to {} timed out", addr);
                }
            }
        }

        false
    }

    /// Prepare a request for callers that need the WASM-style preflight.
    ///
    /// Native builds do not rewrite URLs; we only detect pubky hosts and return the
    /// `pubky-host` value when applicable.
    ///
    /// # Errors
    /// - Returns [`crate::errors::RequestError::Validation`] if the host uses a `pubky` prefix.
    #[allow(
        clippy::unused_async,
        reason = "keep async signature aligned with WASM build"
    )]
    pub async fn prepare_request(&self, url: &mut Url) -> Result<Option<String>> {
        let host = url.host_str().unwrap_or("");

        if let Some(stripped) = host.strip_prefix("_pubky.") {
            if PublicKey::is_pubky_prefixed(stripped) {
                return Err(RequestError::Validation {
                    message: "pubky prefix is not allowed in transport hosts; use raw z32"
                        .to_string(),
                }
                .into());
            }
            if PublicKey::try_from_z32(stripped).is_ok() {
                return Ok(Some(stripped.to_string()));
            }
        } else {
            if PublicKey::is_pubky_prefixed(host) {
                return Err(RequestError::Validation {
                    message: "pubky prefix is not allowed in transport hosts; use raw z32"
                        .to_string(),
                }
                .into());
            }
            if PublicKey::try_from_z32(host).is_ok() {
                return Ok(Some(host.to_string()));
            }
        }

        Ok(None)
    }

    /// Start building a `Request` with the `Method` and `Url` (native-only)
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// the request body before sending.
    ///
    /// Differs from [`reqwest::Client::request`], in that it can make requests to:
    /// 1. HTTPS URLs with a [`crate::PublicKey`] as top-level domain, by resolving
    ///    corresponding endpoints, and verifying TLS certificates accordingly.
    ///    (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. `_pubky.<public-key>` URLs like `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
    /// # Errors
    ///
    /// This method fails whenever the supplied `Url` cannot be parsed.
    pub fn request<U: IntoUrl>(&self, method: Method, url: &U) -> RequestBuilder {
        let url_str = url.as_str();

        let host = Url::parse(url_str)
            .ok()
            .and_then(|url| url.host_str().map(str::to_owned));

        if let Some(ref host) = host {
            match classify_host(host) {
                HostKind::ResolvedPubky => {
                    cross_log!(
                        debug,
                        "Routing request for resolved _pubky host {} via Pubky TLS",
                        host
                    );
                    return self.http.request(method, url_str);
                }
                HostKind::Icann => {
                    // TODO: remove icann_http when we can control reqwest connection
                    // and or create a tls config per connection.
                    cross_log!(
                        debug,
                        "Routing request for ICANN host {} via standard TLS",
                        host
                    );
                    return self.icann_http.request(method, url_str);
                }
                HostKind::Pubky => {
                    cross_log!(
                        debug,
                        "Routing request for pubky host {} via PubkyTLS",
                        host
                    );
                }
            }
        }

        self.http.request(method, url_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Keypair;
    use crate::errors::Error;
    use pkarr::dns::rdata::SVCB;
    use reqwest::Method;

    fn build_signed_packet_with_endpoints(
        kp: &Keypair,
        pubky_tls_port: u16,
        pubky_tls_ip: std::net::Ipv4Addr,
        icann_domain: &str,
        icann_port: u16,
    ) -> pkarr::SignedPacket {
        let root: pkarr::dns::Name = ".".try_into().unwrap();

        let mut pubky_svcb = SVCB::new(1, root.clone());
        pubky_svcb.set_port(pubky_tls_port);
        pubky_svcb
            .set_ipv4hint([pubky_tls_ip.into()])
            .expect("valid ipv4hint");

        let mut icann_svcb = SVCB::new(10, root.clone());
        icann_svcb.target = icann_domain.try_into().unwrap();
        icann_svcb.set_port(icann_port);

        pkarr::SignedPacket::builder()
            .https(root.clone(), pubky_svcb, 3600)
            .https(root.clone(), icann_svcb, 3600)
            .address(root, pubky_tls_ip.into(), 3600)
            .sign(kp)
            .unwrap()
    }

    fn cache_packet(client: &pkarr::Client, packet: &pkarr::SignedPacket) {
        let cache = client.cache().expect("pkarr client should have a cache");
        let pk = packet.public_key();
        let cache_key: pkarr::CacheKey = pk.into();
        cache.put(&cache_key, packet);
    }

    #[test]
    fn classify_host_routes_invalid_pubky_subdomain_as_icann() {
        assert_eq!(classify_host("_pubky.not-a-valid-z32"), HostKind::Icann);
    }

    #[tokio::test]
    async fn prepare_request_rejects_prefixed_pubky_transport_host() {
        let client = PubkyHttpClient::new().unwrap();
        let kp = Keypair::random();
        let prefixed = format!("pubky{}", kp.public_key().z32());
        let mut url = Url::parse(&format!("https://{prefixed}/signup")).unwrap();

        let result = client.prepare_request(&mut url).await;

        let err = result.expect_err("prefixed hosts should be rejected");
        let Error::Request(RequestError::Validation { message }) = err else {
            panic!("expected RequestError::Validation, got {err:?}");
        };
        assert!(message.contains("pubky prefix is not allowed"));
    }

    #[tokio::test]
    async fn cross_request_rejects_prefixed_pubky_subdomain_host() {
        let client = PubkyHttpClient::new().unwrap();
        let kp = Keypair::random();
        let prefixed = format!("pubky{}", kp.public_key().z32());
        let url = Url::parse(&format!("https://_pubky.{prefixed}/signup")).unwrap();

        let result = client.cross_request(Method::GET, url).await;

        let err = result.expect_err("prefixed _pubky hosts should be rejected");
        let Error::Request(RequestError::Validation { message }) = err else {
            panic!("expected RequestError::Validation, got {err:?}");
        };
        assert!(message.contains("pubky prefix is not allowed"));
    }

    #[tokio::test]
    async fn fallback_returns_none_when_pubky_tls_reachable() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let kp = Keypair::random();
        let packet = build_signed_packet_with_endpoints(
            &kp,
            addr.port(),
            "127.0.0.1".parse().unwrap(),
            "fallback.example.com",
            443,
        );

        let client = PubkyHttpClient::new().unwrap();
        cache_packet(client.pkarr(), &packet);

        let z32 = kp.public_key().z32();
        let url = Url::parse(&format!("https://{z32}/signup")).unwrap();

        assert!(
            client.try_icann_fallback(&z32, &url).await.is_none(),
            "should not fall back when Pubky TLS is reachable"
        );

        drop(listener);
    }

    #[tokio::test]
    async fn fallback_returns_icann_url_when_pubky_tls_unreachable() {
        let kp = Keypair::random();
        let packet = build_signed_packet_with_endpoints(
            &kp,
            19999,
            "10.255.255.1".parse().unwrap(),
            "fallback.example.com",
            8443,
        );

        let client = PubkyHttpClient::new().unwrap();
        cache_packet(client.pkarr(), &packet);

        let z32 = kp.public_key().z32();
        let url = Url::parse(&format!("https://{z32}/signup")).unwrap();
        let icann_url = client
            .try_icann_fallback(&z32, &url)
            .await
            .expect("should fall back to ICANN when Pubky TLS is unreachable");

        assert_eq!(icann_url.host_str(), Some("fallback.example.com"));
        assert_eq!(icann_url.port(), Some(8443));
        assert_eq!(icann_url.path(), "/signup");
    }
}

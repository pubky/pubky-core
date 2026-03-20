use std::collections::HashMap;
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use futures_util::StreamExt;
use tokio::net::TcpStream;

use crate::errors::RequestError;
use crate::{PubkyHttpClient, PublicKey, Result, cross_log};
use reqwest::{IntoUrl, Method, RequestBuilder};
use url::Url;

const TRANSPORT_CACHE_TTL: Duration = Duration::from_secs(60);
const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

#[derive(Debug, Clone)]
pub(crate) enum ResolvedTransport {
    PubkyTls,
    Icann { domain: String, port: Option<u16> },
}

/// Per-client cache of transport decisions.
pub(crate) type TransportCache = Arc<RwLock<HashMap<String, (Instant, ResolvedTransport)>>>;

/// Per-key guards to coalesce concurrent in-flight transport resolutions.
pub(crate) type ResolveGuards = Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>;

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
    } else if PublicKey::is_pubky_prefixed(host) || PublicKey::try_from_z32(host).is_err() {
        return HostKind::Icann;
    }
    HostKind::Pubky
}

impl PubkyHttpClient {
    /// Constructs a [`reqwest::RequestBuilder`] for the given HTTP `method` and `url`.
    ///
    /// For pubky hosts, resolves PKARR endpoints and selects `PubkyTLS` or ICANN
    /// fallback (with `pubky-host` header). When both endpoints exist, the direct
    /// endpoint is TCP-probed; if unreachable, falls back to ICANN.
    pub(crate) async fn cross_request(
        &self,
        method: Method,
        mut url: Url,
    ) -> Result<RequestBuilder> {
        let pubky_host = self.prepare_request(&mut url).await?;

        let Some(pk) = pubky_host else {
            return Ok(self.request(method, &url));
        };

        // Fast path: check cache.
        let transport = {
            let cache = self
                .transport_cache
                .read()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            cache
                .get(&pk)
                .filter(|(ts, _)| ts.elapsed() < TRANSPORT_CACHE_TTL)
                .map(|(_, t)| t.clone())
        };

        let transport = if let Some(t) = transport {
            t
        } else {
            // Acquire a per-key guard so concurrent requests for the same host
            // coalesce on a single resolution instead of duplicating probes.
            let guard = {
                let mut guards = self
                    .resolve_guards
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                Arc::clone(guards.entry(pk.clone()).or_default())
            };
            let _lock = guard.lock().await;

            // Re-check cache — another task may have resolved while we waited.
            let cached = {
                let cache = self
                    .transport_cache
                    .read()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                cache
                    .get(&pk)
                    .filter(|(ts, _)| ts.elapsed() < TRANSPORT_CACHE_TTL)
                    .map(|(_, t)| t.clone())
            };

            if let Some(t) = cached {
                t
            } else {
                let t = self.resolve_transport(&pk).await;
                self.transport_cache
                    .write()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .insert(pk.clone(), (Instant::now(), t.clone()));
                t
            }
        };

        // Build request with the chosen transport.
        match &transport {
            ResolvedTransport::PubkyTls => Ok(self.http.request(method, url.as_str())),
            ResolvedTransport::Icann { domain, port } => {
                let mut icann_url = url.clone();
                icann_url.set_host(Some(domain))?;
                if let Some(p) = port {
                    icann_url
                        .set_port(Some(*p))
                        .map_err(|_err| url::ParseError::InvalidPort)?;
                }
                cross_log!(debug, "ICANN fallback for {pk} via {domain}");
                Ok(self
                    .icann_http
                    .request(method, icann_url.as_str())
                    .header("pubky-host", pk))
            }
        }
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

    /// Start building a `Request` with the `Method` and `Url` (native-only).
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
                    cross_log!(debug, "PubkyTLS request for resolved _pubky host {}", host);
                    return self.http.request(method, url_str);
                }
                HostKind::Icann => {
                    cross_log!(debug, "Standard TLS request for ICANN host {}", host);
                    return self.icann_http.request(method, url_str);
                }
                HostKind::Pubky => {
                    cross_log!(debug, "PubkyTLS request for pubky host {}", host);
                }
            }
        }

        self.http.request(method, url_str)
    }

    /// Resolve which transport to use by inspecting PKARR endpoints.
    async fn resolve_transport(&self, qname: &str) -> ResolvedTransport {
        let stream = self.pkarr.resolve_https_endpoints(qname);
        futures_util::pin_mut!(stream);

        let mut has_direct = false;
        let mut direct_addrs = Vec::new();
        let mut icann: Option<(String, Option<u16>)> = None;

        while let Some(ep) = stream.next().await {
            if let Some(domain) = ep.domain() {
                if icann.is_none() {
                    icann = Some((domain.to_string(), ep.port()));
                }
            } else {
                has_direct = true;
                direct_addrs.extend(ep.to_socket_addrs());
            }
        }

        let Some((domain, port)) = icann else {
            return ResolvedTransport::PubkyTls;
        };

        if !has_direct {
            return ResolvedTransport::Icann { domain, port };
        }

        // Both exist — probe direct endpoint reachability.
        if Self::probe_reachable(&direct_addrs).await {
            ResolvedTransport::PubkyTls
        } else {
            cross_log!(
                warn,
                "Direct endpoint unreachable for {qname}, falling back to ICANN ({domain})"
            );
            ResolvedTransport::Icann { domain, port }
        }
    }

    async fn probe_reachable(addrs: &[std::net::SocketAddr]) -> bool {
        for addr in addrs {
            if let Ok(Ok(_)) = tokio::time::timeout(PROBE_TIMEOUT, TcpStream::connect(addr)).await {
                return true;
            }
        }
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pkarr::dns::rdata::SVCB;
    use pkarr::{Keypair, SignedPacket};

    #[test]
    fn classify_hosts() {
        assert_eq!(classify_host("example.com"), HostKind::Icann);
        let z32 = "o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy";
        assert_eq!(
            classify_host(&format!("_pubky.{z32}")),
            HostKind::ResolvedPubky
        );
        assert_eq!(classify_host(z32), HostKind::Pubky);
    }

    #[tokio::test]
    async fn probe_unreachable_returns_false() {
        let addr = "192.0.2.1:1".parse().unwrap(); // TEST-NET-1, RFC 5737
        assert!(!PubkyHttpClient::probe_reachable(&[addr]).await);
    }

    /// Helper: build a client with a pre-cached signed packet (no real network).
    fn client_with_packet(keypair: &Keypair, packet: &SignedPacket) -> PubkyHttpClient {
        let mut builder = PubkyHttpClient::builder();
        // Need at least one bootstrap so pkarr doesn't reject with NoNetwork.
        // The DHT won't be reached since we pre-populate the cache.
        builder.pkarr(|b| b.no_default_network().bootstrap(&["127.0.0.1:1"]));
        let client = builder.build().unwrap();
        let cache_key: pkarr::CacheKey = keypair.public_key().into();
        client.pkarr.cache().unwrap().put(&cache_key, packet);
        client
    }

    #[tokio::test]
    async fn resolve_transport_direct_only() {
        let kp = Keypair::random();
        let mut svcb = SVCB::new(1, ".".try_into().unwrap());
        svcb.set_port(6881);
        let packet = SignedPacket::builder()
            .https(".".try_into().unwrap(), svcb, 3600)
            .address(".".try_into().unwrap(), "192.0.2.1".parse().unwrap(), 3600)
            .sign(&kp)
            .unwrap();
        let client = client_with_packet(&kp, &packet);

        let t = client.resolve_transport(&kp.public_key().to_string()).await;
        assert!(matches!(t, ResolvedTransport::PubkyTls));
    }

    #[tokio::test]
    async fn resolve_transport_icann_only() {
        let kp = Keypair::random();
        let svcb = SVCB::new(1, "example.com".try_into().unwrap());
        let packet = SignedPacket::builder()
            .https(".".try_into().unwrap(), svcb, 3600)
            .sign(&kp)
            .unwrap();
        let client = client_with_packet(&kp, &packet);

        let t = client.resolve_transport(&kp.public_key().to_string()).await;
        assert!(matches!(t, ResolvedTransport::Icann { .. }));
        if let ResolvedTransport::Icann { domain, .. } = t {
            assert_eq!(domain, "example.com");
        }
    }

    #[tokio::test]
    async fn resolve_transport_both_unreachable_direct_falls_back() {
        let kp = Keypair::random();
        // Direct endpoint with unreachable IP (TEST-NET-1)
        let mut direct = SVCB::new(1, ".".try_into().unwrap());
        direct.set_port(6881);
        // ICANN endpoint
        let icann = SVCB::new(10, "example.com".try_into().unwrap());
        let packet = SignedPacket::builder()
            .https(".".try_into().unwrap(), direct, 3600)
            .https(".".try_into().unwrap(), icann, 3600)
            .address(".".try_into().unwrap(), "192.0.2.1".parse().unwrap(), 3600)
            .sign(&kp)
            .unwrap();
        let client = client_with_packet(&kp, &packet);

        let t = client.resolve_transport(&kp.public_key().to_string()).await;
        assert!(
            matches!(t, ResolvedTransport::Icann { ref domain, .. } if domain == "example.com"),
            "expected ICANN fallback, got {t:?}"
        );
    }
}

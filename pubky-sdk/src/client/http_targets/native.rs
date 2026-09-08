use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError, RwLock};
use std::time::{Duration, Instant};

use super::{TransportHost, classify_transport_host, homeserver_url};
use futures_util::StreamExt;
use tokio::net::TcpStream;

use crate::errors::PkarrError;
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

#[derive(Debug)]
enum RequestTransport {
    Standard,
    Pubky(ResolvedTransport),
}

#[derive(Debug)]
struct ResolvedRequest {
    url: Url,
    transport: RequestTransport,
    pubky_host: Option<String>,
}

/// Resolves and caches per-host transport decisions (`PubkyTLS` vs ICANN).
///
/// Accepts a `&pkarr::Client` reference when resolution is needed — does not
/// own the pkarr client, which is shared across the SDK.
#[derive(Debug, Clone)]
pub(crate) struct TransportResolver {
    cache: Arc<RwLock<HashMap<String, (Instant, ResolvedTransport)>>>,
    guards: Arc<Mutex<HashMap<String, Arc<tokio::sync::Mutex<()>>>>>,
}

impl TransportResolver {
    pub(crate) fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
            guards: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Look up the transport for `qname`, resolving via PKARR on cache miss.
    pub(crate) async fn resolve(
        &self,
        qname: &str,
        pkarr: &pkarr::Client,
    ) -> Result<ResolvedTransport> {
        if let Some(t) = self.cached(qname) {
            return Ok(t);
        }
        self.resolve_and_cache(qname, pkarr).await
    }

    /// Fast path: return a cached, non-expired transport decision.
    fn cached(&self, qname: &str) -> Option<ResolvedTransport> {
        let cache = self.cache.read().unwrap_or_else(PoisonError::into_inner);
        cache
            .get(qname)
            .filter(|(ts, _)| ts.elapsed() < TRANSPORT_CACHE_TTL)
            .map(|(_, t)| t.clone())
    }

    /// Slow path: acquire a per-qname guard, double-check the cache, resolve,
    /// and store the result.
    async fn resolve_and_cache(
        &self,
        qname: &str,
        pkarr: &pkarr::Client,
    ) -> Result<ResolvedTransport> {
        let guard = {
            let mut guards = self.guards.lock().unwrap_or_else(PoisonError::into_inner);
            Arc::clone(guards.entry(qname.to_string()).or_default())
        };
        let _lock = guard.lock().await;

        // Another task may have resolved while we waited for the guard.
        if let Some(t) = self.cached(qname) {
            return Ok(t);
        }

        let t = Self::resolve_from_pkarr(pkarr, qname).await?;
        self.cache
            .write()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(qname.to_string(), (Instant::now(), t.clone()));
        Ok(t)
    }

    /// Inspect PKARR endpoints and probe reachability to pick a transport.
    async fn resolve_from_pkarr(pkarr: &pkarr::Client, qname: &str) -> Result<ResolvedTransport> {
        let stream = pkarr.try_resolve_endpoints(qname, true);
        futures_util::pin_mut!(stream);

        let mut direct_addrs = Vec::new();
        let mut icann: Option<(String, Option<u16>)> = None;
        let mut resolution_error = None;

        while let Some(result) = stream.next().await {
            let ep = match result {
                Ok(endpoint) => endpoint,
                Err(error) => {
                    resolution_error.get_or_insert(error);
                    continue;
                }
            };
            if let Some(domain) = ep.domain() {
                if icann.is_none() {
                    icann = Some((domain.to_string(), ep.port()));
                }
            } else {
                direct_addrs.extend(ep.to_socket_addrs());
            }
        }

        let Some((domain, port)) = icann else {
            if !direct_addrs.is_empty() {
                return Ok(ResolvedTransport::PubkyTls);
            }
            return Err(resolution_error.map_or_else(
                || {
                    PkarrError::InvalidRecord(format!("no usable HTTPS endpoints for {qname}"))
                        .into()
                },
                crate::Error::from,
            ));
        };
        if direct_addrs.is_empty() {
            return Ok(ResolvedTransport::Icann { domain, port });
        }

        // Both exist — probe direct endpoint reachability.
        if probe_reachable(&direct_addrs, PROBE_TIMEOUT).await {
            Ok(ResolvedTransport::PubkyTls)
        } else {
            cross_log!(
                warn,
                "Direct endpoint unreachable for {qname}; ICANN fallback to {domain}"
            );
            Ok(ResolvedTransport::Icann { domain, port })
        }
    }
}

async fn probe_reachable(addrs: &[std::net::SocketAddr], timeout: Duration) -> bool {
    for addr in addrs {
        if let Ok(Ok(_)) = tokio::time::timeout(timeout, TcpStream::connect(addr)).await {
            return true;
        }
    }
    false
}

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
    /// Constructs a [`reqwest::RequestBuilder`] for the given HTTP `method` and `url`,
    /// routing through the client's unified request path.
    ///
    /// This method ensures that special Pubky and pkarr hosts are resolved according to
    /// platform-specific rules (native or WASM), including:
    /// - Detecting `_pubky.<public-key>` hosts and applying the correct TLS handling.
    /// - Routing standard ICANN domains through the `icann_http` client on native builds.
    /// - When both a direct (IP:PORT) and an ICANN (domain) endpoint exist, TCP-probing
    ///   the direct endpoint and falling back to ICANN if unreachable.
    ///
    /// Transport decisions are cached per host with a short TTL.
    ///
    /// Returns a [`Result`] containing the prepared `RequestBuilder`, or a URL/transport
    /// parsing error if the supplied `url` is invalid.
    pub(crate) async fn cross_request(&self, method: Method, url: Url) -> Result<RequestBuilder> {
        let request = self.resolve_request(url).await?;
        self.build_request(method, request)
    }

    /// Native has no ambient browser cookie jar, so this is `cross_request`.
    pub(crate) async fn cross_request_anonymous(
        &self,
        method: Method,
        url: Url,
    ) -> Result<RequestBuilder> {
        self.cross_request(method, url).await
    }

    /// Route an authority-addressed endpoint through `homeserver` for `pubky_host`.
    pub(crate) async fn cross_request_via_homeserver(
        &self,
        method: Method,
        homeserver: &PublicKey,
        pubky_host: &PublicKey,
        path: &str,
    ) -> Result<RequestBuilder> {
        let url = homeserver_url(homeserver, path)?;
        let homeserver_z32 = homeserver.z32();
        let pubky_host_z32 = pubky_host.z32();
        let transport = self.transport.resolve(&homeserver_z32, &self.pkarr).await?;

        self.build_request(
            method,
            ResolvedRequest {
                url,
                transport: RequestTransport::Pubky(transport),
                pubky_host: Some(pubky_host_z32),
            },
        )
    }

    pub(super) async fn homeserver_info_request(
        &self,
        homeserver: &PublicKey,
    ) -> Result<RequestBuilder> {
        // Bypass cross_request so discovery cannot recursively trigger itself.
        let url = homeserver_url(homeserver, "/info")?;
        let transport = self
            .transport
            .resolve(&homeserver.z32(), &self.pkarr)
            .await?;

        self.build_transport_request(Method::GET, &url, &transport)
    }

    fn build_transport_request(
        &self,
        method: Method,
        url: &Url,
        transport: &ResolvedTransport,
    ) -> Result<RequestBuilder> {
        match transport {
            ResolvedTransport::PubkyTls => Ok(self.http.request(method, url.as_str())),
            ResolvedTransport::Icann { domain, port } => {
                let mut icann_url = url.clone();
                icann_url.set_host(Some(domain))?;
                if let Some(port) = port {
                    icann_url
                        .set_port(Some(*port))
                        .map_err(|_err| url::ParseError::InvalidPort)?;
                }
                cross_log!(debug, "ICANN fallback via {domain}");
                Ok(self.icann_http.request(method, icann_url.as_str()))
            }
        }
    }

    fn build_request(&self, method: Method, resolved: ResolvedRequest) -> Result<RequestBuilder> {
        let request = match &resolved.transport {
            RequestTransport::Standard => self.request(method, &resolved.url),
            RequestTransport::Pubky(transport) => {
                self.build_transport_request(method, &resolved.url, transport)?
            }
        };

        Ok(match resolved.pubky_host {
            Some(pubky_host) => request.header("pubky-host", pubky_host),
            None => request,
        })
    }

    async fn resolve_request(&self, mut url: Url) -> Result<ResolvedRequest> {
        let (addressing, transport_pubky_host) = self.prepare_request_parts(&mut url).await?;
        let Some(pubky_host) = transport_pubky_host else {
            let pubky_host = addressing.into_pubky_host(None);

            return Ok(ResolvedRequest {
                url,
                transport: RequestTransport::Standard,
                pubky_host,
            });
        };

        // `_pubky.<pk>` endpoints live under the full qname, not the bare key apex.
        let qname = url.host_str().unwrap_or(&pubky_host).to_string();
        let transport = self.transport.resolve(&qname, &self.pkarr).await?;
        let standard_pubky_host =
            matches!(&transport, ResolvedTransport::Icann { .. }).then_some(pubky_host);
        let pubky_host = addressing.into_pubky_host(standard_pubky_host);

        Ok(ResolvedRequest {
            url,
            transport: RequestTransport::Pubky(transport),
            pubky_host,
        })
    }

    // Use the group because newer Clippy versions split this into a lint unknown to our MSRV.
    #[allow(
        clippy::pedantic,
        reason = "keep async signature aligned with WASM build"
    )]
    pub(super) async fn prepare_transport_request(&self, url: &mut Url) -> Result<Option<String>> {
        let public_key = match classify_transport_host(url.host_str().unwrap_or_default())? {
            TransportHost::PubkyQname(public_key) | TransportHost::BarePublicKey(public_key) => {
                public_key
            }
            TransportHost::Other => return Ok(None),
        };

        Ok(Some(public_key.z32()))
    }

    /// Start building a `Request` with the `Method` and `Url` (native-only).
    ///
    /// Returns a `RequestBuilder`, which will allow setting headers and
    /// the request body before sending.
    ///
    /// This synchronous method does not negotiate storage addressing or resolve ICANN
    /// fallback endpoints. Use [`Self::request_async`] for Pubky and PKDNS URLs.
    ///
    /// Differs from [`reqwest::Client::request`], in that it can make requests to:
    /// 1. HTTPS URLs with a [`crate::PublicKey`] as top-level domain, by resolving
    ///    corresponding endpoints, and verifying TLS certificates accordingly.
    ///    (example: `https://o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`)
    /// 2. `_pubky.<public-key>` URLs like `https://_pubky.o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy`
    ///
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
}

#[cfg(test)]
mod tests {
    use std::num::NonZeroUsize;

    use super::*;
    use crate::Keypair as PubkyKeypair;
    use pkarr::dns::rdata::SVCB;
    use pkarr::{Cache, InMemoryCache, Keypair, SignedPacket};

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
        assert!(!probe_reachable(&[addr], Duration::from_millis(100)).await);
    }

    /// Helper: build a pkarr client with a pre-cached signed packet (no real network).
    fn pkarr_with_packet(keypair: &Keypair, packet: &SignedPacket) -> pkarr::Client {
        let cache = Arc::new(InMemoryCache::new(NonZeroUsize::MIN));
        let mut builder = PubkyHttpClient::builder();
        builder
            .isolated_pkarr_test()
            .pkarr(|b| b.cache(Arc::<InMemoryCache>::clone(&cache)));
        let client = builder.build().unwrap();
        let cache_key: pkarr::CacheKey = keypair.public_key().into();
        cache.put(&cache_key, packet);
        client.pkarr
    }

    #[test]
    fn build_request_uses_the_resolved_icann_target_and_header() {
        let client = PubkyHttpClient::builder()
            .isolated_pkarr_test()
            .build()
            .unwrap();
        let z32 = "o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy";
        let url = Url::parse(&format!("https://{z32}/pub/app/file.txt")).unwrap();
        let transport = ResolvedTransport::Icann {
            domain: "example.com".to_string(),
            port: Some(8443),
        };

        let req = client
            .build_request(
                Method::GET,
                ResolvedRequest {
                    url,
                    transport: RequestTransport::Pubky(transport),
                    pubky_host: Some(z32.to_string()),
                },
            )
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(req.url().host_str(), Some("example.com"));
        assert_eq!(req.url().port(), Some(8443));
        assert_eq!(req.url().path(), "/pub/app/file.txt");
        assert_eq!(req.headers().get("pubky-host").unwrap(), z32);
    }

    #[test]
    fn build_request_retains_path_addressing_without_a_header() {
        let client = PubkyHttpClient::builder()
            .isolated_pkarr_test()
            .build()
            .unwrap();
        let z32 = "o4dksfbqk85ogzdb5osziw6befigbuxmuxkuxq8434q89uj56uyy";
        let url = Url::parse(&format!(
            "https://_pubky.{z32}/storage/{z32}/pub/app/file.txt?cursor=hello%20world"
        ))
        .unwrap();
        let transport = ResolvedTransport::Icann {
            domain: "example.com".to_string(),
            port: Some(8443),
        };

        let req = client
            .build_request(
                Method::GET,
                ResolvedRequest {
                    url,
                    transport: RequestTransport::Pubky(transport),
                    pubky_host: None,
                },
            )
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(req.url().host_str(), Some("example.com"));
        assert_eq!(req.url().port(), Some(8443));
        assert_eq!(req.url().path(), format!("/storage/{z32}/pub/app/file.txt"));
        assert_eq!(req.url().query(), Some("cursor=hello%20world"));
        assert!(!req.headers().contains_key("pubky-host"));
    }

    #[tokio::test]
    async fn legacy_storage_attaches_the_path_owner_on_pubky_tls() {
        let client = PubkyHttpClient::builder()
            .isolated_pkarr_test()
            .build()
            .unwrap();
        let homeserver = PubkyKeypair::random().public_key();
        let owner = PubkyKeypair::random().public_key();
        client.features.insert(&homeserver, &[]);
        client.transport.cache.write().unwrap().insert(
            homeserver.z32(),
            (Instant::now(), ResolvedTransport::PubkyTls),
        );
        let url = Url::parse(&format!(
            "https://{}/storage/{}/pub/file.txt",
            homeserver.z32(),
            owner.z32()
        ))
        .unwrap();

        let request = client
            .cross_request(Method::GET, url)
            .await
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(request.url().path(), "/pub/file.txt");
        assert_eq!(request.headers().get("pubky-host").unwrap(), &owner.z32());
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
        let pkarr = pkarr_with_packet(&kp, &packet);

        let t = TransportResolver::resolve_from_pkarr(&pkarr, &kp.public_key().to_string()).await;
        assert!(matches!(t, Ok(ResolvedTransport::PubkyTls)));
    }

    #[tokio::test]
    async fn failed_transport_resolution_recovers_without_cache_expiry() {
        let server = httpmock::MockServer::start_async().await;
        let unavailable = server
            .mock_async(|when, then| {
                when.method(httpmock::Method::GET);
                then.status(503);
            })
            .await;
        let cache = Arc::new(InMemoryCache::new(NonZeroUsize::MIN));
        let client = PubkyHttpClient::builder()
            .pkarr(|builder| {
                builder
                    .no_dht()
                    .relays(&[server.base_url()])
                    .unwrap()
                    .cache(Arc::<InMemoryCache>::clone(&cache))
            })
            .build()
            .unwrap();
        let key = Keypair::random();
        let name = key.public_key().to_string();
        assert!(matches!(
            client.transport.resolve(&name, &client.pkarr).await,
            Err(crate::Error::Pkarr(PkarrError::Resolve(_)))
        ));
        assert!(client.transport.cached(&name).is_none());
        unavailable.assert_hits_async(1).await;

        let packet = SignedPacket::builder()
            .https(
                ".".try_into().unwrap(),
                SVCB::new(1, "example.com".try_into().unwrap()),
                3600,
            )
            .sign(&key)
            .unwrap();
        cache.put(&key.public_key().into(), &packet);
        let clone = client.clone();
        assert!(matches!(clone.transport.resolve(&name, &clone.pkarr).await,
            Ok(ResolvedTransport::Icann { domain, .. }) if domain == "example.com"));
        assert!(matches!(
            client.transport.cached(&name),
            Some(ResolvedTransport::Icann { .. })
        ));
        unavailable.assert_hits_async(1).await;
    }

    #[tokio::test]
    async fn empty_transport_resolution_is_not_cached() {
        let key = Keypair::random();
        let packet = SignedPacket::builder().sign(&key).unwrap();
        let pkarr = pkarr_with_packet(&key, &packet);
        let transport = TransportResolver::new();
        let name = key.public_key().to_string();
        assert!(matches!(
            transport.resolve(&name, &pkarr).await,
            Err(crate::Error::Pkarr(PkarrError::InvalidRecord(_)))
        ));
        assert!(transport.cached(&name).is_none());
    }

    #[tokio::test]
    async fn direct_transport_without_addresses_is_not_cached() {
        let key = Keypair::random();
        let packet = SignedPacket::builder()
            .https(
                ".".try_into().unwrap(),
                SVCB::new(1, ".".try_into().unwrap()),
                3600,
            )
            .sign(&key)
            .unwrap();
        let pkarr = pkarr_with_packet(&key, &packet);
        let transport = TransportResolver::new();
        let name = key.public_key().to_string();
        assert!(matches!(
            transport.resolve(&name, &pkarr).await,
            Err(crate::Error::Pkarr(PkarrError::InvalidRecord(_)))
        ));
        assert!(transport.cached(&name).is_none());
    }

    #[tokio::test]
    async fn resolve_transport_icann_only() {
        let kp = Keypair::random();
        let svcb = SVCB::new(1, "example.com".try_into().unwrap());
        let packet = SignedPacket::builder()
            .https(".".try_into().unwrap(), svcb, 3600)
            .sign(&kp)
            .unwrap();
        let pkarr = pkarr_with_packet(&kp, &packet);

        let t = TransportResolver::resolve_from_pkarr(&pkarr, &kp.public_key().to_string()).await;
        assert!(matches!(t, Ok(ResolvedTransport::Icann { .. })));
        if let Ok(ResolvedTransport::Icann { domain, .. }) = t {
            assert_eq!(domain, "example.com");
        }
    }

    #[tokio::test]
    async fn request_async_resolves_icann_path_addressed_storage() {
        // Homeserver apex: unreachable direct endpoint + reachable ICANN domain.
        let homeserver = Keypair::random();
        let mut direct = SVCB::new(1, ".".try_into().unwrap());
        direct.set_port(6287);
        let icann = SVCB::new(10, "example.com".try_into().unwrap());
        let homeserver_packet = SignedPacket::builder()
            .https(".".try_into().unwrap(), direct, 3600)
            .https(".".try_into().unwrap(), icann, 3600)
            .address(".".try_into().unwrap(), "192.0.2.1".parse().unwrap(), 3600)
            .sign(&homeserver)
            .unwrap();

        // User `_pubky` record aliasing to the homeserver key, mirroring
        // `Pkdns::build_homeserver_packet`.
        let user = Keypair::random();
        let homeserver_z32 = homeserver.public_key().to_string();
        let alias = SVCB::new(0, homeserver_z32.as_str().try_into().unwrap());
        let user_packet = SignedPacket::builder()
            .https("_pubky".try_into().unwrap(), alias, 3600)
            .sign(&user)
            .unwrap();

        let cache = Arc::new(InMemoryCache::new(NonZeroUsize::new(2).unwrap()));
        let mut builder = PubkyHttpClient::builder();
        builder
            .isolated_pkarr_test()
            .pkarr(|b| b.cache(Arc::<InMemoryCache>::clone(&cache)));
        let client = builder.build().unwrap();
        cache.put(&homeserver.public_key().into(), &homeserver_packet);
        cache.put(&user.public_key().into(), &user_packet);

        let user_z32 = user.public_key().to_string();
        let homeserver_pk = PublicKey::try_from_z32(&homeserver_z32).unwrap();
        client.features.insert(
            &homeserver_pk,
            &[pubky_common::constants::features::PATH_ADDRESSED_STORAGE],
        );
        let url = Url::parse(&format!(
            "https://_pubky.{user_z32}/storage/{user_z32}/pub/file.txt"
        ))
        .unwrap();
        let req = client
            .request_async(Method::GET, url)
            .await
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(
            req.url().host_str(),
            Some("example.com"),
            "expected ICANN fallback for _pubky host, got {}",
            req.url()
        );
        assert_eq!(
            req.url().path(),
            format!("/storage/{user_z32}/pub/file.txt")
        );
        assert!(!req.headers().contains_key("pubky-host"));
    }

    #[tokio::test]
    async fn cross_request_via_homeserver_routes_to_homeserver_with_user_pubky_host() {
        let homeserver = Keypair::random();
        let user = Keypair::random();
        let icann = SVCB::new(10, "example.com".try_into().unwrap());
        let homeserver_packet = SignedPacket::builder()
            .https(".".try_into().unwrap(), icann, 3600)
            .sign(&homeserver)
            .unwrap();

        let cache = Arc::new(InMemoryCache::new(NonZeroUsize::MIN));
        let mut builder = PubkyHttpClient::builder();
        builder
            .isolated_pkarr_test()
            .pkarr(|b| b.cache(Arc::<InMemoryCache>::clone(&cache)));
        let client = builder.build().unwrap();
        cache.put(&homeserver.public_key().into(), &homeserver_packet);
        let homeserver_pk = PublicKey::try_from_z32(&homeserver.public_key().to_string()).unwrap();
        let user_pk = PublicKey::try_from_z32(&user.public_key().to_string()).unwrap();

        let req = client
            .cross_request_via_homeserver(Method::POST, &homeserver_pk, &user_pk, "/session")
            .await
            .unwrap()
            .build()
            .unwrap();

        assert_eq!(req.url().host_str(), Some("example.com"));
        assert_eq!(req.url().path(), "/session");
        assert_eq!(req.headers().get("pubky-host").unwrap(), &user_pk.z32());
    }

    #[tokio::test]
    async fn resolve_transport_both_unreachable_direct_falls_back() {
        let kp = Keypair::random();
        let mut direct = SVCB::new(1, ".".try_into().unwrap());
        direct.set_port(6881);
        let icann = SVCB::new(10, "example.com".try_into().unwrap());
        let packet = SignedPacket::builder()
            .https(".".try_into().unwrap(), direct, 3600)
            .https(".".try_into().unwrap(), icann, 3600)
            .address(".".try_into().unwrap(), "192.0.2.1".parse().unwrap(), 3600)
            .sign(&kp)
            .unwrap();
        let pkarr = pkarr_with_packet(&kp, &packet);

        let t = TransportResolver::resolve_from_pkarr(&pkarr, &kp.public_key().to_string()).await;
        assert!(
            matches!(t, Ok(ResolvedTransport::Icann { ref domain, .. }) if domain == "example.com"),
            "expected ICANN fallback, got {t:?}"
        );
    }
}

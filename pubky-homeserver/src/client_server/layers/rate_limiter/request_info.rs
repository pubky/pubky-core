//! Pre-extracted request metadata for the rate limiter.
//!
//! Extracts authentication status, method, user pubkey, and client IP
//! from a request so that async bandwidth resolution doesn't need to
//! hold a borrow on the `!Send` `Request<Body>`.

use axum::body::Body;
use axum::http::{Method, Request};
use tower_cookies::Cookies;

use crate::client_server::extractors::PubkyHost;

use super::extract_ip::extract_ip;

/// Pre-extracted request metadata so that `resolve_bandwidth_throttlers`
/// does not need to borrow the `!Send` `Request<Body>` across `.await`.
pub(super) struct RequestInfo {
    pub authenticated: bool,
    pub method: Method,
    pub user_pubkey: Option<pubky_common::crypto::PublicKey>,
    pub client_ip: Result<std::net::IpAddr, anyhow::Error>,
}

impl RequestInfo {
    pub fn from_request(req: &Request<Body>) -> Self {
        Self {
            authenticated: is_authenticated(req),
            method: req.method().clone(),
            user_pubkey: req
                .extensions()
                .get::<PubkyHost>()
                .map(|pk| pk.public_key().clone()),
            client_ip: extract_ip(req),
        }
    }
}

/// Check if the request is authenticated by looking for a session cookie
/// matching the PubkyHost. This is a cheap cookie check with no DB hit.
///
/// **Not a security boundary.** A client can forge a cookie with any public
/// key. This is acceptable because:
/// - The real session validation happens downstream in the tenant auth layer,
///   which will reject requests with invalid/missing sessions.
/// - For *unknown* users (no DB row), `resolve_bandwidth_throttlers` falls
///   back to the unauthenticated IP rate, so forging a cookie for a
///   non-existent user doesn't help.
/// - For *known* users a forged cookie lets the attacker inherit that user's
///   rate limits instead of the IP limit. This is a minor information-free
///   advantage: the attacker can only read public data (writes are rejected
///   by auth), and reads are already rate-limited by the user's read rate.
fn is_authenticated(req: &Request<Body>) -> bool {
    req.extensions()
        .get::<Cookies>()
        .and_then(|cookies| {
            let pk = req.extensions().get::<PubkyHost>()?;
            cookies.get(&pk.public_key().z32())?;
            Some(())
        })
        .is_some()
}

/// Returns true for HTTP methods that represent writes (uploads).
pub(super) fn is_write_method(method: &Method) -> bool {
    matches!(
        *method,
        Method::PUT | Method::POST | Method::PATCH | Method::DELETE
    )
}

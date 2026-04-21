//! Cookie session construction from a signed [`AuthToken`].
//!
//! POSTs the token to the homeserver's `/session` endpoint and constructs a
//! cookie-based [`PubkySession`]. Also hosts the thin wrapper
//! [`session_from_cookie_response`] used by the signer during signup.

use std::sync::Arc;

use reqwest::Method;

use super::credential::CookieCredential;
use super::super::SessionCredential;
use crate::actors::session::core::PubkySession;
use crate::actors::storage::resource::resolve_pubky;
use crate::{AuthToken, PubkyHttpClient, Result, cross_log, util::check_http_status};

/// Establish a session from a signed [`AuthToken`] (legacy cookie flow).
///
/// POSTs the token to the homeserver's `/session` endpoint and constructs a
/// cookie-based [`PubkySession`].
pub(crate) async fn credential_from_auth_token(
    token: &AuthToken,
    client: &PubkyHttpClient,
) -> Result<Arc<dyn SessionCredential>> {
    let url = format!("pubky{}/session", token.public_key().z32());
    cross_log!(
        info,
        "Establishing new session exchange for {}",
        token.public_key()
    );
    let resolved = resolve_pubky(&url)?;
    let response = client
        .cross_request(Method::POST, resolved)
        .await?
        .body(token.serialize())
        .send()
        .await?;

    let response = check_http_status(response).await?;
    cross_log!(
        info,
        "Session exchange for {} succeeded; constructing credential",
        token.public_key()
    );
    let credential = CookieCredential::from_response(response).await?;
    Ok(Arc::new(credential))
}

pub(crate) async fn session_from_auth_token(
    token: &AuthToken,
    client: PubkyHttpClient,
) -> Result<PubkySession> {
    let credential = credential_from_auth_token(token, &client).await?;
    Ok(PubkySession::from_credential(client, credential))
}

pub(crate) async fn session_from_cookie_response(
    client: PubkyHttpClient,
    response: reqwest::Response,
) -> Result<PubkySession> {
    let credential: Arc<dyn SessionCredential> =
        Arc::new(CookieCredential::from_response(response).await?);
    Ok(PubkySession::from_credential(client, credential))
}

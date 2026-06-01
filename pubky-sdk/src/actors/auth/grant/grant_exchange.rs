//! grant-mode session construction functions.
//!
//! This module is the SDK's gateway to the homeserver's grant-based auth flow:
//! - [`credential_from_grant_exchange`] turns a fresh user-signed grant
//!   into a ready-to-use grant credential.
//! - [`signup_account_from_grant`] creates a user via grant + `PoP` without
//!   minting a session.
//!
//! Current grant-session operations (`current_bearer`, `force_refresh`,
//! `grant_id`) live on [`super::view::GrantSessionView`].

use pubky_common::{
    auth::{grant::GrantClaims, grant_session_responses::GrantSessionResponse},
    crypto::{Keypair, PublicKey},
};
use reqwest::Method;

use super::credential::{GrantCredential, sign_pop_for_grant};
use crate::actors::storage::resource::resolve_pubky;
use crate::errors::{RequestError, Result};
use crate::util::check_http_status;
use crate::{PubkyHttpClient, cross_log};

/// Establish a grant-backed session by exchanging a user-signed grant for
/// an opaque bearer at the user's homeserver.
///
/// Used by [`PubkyGrantAuthFlow`](crate::PubkyGrantAuthFlow) once the signer (Ring)
/// has delivered an encrypted grant via the relay channel.
///
/// # Errors
/// - Propagates HTTP transport / server errors from `POST /auth/grant/session`.
/// - Returns [`crate::errors::Error::Authentication`] if the response bearer
///   cannot be decoded.
pub(crate) async fn credential_from_grant_exchange(
    client: &PubkyHttpClient,
    grant_jws: String,
    grant_claims: GrantClaims,
    client_keypair: Keypair,
    homeserver_pubkey: PublicKey,
) -> Result<GrantCredential> {
    cross_log!(
        info,
        "Exchanging grant for grant credential (user={}, hs={})",
        grant_claims.iss.z32(),
        homeserver_pubkey.z32()
    );
    let response = post_grant_session(
        client,
        &grant_jws,
        &grant_claims,
        &client_keypair,
        &homeserver_pubkey,
    )
    .await?;
    Ok(GrantCredential::from_response(
        response,
        grant_jws,
        grant_claims,
        client_keypair,
        homeserver_pubkey,
    ))
}

/// Create a user via `POST /auth/grant/signup` without minting a session.
pub(crate) async fn signup_account_from_grant(
    client: &PubkyHttpClient,
    grant_jws: &str,
    grant_claims: &GrantClaims,
    client_keypair: &Keypair,
    homeserver_pk: &PublicKey,
    signup_token: Option<&str>,
) -> Result<()> {
    let pop_jws = sign_pop_for_grant(client_keypair, homeserver_pk, &grant_claims.jti);
    let body = serde_json::json!({ "grant": grant_jws, "pop": pop_jws });
    let mut url = url::Url::parse(&format!(
        "https://{}/auth/grant/signup",
        homeserver_pk.z32()
    ))
    .map_err(|e| RequestError::Validation {
        message: format!("invalid signup url: {e}"),
    })?;
    if let Some(token) = signup_token {
        url.query_pairs_mut().append_pair("signup_token", token);
    }
    let resp = client
        .cross_request(Method::POST, url)
        .await?
        .json(&body)
        .send()
        .await?;
    check_http_status(resp).await?;
    Ok(())
}

/// `POST` a grant + `PoP` proof to `/auth/grant/session`.
async fn post_grant_session(
    client: &PubkyHttpClient,
    grant_jws: &str,
    grant_claims: &GrantClaims,
    client_keypair: &Keypair,
    homeserver_pk: &PublicKey,
) -> Result<GrantSessionResponse> {
    let pop_jws = sign_pop_for_grant(client_keypair, homeserver_pk, &grant_claims.jti);
    let body = serde_json::json!({ "grant": grant_jws, "pop": pop_jws });

    let url = format!("pubky://{}/auth/grant/session", grant_claims.iss.z32());
    let resolved = resolve_pubky(&url)?;
    let resp = client
        .cross_request(Method::POST, resolved)
        .await?
        .json(&body)
        .send()
        .await?;
    let resp = check_http_status(resp).await?;
    resp.json().await.map_err(|e| {
        RequestError::DecodeJson {
            message: format!("decoding grant session response: {e}"),
        }
        .into()
    })
}

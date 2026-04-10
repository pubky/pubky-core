//! JWT-mode session construction functions.
//!
//! This module is the SDK's gateway to the homeserver's grant-based auth flow:
//! - [`session_from_grant_exchange`] turns a fresh user-signed grant
//!   into a ready-to-use, JWT-backed session.
//! - [`session_from_grant_signup`] does the same but creates the user
//!   first.
//!
//! All grant-management operations (`list_grants`, `revoke_grant`,
//! `current_jwt`, `force_refresh`, `grant_id`) live on
//! [`super::view::JwtSessionView`] — they are reachable via
//! [`PubkySession::as_jwt`] and only compile when the credential is JWT.

use std::sync::Arc;

use pubky_common::{
    auth::{grant::GrantClaims, grant_session::GrantSessionResponse},
    crypto::{Keypair, PublicKey},
};
use reqwest::Method;

use super::core::PubkySession;
use super::credential::{JwtCredential, SessionCredential, jwt::sign_pop_for_grant};
use crate::actors::storage::resource::resolve_pubky;
use crate::errors::{RequestError, Result};
use crate::util::check_http_status;
use crate::{PubkyHttpClient, cross_log};

/// Whether the homeserver should look up an existing user (signin) or create
/// one (signup) for this grant exchange.
#[derive(Debug, Clone)]
pub(crate) enum GrantExchangeMode {
    /// Existing user — homeserver is resolved via PKARR from `pubky://<user>`.
    Signin,
    /// New user — homeserver is addressed directly via its z32 host since
    /// the user has no `_pubky` record yet. The optional signup token is
    /// passed as a query param.
    Signup { signup_token: Option<String> },
}

/// Establish a JWT-backed session by exchanging a user-signed grant for
/// an access JWT at the user's homeserver.
///
/// Used by [`PubkyAuthFlow`](crate::PubkyAuthFlow) once the signer (Ring)
/// has delivered an encrypted grant via the relay channel.
///
/// # Errors
/// - Propagates HTTP transport / server errors from `POST /auth/jwt/session`.
/// - Returns [`crate::errors::Error::Authentication`] if the response JWT
///   cannot be decoded.
pub(crate) async fn session_from_grant_exchange(
    client: PubkyHttpClient,
    grant_jws: String,
    grant_claims: GrantClaims,
    client_keypair: Keypair,
    homeserver_pubkey: PublicKey,
) -> Result<PubkySession> {
    cross_log!(
        info,
        "Exchanging grant for JWT session (user={}, hs={})",
        grant_claims.iss.z32(),
        homeserver_pubkey.z32()
    );
    let response = post_grant_session(
        &client,
        &grant_jws,
        &grant_claims,
        &client_keypair,
        &homeserver_pubkey,
        &GrantExchangeMode::Signin,
    )
    .await?;
    wrap_jwt_response(
        client,
        response,
        grant_jws,
        grant_claims,
        client_keypair,
        homeserver_pubkey,
    )
}

/// Like [`session_from_grant_exchange`] but hits
/// `POST /auth/jwt/signup?signup_token=…` so the homeserver creates the
/// user first.
///
/// # Errors
/// - Propagates HTTP transport / server errors from `POST /auth/jwt/signup`.
/// - Returns [`crate::errors::Error::Authentication`] if the response JWT
///   cannot be decoded.
pub(crate) async fn session_from_grant_signup(
    client: PubkyHttpClient,
    grant_jws: String,
    grant_claims: GrantClaims,
    client_keypair: Keypair,
    homeserver_pk: PublicKey,
    signup_token: Option<&str>,
) -> Result<PubkySession> {
    cross_log!(
        info,
        "Signup grant for JWT session (user={}, hs={})",
        grant_claims.iss.z32(),
        homeserver_pk.z32()
    );
    let response = post_grant_session(
        &client,
        &grant_jws,
        &grant_claims,
        &client_keypair,
        &homeserver_pk,
        &GrantExchangeMode::Signup {
            signup_token: signup_token.map(str::to_string),
        },
    )
    .await?;
    wrap_jwt_response(
        client,
        response,
        grant_jws,
        grant_claims,
        client_keypair,
        homeserver_pk,
    )
}

fn wrap_jwt_response(
    client: PubkyHttpClient,
    response: GrantSessionResponse,
    grant_jws: String,
    grant_claims: GrantClaims,
    client_keypair: Keypair,
    homeserver_pk: PublicKey,
) -> Result<PubkySession> {
    let credential = JwtCredential::from_response(
        response,
        grant_jws,
        grant_claims,
        client_keypair,
        homeserver_pk,
    )?;
    let credential: Arc<dyn SessionCredential> = Arc::new(credential);
    Ok(PubkySession::from_credential(client, credential))
}

/// `POST` a grant + `PoP` proof to either `/auth/jwt/session` or
/// `/auth/jwt/signup`.
///
/// Routing rules:
/// - **Signin / refresh**: the user already exists, so we address the
///   homeserver via `pubky://<user_pk>/...`. PKARR resolves the user's
///   `_pubky` record to find their homeserver.
/// - **Signup**: the user doesn't exist yet — there is no `_pubky` record
///   to resolve — so we address the homeserver directly via its z32 host
///   (`https://<hs_pk>/auth/jwt/signup`).
async fn post_grant_session(
    client: &PubkyHttpClient,
    grant_jws: &str,
    grant_claims: &GrantClaims,
    client_keypair: &Keypair,
    homeserver_pk: &PublicKey,
    mode: &GrantExchangeMode,
) -> Result<GrantSessionResponse> {
    let pop_jws = sign_pop_for_grant(client_keypair, homeserver_pk, &grant_claims.jti);
    let body = serde_json::json!({ "grant": grant_jws, "pop": pop_jws });

    let resp = match mode {
        GrantExchangeMode::Signin => {
            let url = format!("pubky://{}/auth/jwt/session", grant_claims.iss.z32());
            let resolved = resolve_pubky(&url)?;
            client
                .cross_request(Method::POST, resolved)
                .await?
                .json(&body)
                .send()
                .await?
        }
        GrantExchangeMode::Signup { signup_token } => {
            let url = match signup_token {
                Some(token) => format!(
                    "https://{}/auth/jwt/signup?signup_token={}",
                    homeserver_pk.z32(),
                    token
                ),
                None => format!("https://{}/auth/jwt/signup", homeserver_pk.z32()),
            };
            let parsed = url::Url::parse(&url).map_err(|e| RequestError::Validation {
                message: format!("invalid signup url: {e}"),
            })?;
            client
                .cross_request(Method::POST, parsed)
                .await?
                .json(&body)
                .send()
                .await?
        }
    };
    let resp = check_http_status(resp).await?;
    resp.json().await.map_err(|e| {
        RequestError::DecodeJson {
            message: format!("decoding grant session response: {e}"),
        }
        .into()
    })
}

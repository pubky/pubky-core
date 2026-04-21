//! JWT credential — grant + Proof-of-Possession + access JWT.
//!
//! This is the **default** session credential. A user-signed grant JWS is
//! exchanged at the homeserver for a short-lived access JWT and a session
//! record. The SDK refreshes the JWT transparently using the stored grant
//! and a fresh `PoP` proof.

use std::sync::Arc;

use async_trait::async_trait;
use pubky_common::{
    auth::{
        access_jwt::AccessJwtClaims,
        grant::GrantClaims,
        grant_session::{GrantSessionInfo, GrantSessionResponse},
        jws::PopNonce,
    },
    crypto::{Keypair, PublicKey},
};

use reqwest::{Method, RequestBuilder};
use tokio::sync::Mutex;

use super::super::{SessionCredential, credential_session_missing};
use crate::{
    PubkyHttpClient,
    actors::session::SessionInfo,
    actors::storage::resource::resolve_pubky,
    cross_log,
    errors::{AuthError, RequestError, Result},
    util::check_http_status,
};

/// Refresh the JWT proactively when it has less than this many seconds left.
pub(crate) const REFRESH_SLACK_SECS: u64 = 300;

/// Current Unix timestamp in seconds, cross-target.
pub(crate) fn now_unix() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Mutable JWT credential state. Always wrapped in `Arc<Mutex<...>>`.
///
/// Refresh paths take the mutex and hold it across the HTTP call so
/// concurrent refreshes serialize.
#[derive(Debug)]
pub(crate) struct JwtState {
    /// Current bearer token (JWS Compact string).
    pub jwt: String,
    /// Decoded claims (used for proactive refresh checks).
    pub claims: AccessJwtClaims,
    /// The grant JWS used to mint this and future JWTs (refresh material).
    pub grant_jws: String,
    /// Decoded grant claims — exposes `iss`, `client_id`, `cnf`, `jti`, …
    pub grant_claims: GrantClaims,
    /// `PoP` keypair bound to the grant's `cnf` claim. Signs refresh proofs.
    pub client_keypair: Keypair,
    /// Homeserver public key (`PoP` audience).
    pub homeserver_pk: PublicKey,
    /// Latest server-reported session metadata.
    pub session: GrantSessionInfo,
}

/// Cheap-to-clone JWT credential. The mutable token state is shared across
/// clones via `Arc<Mutex<…>>` so every `PubkySession` clone observes the
/// same refreshes. Session info is derived from the immutable grant and
/// never changes.
#[derive(Clone, Debug)]
pub(crate) struct JwtCredential {
    pub(crate) state: Arc<Mutex<JwtState>>,
    pub(crate) info: SessionInfo,
}

impl JwtCredential {
    /// Build a JWT credential from a `GrantSessionResponse` returned by
    /// `POST /auth/jwt/session` or `POST /auth/jwt/signup`.
    pub(crate) fn from_response(
        response: GrantSessionResponse,
        grant_jws: String,
        grant_claims: GrantClaims,
        client_keypair: Keypair,
        homeserver_pk: PublicKey,
    ) -> Result<Self> {
        let claims = AccessJwtClaims::decode(&response.token)
            .map_err(|e| AuthError::Validation(format!("invalid access JWT in response: {e}")))?;
        let info = to_session_info(&response.session);
        let state = JwtState {
            jwt: response.token,
            claims,
            grant_jws,
            grant_claims,
            client_keypair,
            homeserver_pk,
            session: response.session,
        };
        Ok(Self {
            state: Arc::new(Mutex::new(state)),
            info,
        })
    }

    /// Snapshot of the current bearer token (released immediately).
    pub(crate) async fn current_jwt(&self) -> String {
        self.state.lock().await.jwt.clone()
    }

    /// Refresh the JWT credential by exchanging the stored grant for a new
    /// bearer token.
    ///
    /// Holds the credential mutex for the entire refresh so concurrent
    /// refreshes serialize on the same `Arc<Mutex<…>>`.
    pub(crate) async fn refresh(&self, client: &PubkyHttpClient) -> Result<()> {
        cross_log!(info, "Refreshing JWT credential");
        let mut state = self.state.lock().await;

        // Double-check pattern: by the time we acquired the lock, another
        // task may have already refreshed. Skip the network call if the
        // token is comfortably fresh now.
        if !state
            .claims
            .is_near_expiry(now_unix(), REFRESH_SLACK_SECS / 2)
        {
            return Ok(());
        }

        let pop_jws = sign_pop_for_grant(
            &state.client_keypair,
            &state.homeserver_pk,
            &state.grant_claims.jti,
        );
        let body = serde_json::json!({ "grant": &state.grant_jws, "pop": pop_jws });

        let url = format!("pubky{}/auth/jwt/session", state.grant_claims.iss.z32());
        let resolved = resolve_pubky(&url)?;
        let resp = client
            .cross_request(Method::POST, resolved)
            .await?
            .json(&body)
            .send()
            .await?;
        let resp = check_http_status(resp).await?;
        let parsed: GrantSessionResponse =
            resp.json().await.map_err(|e| RequestError::DecodeJson {
                message: format!("decoding /auth/jwt/session response: {e}"),
            })?;

        let new_claims = AccessJwtClaims::decode(&parsed.token)
            .map_err(|e| AuthError::Validation(format!("invalid access JWT in response: {e}")))?;

        state.jwt = parsed.token;
        state.claims = new_claims;
        state.session = parsed.session;
        Ok(())
    }
}

// Mirrors the cfg pair on the trait definition: native gets `Send` bounds
// for tokio, WASM uses `?Send` because `wasm-bindgen-futures` are not
// `Send`. See `super::super::credential::SessionCredential` for the full
// rationale.
#[cfg_attr(not(target_arch = "wasm32"), async_trait)]
#[cfg_attr(target_arch = "wasm32", async_trait(?Send))]
impl SessionCredential for JwtCredential {
    fn info(&self) -> SessionInfo {
        self.info.clone()
    }

    async fn signout(&self, client: &PubkyHttpClient) -> Result<()> {
        // Hit the auth endpoint directly and attach the bearer ourselves —
        // `/auth/jwt/session` is not a storage URL.
        let user_pk = self.state.lock().await.grant_claims.iss.clone();
        let url = format!("pubky{}/auth/jwt/session", user_pk.z32());
        let resolved = resolve_pubky(&url)?;
        let bearer = self.current_jwt().await;
        let response = client
            .cross_request(Method::DELETE, resolved)
            .await?
            .bearer_auth(&bearer)
            .send()
            .await
            .map_err(crate::Error::from)?;
        check_http_status(response).await?;
        Ok(())
    }

    async fn attach(&self, rb: RequestBuilder, client: &PubkyHttpClient) -> Result<RequestBuilder> {
        // Snapshot expiry quickly so we don't hold the lock across the
        // network call when no refresh is needed.
        let needs_refresh = {
            let jwt_state = self.state.lock().await;
            jwt_state
                .claims
                .is_near_expiry(now_unix(), REFRESH_SLACK_SECS)
        };
        if needs_refresh {
            self.refresh(client).await?;
        }
        let bearer = self.state.lock().await.jwt.clone();
        Ok(rb.bearer_auth(bearer))
    }

    async fn revalidate(
        &self,
        client: &PubkyHttpClient,
        _user: &PublicKey,
    ) -> Result<Option<SessionInfo>> {
        // We hit the auth endpoint directly (not the storage path) and
        // attach the bearer ourselves — `/auth/jwt/session` is not a
        // storage URL.
        let user_pk = self.state.lock().await.grant_claims.iss.clone();
        let url = format!("pubky{}/auth/jwt/session", user_pk.z32());
        let resolved = resolve_pubky(&url)?;
        let bearer = self.current_jwt().await;
        let response = client
            .cross_request(Method::GET, resolved)
            .await?
            .bearer_auth(&bearer)
            .send()
            .await
            .map_err(crate::Error::from)?;
        if credential_session_missing(&response) {
            return Ok(None);
        }
        let response = check_http_status(response).await?;
        let session: GrantSessionInfo =
            response
                .json()
                .await
                .map_err(|e| RequestError::DecodeJson {
                    message: format!("decoding /auth/jwt/session response: {e}"),
                })?;
        Ok(Some(to_session_info(&session)))
    }

    fn as_jwt(&self) -> Option<&JwtCredential> {
        Some(self)
    }
}

/// Build a minimal [`SessionInfo`] from a [`GrantSessionInfo`].
fn to_session_info(session: &GrantSessionInfo) -> SessionInfo {
    SessionInfo::new(session.pubky.clone(), session.capabilities.clone())
}

/// Sign a Proof-of-Possession proof JWS for a given grant.
///
/// Builds the canonical `pubky-pop` claims (`aud`, `gid`, `nonce`, `iat`)
/// and signs them with the client keypair via
/// [`pubky_common::auth::jws::sign_jws`].
pub(crate) fn sign_pop_for_grant(
    client_keypair: &Keypair,
    homeserver_pk: &PublicKey,
    grant_id: &pubky_common::auth::jws::GrantId,
) -> String {
    let claims = serde_json::json!({
        "aud": homeserver_pk.z32(),
        "gid": grant_id,
        "nonce": PopNonce::generate(),
        "iat": now_unix(),
    });
    pubky_common::auth::jws::sign_jws(client_keypair, "pubky-pop", &claims)
}

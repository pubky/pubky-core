use wasm_bindgen::prelude::*;

use crate::js_error::{JsResult, PubkyError, PubkyErrorName};
use crate::wrappers::keys::PublicKey;
use pubky_common::auth::jws::GrantId;
use serde::{Deserialize, Serialize};

const DELEGATED_GRANT_CREDENTIAL_VERSION: &str = "pubky-delegated-grant-credential-v1";

#[derive(Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DelegatedGrantCredentialJson {
    version: String,
    grant_jws: String,
    homeserver_public_key: String,
    client_public_key: String,
    key_id: String,
}

/// Grant-only view over a grant-backed `Session`.
///
/// Cookie-backed sessions do not expose this view; use `session.grant` and
/// check for `undefined` before calling grant management methods.
#[wasm_bindgen]
pub struct GrantSession(pub(crate) pubky::PubkySession);

#[wasm_bindgen]
impl GrantSession {
    /// Full grant session metadata.
    ///
    /// @returns {Promise<GrantSessionInfo>}
    #[wasm_bindgen(js_name = "sessionInfo")]
    pub async fn session_info(&self) -> JsResult<GrantSessionInfo> {
        let grant = self.as_grant()?;
        Ok(GrantSessionInfo(grant.session_info().await))
    }

    /// Current grant id (`jti`) backing this session.
    ///
    /// @returns {Promise<string>}
    #[wasm_bindgen(js_name = "grantId")]
    pub async fn grant_id(&self) -> JsResult<String> {
        let grant = self.as_grant()?;
        Ok(grant.grant_id().await.to_string())
    }

    /// List all active grants for this user.
    ///
    /// Requires a root-capability grant session. Non-root sessions surface the
    /// homeserver `403` as the standard request error.
    ///
    /// @returns {Promise<GrantInfo[]>}
    #[wasm_bindgen(js_name = "listGrants")]
    pub async fn list_grants(&self) -> JsResult<Vec<GrantInfo>> {
        let grant = self.as_grant()?;
        Ok(grant
            .list_grants()
            .await?
            .into_iter()
            .map(GrantInfo)
            .collect())
    }

    /// Revoke a specific grant by id.
    ///
    /// Requires a root-capability grant session. Malformed ids throw
    /// `InvalidInput`.
    ///
    /// @param {string} grantId
    /// @returns {Promise<void>}
    #[wasm_bindgen(js_name = "revokeGrant")]
    pub async fn revoke_grant(&self, grant_id: String) -> JsResult<()> {
        let grant_id = GrantId::parse(&grant_id).map_err(|e| {
            PubkyError::new(
                PubkyErrorName::InvalidInput,
                format!("Invalid grant id: {e}"),
            )
        })?;
        let grant = self.as_grant()?;
        grant.revoke_grant(&grant_id).await?;
        Ok(())
    }

    /// Export the durable refresh material needed to restore this grant session.
    ///
    /// Treat the returned string as bearer-equivalent secret material until the
    /// grant expires or is revoked.
    ///
    /// @returns {Promise<string>}
    #[wasm_bindgen(js_name = "exportSecret")]
    pub async fn export_secret(&self) -> JsResult<String> {
        let grant = self.as_grant()?;
        if grant.export_delegated_state().await.is_some() {
            return Err(PubkyError::new(
                PubkyErrorName::ClientStateError,
                "Delegated grant sessions cannot export raw secret material. Use exportDelegatedState().",
            ));
        }
        Ok(grant.export_secret().await)
    }

    /// Export non-secret browser delegated restore metadata.
    ///
    /// The returned string is origin-bound and only works while the matching
    /// non-extractable WebCrypto key remains in IndexedDB.
    #[wasm_bindgen(js_name = "exportDelegatedState")]
    pub async fn export_delegated_state(&self) -> JsResult<String> {
        let grant = self.as_grant()?;
        let state = grant.export_delegated_state().await.ok_or_else(|| {
            PubkyError::new(
                PubkyErrorName::ClientStateError,
                "This grant session is not delegated.",
            )
        })?;
        encode_delegated_grant_state(state)
    }
}

impl GrantSession {
    fn as_grant(&self) -> JsResult<pubky::GrantSessionView<'_>> {
        self.0.as_grant().ok_or_else(|| {
            PubkyError::new(
                PubkyErrorName::ClientStateError,
                "Session is not grant-backed.",
            )
        })
    }
}

pub(crate) fn encode_delegated_grant_state(
    state: pubky::DelegatedGrantCredentialState,
) -> JsResult<String> {
    let json = DelegatedGrantCredentialJson {
        version: DELEGATED_GRANT_CREDENTIAL_VERSION.to_string(),
        grant_jws: state.grant_jws,
        homeserver_public_key: state.homeserver_pk.z32(),
        client_public_key: state.client_pk.z32(),
        key_id: state.key_id,
    };
    serde_json::to_string(&json).map_err(|e| {
        PubkyError::new(
            PubkyErrorName::InternalError,
            format!("Failed to serialize delegated grant state: {e}"),
        )
    })
}

pub(crate) fn decode_delegated_grant_state(
    saved_state: &str,
) -> JsResult<pubky::DelegatedGrantCredentialState> {
    let json: DelegatedGrantCredentialJson = serde_json::from_str(saved_state).map_err(|e| {
        PubkyError::new(
            PubkyErrorName::InvalidInput,
            format!("Invalid delegated grant state: {e}"),
        )
    })?;
    if json.version != DELEGATED_GRANT_CREDENTIAL_VERSION {
        return Err(PubkyError::new(
            PubkyErrorName::InvalidInput,
            "Unsupported delegated grant state version.",
        ));
    }
    Ok(pubky::DelegatedGrantCredentialState {
        grant_jws: json.grant_jws,
        homeserver_pk: pubky::PublicKey::try_from_z32(&json.homeserver_public_key)
            .map_err(|e| PubkyError::new(PubkyErrorName::InvalidInput, e))?,
        client_pk: pubky::PublicKey::try_from_z32(&json.client_public_key)
            .map_err(|e| PubkyError::new(PubkyErrorName::InvalidInput, e))?,
        key_id: json.key_id,
    })
}

/// Summary of an active grant returned by `grant.listGrants()`.
#[wasm_bindgen]
pub struct GrantInfo(pub(crate) pubky_common::auth::grant_session_responses::GrantInfo);

#[wasm_bindgen]
impl GrantInfo {
    /// Grant identifier used for revocation.
    #[wasm_bindgen(js_name = "grantId", getter)]
    pub fn grant_id(&self) -> String {
        self.0.grant_id.to_string()
    }

    /// Application identifier.
    #[wasm_bindgen(js_name = "clientId", getter)]
    pub fn client_id(&self) -> String {
        self.0.client_id.clone()
    }

    /// Comma-separated capabilities authorized by the grant.
    #[wasm_bindgen(getter)]
    pub fn capabilities(&self) -> String {
        self.0.capabilities.clone()
    }

    /// Issued-at timestamp, in Unix seconds.
    #[wasm_bindgen(js_name = "issuedAt", getter)]
    pub fn issued_at(&self) -> f64 {
        self.0.issued_at as f64
    }

    /// Expiry timestamp, in Unix seconds.
    #[wasm_bindgen(js_name = "expiresAt", getter)]
    pub fn expires_at(&self) -> f64 {
        self.0.expires_at as f64
    }
}

/// Grant-specific session metadata returned by `grant.sessionInfo()`.
#[wasm_bindgen]
pub struct GrantSessionInfo(
    pub(crate) pubky_common::auth::grant_session_responses::GrantSessionInfo,
);

#[wasm_bindgen]
impl GrantSessionInfo {
    /// Homeserver that issued this session.
    #[wasm_bindgen(getter)]
    pub fn homeserver(&self) -> PublicKey {
        self.0.homeserver.clone().into()
    }

    /// User public key for this session.
    #[wasm_bindgen(js_name = "publicKey", getter)]
    pub fn public_key(&self) -> PublicKey {
        self.0.pubky.clone().into()
    }

    /// Application identifier.
    #[wasm_bindgen(js_name = "clientId", getter)]
    pub fn client_id(&self) -> String {
        self.0.client_id.to_string()
    }

    /// Authorized capabilities for this session.
    #[wasm_bindgen(getter)]
    pub fn capabilities(&self) -> Vec<String> {
        self.0
            .capabilities
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    /// Grant id this session was minted from.
    #[wasm_bindgen(js_name = "grantId", getter)]
    pub fn grant_id(&self) -> String {
        self.0.grant_id.to_string()
    }

    /// Bearer token expiry timestamp, in Unix seconds.
    #[wasm_bindgen(js_name = "tokenExpiresAt", getter)]
    pub fn token_expires_at(&self) -> f64 {
        self.0.token_expires_at as f64
    }

    /// Underlying grant expiry timestamp, in Unix seconds.
    #[wasm_bindgen(js_name = "grantExpiresAt", getter)]
    pub fn grant_expires_at(&self) -> f64 {
        self.0.grant_expires_at as f64
    }

    /// Session creation timestamp, in Unix seconds.
    #[wasm_bindgen(js_name = "createdAt", getter)]
    pub fn created_at(&self) -> f64 {
        self.0.created_at as f64
    }
}

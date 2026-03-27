//! Grant-based session creation handler.
//!
//! Accepts a Grant JWS + PoP proof, verifies both, and returns an Access JWT.
//! This is the grant-based alternative to the deprecated cookie-based `signin()` in `auth.rs`.

use axum::{extract::State, http::StatusCode, response::IntoResponse, Json};
use chrono::Utc;
use pubky_common::{
    auth::access_jwt::AccessJwtClaims,
    auth::grant_session::{GrantSessionInfo, GrantSessionResponse},
    auth::jws::TokenId,
};

/// Default JWT lifetime: 1 hour.
const DEFAULT_JWT_LIFETIME_SECS: u64 = 3600;
use serde::Deserialize;

use crate::{
    client_server::{
        auth::{
            access_jwt_issuer::AccessJwt,
            grant_verifier::Grant,
            jws_crypto::JwsCompact,
            pop_verifier::{PopProof, PopVerificationContext, POP_NONCE_GC_THRESHOLD_SECS},
        },
        err_if_user_is_invalid::get_user_or_http_error,
        AppState,
    },
    persistence::sql::{
        grant::{GrantRepository, NewGrant},
        grant_session::{GrantSessionRepository, NewGrantSession},
        pop_nonce::PopNonceRepository,
        user::UserEntity,
    },
    shared::{HttpError, HttpResult},
};

/// JSON request body for grant-based session creation.
#[derive(Deserialize)]
pub struct CreateGrantSessionRequest {
    /// Grant JWS (user-signed).
    pub grant: JwsCompact,
    /// PoP proof JWS (client-signed).
    pub pop: JwsCompact,
}

/// Handle `POST /session` with JSON body (grant-based auth).
pub async fn create_grant_session(
    State(state): State<AppState>,
    Json(request): Json<CreateGrantSessionRequest>,
) -> HttpResult<impl IntoResponse> {
    let grant = verify_grant(&request.grant)?;
    let user = find_user(&state, &grant).await?;
    let pop = verify_pop_proof(&state, &request.pop, &grant)?;
    check_nonce_replay(&state, &pop).await?;
    check_grant_not_revoked(&state, &grant).await?;
    store_grant(&state, &grant, &user).await?;
    let response = mint_session(&state, &grant, &user).await?;
    Ok(Json(response))
}

fn verify_grant(compact: &JwsCompact) -> HttpResult<Grant> {
    Grant::verify(compact).map_err(|e| {
        HttpError::new_with_message(StatusCode::BAD_REQUEST, format!("Invalid grant: {e}"))
    })
}

async fn find_user(state: &AppState, grant: &Grant) -> HttpResult<UserEntity> {
    get_user_or_http_error(&grant.issuer_key, &mut state.sql_db.pool().into(), false).await
}

fn verify_pop_proof(state: &AppState, compact: &JwsCompact, grant: &Grant) -> HttpResult<PopProof> {
    let hs_pubkey_z32 = state.homeserver_keypair.public_key().z32();
    let context = PopVerificationContext {
        cnf_key: &grant.cnf_key,
        expected_audience: &hs_pubkey_z32,
    };
    PopProof::verify(compact, &context).map_err(|e| {
        HttpError::new_with_message(StatusCode::UNAUTHORIZED, format!("Invalid PoP proof: {e}"))
    })
}

async fn check_nonce_replay(state: &AppState, pop: &PopProof) -> HttpResult<()> {
    // Lazy GC: clean old nonces before checking
    let _ = PopNonceRepository::garbage_collect(
        POP_NONCE_GC_THRESHOLD_SECS,
        &mut state.sql_db.pool().into(),
    )
    .await;

    PopNonceRepository::check_and_track(&pop.nonce, &mut state.sql_db.pool().into())
        .await
        .map_err(|_| {
            HttpError::new_with_message(StatusCode::UNAUTHORIZED, "PoP nonce already used")
        })
}

async fn check_grant_not_revoked(state: &AppState, grant: &Grant) -> HttpResult<()> {
    match GrantRepository::is_revoked(&grant.grant_id, &mut state.sql_db.pool().into()).await {
        Ok(true) => Err(HttpError::new_with_message(
            StatusCode::UNAUTHORIZED,
            "Grant has been revoked",
        )),
        Ok(false) => Ok(()),
        // Grant not found in DB yet — that's fine, it will be stored next
        Err(sqlx::Error::RowNotFound) => Ok(()),
        Err(e) => Err(e.into()),
    }
}

async fn store_grant(state: &AppState, grant: &Grant, user: &UserEntity) -> HttpResult<()> {
    let new_grant = NewGrant {
        grant_id: grant.grant_id.clone(),
        user_id: user.id,
        client_id: grant.client_id.clone(),
        client_cnf_key: grant.cnf_key.z32(),
        capabilities: grant.capabilities.clone(),
        issued_at: grant.issued_at.timestamp() as u64,
        expires_at: grant.expires_at.timestamp() as u64,
    };
    GrantRepository::create(&new_grant, &mut state.sql_db.pool().into()).await?;
    Ok(())
}

async fn mint_session(
    state: &AppState,
    grant: &Grant,
    user: &UserEntity,
) -> HttpResult<GrantSessionResponse> {
    let now = Utc::now().timestamp() as u64;
    let token_id = TokenId::generate();
    let jwt_exp = now + DEFAULT_JWT_LIFETIME_SECS;

    let raw_jwt = AccessJwtClaims {
        iss: state.homeserver_keypair.public_key(),
        sub: grant.issuer_key.clone(),
        gid: grant.grant_id.clone(),
        jti: token_id.clone(),
        iat: now,
        exp: jwt_exp,
    };

    let token = AccessJwt::mint(&state.homeserver_keypair, &raw_jwt);

    // Store session record (enforces max-2-per-grant)
    let new_session = NewGrantSession {
        token_id: token_id.clone(),
        grant_id: grant.grant_id.clone(),
        expires_at: jwt_exp,
    };
    GrantSessionRepository::create(&new_session, &mut state.sql_db.pool().into()).await?;

    Ok(GrantSessionResponse {
        token,
        session: GrantSessionInfo {
            homeserver: state.homeserver_keypair.public_key(),
            pubky: grant.issuer_key.clone(),
            client_id: grant.client_id.clone(),
            capabilities: grant.capabilities.to_vec(),
            grant_id: grant.grant_id.clone(),
            token_expires_at: jwt_exp,
            grant_expires_at: grant.expires_at.timestamp() as u64,
            created_at: now,
        },
    })
}

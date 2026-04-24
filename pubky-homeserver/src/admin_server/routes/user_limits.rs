use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pubky_common::crypto::PublicKey;

use crate::{
    data_directory::user_limit_config::UserLimitConfig,
    persistence::sql::user::{UserEntity, UserRepository},
    shared::{HttpError, HttpResult},
};

use super::super::app_state::AppState;

/// Parse a z32 public key path param and fetch the corresponding user entity.
async fn resolve_user(state: &AppState, pubkey_str: &str) -> HttpResult<UserEntity> {
    let pubkey = PublicKey::try_from_z32(pubkey_str).map_err(|_| {
        HttpError::new_with_message(StatusCode::BAD_REQUEST, "Invalid public key")
    })?;

    UserRepository::get(&pubkey, &mut state.sql_db.pool().into())
        .await
        .map_err(|e| match e {
            sqlx::Error::RowNotFound => {
                HttpError::new_with_message(StatusCode::NOT_FOUND, "User not found")
            }
            other => other.into(),
        })
}

/// GET /users/{pubkey}/limits — return the user's effective limits.
///
/// If the user has custom limits, returns those. Otherwise returns deploy-time defaults.
pub async fn get_user_limits(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
) -> HttpResult<impl IntoResponse> {
    let user = resolve_user(&state, &pubkey_str).await?;

    let effective = user
        .custom_limits()
        .unwrap_or_else(|| state.default_user_limits.clone());

    Ok(Json(effective))
}

/// PUT /users/{pubkey}/limits — set per-user custom limits (replaces entirely).
///
/// All fields in the JSON body are optional. Omitted fields = unlimited.
/// To revert to deploy-time defaults, use DELETE instead.
pub async fn put_user_limits(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
    Json(config): Json<UserLimitConfig>,
) -> HttpResult<impl IntoResponse> {
    config.validate().map_err(|msg| {
        HttpError::new_with_message(StatusCode::BAD_REQUEST, msg)
    })?;

    let user = resolve_user(&state, &pubkey_str).await?;

    UserRepository::set_custom_limits(user.id, &config, &mut state.sql_db.pool().into()).await?;

    // Evict from shared cache so the next request re-resolves from DB
    state.user_limits_cache.remove(&user.public_key);

    Ok(StatusCode::OK)
}

/// DELETE /users/{pubkey}/limits — remove per-user custom limits (revert to defaults).
pub async fn delete_user_limits(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
) -> HttpResult<impl IntoResponse> {
    let user = resolve_user(&state, &pubkey_str).await?;

    UserRepository::clear_custom_limits(user.id, &mut state.sql_db.pool().into()).await?;

    // Evict from shared cache so the next request re-resolves from DB
    state.user_limits_cache.remove(&user.public_key);

    Ok(StatusCode::OK)
}

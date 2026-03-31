use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pubky_common::crypto::PublicKey;
use serde::Deserialize;

use crate::{
    data_directory::quota_config::BandwidthBudget,
    data_directory::user_limit_config::UserLimitConfig,
    persistence::sql::user::{UserEntity, UserRepository},
    shared::{HttpError, HttpResult},
};

use super::super::app_state::AppState;

/// Admin API input for setting user limits.
///
/// Unlike [`UserLimitConfig`], every field is **required** — omitting a field
/// causes a 422 deserialization error. Use explicit `null` to mean "unlimited".
/// This prevents accidentally granting unlimited access by forgetting a field.
///
/// Example:
/// ```json
/// {
///   "storage_quota_mb": 500,
///   "max_sessions": 5,
///   "rate_read": "500mb/d",
///   "rate_write": null
/// }
/// ```
#[derive(Debug, Clone)]
pub(crate) struct ExplicitUserLimitConfig {
    /// Maximum storage in MB. `null` = unlimited.
    pub storage_quota_mb: Option<u64>,
    /// Maximum concurrent sessions. `null` = unlimited.
    pub max_sessions: Option<u32>,
    /// Per-user read bandwidth budget (e.g. "500mb/d"). `null` = unlimited.
    pub rate_read: Option<BandwidthBudget>,
    /// Per-user write bandwidth budget (e.g. "100mb/h"). `null` = unlimited.
    pub rate_write: Option<BandwidthBudget>,
}

impl<'de> Deserialize<'de> for ExplicitUserLimitConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Required {
            storage_quota_mb: Option<u64>,
            max_sessions: Option<u32>,
            rate_read: Option<BandwidthBudget>,
            rate_write: Option<BandwidthBudget>,
        }

        // serde treats Option<T> as implicitly optional (absent → None).
        // To detect missing keys we deserialize into a Value first and check keys.
        let value = serde_json::Value::deserialize(deserializer)?;
        let obj = value
            .as_object()
            .ok_or_else(|| serde::de::Error::custom("expected a JSON object"))?;

        for field in [
            "storage_quota_mb",
            "max_sessions",
            "rate_read",
            "rate_write",
        ] {
            if !obj.contains_key(field) {
                return Err(serde::de::Error::missing_field(field));
            }
        }

        let r: Required = serde_json::from_value(value).map_err(serde::de::Error::custom)?;

        Ok(ExplicitUserLimitConfig {
            storage_quota_mb: r.storage_quota_mb,
            max_sessions: r.max_sessions,
            rate_read: r.rate_read,
            rate_write: r.rate_write,
        })
    }
}

impl From<ExplicitUserLimitConfig> for UserLimitConfig {
    fn from(e: ExplicitUserLimitConfig) -> Self {
        Self {
            storage_quota_mb: e.storage_quota_mb,
            max_sessions: e.max_sessions,
            rate_read: e.rate_read,
            rate_write: e.rate_write,
        }
    }
}

/// Parse a z32 public key path param and fetch the corresponding user entity.
async fn resolve_user(state: &AppState, pubkey_str: &str) -> HttpResult<UserEntity> {
    let pubkey = PublicKey::try_from_z32(pubkey_str)
        .map_err(|_| HttpError::new_with_message(StatusCode::BAD_REQUEST, "Invalid public key"))?;

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
/// Every user row has explicit limit columns (set during signup or migration
/// backfill), so `limits()` is always the source of truth.
pub async fn get_user_limits(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
) -> HttpResult<impl IntoResponse> {
    let user = resolve_user(&state, &pubkey_str).await?;

    Ok(Json(user.limits()))
}

/// PUT /users/{pubkey}/limits — set per-user custom limits (replaces entirely).
///
/// All four fields are **required**. Use `null` for unlimited.
/// Omitting a field returns 422, preventing accidental unlimited grants.
pub async fn put_user_limits(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
    Json(explicit): Json<ExplicitUserLimitConfig>,
) -> HttpResult<impl IntoResponse> {
    let config: UserLimitConfig = explicit.into();
    let user = resolve_user(&state, &pubkey_str).await?;

    UserRepository::set_custom_limits(user.id, &config, &mut state.sql_db.pool().into()).await?;

    // Evict from shared cache so the next request re-resolves from DB
    state.user_limits_cache.remove(&user.public_key);

    Ok(StatusCode::OK)
}

/// DELETE /users/{pubkey}/limits — clear all per-user limit columns (set to NULL).
///
/// This makes the user **unlimited** on all dimensions (storage, sessions, rates).
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

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    #[test]
    fn test_explicit_config_requires_all_fields() {
        let result: Result<ExplicitUserLimitConfig, _> =
            serde_json::from_str(r#"{"storage_quota_mb": 500}"#);
        assert!(result.is_err(), "Missing fields should be rejected");

        let result: Result<ExplicitUserLimitConfig, _> = serde_json::from_str(r#"{}"#);
        assert!(result.is_err(), "Empty object should be rejected");
    }

    #[test]
    fn test_explicit_config_accepts_all_fields_with_nulls() {
        let json = r#"{
            "storage_quota_mb": 500,
            "max_sessions": null,
            "rate_read": "100mb/m",
            "rate_write": null
        }"#;
        let explicit: ExplicitUserLimitConfig = serde_json::from_str(json).unwrap();
        let config: UserLimitConfig = explicit.into();
        assert_eq!(config.storage_quota_mb, Some(500));
        assert_eq!(config.max_sessions, None);
        assert_eq!(
            config.rate_read,
            Some(BandwidthBudget::from_str("100mb/m").unwrap())
        );
        assert_eq!(config.rate_write, None);
    }

    #[test]
    fn test_explicit_config_all_null_is_all_unlimited() {
        let json = r#"{
            "storage_quota_mb": null,
            "max_sessions": null,
            "rate_read": null,
            "rate_write": null
        }"#;
        let explicit: ExplicitUserLimitConfig = serde_json::from_str(json).unwrap();
        let config: UserLimitConfig = explicit.into();
        assert_eq!(config, UserLimitConfig::default());
    }
}

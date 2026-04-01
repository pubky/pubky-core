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
    data_directory::user_resource_quota::UserResourceQuota,
    persistence::sql::user::{UserEntity, UserRepository},
    shared::{HttpError, HttpResult},
};

use super::super::app_state::AppState;

/// Admin API input for setting user limits.
///
/// Unlike [`UserResourceQuota`], every field is **required** — omitting a field
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
pub(crate) struct ExplicitUserResourceQuota {
    /// Maximum storage in MB. `null` = unlimited.
    pub storage_quota_mb: Option<u64>,
    /// Maximum concurrent sessions. `null` = unlimited.
    pub max_sessions: Option<u32>,
    /// Per-user read bandwidth budget (e.g. "500mb/d"). `null` = unlimited.
    pub rate_read: Option<BandwidthBudget>,
    /// Per-user write bandwidth budget (e.g. "100mb/h"). `null` = unlimited.
    pub rate_write: Option<BandwidthBudget>,
}

impl<'de> Deserialize<'de> for ExplicitUserResourceQuota {
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

        Ok(ExplicitUserResourceQuota {
            storage_quota_mb: r.storage_quota_mb,
            max_sessions: r.max_sessions,
            rate_read: r.rate_read,
            rate_write: r.rate_write,
        })
    }
}

impl From<ExplicitUserResourceQuota> for UserResourceQuota {
    fn from(e: ExplicitUserResourceQuota) -> Self {
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

/// GET /users/{pubkey}/resource-quotas — return the user's effective limits.
///
/// Every user row has explicit limit columns (set during signup or migration
/// backfill), so `limits()` is always the source of truth.
pub async fn get_user_resource_quota(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
) -> HttpResult<impl IntoResponse> {
    let user = resolve_user(&state, &pubkey_str).await?;

    Ok(Json(user.resource_quota()))
}

/// PUT /users/{pubkey}/resource-quotas — set per-user custom limits (replaces entirely).
///
/// All four fields are **required**. Use `null` for unlimited.
/// Omitting a field returns 422, preventing accidental unlimited grants.
pub async fn put_user_resource_quota(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
    Json(explicit): Json<ExplicitUserResourceQuota>,
) -> HttpResult<impl IntoResponse> {
    let config: UserResourceQuota = explicit.into();
    let user = resolve_user(&state, &pubkey_str).await?;

    UserRepository::set_resource_quota(user.id, &config, &mut state.sql_db.pool().into()).await?;

    // Evict from shared cache so the next request re-resolves from DB
    state.user_resource_quota_cache.remove(&user.public_key);

    Ok(StatusCode::OK)
}

/// DELETE /users/{pubkey}/resource-quotas — clear all per-user limit columns (set to NULL).
///
/// **Warning:** This makes the user **fully unlimited** on all dimensions
/// (storage, sessions, rates). It does NOT revert to deploy-time defaults —
/// there is no "use defaults" state. To apply specific limits, use
/// `PUT /users/{pubkey}/resource-quotas` instead.
pub async fn delete_user_resource_quota(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
) -> HttpResult<impl IntoResponse> {
    let user = resolve_user(&state, &pubkey_str).await?;

    UserRepository::clear_resource_quota(user.id, &mut state.sql_db.pool().into()).await?;

    // Evict from shared cache so the next request re-resolves from DB
    state.user_resource_quota_cache.remove(&user.public_key);

    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use axum_test::TestServer;

    use super::*;
    use crate::admin_server::app::create_app;
    use crate::persistence::files::FileService;
    use crate::AppContext;

    #[test]
    fn test_explicit_config_requires_all_fields() {
        let result: Result<ExplicitUserResourceQuota, _> =
            serde_json::from_str(r#"{"storage_quota_mb": 500}"#);
        assert!(result.is_err(), "Missing fields should be rejected");

        let result: Result<ExplicitUserResourceQuota, _> = serde_json::from_str(r#"{}"#);
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
        let explicit: ExplicitUserResourceQuota = serde_json::from_str(json).unwrap();
        let config: UserResourceQuota = explicit.into();
        assert_eq!(config.storage_quota_mb, Some(500));
        assert_eq!(config.max_sessions, None);
        assert_eq!(
            config.rate_read,
            Some(BandwidthBudget::from_str("100mb/m").unwrap())
        );
        assert_eq!(config.rate_write, None);
    }

    fn create_test_server(context: &AppContext) -> TestServer {
        TestServer::new(create_app(
            AppState::new(
                context.sql_db.clone(),
                FileService::new_from_context(context).unwrap(),
                "",
            ),
            "test",
        ))
        .unwrap()
    }

    #[test]
    fn test_explicit_config_all_null_is_all_unlimited() {
        let json = r#"{
            "storage_quota_mb": null,
            "max_sessions": null,
            "rate_read": null,
            "rate_write": null
        }"#;
        let explicit: ExplicitUserResourceQuota = serde_json::from_str(json).unwrap();
        let config: UserResourceQuota = explicit.into();
        assert_eq!(config, UserResourceQuota::default());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_user_resource_quota_crud() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let url = format!("/users/{}/resource-quotas", pubkey.z32());

        // GET defaults (no overrides set)
        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();

        // PUT overrides (all fields required — null = unlimited)
        let body = serde_json::json!({
            "storage_quota_mb": 500,
            "max_sessions": 10,
            "rate_read": "100mb/m",
            "rate_write": null
        });
        let response = server
            .put(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_success()
            .await;
        response.assert_status_ok();

        // GET reflects the overrides
        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();
        let json: serde_json::Value = response.json();
        assert_eq!(json["storage_quota_mb"], 500);
        assert_eq!(json["max_sessions"], 10);
        assert_eq!(json["rate_read"], "100mb/m");

        // DELETE overrides
        let response = server
            .delete(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();

        // GET after delete should show unlimited (null = no limit)
        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();
        let json: serde_json::Value = response.json();
        assert!(json["storage_quota_mb"].is_null());
        assert!(json["max_sessions"].is_null());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_user_resource_quota_invalid_rate_rejected() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let url = format!("/users/{}/resource-quotas", pubkey.z32());

        // PUT with invalid rate string should be rejected (422 from serde validation)
        let body = serde_json::json!({
            "storage_quota_mb": null,
            "max_sessions": null,
            "rate_read": "rubbish",
            "rate_write": null
        });
        let response = server
            .put(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_failure()
            .await;
        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }

    /// Omitting a required field in the PUT body should return 422.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_user_resource_quota_missing_field_rejected() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let url = format!("/users/{}/resource-quotas", pubkey.z32());

        // Missing rate_write field — should be rejected
        let body = serde_json::json!({
            "storage_quota_mb": 500,
            "max_sessions": 10,
            "rate_read": "100mb/m"
        });
        let response = server
            .put(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_failure()
            .await;
        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);

        // Empty body — also rejected
        let response = server
            .put(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(b"{}".to_vec().into())
            .expect_failure()
            .await;
        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_user_resource_quota_nonexistent_user() {
        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let pubkey = pubky_common::crypto::Keypair::random().public_key();

        let url = format!("/users/{}/resource-quotas", pubkey.z32());

        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_failure()
            .await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    /// PUT /users/{pubkey}/resource-quotas replaces the entire custom config.
    /// Fields not included in the JSON body default to None (unlimited).
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_put_user_resource_quota_replaces_all_fields() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let url = format!("/users/{}/resource-quotas", pubkey.z32());

        // 1) PUT all four fields
        let body = serde_json::json!({
            "storage_quota_mb": 500,
            "max_sessions": 10,
            "rate_read": "100mb/m",
            "rate_write": "50mb/m"
        });
        server
            .put(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_success()
            .await;

        // 2) PUT with storage_quota_mb=200, others explicitly unlimited
        let body = serde_json::json!({
            "storage_quota_mb": 200,
            "max_sessions": null,
            "rate_read": null,
            "rate_write": null
        });
        server
            .put(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_success()
            .await;

        // 3) Verify: storage_quota_mb is 200, others are unlimited (null)
        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        let json: serde_json::Value = response.json();
        assert_eq!(json["storage_quota_mb"], 200);
        assert!(
            json["max_sessions"].is_null(),
            "max_sessions should be unlimited after PUT replace"
        );
        assert!(
            json["rate_read"].is_null(),
            "rate_read should be unlimited after PUT replace"
        );
        assert!(
            json["rate_write"].is_null(),
            "rate_write should be unlimited after PUT replace"
        );
    }
}

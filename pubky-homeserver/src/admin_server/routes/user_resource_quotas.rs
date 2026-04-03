use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pubky_common::crypto::PublicKey;

use crate::{
    data_directory::user_resource_quota::UserResourceQuota,
    persistence::sql::user::{UserEntity, UserRepository},
    shared::{HttpError, HttpResult},
};

use super::super::app_state::AppState;

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
pub async fn get_user_resource_quota(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
) -> HttpResult<impl IntoResponse> {
    let user = resolve_user(&state, &pubkey_str).await?;

    Ok(Json(user.resource_quota()))
}

/// PUT /users/{pubkey}/resource-quotas — set per-user custom limits (replaces entirely).
///
/// Accepts a partial JSON body:
/// - `storage_quota_mb` / `max_sessions`: absent or null → no limit, value → explicit limit
/// - `rate_read` / `rate_write`: absent → Default, null → Unlimited, value → explicit limit
pub async fn put_user_resource_quota(
    State(state): State<AppState>,
    Path(pubkey_str): Path<String>,
    Json(config): Json<UserResourceQuota>,
) -> HttpResult<impl IntoResponse> {
    // Validate rate strings before touching the DB — return 422 for bad values.
    config
        .validate_rate_roundtrips()
        .map_err(|e| HttpError::new_with_message(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    let user = resolve_user(&state, &pubkey_str).await?;

    UserRepository::set_resource_quota(user.id, &config, &mut state.sql_db.pool().into()).await?;

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
    use crate::data_directory::quota_config::BandwidthRate;
    use crate::data_directory::user_resource_quota::QuotaOverride;
    use crate::persistence::files::FileService;
    use crate::AppContext;

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
    fn test_partial_body_absent_fields_are_default() {
        let json = r#"{"storage_quota_mb": 500}"#;
        let config: UserResourceQuota = serde_json::from_str(json).unwrap();
        assert_eq!(config.storage_quota_mb, Some(500));
        assert_eq!(config.max_sessions, None);
        assert_eq!(config.rate_read, QuotaOverride::Default);
        assert_eq!(config.rate_write, QuotaOverride::Default);
    }

    #[test]
    fn test_null_fields() {
        let json = r#"{
            "storage_quota_mb": null,
            "max_sessions": null,
            "rate_read": null,
            "rate_write": null
        }"#;
        let config: UserResourceQuota = serde_json::from_str(json).unwrap();
        // null maps to None (no limit) for storage/sessions
        assert_eq!(config.storage_quota_mb, None);
        assert_eq!(config.max_sessions, None);
        // null maps to Unlimited for rate fields
        assert_eq!(config.rate_read, QuotaOverride::Unlimited);
        assert_eq!(config.rate_write, QuotaOverride::Unlimited);
    }

    #[test]
    fn test_empty_body_is_all_default() {
        let config: UserResourceQuota = serde_json::from_str("{}").unwrap();
        assert_eq!(config, UserResourceQuota::default());
    }

    #[test]
    fn test_mixed_fields() {
        let json = r#"{
            "storage_quota_mb": 500,
            "max_sessions": null,
            "rate_read": "100mb/m"
        }"#;
        let config: UserResourceQuota = serde_json::from_str(json).unwrap();
        assert_eq!(config.storage_quota_mb, Some(500));
        assert_eq!(config.max_sessions, None);
        assert_eq!(
            config.rate_read,
            QuotaOverride::Value(BandwidthRate::from_str("100mb/m").unwrap())
        );
        assert_eq!(config.rate_write, QuotaOverride::Default);
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

        // GET defaults (all fields Default)
        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();
        // Default user should have empty JSON (all fields Default → omitted)
        let json: serde_json::Value = response.json();
        assert_eq!(json, serde_json::json!({}));

        // PUT with partial body (absent fields = Default)
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
            .expect_success()
            .await;
        response.assert_status_ok();

        // GET reflects the overrides — Default fields should be absent
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
        // rate_write was Default → should be absent from JSON
        assert!(json.get("rate_write").is_none());

        // PUT with null fields to make unlimited
        let body = serde_json::json!({
            "storage_quota_mb": null,
            "max_sessions": null,
            "rate_read": null,
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

        // GET after all-null PUT: storage/sessions are None (omitted),
        // rate fields are Unlimited (serialized as null)
        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();
        let json: serde_json::Value = response.json();
        // storage_quota_mb and max_sessions: null → None → omitted from JSON
        assert!(json.get("storage_quota_mb").is_none());
        assert!(json.get("max_sessions").is_none());
        // rate fields: null → Unlimited → serialized as null
        assert!(json["rate_read"].is_null());
        assert!(json["rate_write"].is_null());
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

        // PUT with invalid rate string should be rejected
        let body = serde_json::json!({
            "rate_read": "rubbish"
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

    /// PUT /users/{pubkey}/resource-quotas replaces the entire config.
    /// Absent fields become Default (NULL in DB).
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

        // 1) PUT all four fields with values
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

        // 2) PUT with only storage_quota_mb — others become Default
        let body = serde_json::json!({
            "storage_quota_mb": 200
        });
        server
            .put(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_success()
            .await;

        // 3) Verify: storage_quota_mb is 200, others are Default (absent from JSON)
        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        let json: serde_json::Value = response.json();
        assert_eq!(json["storage_quota_mb"], 200);
        assert!(
            json.get("max_sessions").is_none(),
            "max_sessions should be Default (absent) after PUT replace"
        );
        assert!(
            json.get("rate_read").is_none(),
            "rate_read should be Default (absent) after PUT replace"
        );
        assert!(
            json.get("rate_write").is_none(),
            "rate_write should be Default (absent) after PUT replace"
        );
    }

    /// Test that Default vs Unlimited are distinguishable for rate fields in GET response.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_default_vs_unlimited_distinguishable_for_rates() {
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

        // PUT: rate_read = null (Unlimited), rate_write absent (Default)
        let body = serde_json::json!({
            "rate_read": null
        });
        server
            .put(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_success()
            .await;

        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        let json: serde_json::Value = response.json();
        // Unlimited → present as null
        assert!(
            json.get("rate_read").is_some(),
            "Unlimited rate field should be present in JSON"
        );
        assert!(json["rate_read"].is_null());
        // Default → absent
        assert!(
            json.get("rate_write").is_none(),
            "Default rate field should be absent from JSON"
        );
        // storage/sessions: null and absent both map to None → both omitted
        assert!(json.get("storage_quota_mb").is_none());
        assert!(json.get("max_sessions").is_none());
    }
}

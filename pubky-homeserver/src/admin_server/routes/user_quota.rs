use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};

use crate::{
    persistence::sql::user::UserRepository,
    persistence::user_quota::UserQuotaPatch,
    shared::{HttpError, HttpResult, Z32Pubkey},
};

use super::super::app_state::AppState;

/// Map a sqlx error to an HTTP error, turning `RowNotFound` into 404.
fn map_user_not_found(e: sqlx::Error) -> HttpError {
    match e {
        sqlx::Error::RowNotFound => {
            HttpError::new_with_message(StatusCode::NOT_FOUND, "User not found")
        }
        other => other.into(),
    }
}

/// GET /users/{pubkey}/quota — return the user's effective limits.
pub async fn get_user_quota(
    State(state): State<AppState>,
    Path(pubkey): Path<Z32Pubkey>,
) -> HttpResult<impl IntoResponse> {
    let user = UserRepository::get(&pubkey.0, &mut state.sql_db.pool().into())
        .await
        .map_err(map_user_not_found)?;

    Ok(Json(user.quota()))
}

/// PATCH /users/{pubkey}/quota — update per-user custom limits.
///
/// Only fields present in the JSON body are updated; absent fields are left unchanged.
/// All fields follow the same semantics:
/// - absent → keep existing value
/// - `null` → reset to Default (use system default)
/// - `"unlimited"` → Unlimited (no limit)
/// - value → explicit custom limit
pub async fn patch_user_quota(
    State(state): State<AppState>,
    Path(pubkey): Path<Z32Pubkey>,
    Json(patch): Json<UserQuotaPatch>,
) -> HttpResult<impl IntoResponse> {
    patch
        .validate_rate_roundtrips()
        .map_err(|e| HttpError::new_with_message(StatusCode::UNPROCESSABLE_ENTITY, e))?;

    UserRepository::patch_quota(&pubkey.0, &patch, state.sql_db.pool())
        .await
        .map_err(map_user_not_found)?;

    // Evict from shared cache so the next request re-resolves from DB
    state.user_quota_cache.remove(&pubkey.0);

    Ok(StatusCode::OK)
}

#[cfg(test)]
mod tests {
    use axum_test::TestServer;

    use super::*;
    use crate::admin_server::app::create_app;
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

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_user_quota_crud() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let url = format!("/users/{}/quota", pubkey.z32());

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

        // PATCH with partial body (absent fields = keep existing)
        let body = serde_json::json!({
            "storage_quota_mb": 500,
            "rate_read": "100mb/m"
        });
        let response = server
            .patch(&url)
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
        assert_eq!(json["rate_read"], "100mb/m");
        // rate_write was Default → should be absent from JSON
        assert!(json.get("rate_write").is_none());

        // PATCH with null fields to reset to Default
        let body = serde_json::json!({
            "storage_quota_mb": null,
            "rate_read": null,
            "rate_write": null
        });
        let response = server
            .patch(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_success()
            .await;
        response.assert_status_ok();

        // GET after all-null PATCH: all fields are Default → omitted from JSON
        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();
        let json: serde_json::Value = response.json();
        assert_eq!(json, serde_json::json!({}));
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_user_quota_invalid_rate_rejected() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let url = format!("/users/{}/quota", pubkey.z32());

        // PATCH with invalid rate string should be rejected
        let body = serde_json::json!({
            "rate_read": "rubbish"
        });
        let response = server
            .patch(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_failure()
            .await;
        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_user_quota_nonexistent_user() {
        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let pubkey = pubky_common::crypto::Keypair::random().public_key();

        let url = format!("/users/{}/quota", pubkey.z32());

        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_failure()
            .await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }

    /// Test that Default vs Unlimited are distinguishable in GET response.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_default_vs_unlimited_distinguishable() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let url = format!("/users/{}/quota", pubkey.z32());

        // PATCH: rate_read = "unlimited", rate_write absent (unchanged = Default)
        let body = serde_json::json!({
            "rate_read": "unlimited"
        });
        server
            .patch(&url)
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
        // Unlimited → present as "unlimited"
        assert_eq!(
            json["rate_read"], "unlimited",
            "Unlimited rate field should be 'unlimited' in JSON"
        );
        // Default → absent
        assert!(
            json.get("rate_write").is_none(),
            "Default rate field should be absent from JSON"
        );
        assert!(json.get("storage_quota_mb").is_none());
    }

    /// PATCH only updates the fields present in the body; absent fields are left unchanged.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_patch_user_quota_merges() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let url = format!("/users/{}/quota", pubkey.z32());

        // 1) Set all fields
        let body = serde_json::json!({
            "storage_quota_mb": 500,
            "rate_read": "100mb/m",
            "rate_write": "50mb/m"
        });
        server
            .patch(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&body).unwrap().into())
            .expect_success()
            .await;

        // 2) PATCH only storage_quota_mb — others should be unchanged
        let patch = serde_json::json!({
            "storage_quota_mb": 200
        });
        server
            .patch(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&patch).unwrap().into())
            .expect_success()
            .await;

        // 3) Verify: storage_quota_mb changed, all others preserved
        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        let json: serde_json::Value = response.json();
        assert_eq!(json["storage_quota_mb"], 200);
        assert_eq!(json["rate_read"], "100mb/m");
        assert_eq!(json["rate_write"], "50mb/m");

        // 4) PATCH rate_write to "unlimited", leave rest unchanged
        let patch = serde_json::json!({
            "rate_write": "unlimited"
        });
        server
            .patch(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&patch).unwrap().into())
            .expect_success()
            .await;

        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        let json: serde_json::Value = response.json();
        assert_eq!(json["storage_quota_mb"], 200);
        assert_eq!(json["rate_read"], "100mb/m");
        assert_eq!(
            json["rate_write"], "unlimited",
            "rate_write should be Unlimited"
        );

        // 5) Empty PATCH should change nothing
        let patch = serde_json::json!({});
        server
            .patch(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&patch).unwrap().into())
            .expect_success()
            .await;

        let response = server
            .get(&url)
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        let json: serde_json::Value = response.json();
        assert_eq!(json["storage_quota_mb"], 200);
        assert_eq!(json["rate_read"], "100mb/m");
        assert_eq!(json["rate_write"], "unlimited");
    }

    /// PATCH with invalid rate string should be rejected with 422.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_patch_invalid_rate_rejected() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let keypair = Keypair::random();
        let pubkey = keypair.public_key();

        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let url = format!("/users/{}/quota", pubkey.z32());
        let patch = serde_json::json!({
            "rate_read": "rubbish"
        });
        let response = server
            .patch(&url)
            .add_header("X-Admin-Password", "test")
            .content_type("application/json")
            .bytes(serde_json::to_vec(&patch).unwrap().into())
            .expect_failure()
            .await;
        response.assert_status(axum::http::StatusCode::UNPROCESSABLE_ENTITY);
    }
}

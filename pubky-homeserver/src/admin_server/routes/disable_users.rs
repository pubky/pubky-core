use super::super::app_state::AppState;
use crate::{
    persistence::sql::{uexecutor, user::UserRepository},
    shared::{HttpError, HttpResult, Z32Pubkey},
};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use pubky_common::crypto::PublicKey;
use serde::{Deserialize, Serialize};

/// Delete a single entry from the database.
///
/// # Errors
///
/// - `400` if the pubkey is invalid.
/// - `404` if the entry does not exist.
///
pub async fn disable_user(
    State(state): State<AppState>,
    Path(pubkey): Path<Z32Pubkey>,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.sql_db.pool().begin().await?;
    let mut user = match UserRepository::get(&pubkey.0, uexecutor!(tx)).await {
        Ok(user) => user,
        Err(sqlx::Error::RowNotFound) => {
            return Err(HttpError::new_with_message(
                StatusCode::NOT_FOUND,
                "User not found",
            ))
        }
        Err(e) => return Err(e.into()),
    };
    user.disabled = true;
    UserRepository::update(&user, uexecutor!(tx)).await?;
    tx.commit().await?;

    Ok((StatusCode::OK, "Ok"))
}

/// Delete a single entry from the database.
///
/// # Errors
///
/// - `400` if the pubkey is invalid.
/// - `404` if the entry does not exist.
///
pub async fn enable_user(
    State(state): State<AppState>,
    Path(pubkey): Path<Z32Pubkey>,
) -> HttpResult<impl IntoResponse> {
    let mut tx = state.sql_db.pool().begin().await?;
    let mut user = match UserRepository::get(&pubkey.0, uexecutor!(tx)).await {
        Ok(user) => user,
        Err(sqlx::Error::RowNotFound) => {
            return Err(HttpError::new_with_message(
                StatusCode::NOT_FOUND,
                "User not found",
            ))
        }
        Err(e) => return Err(e.into()),
    };
    user.disabled = false;
    UserRepository::update(&user, uexecutor!(tx)).await?;
    tx.commit().await?;

    Ok((StatusCode::OK, "Ok"))
}

#[derive(Debug, Deserialize)]
pub struct ListDisabledUsersQuery {
    limit: Option<u16>,
    cursor: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DisabledUser {
    pubkey: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ListDisabledUsersResponse {
    items: Vec<DisabledUser>,
    next_cursor: Option<String>,
}

/// List disabled users with cursor-based pagination.
///
/// # Errors
///
/// - `400` if `cursor` is invalid.
pub async fn list_disabled_users(
    State(state): State<AppState>,
    Query(query): Query<ListDisabledUsersQuery>,
) -> HttpResult<(StatusCode, Json<ListDisabledUsersResponse>)> {
    let cursor = query
        .cursor
        .as_deref()
        .map(PublicKey::try_from_z32)
        .transpose()
        .map_err(|_| HttpError::bad_request("Invalid cursor"))?;

    let page =
        UserRepository::list_disabled(query.limit, cursor, &mut state.sql_db.pool().into()).await?;

    let body = ListDisabledUsersResponse {
        items: page
            .users
            .into_iter()
            .map(|pubkey| DisabledUser {
                pubkey: pubkey.z32(),
            })
            .collect(),
        next_cursor: page.next_cursor.map(|cursor| cursor.z32()),
    };

    Ok((StatusCode::OK, Json(body)))
}

#[cfg(test)]
mod tests {
    use super::super::super::app_state::AppState;
    use super::*;
    use crate::{persistence::files::FileService, AppContext};
    use axum::routing::{get, post};
    use axum::Router;
    use pubky_common::crypto::Keypair;

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_disable_enable_user() {
        let context = AppContext::test().await;
        let pubkey = Keypair::random().public_key();

        // Create new user
        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        // Check that the tenant is enabled
        let user = UserRepository::get(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();
        assert!(!user.disabled);

        // Setup server
        let app_state = AppState::new(
            context.sql_db.clone(),
            FileService::new_from_context(&context).unwrap(),
            "",
        );
        let router = Router::new()
            .route("/users/{pubkey}/disable", post(disable_user))
            .route("/users/{pubkey}/enable", post(enable_user))
            .route("/users/disabled", get(list_disabled_users))
            .with_state(app_state);

        // Disable the tenant
        let server = axum_test::TestServer::new(router).unwrap();
        let pubkey_path = pubkey.z32();
        let response = server
            .post(format!("/users/{}/disable", pubkey_path).as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);

        // Check that the tenant is disabled
        let user = UserRepository::get(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();
        assert!(user.disabled);

        // Enable the tenant again
        let response = server
            .post(format!("/users/{}/enable", pubkey_path).as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);

        // Check that the tenant is enabled
        let user = UserRepository::get(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();
        assert!(!user.disabled);
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_list_disabled_users() {
        let context = AppContext::test().await;
        let user_a = Keypair::random().public_key();
        let user_b = Keypair::random().public_key();
        let user_c = Keypair::random().public_key();

        let mut user_a_entity = UserRepository::create(&user_a, &mut context.sql_db.pool().into())
            .await
            .unwrap();
        let mut user_b_entity = UserRepository::create(&user_b, &mut context.sql_db.pool().into())
            .await
            .unwrap();
        let _ = UserRepository::create(&user_c, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        user_a_entity.disabled = true;
        user_b_entity.disabled = true;
        UserRepository::update(&user_a_entity, &mut context.sql_db.pool().into())
            .await
            .unwrap();
        UserRepository::update(&user_b_entity, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let app_state = AppState::new(
            context.sql_db.clone(),
            FileService::new_from_context(&context).unwrap(),
            "",
        );
        let router = Router::new()
            .route("/users/disabled", get(list_disabled_users))
            .with_state(app_state);
        let server = axum_test::TestServer::new(router).unwrap();

        let response = server.get("/users/disabled?limit=1").await;
        assert_eq!(response.status_code(), StatusCode::OK);
        let body: ListDisabledUsersResponse = response.json();
        assert_eq!(body.items.len(), 1);
        assert!(body.next_cursor.is_some());

        let cursor = body.next_cursor.expect("limit=1 should produce cursor");
        let response = server
            .get(format!("/users/disabled?limit=10&cursor={cursor}").as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);
        let body: ListDisabledUsersResponse = response.json();
        assert_eq!(body.items.len(), 1);
        assert!(body.next_cursor.is_none());
        assert_ne!(body.items[0].pubkey, user_c.z32());
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_list_disabled_users_invalid_cursor() {
        let context = AppContext::test().await;
        let app_state = AppState::new(
            context.sql_db.clone(),
            FileService::new_from_context(&context).unwrap(),
            "",
        );
        let router = Router::new()
            .route("/users/disabled", get(list_disabled_users))
            .with_state(app_state);
        let server = axum_test::TestServer::new(router).unwrap();

        let response = server.get("/users/disabled?cursor=not-a-pubky").await;
        assert_eq!(response.status_code(), StatusCode::BAD_REQUEST);
    }
}

use super::super::app_state::AppState;
use crate::{
    persistence::{lmdb::tables::users::UserQueryError, sql::user::UserRepository},
    shared::{HttpError, HttpResult, Z32Pubkey},
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::IntoResponse,
};

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
    let mut user = match UserRepository::get(&pubkey.0, &mut (&mut tx).into()).await {
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
    UserRepository::update(&user, &mut (&mut tx).into()).await?;
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
    let mut user = match UserRepository::get(&pubkey.0, &mut (&mut tx).into()).await {
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
    UserRepository::update(&user, &mut (&mut tx).into()).await?;
    tx.commit().await?;

    Ok((StatusCode::OK, "Ok"))
}

#[cfg(test)]
mod tests {
    use super::super::super::app_state::AppState;
    use super::*;
    use crate::{persistence::files::FileService, AppContext};
    use axum::routing::post;
    use axum::Router;
    use pkarr::Keypair;

    #[tokio::test(flavor = "multi_thread")]
    async fn test_disable_enable_user() {
        let context = AppContext::test().await;
        let pubkey = Keypair::random().public_key();

        // Create new user
        UserRepository::create(&pubkey, &mut context.sql_db.pool().into()).await.unwrap();

        // Check that the tenant is enabled
        let user = UserRepository::get(&pubkey, &mut context.sql_db.pool().into()).await.unwrap();
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
            .with_state(app_state);

        // Disable the tenant
        let server = axum_test::TestServer::new(router).unwrap();
        let response = server
            .post(format!("/users/{}/disable", pubkey).as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);

        // Check that the tenant is disabled
        let user = UserRepository::get(&pubkey, &mut context.sql_db.pool().into()).await.unwrap();
        assert!(user.disabled);

        // Enable the tenant again
        let response = server
            .post(format!("/users/{}/enable", pubkey).as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);

        // Check that the tenant is enabled
        let user = UserRepository::get(&pubkey, &mut context.sql_db.pool().into()).await.unwrap();
        assert!(!user.disabled);
    }
}

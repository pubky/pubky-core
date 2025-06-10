use super::super::app_state::AppState;
use crate::{
    persistence::lmdb::tables::users::UserQueryError,
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
    let mut tx = state.db.env.write_txn()?;
    if let Err(e) = state.db.disable_user(&pubkey.0, &mut tx) {
        match e {
            UserQueryError::UserNotFound => {
                return Err(HttpError::new(
                    StatusCode::NOT_FOUND,
                    Some("User not found"),
                ))
            }
            UserQueryError::DatabaseError(_) => {
                return Err(HttpError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Some("Database error"),
                ))
            }
        };
    }
    tx.commit()?;
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
    let mut tx = state.db.env.write_txn()?;
    if let Err(e) = state.db.enable_user(&pubkey.0, &mut tx) {
        match e {
            UserQueryError::UserNotFound => {
                return Err(HttpError::new(
                    StatusCode::NOT_FOUND,
                    Some("User not found"),
                ))
            }
            UserQueryError::DatabaseError(_) => {
                return Err(HttpError::new(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Some("Database error"),
                ))
            }
        };
    }
    tx.commit()?;
    Ok((StatusCode::OK, "Ok"))
}

#[cfg(test)]
mod tests {
    use super::super::super::app_state::AppState;
    use super::*;
    use crate::persistence::files::FileService;
    use crate::persistence::lmdb::LmDB;
    use axum::routing::post;
    use axum::Router;
    use pkarr::Keypair;

    #[tokio::test]
    async fn test_disable_enable_user() {
        let pubkey = Keypair::random().public_key();

        // Create new user
        let db = LmDB::test();
        db.create_user(&pubkey).unwrap();

        // Check that the tenant is enabled
        let user = db
            .get_user(&pubkey, &mut db.env.read_txn().unwrap())
            .unwrap().unwrap();
        assert!(!user.disabled);

        // Setup server
        let app_state = AppState::new(db.clone(), FileService::test(db.clone()));
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
        let user = db
            .get_user(&pubkey, &mut db.env.read_txn().unwrap())
            .unwrap().unwrap();
        assert!(user.disabled);

        // Enable the tenant again
        let response = server
            .post(format!("/users/{}/enable", pubkey).as_str())
            .await;
        assert_eq!(response.status_code(), StatusCode::OK);

        // Check that the tenant is enabled
        let user = db
            .get_user(&pubkey, &mut db.env.read_txn().unwrap())
            .unwrap().unwrap();
        assert!(!user.disabled);
    }
}

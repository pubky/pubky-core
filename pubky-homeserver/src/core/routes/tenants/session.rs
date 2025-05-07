use axum::{extract::State, response::IntoResponse};

use crate::core::{
    error::Result,
    sessions::UserSession,
    AppState,
};

/// Get the information about the current session.
pub async fn get_session(UserSession { session, .. }: UserSession) -> Result<impl IntoResponse> {
    Ok(session.serialize())
}


pub async fn signout(
    State(mut state): State<AppState>,
    UserSession { id, .. }: UserSession,
) -> Result<impl IntoResponse> {
    // TODO: Set expired cookie to delete the cookie on client side.

    state.db.delete_session(&id)?;

    // Idempotent Success Response (200 OK)
    Ok(())
}

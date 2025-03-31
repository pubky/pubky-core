use std::net::SocketAddr;
use std::time::Duration;

use super::routes::{generate_signup_token, root};
use super::{app_state::AppState, auth_middleware::AdminAuthLayer};
use crate::app_context::AppContext;
use axum::{routing::get, Router};
use axum_server::Handle;

/// Folder /admin router
/// Admin password required.
fn create_admin_router(password: &str) -> Router<AppState> {
    Router::new()
        .route(
            "/generate_signup_token",
            get(generate_signup_token::generate_signup_token),
        )
        .layer(AdminAuthLayer::new(password.to_string()))
}

/// main / router
/// This part is not protected by the admin auth middleware
fn create_app(state: AppState, password: &str) -> axum::routing::IntoMakeService<Router> {
    let admin_router = create_admin_router(password);

    Router::new()
        .nest("/admin", admin_router)
        .route("/", get(root::root))
        .with_state(state)
        .into_make_service()
}

/// Admin server
///
/// This server is protected by the admin auth middleware.
///
/// When dropped, the server will stop.
pub struct AdminServer {
    handle: Handle,
    socket: SocketAddr,
}

impl AdminServer {
    pub async fn run(context: &AppContext) -> anyhow::Result<Self> {
        let state = AppState::new(context.db.clone());
        let socket = context.config_toml.admin.listen_socket;
        let app = create_app(state, context.config_toml.admin.admin_password.as_str());
        let listener = std::net::TcpListener::bind(socket)?;
        let http_handle = Handle::new();
        axum_server::from_tcp(listener)
            .handle(http_handle.clone())
            .serve(app)
            .await?;
        Ok(Self {
            handle: http_handle,
            socket,
        })
    }

    pub fn listen_socket(&self) -> SocketAddr {
        self.socket
    }
}

impl Drop for AdminServer {
    fn drop(&mut self) {
        self.handle.graceful_shutdown(Some(Duration::from_secs(5)));
    }
}


#[cfg(test)]
mod tests {
    use axum_test::TestServer;

    use crate::persistence::lmdb::LmDB;

    use super::*;

    #[tokio::test]
    async fn test_root() {
        let server = TestServer::new(create_app(AppState::new(LmDB::test()), "test")).unwrap();
        let response = server.get("/").expect_success().await;
        response.assert_status_ok();
    }

    #[tokio::test]
    async fn test_generate_signup_token_fail() {
        let server = TestServer::new(create_app(AppState::new(LmDB::test()), "test")).unwrap();
        // No password
        let response = server
            .get("/admin/generate_signup_token")
            .expect_failure()
            .await;
        response.assert_status_unauthorized();

        // wrong password
        let response = server
            .get("/admin/generate_signup_token")
            .add_header("X-Admin-Password", "wrongpassword")
            .expect_failure()
            .await;
        response.assert_status_unauthorized();
    }

    #[tokio::test]
    async fn test_generate_signup_token_success() {
        let server = TestServer::new(create_app(AppState::new(LmDB::test()), "test")).unwrap();
        let response = server
            .get("/admin/generate_signup_token")
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();
    }
}

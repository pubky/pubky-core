use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use super::routes::{
    delete_entry,
    disable_users::{disable_user, enable_user},
    generate_signup_token, info, root,
};
use super::trace::with_trace_layer;
use super::{app_state::AppState, auth_middleware::AdminAuthLayer};
use crate::app_context::AppContext;
use crate::{AppContextConversionError, MockDataDir, PersistentDataDir};
use axum::routing::{delete, post};
use axum::{routing::get, Router};
use axum_server::Handle;
use tokio::task::JoinHandle;
use tower_http::cors::CorsLayer;

/// Admin password protected router.
fn create_protected_router(password: &str) -> Router<AppState> {
    Router::new()
        .route(
            "/generate_signup_token",
            get(generate_signup_token::generate_signup_token),
        )
        .route("/info", get(info::info))
        .route("/webdav/{*entry_path}", delete(delete_entry::delete_entry))
        .route("/users/{pubkey}/disable", post(disable_user))
        .route("/users/{pubkey}/enable", post(enable_user))
        .layer(AdminAuthLayer::new(password.to_string()))
}

/// Public router without any authentication.
/// NO PASSWORD PROTECTION!
fn create_public_router() -> Router<AppState> {
    Router::new().route("/", get(root::root))
}

/// Create the app
fn create_app(state: AppState, password: &str) -> axum::routing::IntoMakeService<Router> {
    let admin_router = create_protected_router(password);
    let public_router = create_public_router();
    let app = Router::new()
        .merge(admin_router)
        .merge(public_router)
        .with_state(state)
        .layer(CorsLayer::very_permissive());

    with_trace_layer(app).into_make_service()
}

/// Errors that can occur when building a `AdminServer`.
#[derive(thiserror::Error, Debug)]
pub enum AdminServerBuildError {
    /// Failed to create admin server.
    #[error("Failed to create admin server: {0}")]
    Server(anyhow::Error),

    /// Failed to boostrap from the data directory.
    #[error("Failed to boostrap from the data directory: {0}")]
    DataDir(AppContextConversionError),
}

/// Admin server
///
/// This server is protected by the admin auth middleware.
///
/// When dropped, the server will stop.
pub struct AdminServer {
    http_handle: Handle,
    join_handle: JoinHandle<()>,
    socket: SocketAddr,
    password: String,
}

impl AdminServer {
    /// Create a new admin server from a data directory.
    pub async fn from_data_dir(data_dir: PersistentDataDir) -> Result<Self, AdminServerBuildError> {
        let context = AppContext::try_from(data_dir).map_err(AdminServerBuildError::DataDir)?;
        Self::start(&context).await
    }

    /// Create a new admin server from a data directory path.
    pub async fn from_data_dir_path(data_dir_path: PathBuf) -> Result<Self, AdminServerBuildError> {
        let data_dir = PersistentDataDir::new(data_dir_path);
        Self::from_data_dir(data_dir).await
    }

    /// Create a new admin server from a mock data directory.
    pub async fn from_mock_dir(mock_dir: MockDataDir) -> Result<Self, AdminServerBuildError> {
        let context = AppContext::try_from(mock_dir).map_err(AdminServerBuildError::DataDir)?;
        Self::start(&context).await
    }

    /// Run the admin server.
    pub async fn start(context: &AppContext) -> Result<Self, AdminServerBuildError> {
        let password = context.config_toml.admin.admin_password.clone();
        let state = AppState::new(context.db.clone());
        let socket = context.config_toml.admin.listen_socket;
        let app = create_app(state, password.as_str());
        let listener = std::net::TcpListener::bind(socket)
            .map_err(|e| AdminServerBuildError::Server(e.into()))?;
        let socket = listener
            .local_addr()
            .map_err(|e| AdminServerBuildError::Server(e.into()))?;
        let http_handle = Handle::new();
        let inner_http_handle = http_handle.clone();
        let join_handle = tokio::spawn(async move {
            axum_server::from_tcp(listener)
                .handle(inner_http_handle)
                .serve(app)
                .await
                .unwrap_or_else(|e| tracing::error!("Admin server error: {}", e));
        });
        Ok(Self {
            http_handle,
            socket,
            join_handle,
            password,
        })
    }

    /// Get the socket address of the admin server.
    pub fn listen_socket(&self) -> SocketAddr {
        self.socket
    }

    /// Create a signup token for the given homeserver.
    pub async fn create_signup_token(&self) -> anyhow::Result<String> {
        let admin_socket = self.listen_socket();
        let url = format!("http://{}/generate_signup_token", admin_socket);
        let response = reqwest::Client::new()
            .get(url)
            .header("X-Admin-Password", &self.password)
            .send()
            .await?;
        let response = response.error_for_status()?;
        let body = response.text().await?;
        Ok(body)
    }
}

impl Drop for AdminServer {
    fn drop(&mut self) {
        self.http_handle
            .graceful_shutdown(Some(Duration::from_secs(5)));
        self.join_handle.abort();
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
        let response = server.get("/generate_signup_token").expect_failure().await;
        response.assert_status_unauthorized();

        // wrong password
        let response = server
            .get("/generate_signup_token")
            .add_header("X-Admin-Password", "wrongpassword")
            .expect_failure()
            .await;
        response.assert_status_unauthorized();
    }

    #[tokio::test]
    async fn test_generate_signup_token_success() {
        let server = TestServer::new(create_app(AppState::new(LmDB::test()), "test")).unwrap();
        let response = server
            .get("/generate_signup_token")
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();
    }
}

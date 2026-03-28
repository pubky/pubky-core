use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use super::routes::{
    dav_handler, delete_entry,
    disable_users::{disable_user, enable_user},
    generate_signup_token, info, root,
};
use super::trace::with_trace_layer;
use super::{app_state::AppState, auth_middleware::AdminAuthLayer};
use crate::AppContext;
#[cfg(any(test, feature = "testing"))]
use crate::MockSetupSource;
use crate::{AppContextConversionError, HomeserverPaths};
use axum::routing::{any, delete, post};
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
    Router::new().route("/", get(root::handler))
}

/// Create the app
fn create_app(state: AppState, password: &str) -> axum::routing::IntoMakeService<Router> {
    let admin_router = create_protected_router(password);
    let public_router = create_public_router();
    let app = Router::new()
        .merge(admin_router)
        .merge(public_router)
        .route("/dav{*path}", any(dav_handler::dav_handler))
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

    /// Failed to bootstrap from the setup source.
    #[error("Failed to bootstrap from the setup source: {0}")]
    SetupSource(AppContextConversionError),
}

/// Admin server
///
/// This server is protected by the admin auth middleware.
///
/// When dropped, the server will stop.
pub struct AdminServer {
    http_handle: Handle<SocketAddr>,
    join_handle: JoinHandle<()>,
    socket: SocketAddr,
    password: String,
}

impl AdminServer {
    /// Create a new admin server from homeserver paths.
    pub async fn from_homeserver_paths(
        paths: HomeserverPaths,
    ) -> Result<Self, AdminServerBuildError> {
        let context = AppContext::read_from(paths)
            .await
            .map_err(AdminServerBuildError::SetupSource)?;
        Self::start(&context).await
    }

    /// Create a new admin server from a setup path.
    pub async fn from_setup_path(setup_path: PathBuf) -> Result<Self, AdminServerBuildError> {
        let paths = HomeserverPaths::new(setup_path);
        Self::from_homeserver_paths(paths).await
    }

    /// Create a new admin server from a mock setup source.
    #[cfg(any(test, feature = "testing"))]
    pub async fn from_mock_setup_source(
        setup_source: MockSetupSource,
    ) -> Result<Self, AdminServerBuildError> {
        let context = AppContext::read_from(setup_source)
            .await
            .map_err(AdminServerBuildError::SetupSource)?;
        Self::start(&context).await
    }

    /// Run the admin server.
    pub async fn start(context: &AppContext) -> Result<Self, AdminServerBuildError> {
        let password = context.config_toml.admin.admin_password.clone();
        let state = AppState::new(
            context.sql_db.clone(),
            context.file_service.clone(),
            &password,
        )
        .with_metadata_from_config(
            context.keypair.public_key().z32(),
            &context.config_toml,
            env!("CARGO_PKG_VERSION"),
        );
        let socket = context.config_toml.admin.listen_socket;
        let app = create_app(state, password.as_str());
        let listener = std::net::TcpListener::bind(socket)
            .map_err(|e| AdminServerBuildError::Server(e.into()))?;
        listener
            .set_nonblocking(true)
            .map_err(|e| AdminServerBuildError::Server(e.into()))?;
        let socket = listener
            .local_addr()
            .map_err(|e| AdminServerBuildError::Server(e.into()))?;
        let http_handle = Handle::new();
        let inner_http_handle = http_handle.clone();
        let server =
            axum_server::from_tcp(listener).map_err(|e| AdminServerBuildError::Server(e.into()))?;
        let join_handle = tokio::spawn(async move {
            server
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
    use axum::http::Method;
    use axum_test::TestServer;
    use base64::Engine;

    use crate::persistence::files::FileService;

    use super::*;

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
    async fn test_root() {
        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let response = server.get("/").expect_success().await;
        response.assert_status_ok();
    }

    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_generate_signup_token_fail() {
        let context = AppContext::test().await;
        let server = create_test_server(&context);
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
    #[pubky_test_utils::test]
    async fn test_generate_signup_token_success() {
        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let response = server
            .get("/generate_signup_token")
            .add_header("X-Admin-Password", "test")
            .expect_success()
            .await;
        response.assert_status_ok();
    }

    fn auth_header() -> String {
        // AppState is created with password "" in create_test_server
        let auth = base64::engine::general_purpose::STANDARD.encode("admin:");
        format!("Basic {auth}")
    }

    /// PROPFIND and GET on /dav/ root should succeed.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_dav_root_propfind_and_get() {
        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let auth_value = auth_header();

        let propfind = Method::from_bytes(b"PROPFIND").unwrap();
        let response = server
            .method(propfind, "/dav/")
            .add_header("Authorization", auth_value.as_str())
            .add_header("Depth", "1")
            .expect_success()
            .await;
        // WebDAV PROPFIND returns 207 Multi-Status on success
        response.assert_status(axum::http::StatusCode::MULTI_STATUS);

        let response = server
            .get("/dav/")
            .add_header("Authorization", auth_value.as_str())
            .expect_success()
            .await;
        response.assert_status_ok();
    }

    /// PUT a file via WebDAV, GET it back, then DELETE it.
    #[tokio::test]
    #[pubky_test_utils::test]
    async fn test_dav_put_get_delete_file() {
        use crate::persistence::sql::user::UserRepository;
        use pubky_common::crypto::Keypair;

        let context = AppContext::test().await;
        let server = create_test_server(&context);
        let auth_value = auth_header();

        // Register a user so writes are accepted by the entry layer
        let keypair = Keypair::from_secret(&[0; 32]);
        let pubkey = keypair.public_key();
        UserRepository::create(&pubkey, &mut context.sql_db.pool().into())
            .await
            .unwrap();

        let file_content = b"hello webdav";
        let file_url = format!("/dav/{}/pub/test.txt", pubkey.z32());

        // PUT a file
        let response = server
            .put(&file_url)
            .add_header("Authorization", auth_value.as_str())
            .bytes(file_content.to_vec().into())
            .expect_success()
            .await;
        response.assert_status(axum::http::StatusCode::CREATED);

        // GET it back
        let response = server
            .get(&file_url)
            .add_header("Authorization", auth_value.as_str())
            .expect_success()
            .await;
        response.assert_status_ok();
        assert_eq!(response.as_bytes().as_ref(), file_content);

        // PROPFIND on the user's pub directory should list the file
        let propfind = Method::from_bytes(b"PROPFIND").unwrap();
        let dir_url = format!("/dav/{}/pub/", pubkey.z32());
        let response = server
            .method(propfind, &dir_url)
            .add_header("Authorization", auth_value.as_str())
            .add_header("Depth", "1")
            .expect_success()
            .await;
        response.assert_status(axum::http::StatusCode::MULTI_STATUS);
        let body = response.text();
        assert!(body.contains("test.txt"), "PROPFIND should list the file");

        // DELETE the file
        let response = server
            .delete(&file_url)
            .add_header("Authorization", auth_value.as_str())
            .expect_success()
            .await;
        response.assert_status(axum::http::StatusCode::NO_CONTENT);

        // GET should now return 404
        let response = server
            .get(&file_url)
            .add_header("Authorization", auth_value.as_str())
            .expect_failure()
            .await;
        response.assert_status(axum::http::StatusCode::NOT_FOUND);
    }
}

use axum::{
    routing::get,
    Router,
};
use crate::context::AppContext;
use super::{auth_middleware::AdminAuthLayer, app_state::AppState};
use super::routes::{generate_signup_token, root};


/// Folder /admin router
/// Admin password required.
fn create_admin_router(password: &str) -> Router<AppState> {
    let router = Router::new()
    .route("/generate_signup_token", get(generate_signup_token::generate_signup_token))
    .layer(AdminAuthLayer::new(password.to_string()));
    router
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

/// Run the admin server
/// 
/// # Arguments
/// 
/// * `db` - The database to use
/// * `password` - The password to protect the admin routes
/// * `listen` - The address to listen on
pub async fn run_admin_server(context: &AppContext) -> anyhow::Result<()> {
    let state = AppState::new(context.db.clone());
    let app = create_app(state, &context.config_toml.admin.admin_password.as_str());
    let listener = tokio::net::TcpListener::bind(context.config_toml.admin.listen_socket).await?;
    tracing::debug!("Admin server listening on {}", context.config_toml.admin.listen_socket);
    axum::serve(listener, app).await?;
    Ok(())
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
        let response = server.get("/admin/generate_signup_token").expect_failure().await;
        response.assert_status_unauthorized();

        // wrong password
        let response = server.get("/admin/generate_signup_token").add_header("X-Admin-Password", "wrongpassword").expect_failure().await;
        response.assert_status_unauthorized();
    }

    #[tokio::test]
    async fn test_generate_signup_token_success() {
        let server = TestServer::new(create_app(AppState::new(LmDB::test()), "test")).unwrap();
        let response = server.get("/admin/generate_signup_token").add_header("X-Admin-Password", "test").expect_success().await;
        response.assert_status_ok();
    }
}

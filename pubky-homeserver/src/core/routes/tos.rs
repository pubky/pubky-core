use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};

use crate::{
    core::AppState,
    shared::{HttpError, HttpResult},
};

pub async fn handler(State(state): State<AppState>) -> HttpResult<impl IntoResponse> {
    if let Some(tos_path) = &state.enforce_tos_with {
        let tos_content = tokio::fs::read_to_string(tos_path).await?;

        Ok(Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/markdown; charset=utf-8")
            .body(Body::from(tos_content))?)
    } else {
        Err(HttpError::not_found())
    }
}

#[cfg(test)]
mod tests {
    use axum_test::TestServer;

    use crate::{
        app_context::AppContext, core::HomeserverCore, data_directory::MockDataDir, ConfigToml,
    };
    #[tokio::test]
    async fn tos_endpoint_returns_content() {
        // 1. Test that the endpoint returns 404 when ToS is not configured
        let config_disabled = ConfigToml::test();
        let data_dir_disabled = MockDataDir::new(config_disabled, None).unwrap();
        let context_disabled = AppContext::try_from(data_dir_disabled).unwrap();
        let router_disabled = HomeserverCore::create_router(&context_disabled);
        let server_disabled = TestServer::new(router_disabled).unwrap();
        server_disabled
            .get("/tos")
            .await
            .assert_status(axum::http::StatusCode::NOT_FOUND);

        // 2. Test that it serves the correct file with the correct content type
        let tos_file = tempfile::Builder::new().suffix(".md").tempfile().unwrap();
        let tos_content = "# My Custom ToS";
        std::fs::write(tos_file.path(), tos_content).unwrap();

        let mut config = ConfigToml::test();
        config.general.enforce_tos_with = tos_file.path().to_string_lossy().to_string();
        let data_dir = MockDataDir::new(config, None).unwrap();
        let context = AppContext::try_from(data_dir).unwrap();
        let router = HomeserverCore::create_router(&context);
        let server = axum_test::TestServer::new(router.clone()).unwrap();

        let response = server.get("/tos").expect_success().await;

        response.assert_status_ok();
        response.assert_header("content-type", "text/markdown; charset=utf-8");
        let body = response.text();
        assert_eq!(body, tos_content)
    }
}

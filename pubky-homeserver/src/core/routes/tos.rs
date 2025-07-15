use axum::{
    body::Body,
    extract::State,
    http::{header, Response, StatusCode},
    response::IntoResponse,
};

use crate::{core::AppState, shared::HttpResult};

pub async fn handler(State(state): State<AppState>) -> HttpResult<impl IntoResponse> {
    let tos_path = state.data_dir.path().join("tos.html");
    let tos_content = tokio::fs::read_to_string(tos_path).await?;

    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(Body::from(tos_content))
        .unwrap())
}

#[cfg(test)]
mod tests {
    use crate::{
        app_context::AppContext, core::HomeserverCore, data_directory::MockDataDir, ConfigToml,
    };
    #[tokio::test]
    async fn tos_endpoint_returns_content() {
        let mut config = ConfigToml::test();
        config.general.enforce_tos = true; // Not strictly needed, but good for testing file creation.
        let data_dir = MockDataDir::new(config, None).unwrap();
        let context = AppContext::try_from(data_dir).unwrap();
        let router = HomeserverCore::create_router(&context);
        let server = axum_test::TestServer::new(router.clone()).unwrap();

        // Check that default file was created.
        let tos_path = context.data_dir.path().join("tos.html");
        assert!(tos_path.exists());

        let response = server.get("/tos").expect_success().await;

        response.assert_status_ok();
        response.assert_header("content-type", "text/html; charset=utf-8");
        let body = response.text();
        assert!(body.contains("Terms of Service Not Yet Defined"));

        // Now, let's test with a custom ToS file
        let custom_tos = "<h1>My Custom ToS</h1>";
        std::fs::write(&tos_path, custom_tos).unwrap();

        let response2 = server.get("/tos").expect_success().await;
        response2.assert_status_ok();
        let body2 = response2.text();
        assert_eq!(body2, custom_tos);
    }
}

use crate::core::AppState;
use crate::shared::HttpResult;
use axum::{extract::State, response::IntoResponse, Json};
use serde::Serialize;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct RootResponse {
    description: &'static str,
    version: &'static str,
    public_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tos_pubky_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tos_icann_url: Option<String>,
}

pub async fn handler(State(state): State<AppState>) -> HttpResult<impl IntoResponse> {
    let (tos_pubky_url, tos_icann_url) = if state.enforce_tos_with.is_some() {
        let pubky_url = format!("https://{}/tos", state.server_public_key);
        let icann_url = state
            .icann_domain
            .as_ref()
            .map(|domain| format!("https://{}/tos", domain));
        (Some(pubky_url), icann_url)
    } else {
        (None, None)
    };

    let response = RootResponse {
        description: env!("CARGO_PKG_DESCRIPTION"),
        version: env!("CARGO_PKG_VERSION"),
        public_key: state.server_public_key.to_string(),
        tos_pubky_url,
        tos_icann_url,
    };

    Ok(Json(response))
}

#[cfg(test)]
mod tests {
    use crate::{app_context::AppContext, core::HomeserverCore, ConfigToml, MockDataDir};
    use axum_test::TestServer;

    #[tokio::test]
    async fn test_root_endpoint() {
        let context = AppContext::test();
        let router = HomeserverCore::create_router(&context);
        let server = TestServer::new(router).unwrap();

        let response = server.get("/").expect_success().await;
        let json = response.json::<serde_json::Value>();

        assert_eq!(json["version"], env!("CARGO_PKG_VERSION"));
        assert_eq!(json["description"], env!("CARGO_PKG_DESCRIPTION"));
        assert_eq!(json["publicKey"], context.keypair.public_key().to_string());
        assert!(json["tosPubkyUrl"].is_null());
        assert!(json["tosIcannUrl"].is_null());
    }

    #[tokio::test]
    async fn test_root_endpoint_with_tos() {
        // 1. Create a temporary ToS file
        let tos_file = tempfile::Builder::new().suffix(".md").tempfile().unwrap();
        std::fs::write(tos_file.path(), "# Terms").unwrap();

        // 2. Create a config that enforces ToS
        let config_str = format!(
            r#"[general]
            enforce_tos_with = "{}"

            [pkdns]
            icann_domain = "example.com"
            "#,
            tos_file.path().display()
        );
        let config = ConfigToml::from_str_with_defaults(&config_str).unwrap();
        let data_dir = MockDataDir::new(config, None).unwrap();
        let context = AppContext::try_from(data_dir).unwrap();
        let router = HomeserverCore::create_router(&context);
        let server = TestServer::new(router).unwrap();

        // 3. Make the request and get the JSON response
        let response = server.get("/").expect_success().await;
        let json = response.json::<serde_json::Value>();

        let server_pubkey = context.keypair.public_key();

        // 4. Assert the response contains the correct URLs
        assert_eq!(json["publicKey"], server_pubkey.to_string());

        assert_eq!(
            json["tosPubkyUrl"],
            format!("https://{}/tos", server_pubkey)
        );
        assert_eq!(json["tosIcannUrl"], "https://example.com/tos");
    }
}

use pubky_testnet::pubky::{Keypair, Method, PubkyHttpClient, StatusCode};
use pubky_testnet::{
    pubky_homeserver::{ConfigToml, Domain, MockDataDir},
    Testnet,
};
use serde::Deserialize;
use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;

#[derive(Deserialize)]
struct InfoResponse {
    public_key: String,
    pkarr_pubky_address: Option<String>,
    pkarr_icann_domain: Option<String>,
    version: String,
}

#[tokio::test]
#[pubky_testnet::test]
async fn admin_info_includes_metadata() {
    let mut config = ConfigToml::default_test_config();
    config.pkdns.public_ip = IpAddr::V4(Ipv4Addr::LOCALHOST);
    config.pkdns.public_pubky_tls_port = Some(9443);
    config.pkdns.icann_domain = Some(Domain::from_str("example.test").unwrap());
    config.pkdns.public_icann_http_port = Some(8081);

    let expected_pubky_endpoint = format!(
        "{}:{}",
        config.pkdns.public_ip,
        config
            .pkdns
            .public_pubky_tls_port
            .expect("test should set pubky port"),
    );
    let expected_icann_endpoint = format!(
        "{}:{}",
        config
            .pkdns
            .icann_domain
            .as_ref()
            .expect("test should set icann domain"),
        config
            .pkdns
            .public_icann_http_port
            .expect("test should set icann port"),
    );
    let admin_password = config.admin.admin_password.clone();

    let mock_dir = MockDataDir::new(config, Some(Keypair::random())).unwrap();

    let mut testnet = Testnet::new().await.unwrap();
    let homeserver = testnet
        .create_homeserver_app_with_mock(mock_dir)
        .await
        .unwrap();

    let admin_socket = homeserver
        .admin_server()
        .expect("admin server should be enabled")
        .listen_socket();

    let response = PubkyHttpClient::new()
        .unwrap()
        .request(Method::GET, &format!("http://{admin_socket}/info"))
        .header("X-Admin-Password", admin_password)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let body: InfoResponse = response.json().await.unwrap();
    assert_eq!(body.public_key, homeserver.public_key().z32());
    assert_eq!(body.pkarr_pubky_address, Some(expected_pubky_endpoint));
    assert_eq!(body.pkarr_icann_domain, Some(expected_icann_endpoint));
    assert_eq!(body.version, env!("CARGO_PKG_VERSION"));
}

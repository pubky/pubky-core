use pubky_testnet::pubky::{
    errors::RequestError, Error, Keypair, Method, PubkyHttpClient, StatusCode,
};
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

/// Test that per-user quota set via admin API is enforced.
/// User A gets a 1 MB custom quota via admin API; user B has no custom quota (unlimited).
/// User A is blocked when exceeding 1 MB; user B can write freely.
#[tokio::test]
#[pubky_testnet::test]
async fn per_user_quota_via_admin_api() {
    let config = ConfigToml::default_test_config();
    let admin_password = config.admin.admin_password.clone();

    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();
    let mock_dir = MockDataDir::new(config, Some(Keypair::random())).unwrap();
    let server = testnet
        .create_homeserver_app_with_mock(mock_dir)
        .await
        .unwrap();

    let admin_socket = server
        .admin_server()
        .expect("admin server should be enabled")
        .listen_socket();

    // Create two users
    let signer_a = pubky.signer(Keypair::random());
    let session_a = signer_a.signup(&server.public_key(), None).await.unwrap();
    let pubkey_a = signer_a.public_key().z32();

    let signer_b = pubky.signer(Keypair::random());
    let session_b = signer_b.signup(&server.public_key(), None).await.unwrap();

    // Set a 1 MB quota on user A via admin API
    let admin_client = PubkyHttpClient::new().unwrap();
    let resp = admin_client
        .request(
            Method::PATCH,
            &format!("http://{admin_socket}/users/{pubkey_a}/quota"),
        )
        .header("X-Admin-Password", &admin_password)
        .header("content-type", "application/json")
        .body(r#"{"storage_quota_mb": 1, "rate_read": null, "rate_write": null}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // User A: 600 KB write → OK
    let data_600k: Vec<u8> = vec![0; 600_000];
    let resp = session_a
        .storage()
        .put("/pub/data", data_600k.clone())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // User A: another 600 KB at a different path (total 1.2 MB) → 507
    let err = session_a
        .storage()
        .put("/pub/data2", data_600k.clone())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::Request(RequestError::Server { status, .. })
            if status == StatusCode::INSUFFICIENT_STORAGE
    ));

    // User B: has no custom quota (unlimited) — same 600 KB writes should both succeed
    let resp = session_b
        .storage()
        .put("/pub/data", data_600k.clone())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    let resp = session_b
        .storage()
        .put("/pub/data2", data_600k)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

/// Test that per-user write speed override set via admin API throttles uploads.
///
/// User A is overridden to 1kb/s write speed; user B keeps the server default.
/// A 3 KB upload at 1kb/s should take >2s, while user B uploads quickly.
#[tokio::test]
#[pubky_testnet::test]
async fn per_user_speed_override_throttles_via_admin_api() {
    use std::time::{Duration, Instant};

    let mut config = ConfigToml::default_test_config();
    // Server-level default write speed — user A will be overridden below.
    config.default_quotas.rate_write = Some("1mb/s".parse().unwrap());
    let admin_password = config.admin.admin_password.clone();

    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();
    let mock_dir = MockDataDir::new(config, Some(Keypair::random())).unwrap();
    let server = testnet
        .create_homeserver_app_with_mock(mock_dir)
        .await
        .unwrap();

    let admin_socket = server
        .admin_server()
        .expect("admin server should be enabled")
        .listen_socket();

    // Create two users
    let signer_a = pubky.signer(Keypair::random());
    let session_a = signer_a.signup(&server.public_key(), None).await.unwrap();
    let pubkey_a_z32 = signer_a.public_key().z32();

    let signer_b = pubky.signer(Keypair::random());
    let session_b = signer_b.signup(&server.public_key(), None).await.unwrap();

    // Override user A to 1kb/s write speed via admin API
    let admin_client = PubkyHttpClient::new().unwrap();
    let resp = admin_client
        .request(
            Method::PATCH,
            &format!("http://{admin_socket}/users/{pubkey_a_z32}/quota"),
        )
        .header("X-Admin-Password", &admin_password)
        .header("content-type", "application/json")
        .body(r#"{"storage_quota_mb": null, "rate_read": null, "rate_write": "1kb/s"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    let body = vec![0u8; 3 * 1024]; // 3 KB

    // User A (1kb/s override): 3 KB upload should take >2s due to throttling
    let start = Instant::now();
    let resp = session_a
        .storage()
        .put("/pub/rate_test", body.clone())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let elapsed_a = start.elapsed();
    assert!(
        elapsed_a > Duration::from_secs(2),
        "User A upload should be throttled to ~1kb/s (elapsed {:?})",
        elapsed_a
    );

    // User B (no override, uses default 1mb/s): same upload should be fast (<2s)
    let start = Instant::now();
    let resp = session_b
        .storage()
        .put("/pub/rate_test", body)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    let elapsed_b = start.elapsed();
    assert!(
        elapsed_b < Duration::from_secs(2),
        "User B upload should use default speed, not be throttled (elapsed {:?})",
        elapsed_b
    );
}

/// Test that per-user read speed override set via admin API throttles downloads.
///
/// User A is overridden to 1kb/s read speed; user B keeps the server default.
/// A 3 KB download at 1kb/s should take >2s, while user B downloads quickly.
#[tokio::test]
#[pubky_testnet::test]
async fn per_user_read_speed_override_throttles_via_admin_api() {
    use std::time::{Duration, Instant};

    let mut config = ConfigToml::default_test_config();
    // Server-level defaults — user A's read rate will be overridden below.
    config.default_quotas.rate_read = Some("1mb/s".parse().unwrap());
    config.default_quotas.rate_write = Some("1mb/s".parse().unwrap());
    let admin_password = config.admin.admin_password.clone();

    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();
    let mock_dir = MockDataDir::new(config, Some(Keypair::random())).unwrap();
    let server = testnet
        .create_homeserver_app_with_mock(mock_dir)
        .await
        .unwrap();

    let admin_socket = server
        .admin_server()
        .expect("admin server should be enabled")
        .listen_socket();

    // Create two users
    let signer_a = pubky.signer(Keypair::random());
    let session_a = signer_a.signup(&server.public_key(), None).await.unwrap();
    let pubkey_a_z32 = signer_a.public_key().z32();

    let signer_b = pubky.signer(Keypair::random());
    let session_b = signer_b.signup(&server.public_key(), None).await.unwrap();

    // Both users upload a 3 KB file (fast, using default 1mb/s)
    let body = vec![0u8; 3 * 1024];
    session_a
        .storage()
        .put("/pub/read_test.txt", body.clone())
        .await
        .unwrap();
    session_b
        .storage()
        .put("/pub/read_test.txt", body)
        .await
        .unwrap();

    // Override user A to 1kb/s read speed via admin API
    let admin_client = PubkyHttpClient::new().unwrap();
    let resp = admin_client
        .request(
            Method::PATCH,
            &format!("http://{admin_socket}/users/{pubkey_a_z32}/quota"),
        )
        .header("X-Admin-Password", &admin_password)
        .header("content-type", "application/json")
        .body(r#"{"rate_read": "1kb/s"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // User A (1kb/s read override): 3 KB download should take >2s
    let start = Instant::now();
    let resp = session_a.storage().get("/pub/read_test.txt").await.unwrap();
    let _ = resp.bytes().await.unwrap(); // consume body to apply throttle
    let elapsed_a = start.elapsed();
    assert!(
        elapsed_a > Duration::from_secs(2),
        "User A download should be throttled to ~1kb/s (elapsed {:?})",
        elapsed_a
    );

    // User B (no override, uses default 1mb/s): same download should be fast (<2s)
    let start = Instant::now();
    let resp = session_b.storage().get("/pub/read_test.txt").await.unwrap();
    let _ = resp.bytes().await.unwrap();
    let elapsed_b = start.elapsed();
    assert!(
        elapsed_b < Duration::from_secs(2),
        "User B download should use default speed, not be throttled (elapsed {:?})",
        elapsed_b
    );
}

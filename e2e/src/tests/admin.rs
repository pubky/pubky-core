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
            Method::PUT,
            &format!("http://{admin_socket}/users/{pubkey_a}/resource-quotas"),
        )
        .header("X-Admin-Password", &admin_password)
        .header("content-type", "application/json")
        .body(r#"{"storage_quota_mb": 1, "max_sessions": null, "rate_read": null, "rate_write": null}"#)
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

/// Test that per-user bandwidth rate limits set via admin API return 429.
/// User A gets a tight write budget (1kb/s); after exhausting it, further writes get 429.
#[tokio::test]
#[pubky_testnet::test]
async fn per_user_rate_limit_429_via_admin_api() {
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

    // Create a user
    let signer = pubky.signer(Keypair::random());
    let session = signer.signup(&server.public_key(), None).await.unwrap();
    let pubkey_z32 = signer.public_key().z32();

    // Set a tight write budget (1kb/s) via admin API
    let admin_client = PubkyHttpClient::new().unwrap();
    let resp = admin_client
        .request(
            Method::PUT,
            &format!("http://{admin_socket}/users/{pubkey_z32}/resource-quotas"),
        )
        .header("X-Admin-Password", &admin_password)
        .header("content-type", "application/json")
        .body(r#"{"storage_quota_mb": null, "max_sessions": null, "rate_read": null, "rate_write": "1kb/s"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // First write: 1024 bytes uses up the full 1kb/s budget
    let resp = session
        .storage()
        .put("/pub/rate_test1", vec![0u8; 1024])
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Second write should be rejected with 429
    let err = session
        .storage()
        .put("/pub/rate_test2", vec![0u8; 512])
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Request(RequestError::Server { status, .. })
                if status == StatusCode::TOO_MANY_REQUESTS
        ),
        "Expected 429 TOO_MANY_REQUESTS, got: {err:?}"
    );
}

/// Test that per-user write bandwidth budget set via admin API is enforced.
///
/// The write budget counts Content-Length bytes with a minimum of 256 bytes per
/// request (MIN_WRITE_COST_BYTES). The first request that crosses the boundary
/// is allowed through (soft limit), but subsequent requests are rejected with 429.
#[tokio::test]
#[pubky_testnet::test]
async fn write_bandwidth_budget_enforced_via_admin_api() {
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

    // Create a user
    let signer = pubky.signer(Keypair::random());
    let session = signer.signup(&server.public_key(), None).await.unwrap();
    let pubkey_z32 = signer.public_key().z32();

    // Set a tight write bandwidth budget (1kb/m = 1024 bytes per minute) via admin API
    let admin_client = PubkyHttpClient::new().unwrap();
    let resp = admin_client
        .request(
            Method::PUT,
            &format!("http://{admin_socket}/users/{pubkey_z32}/resource-quotas"),
        )
        .header("X-Admin-Password", &admin_password)
        .header("content-type", "application/json")
        .body(r#"{"storage_quota_mb": null, "max_sessions": null, "rate_read": null, "rate_write": "1kb/m"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // First write: 512 bytes fits within 1024-byte budget — should succeed
    let resp = session
        .storage()
        .put("/pub/test1.txt", vec![0u8; 512])
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Second write: 1024 bytes pushes over budget (512 already used + 1024 = 1536 > 1024).
    // The request that *crosses* the boundary is allowed through (soft limit).
    let resp = session
        .storage()
        .put("/pub/test2.txt", vec![0u8; 1024])
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Third write: budget is now exhausted (previous >= budget_bytes) — rejected with 429
    let err = session
        .storage()
        .put("/pub/test3.txt", vec![0u8; 256])
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Request(RequestError::Server { status, .. })
                if status == StatusCode::TOO_MANY_REQUESTS
        ),
        "Expected 429 TOO_MANY_REQUESTS, got: {err:?}"
    );
}

/// Test that per-user max_sessions limit is enforced during signin.
/// Set max_sessions=2 via admin API, create 2 sessions, verify the 3rd signin
/// is rejected with 429, then sign out one session and verify a new signin succeeds.
#[tokio::test]
#[pubky_testnet::test]
async fn max_sessions_enforced_via_admin_api() {
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

    // Signup creates session #1
    let signer = pubky.signer(Keypair::random());
    let session1 = signer.signup(&server.public_key(), None).await.unwrap();
    let pubkey_z32 = signer.public_key().z32();

    // Set max_sessions=2 via admin API
    let admin_client = PubkyHttpClient::new().unwrap();
    let resp = admin_client
        .request(
            Method::PUT,
            &format!("http://{admin_socket}/users/{pubkey_z32}/resource-quotas"),
        )
        .header("X-Admin-Password", &admin_password)
        .header("content-type", "application/json")
        .body(r#"{"storage_quota_mb": null, "max_sessions": 2, "rate_read": null, "rate_write": null}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Signin creates session #2 — should succeed
    let session2 = signer.signin().await.unwrap();

    // Signin for session #3 — should be rejected (429)
    let err = signer
        .signin()
        .await
        .expect_err("Third signin should be rejected — max_sessions=2");
    assert!(
        matches!(
            err,
            Error::Request(RequestError::Server { status, .. })
                if status == StatusCode::TOO_MANY_REQUESTS
        ),
        "Expected 429 TOO_MANY_REQUESTS, got: {err:?}"
    );

    // Sign out one session to free a slot
    session1.signout().await.unwrap();

    // Now a new signin should succeed
    let _session3 = signer
        .signin()
        .await
        .expect("Signin should succeed after signing out one session");

    // And the next one should be rejected again
    let err = signer
        .signin()
        .await
        .expect_err("Should be rejected — back at max_sessions=2");
    assert!(
        matches!(
            err,
            Error::Request(RequestError::Server { status, .. })
                if status == StatusCode::TOO_MANY_REQUESTS
        ),
        "Expected 429 TOO_MANY_REQUESTS, got: {err:?}"
    );

    // Clean up: the remaining sessions can still be used
    session2.signout().await.unwrap();
}

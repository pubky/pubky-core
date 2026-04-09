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

/// Test that per-user write speed override set via admin API throttles uploads.
///
/// We configure a base PUT /pub/** speed limit of 1mb/s (user-keyed), then
/// override a specific user to 1kb/s via admin API. A 3 KB upload at 1kb/s
/// should take >2s (same pattern as `test_limit_upload` in rate_limiting.rs).
#[tokio::test]
#[pubky_testnet::test]
async fn per_user_speed_override_throttles_via_admin_api() {
    use pubky_testnet::pubky_homeserver::quota_config::{GlobPattern, LimitKeyType, PathLimit};
    use std::time::{Duration, Instant};

    let mut config = ConfigToml::default_test_config();
    // Add a base user-keyed speed limit for PUT /pub/** — required for per-user overrides.
    config.drive.rate_limits.push(PathLimit::new(
        GlobPattern::new("/pub/**"),
        Method::PUT,
        "1mb/s".parse().unwrap(),
        LimitKeyType::User,
        None,
    ));
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
        .body(r#"{"storage_quota_mb": null, "max_sessions": null, "rate_read": null, "rate_write": "1kb/s"}"#)
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
            Method::PATCH,
            &format!("http://{admin_socket}/users/{pubkey_z32}/quota"),
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

/// Test that `max_sessions = Default` (no override) allows unlimited sessions.
/// Create many sessions without setting a limit — all should succeed.
#[tokio::test]
#[pubky_testnet::test]
async fn max_sessions_default_allows_unlimited() {
    let config = ConfigToml::default_test_config();

    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();
    let mock_dir = MockDataDir::new(config, Some(Keypair::random())).unwrap();
    let server = testnet
        .create_homeserver_app_with_mock(mock_dir)
        .await
        .unwrap();

    // Signup creates session #1 — no max_sessions set (Default)
    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();

    // Create several more sessions — all should succeed
    for i in 2..=5 {
        signer.signin().await.unwrap_or_else(|e| {
            panic!("Session #{i} should succeed with Default max_sessions: {e:?}")
        });
    }
}

/// Test that `max_sessions = "unlimited"` allows unlimited sessions even after
/// being explicitly set via admin API (distinct from Default).
#[tokio::test]
#[pubky_testnet::test]
async fn max_sessions_unlimited_allows_unlimited() {
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
    signer.signup(&server.public_key(), None).await.unwrap();
    let pubkey_z32 = signer.public_key().z32();

    // Explicitly set max_sessions to "unlimited" via admin API
    let admin_client = PubkyHttpClient::new().unwrap();
    let resp = admin_client
        .request(
            Method::PATCH,
            &format!("http://{admin_socket}/users/{pubkey_z32}/quota"),
        )
        .header("X-Admin-Password", &admin_password)
        .header("content-type", "application/json")
        .body(r#"{"max_sessions": "unlimited"}"#)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // Create several sessions — all should succeed
    for i in 2..=5 {
        signer.signin().await.unwrap_or_else(|e| {
            panic!("Session #{i} should succeed with Unlimited max_sessions: {e:?}")
        });
    }
}

/// Test that per-user read speed override set via admin API throttles downloads.
///
/// We configure a base GET /pub/** speed limit of 1mb/s (user-keyed), then
/// override a specific user to 1kb/s via admin API. A 3 KB download at 1kb/s
/// should take >2s, while a user without override downloads quickly.
#[tokio::test]
#[pubky_testnet::test]
async fn per_user_read_speed_override_throttles_via_admin_api() {
    use pubky_testnet::pubky_homeserver::quota_config::{GlobPattern, LimitKeyType, PathLimit};
    use std::time::{Duration, Instant};

    let mut config = ConfigToml::default_test_config();
    // Base user-keyed speed limit for GET /pub/** — required for per-user overrides.
    config.drive.rate_limits.push(PathLimit::new(
        GlobPattern::new("/pub/**"),
        Method::GET,
        "1mb/s".parse().unwrap(),
        LimitKeyType::User,
        None,
    ));
    // Also need a generous PUT limit so uploads are fast
    config.drive.rate_limits.push(PathLimit::new(
        GlobPattern::new("/pub/**"),
        Method::PUT,
        "1mb/s".parse().unwrap(),
        LimitKeyType::User,
        None,
    ));
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

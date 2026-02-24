use std::time::{Duration, Instant};

use pubky_testnet::pubky::{
    errors::RequestError, Error, Keypair, Method, PubkySession, StatusCode,
};
use pubky_testnet::{
    pubky_homeserver::{
        quota_config::{GlobPattern, LimitKey, LimitKeyType, PathLimit},
        ConfigToml, SignupMode,
    },
    EphemeralTestnet,
};

#[tokio::test]
#[pubky_testnet::test]
async fn test_limit_signin_get_session() {
    let mut config = ConfigToml::default_test_config();
    config.drive.rate_limits = vec![
        // Limit sign-ins: POST /session by IP
        PathLimit::new(
            GlobPattern::new("/session"),
            Method::POST,
            "1r/m".parse().unwrap(),
            LimitKeyType::Ip,
            None,
        ),
        // Limit session fetch/validate: GET /session by User
        PathLimit::new(
            GlobPattern::new("/session"),
            Method::GET,
            "1r/m".parse().unwrap(),
            LimitKeyType::User,
            None,
        ),
    ];

    let testnet = EphemeralTestnet::builder()
        .config(config)
        .build()
        .await
        .unwrap();

    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Create a user (signup should not hit the POST /session signin limit)
    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();

    // First signin should be OK
    let session = signer.signin().await.unwrap();

    // First GET /session (validate/fetch) should be OK
    session.revalidate().await.unwrap();

    // Second GET /session should be rate-limited (429)
    let err = session
        .revalidate()
        .await
        .expect_err("Second /session GET should be rate limited");
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::TOO_MANY_REQUESTS)
    );

    // Second signin should be rate-limited (429)
    let err = signer
        .signin()
        .await
        .expect_err("Second signin should be rate limited");
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::TOO_MANY_REQUESTS)
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn test_limit_signin_get_session_whitelist() {
    // Pre-generate the whitelisted user (we need their pubkey in the config)
    let whitelisted_keypair = Keypair::random();
    let whitelisted_pubkey = whitelisted_keypair.public_key();

    // Rate-limit GET /session by user, but whitelist `whitelisted_pubkey`
    let mut config = ConfigToml::default_test_config();
    let mut limit = PathLimit::new(
        GlobPattern::new("/session"),
        Method::GET,
        "1r/m".parse().unwrap(),
        LimitKeyType::User,
        None,
    );
    limit
        .whitelist
        .push(LimitKey::User(whitelisted_pubkey.clone()));
    config.drive.rate_limits = vec![limit];

    let testnet = EphemeralTestnet::builder()
        .config(config)
        .build()
        .await
        .unwrap();

    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // --- Whitelisted user ---
    let whitelisted_signer = pubky.signer(whitelisted_keypair);
    whitelisted_signer
        .signup(&server.public_key(), None)
        .await
        .unwrap();
    let session_w = whitelisted_signer.signin().await.unwrap();

    // First GET /session OK
    session_w.revalidate().await.unwrap();
    // Second GET /session also OK (whitelisted)
    session_w.revalidate().await.unwrap();

    // --- Non-whitelisted user ---
    let other = pubky.signer(Keypair::random());
    other.signup(&server.public_key(), None).await.unwrap();
    let session_o = other.signin().await.unwrap();

    // First GET /session OK
    session_o.revalidate().await.unwrap();
    // Second GET /session should be rate-limited (429)
    let err = session_o
        .revalidate()
        .await
        .expect_err("Should be rate limited because not on whitelist");
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::TOO_MANY_REQUESTS)
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn test_limit_events() {
    // Rate-limit GET /events/ by IP
    let mut config = ConfigToml::default_test_config();
    config.drive.rate_limits = vec![PathLimit::new(
        GlobPattern::new("/events/"),
        Method::GET,
        "1r/m".parse().unwrap(),
        LimitKeyType::Ip,
        None,
    )];

    let testnet = EphemeralTestnet::builder()
        .config(config)
        .build()
        .await
        .unwrap();

    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let client = pubky.client();

    // Events feed URL (pkarr host form)
    let url = format!("https://{}/events/", server.public_key().z32());

    // First request OK
    let res = client.request(Method::GET, &url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Second request should be rate-limited
    let res = client.request(Method::GET, &url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
#[pubky_testnet::test]
async fn test_limit_upload() {
    let mut config = ConfigToml::default_test_config();
    config.drive.rate_limits = vec![PathLimit::new(
        GlobPattern::new("/pub/**"),
        Method::PUT,
        "1kb/s".parse().unwrap(),
        LimitKeyType::User,
        None,
    )];

    let testnet = EphemeralTestnet::builder()
        .config(config)
        .build()
        .await
        .unwrap();

    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // User + session-bound session
    let signer = pubky.signer(Keypair::random());
    let session = signer.signup(&server.public_key(), None).await.unwrap();

    // Upload ~3 KB; at 1 KB/s it should take > 2s total
    let path = "/pub/test.txt";
    let body = vec![0u8; 3 * 1024];

    let start = Instant::now();
    let resp = session.storage().put(path, body).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
    assert!(
        start.elapsed() > Duration::from_secs(2),
        "Upload should be throttled to ~1KB/s (elapsed {:?})",
        start.elapsed()
    );
}

/// Test that 10 clients can write/read to the server concurrently
/// Upload/download rate is limited to 1kb/s per user.
/// 3kb files are used to make the writes/reads take ~2.5s each.
/// Concurrently writing/reading 10 files, the total time taken should be ~3s.
/// If the concurrent writes/reads are not properly handled, the total time taken will be closer to ~25s.
#[tokio::test]
#[pubky_testnet::test]
async fn test_concurrent_write_read() {
    // --- homeserver with per-user throttling on PUT/GET under /pub/**
    let mut config = ConfigToml::default_test_config();
    config.drive.rate_limits = vec![
        PathLimit::new(
            GlobPattern::new("/pub/**"),
            Method::PUT,
            "1kb/s".parse().unwrap(),
            LimitKeyType::User,
            None,
        ),
        PathLimit::new(
            GlobPattern::new("/pub/**"),
            Method::GET,
            "1kb/s".parse().unwrap(),
            LimitKeyType::User,
            None,
        ),
    ];

    let testnet = EphemeralTestnet::builder()
        .config(config)
        .build()
        .await
        .unwrap();

    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // --- create 10 independent users (each has its own per-user limiter)
    let user_count = 10usize;
    let mut sessions: Vec<PubkySession> = Vec::with_capacity(user_count);
    for _ in 0..user_count {
        let signer = pubky.signer(Keypair::random());
        let session = signer.signup(&server.public_key(), None).await.unwrap();
        sessions.push(session);
    }

    let path = "/pub/test.txt";
    let body = vec![0u8; 3 * 1024]; // 3 KB => ~3s at 1 KB/s per user

    // --- concurrent uploads
    let start = Instant::now();
    {
        let mut tasks = Vec::with_capacity(user_count);
        for session in &sessions {
            let session = session.clone();
            let body = body.clone();
            tasks.push(tokio::spawn(async move {
                session.storage().put(path, body).await.unwrap();
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
    }
    let elapsed = start.elapsed();
    // Should be close to ~3s, comfortably under 5s if ops run concurrently per user.
    assert!(
        elapsed < Duration::from_secs(5),
        "concurrent PUTs too slow: {:?}",
        elapsed
    );

    // --- concurrent downloads (consume bodies to apply the throttle fully)
    let start = Instant::now();
    {
        let mut tasks = Vec::with_capacity(user_count);
        for session in &sessions {
            let session = session.clone();
            tasks.push(tokio::spawn(async move {
                let resp = session.storage().get(path).await.unwrap();
                let _ = resp.bytes().await.unwrap(); // read body to apply full 3 KB download
            }));
        }
        for t in tasks {
            t.await.unwrap();
        }
    }
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(5),
        "concurrent GETs too slow: {:?}",
        elapsed
    );
}

/// Test that signup token lookups are rate-limited by IP.
/// This is the default rate limit configured in config.default.toml.
#[tokio::test]
#[pubky_testnet::test]
async fn test_limit_signup_tokens() {
    // Configure with token-required signup mode and rate limit on signup_tokens
    let mut config = ConfigToml::default_test_config();
    config.general.signup_mode = SignupMode::TokenRequired;
    config.drive.rate_limits = vec![PathLimit::new(
        GlobPattern::new("/signup_tokens/*"),
        Method::GET,
        "1r/m".parse().unwrap(),
        LimitKeyType::Ip,
        None,
    )];

    let testnet = EphemeralTestnet::builder()
        .config(config)
        .build()
        .await
        .unwrap();

    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let client = pubky.client();

    // Generate a valid token via admin API
    let valid_token = server
        .admin_server()
        .expect("admin server should be enabled")
        .create_signup_token()
        .await
        .unwrap();

    // Build URL for the signup_tokens endpoint (using ICANN HTTP)
    let url = format!("{}signup_tokens/{}", server.icann_http_url(), valid_token);

    // First request should succeed
    let res = client.request(Method::GET, &url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Second request should be rate-limited (429)
    let res = client.request(Method::GET, &url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

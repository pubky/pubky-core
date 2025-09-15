use std::time::{Duration, Instant};

use pubky_testnet::pubky::PubkySession;
use pubky_testnet::pubky::{errors::RequestError, global::global_client, Error, PubkySigner};
use pubky_testnet::{
    pubky_homeserver::{
        quota_config::{GlobPattern, LimitKey, LimitKeyType, PathLimit},
        ConfigToml, MockDataDir,
    },
    Testnet,
};
use reqwest::{Method, StatusCode};

#[tokio::test]
async fn limit_signin_get_session() {
    // Spin up a testnet and configure homeserver limits
    let mut testnet = Testnet::new().await.unwrap();

    let mut cfg = ConfigToml::test();
    cfg.drive.rate_limits = vec![
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
    let mock = MockDataDir::new(cfg, None).unwrap();
    let server = testnet.create_homeserver_with_mock(mock).await.unwrap();

    // Create a user (signup should not hit the POST /session signin limit)
    let signer = PubkySigner::random().unwrap();
    signer.signup(&server.public_key(), None).await.unwrap();

    // First signin should be OK
    let session = signer.signin().await.unwrap();

    // First GET /session (validate/fetch) should be OK
    session.revalidate_session().await.unwrap();

    // Second GET /session should be rate-limited (429)
    let err = session
        .revalidate_session()
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
async fn limit_signin_get_session_whitelist() {
    let mut testnet = Testnet::new().await.unwrap();

    // Pre-generate the whitelisted user (we need their pubkey in the config)
    let whitelisted_signer = PubkySigner::random().unwrap();
    let whitelisted_pubky = whitelisted_signer.public_key().clone();

    // Rate-limit GET /session by user, but whitelist `whitelisted_pubky`
    let mut cfg = ConfigToml::test();
    let mut limit = PathLimit::new(
        GlobPattern::new("/session"),
        Method::GET,
        "1r/m".parse().unwrap(),
        LimitKeyType::User,
        None,
    );
    limit
        .whitelist
        .push(LimitKey::User(whitelisted_pubky.clone()));
    cfg.drive.rate_limits = vec![limit];

    let mock = MockDataDir::new(cfg, None).unwrap();
    let server = testnet.create_homeserver_with_mock(mock).await.unwrap();

    // --- Whitelisted user ---
    whitelisted_signer
        .signup(&server.public_key(), None)
        .await
        .unwrap();
    let session_w = whitelisted_signer.signin().await.unwrap();

    // First GET /session OK
    session_w.revalidate_session().await.unwrap();
    // Second GET /session also OK (whitelisted)
    session_w.revalidate_session().await.unwrap();

    // --- Non-whitelisted user ---
    let other = PubkySigner::random().unwrap();
    other.signup(&server.public_key(), None).await.unwrap();
    let session_o = other.signin().await.unwrap();

    // First GET /session OK
    session_o.revalidate_session().await.unwrap();
    // Second GET /session should be rate-limited (429)
    let err = session_o
        .revalidate_session()
        .await
        .expect_err("Should be rate limited because not on whitelist");
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::TOO_MANY_REQUESTS)
    );
}

#[tokio::test]
async fn limit_events() {
    let mut testnet = Testnet::new().await.unwrap();
    let client = global_client().unwrap();

    // Rate-limit GET /events/ by IP
    let mut cfg = ConfigToml::test();
    cfg.drive.rate_limits = vec![PathLimit::new(
        GlobPattern::new("/events/"),
        Method::GET,
        "1r/m".parse().unwrap(),
        LimitKeyType::Ip,
        None,
    )];

    let mock = MockDataDir::new(cfg, None).unwrap();
    let server = testnet.create_homeserver_with_mock(mock).await.unwrap();

    // Events feed URL (pkarr host form)
    let url = format!("https://{}/events/", server.public_key());

    // First request OK
    let res = client.request(Method::GET, &url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);

    // Second request should be rate-limited
    let res = client.request(Method::GET, &url).send().await.unwrap();
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn limit_upload() {
    let mut testnet = Testnet::new().await.unwrap();

    // Throttle PUTs under /pub/** to 1 KB/s per user
    let mut cfg = ConfigToml::test();
    cfg.drive.rate_limits = vec![PathLimit::new(
        GlobPattern::new("/pub/**"),
        Method::PUT,
        "1kb/s".parse().unwrap(),
        LimitKeyType::User,
        None,
    )];

    let mock = MockDataDir::new(cfg, None).unwrap();
    let server = testnet.create_homeserver_with_mock(mock).await.unwrap();

    // User + session-bound session
    let signer = PubkySigner::random().unwrap();
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
async fn test_concurrent_write_read() {
    // --- homeserver with per-user throttling on PUT/GET under /pub/**
    let mut testnet = Testnet::new().await.unwrap();
    let mut cfg = ConfigToml::test();
    cfg.drive.rate_limits = vec![
        PathLimit::new(
            GlobPattern::new("/pub/**"),
            reqwest::Method::PUT,
            "1kb/s".parse().unwrap(),
            LimitKeyType::User,
            None,
        ),
        PathLimit::new(
            GlobPattern::new("/pub/**"),
            reqwest::Method::GET,
            "1kb/s".parse().unwrap(),
            LimitKeyType::User,
            None,
        ),
    ];
    let mock = MockDataDir::new(cfg, None).unwrap();
    let server = testnet.create_homeserver_with_mock(mock).await.unwrap();

    // --- create 10 independent users (each has its own per-user limiter)
    let user_count = 10usize;
    let mut sessions: Vec<PubkySession> = Vec::with_capacity(user_count);
    for _ in 0..user_count {
        let signer = PubkySigner::random().unwrap();
        let session = signer.signup(&server.public_key(), None).await.unwrap();
        sessions.push(session);
    }

    let path = "/pub/test.txt";
    let body = vec![0u8; 3 * 1024]; // 3 KB => ~3s at 1 KB/s per user

    // --- concurrent uploads
    let start = Instant::now();
    {
        let mut tasks = Vec::with_capacity(user_count);
        for session in sessions.iter().cloned() {
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
        for session in sessions.iter().cloned() {
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

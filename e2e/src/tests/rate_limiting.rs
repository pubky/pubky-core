use std::time::Duration;

use pkarr::Keypair;
use pubky_testnet::{
    pubky_homeserver::{
        quota_config::{GlobPattern, LimitKey, LimitKeyType, PathLimit},
        ConfigToml, MockDataDir,
    },
    Testnet,
};
use reqwest::{Method, StatusCode, Url};
use tokio::time::Instant;

#[tokio::test]
async fn test_limit_signin_get_session() {
    let mut testnet = Testnet::new().await.unwrap();
    let client = testnet.pubky_client_builder().build().unwrap();

    let mut config = ConfigToml::test();
    config.drive.rate_limits = vec![
        PathLimit::new(
            GlobPattern::new("/session"),
            Method::POST,
            "1r/m".parse().unwrap(),
            LimitKeyType::Ip,
            None,
        ), // Limit signins
        PathLimit::new(
            GlobPattern::new("/session"),
            Method::GET,
            "1r/m".parse().unwrap(),
            LimitKeyType::User,
            None,
        ), // Limit decode sessions
    ];
    let mock_dir = MockDataDir::new(config, None).unwrap();
    let server = testnet
        .create_homeserver_suite_with_mock(mock_dir)
        .await
        .unwrap();

    // Create a new user
    let keypair = Keypair::random();
    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    client.signin(&keypair).await.unwrap(); // First signin should be ok

    client.session(&keypair.public_key()).await.unwrap(); // First session should be ok
    client
        .session(&keypair.public_key())
        .await
        .expect_err("Should be rate limited"); // Second session should be rate limited

    client
        .signin(&keypair)
        .await
        .expect_err("Should be rate limited"); // Second signin should be rate limited
}

#[tokio::test]
async fn test_limit_signin_get_session_whitelist() {
    let keypair = Keypair::random();
    let mut testnet = Testnet::new().await.unwrap();
    let client = testnet.pubky_client_builder().build().unwrap();

    let mut config = ConfigToml::test();
    let mut limit = PathLimit::new(
        GlobPattern::new("/session"),
        Method::GET,
        "1r/m".parse().unwrap(),
        LimitKeyType::User,
        None,
    );
    limit.whitelist.push(LimitKey::User(keypair.public_key()));
    config.drive.rate_limits = vec![
        limit, // Limit decode sessions
    ];
    let mock_dir = MockDataDir::new(config, None).unwrap();
    let server = testnet
        .create_homeserver_suite_with_mock(mock_dir)
        .await
        .unwrap();

    // Create a new user
    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    client
        .session(&keypair.public_key())
        .await
        .expect("Should not be rate limited anyway");
    client
        .session(&keypair.public_key())
        .await
        .expect("Should not be rate limited because on whitelist");

    // Create another new user, not on the whitelist
    let keypair = Keypair::random();
    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    client
        .session(&keypair.public_key())
        .await
        .expect("Should not be rate limited anyway");
    client
        .session(&keypair.public_key())
        .await
        .expect_err("Should be rate limited because not on whitelist");
}

#[tokio::test]
async fn test_limit_events() {
    let mut testnet = Testnet::new().await.unwrap();
    let client = testnet.pubky_client_builder().build().unwrap();

    let mut config = ConfigToml::test();
    config.drive.rate_limits = vec![
        PathLimit::new(
            GlobPattern::new("/events/"),
            Method::GET,
            "1r/m".parse().unwrap(),
            LimitKeyType::Ip,
            None,
        ), // Limit events
    ];
    let mock_dir = MockDataDir::new(config, None).unwrap();
    let server = testnet
        .create_homeserver_suite_with_mock(mock_dir)
        .await
        .unwrap();

    let url = server.pubky_url().join("/events/").unwrap();
    let res = client.get(url.clone()).send().await.unwrap(); // First event should be ok
    assert_eq!(res.status(), StatusCode::OK);

    let res = client.get(url).send().await.unwrap(); // Second event should be rate limited
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn test_limit_upload() {
    let mut testnet = Testnet::new().await.unwrap();
    let client = testnet.pubky_client_builder().build().unwrap();

    let mut config = ConfigToml::test();
    config.drive.rate_limits = vec![
        PathLimit::new(
            GlobPattern::new("/pub/**"),
            Method::PUT,
            "1kb/s".parse().unwrap(),
            LimitKeyType::User,
            None,
        ), // Limit events
    ];
    let mock_dir = MockDataDir::new(config, None).unwrap();
    let server = testnet
        .create_homeserver_suite_with_mock(mock_dir)
        .await
        .unwrap();

    // Create a new user
    let keypair = Keypair::random();
    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let url: Url = format!("pubky://{}/pub/test.txt", keypair.public_key())
        .parse()
        .unwrap();
    let start = Instant::now();
    let res = client
        .put(url)
        .body(vec![0u8; 3 * 1024]) // 2kb
        .send()
        .await
        .unwrap();
    assert_eq!(res.status(), StatusCode::CREATED);
    assert!(start.elapsed() > Duration::from_secs(2));
}

use std::time::Duration;

use pkarr::{Keypair, PublicKey};
use pubky_testnet::{
    pubky::Client,
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
    let client = testnet.pubky_client().unwrap();

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
    let client = testnet.pubky_client().unwrap();

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
    let client = testnet.pubky_client().unwrap();

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
    let client = testnet.pubky_client().unwrap();

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

/// Test that 10 clients can write/read to the server concurrently
/// Upload/download rate is limited to 1kb/s per user.
/// 3kb files are used to make the writes/reads take ~2.5s each.
/// Concurrently writing/reading 10 files, the total time taken should be ~3s.
/// If the concurrent writes/reads are not properly handled, the total time taken will be closer to ~25s.
#[tokio::test]
async fn test_concurrent_write_read() {
    // Setup the testnet
    let mut testnet = Testnet::new().await.unwrap();
    let mut config = ConfigToml::test();
    config.drive.rate_limits = vec![
        PathLimit::new(
            // Limit uploads to 1kb/s per user
            GlobPattern::new("/pub/**"),
            Method::PUT,
            "1kb/s".parse().unwrap(),
            LimitKeyType::User,
            None,
        ),
        PathLimit::new(
            // Limit downloads to 1kb/s per user
            GlobPattern::new("/pub/**"),
            Method::GET,
            "1kb/s".parse().unwrap(),
            LimitKeyType::User,
            None,
        ),
    ];
    let mock_dir = MockDataDir::new(config, None).unwrap();
    let hs_pubkey = {
        let server = testnet
            .create_homeserver_suite_with_mock(mock_dir)
            .await
            .unwrap();
        server.public_key()
    };

    // Create helper struct to handle clients
    #[derive(Clone)]
    struct TestClient {
        pub keypair: Keypair,
        pub client: Client,
    }
    impl TestClient {
        fn new(testnet: &mut Testnet) -> Self {
            let keypair = Keypair::random();
            let client = testnet.pubky_client().unwrap();
            Self { keypair, client }
        }
        pub async fn signup(&self, hs_pubkey: &PublicKey) {
            self.client
                .signup(&self.keypair, hs_pubkey, None)
                .await
                .expect("Failed to signup");
        }
        pub async fn put(&self, url: Url, body: Vec<u8>) {
            self.client
                .put(url)
                .body(body)
                .send()
                .await
                .expect("Failed to put");
        }
        pub async fn get(&self, url: Url) {
            let response = self.client.get(url).send().await.expect("Failed to get");
            assert_eq!(response.status(), StatusCode::OK, "Failed to get");
            response.bytes().await.expect("Failed to get bytes"); // Download the body
        }
    }

    // Signup with the clients
    let user_count: usize = 10;
    let mut clients = vec![0; user_count]
        .into_iter()
        .map(|_| TestClient::new(&mut testnet))
        .collect::<Vec<_>>();
    for client in clients.iter_mut() {
        client.signup(&hs_pubkey).await;
    }

    // --------------------------------------------------------------------------------------------
    // Write to server concurrently
    let start = Instant::now();
    let mut handles = vec![];
    for client in clients.iter() {
        let client = client.clone();
        let handle = tokio::spawn(async move {
            let url: Url = format!("pubky://{}/pub/test.txt", client.keypair.public_key())
                .parse()
                .unwrap();
            let body = vec![0u8; 3 * 1024]; // 2kb
            client.put(url, body).await;
        });
        handles.push(handle);
    }
    // Wait for all the writes to finish
    for handle in handles {
        handle.await.unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5));

    // --------------------------------------------------------------------------------------------
    // Read from server concurrently
    let start = Instant::now();
    let mut handles = vec![];
    for client in clients.iter() {
        let client = client.clone();
        let handle = tokio::spawn(async move {
            let url: Url = format!("pubky://{}/pub/test.txt", client.keypair.public_key())
                .parse()
                .unwrap();
            client.get(url).await;
        });
        handles.push(handle);
    }
    // Wait for all the reads to finish
    for handle in handles {
        handle.await.unwrap();
    }
    let elapsed = start.elapsed();
    assert!(elapsed < Duration::from_secs(5));
}

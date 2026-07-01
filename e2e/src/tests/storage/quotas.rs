use super::*;

#[tokio::test]
#[pubky_testnet::test]
async fn put_quota_applied() {
    // Start a test homeserver with 1 MB user data limit
    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();

    let mut mock_dir = MockDataDir::test();
    mock_dir.config_toml.storage.default_quota_mb = Some(1); // 1 MB
    let server = testnet
        .create_homeserver_app_with_mock(mock_dir)
        .await
        .unwrap();

    // Create a user/session
    let signer = pubky.signer(Keypair::random());
    let session = signer
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();

    let p1 = "/pub/data";
    let p2 = "/pub/data2";

    // First 600 KB → OK (201)
    let data_600k: Vec<u8> = vec![0; 600_000];
    let resp = session.storage().put(p1, data_600k.clone()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Overwrite same 600 KB → still 201
    let resp = session.storage().put(p1, data_600k.clone()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Write 600 KB more at a different path (total 1.2 MB) → 507
    let err = session
        .storage()
        .put(p2, data_600k.clone())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::Request(RequestError::Server { status, .. })
            if status == StatusCode::INSUFFICIENT_STORAGE
    ));

    // Overwrite /pub/data with 1.1 MB → 507
    let data_1100k: Vec<u8> = vec![0; 1_100_000];
    let err = session.storage().put(p1, data_1100k).await.unwrap_err();
    assert!(matches!(
        err,
        Error::Request(RequestError::Server { status, .. })
            if status == StatusCode::INSUFFICIENT_STORAGE
    ));

    // Delete the original 600 KB → 204
    let resp = session.storage().delete(p1).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Write exactly 1025 KB → 507 (exceeds 1 MB quota)
    let data_1025k_minus_256: Vec<u8> = vec![0; 1025 * 1024 - 256];
    let err = session
        .storage()
        .put(p1, data_1025k_minus_256)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::Request(RequestError::Server { status, .. })
            if status == StatusCode::INSUFFICIENT_STORAGE
    ));

    // Write exactly 1 MB (minus the same 256 fudge) → 201 (fits quota)
    let data_1mb_minus_256: Vec<u8> = vec![0; 1024 * 1024 - 256];
    let resp = session.storage().put(p1, data_1mb_minus_256).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

/// A `/priv/` write is rejected with 507 once the shared bucket
/// is exhausted, and freeing `/pub/` space lets the same `/priv/` write succeed.
#[tokio::test]
#[pubky_testnet::test]
async fn priv_writes_count_toward_quota() {
    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();

    let mut mock_dir = MockDataDir::test();
    mock_dir.config_toml.storage.default_quota_mb = Some(1); // 1 MB
    let server = testnet
        .create_homeserver_app_with_mock(mock_dir)
        .await
        .unwrap();

    let signer = pubky.signer(Keypair::random());
    let session = signer
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();

    let data_600k: Vec<u8> = vec![0; 600_000];

    // 600 KB to /pub → OK (201).
    let resp = session
        .storage()
        .put("/pub/data", data_600k.clone())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // 600 KB to /priv → combined 1.2 MB exceeds the shared 1 MB quota → 507.
    let err = session
        .storage()
        .put("/priv/data", data_600k.clone())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::Request(RequestError::Server { status, .. })
            if status == StatusCode::INSUFFICIENT_STORAGE
    ));

    // Free the /pub file → 204.
    let resp = session.storage().delete("/pub/data").await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Now the same /priv write fits in the freed quota → 201.
    let resp = session
        .storage()
        .put("/priv/data", data_600k)
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}
/// Regression test: quota early-rejection still works when bandwidth throttling
/// is active. The bandwidth middleware wraps the request body in a throttled
/// stream that loses `body.size_hint()`. The fix reads Content-Length from
/// headers instead.
#[tokio::test]
#[pubky_testnet::test]
async fn put_quota_applied_with_bandwidth_throttling() {
    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();

    let mut mock_dir = MockDataDir::test();
    mock_dir.config_toml.storage.default_quota_mb = Some(1); // 1 MB
                                                             // Enable bandwidth throttling so the BandwidthQuotaLimitLayer wraps the body.
    mock_dir.config_toml.default_quotas.rate_write = Some("10mb/s".parse().unwrap());
    let server = testnet
        .create_homeserver_app_with_mock(mock_dir)
        .await
        .unwrap();

    let signer = pubky.signer(Keypair::random());
    let session = signer
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();

    // First 600 KB → OK (201)
    let data_600k: Vec<u8> = vec![0; 600_000];
    let resp = session
        .storage()
        .put("/pub/data", data_600k.clone())
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Write another 600 KB at a different path (total 1.2 MB) → should be rejected
    // early via Content-Length header check, even though the bandwidth layer
    // has already replaced the body stream (losing size_hint).
    let err = session
        .storage()
        .put("/pub/data2", data_600k)
        .await
        .unwrap_err();
    assert!(
        matches!(
            err,
            Error::Request(RequestError::Server { status, .. })
                if status == StatusCode::INSUFFICIENT_STORAGE
        ),
        "Expected 507 INSUFFICIENT_STORAGE but got: {:?}",
        err
    );
}

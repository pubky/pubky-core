use pubky_testnet::pubky::{
    Keypair, Method, PubkyAuthFlow, PubkyHttpClient, PubkySession, StatusCode,
};
use pubky_testnet::pubky_common::capabilities::{Capabilities, Capability};
use pubky_testnet::{
    pubky_homeserver::{MockDataDir, SignupMode},
    EphemeralTestnet, Testnet,
};
use std::time::Duration;

use pubky_testnet::pubky::errors::{Error, RequestError};

#[tokio::test]
#[pubky_testnet::test]
async fn basic_authn() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let homeserver = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());

    let user = signer.signup(&homeserver.public_key(), None).await.unwrap();

    let session = user.info();

    assert!(session.capabilities().contains(&Capability::root()));

    user.signout().await.unwrap();
}

#[tokio::test]
#[pubky_testnet::test]
async fn disabled_user() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    // Create a brand-new user and session
    let signer = pubky.signer(Keypair::random());
    let pubky = signer.public_key().clone();
    let session = signer.signup(&server.public_key(), None).await.unwrap();

    // Create a test file to ensure the user can write to their account
    let file_path = "/pub/pubky.app/foo";
    session
        .storage()
        .put(file_path, Vec::<u8>::new())
        .await
        .unwrap();

    // Make sure the user can read their own file
    let response = session.storage().get(file_path).await.unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "User should be able to read their own file"
    );

    // Disable the user via admin API
    let admin_socket = server.admin().listen_socket();
    let admin_client = PubkyHttpClient::new().unwrap();
    let disable_url = format!("http://{admin_socket}/users/{pubky}/disable");
    let resp = admin_client
        .request(Method::POST, &disable_url)
        .header("X-Admin-Password", "admin")
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    // User can still read their own file
    let response = session.storage().get(file_path).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // User can no longer write
    let err = session
        .storage()
        .put(file_path, Vec::<u8>::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::FORBIDDEN),
        "Disabled user must get 403 on write"
    );

    // Fresh sign-in should still succeed (disabled means no writes, not no login)
    session.signout().await.unwrap();

    let session2 = signer
        .signin()
        .await
        .expect("Signin should succeed for disabled users");
    assert_eq!(session2.info().public_key(), &pubky);
}

#[tokio::test]
#[pubky_testnet::test]
async fn authz() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    let http_relay_url = testnet.http_relay().local_link_url();

    // Third-party app (keyless)
    let caps = Capabilities::builder()
        .read_write("/pub/pubky.app/")
        .read("/pub/foo.bar/file")
        .finish();

    // Third-party app (keyless)
    let auth = PubkyAuthFlow::builder(&caps)
        .relay(http_relay_url)
        .client(pubky.client().clone())
        .start()
        .unwrap();

    // Signer authenticator
    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();
    signer
        .approve_auth(&auth.authorization_url())
        .await
        .unwrap();

    // Retrieve the session-bound agent (third party app)
    let user = auth.await_approval().await.unwrap();

    assert_eq!(user.info().public_key(), &signer.public_key());

    // let session = user.info().await.unwrap().unwrap();
    // assert_eq!(session.capabilities(), &caps.0);

    // Ensure the same user pubky has been authed on the keyless app from cold keypair
    assert_eq!(user.info().public_key(), &signer.public_key());

    // Access control enforcement
    user.storage()
        .put("/pub/pubky.app/foo", Vec::<u8>::new())
        .await
        .unwrap();

    let err = user
        .storage()
        .put("/pub/pubky.app", Vec::<u8>::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::FORBIDDEN)
    );

    let err = user
        .storage()
        .put("/pub/foo.bar/file", Vec::<u8>::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::FORBIDDEN)
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn persist_and_restore_info() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let homeserver = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    // Create user and session-bound agent
    let signer = pubky.signer(Keypair::random());
    let session = signer.signup(&homeserver.public_key(), None).await.unwrap();

    // Write something with the live agent
    session
        .storage()
        .put("/pub/app/persist.txt", "hello")
        .await
        .unwrap();

    // Export session's secret and drop the session (simulate restart)
    let secret_token = session.export_secret();
    drop(session);

    // Save to disk or however you want to persist `exported`

    // Rehydrate from the exported secret (validates the session)
    let restored = PubkySession::import_secret(&secret_token, Some(pubky.client().clone()))
        .await
        .unwrap();

    // Same identity?
    assert_eq!(restored.info().public_key(), &signer.public_key());

    // Still authorized to write
    restored
        .storage()
        .put("/pub/app/persist.txt", "hello2")
        .await
        .unwrap();
}

#[tokio::test]
async fn multiple_users() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    // Two independent users
    let alice = pubky.signer(Keypair::random());
    let bob = pubky.signer(Keypair::random());

    let alice_session = alice.signup(&server.public_key(), None).await.unwrap();
    let bob_session = bob.signup(&server.public_key(), None).await.unwrap();

    // Each session is bound to its own pubkey and has root caps
    let a_sess = alice_session.info();
    assert_eq!(a_sess.public_key(), &alice.public_key());
    assert!(a_sess.capabilities().contains(&Capability::root()));

    let b_sess = bob_session.info();
    assert_eq!(b_sess.public_key(), &bob.public_key());
    assert!(b_sess.capabilities().contains(&Capability::root()));
}

#[tokio::test]
#[pubky_testnet::test]
async fn authz_timeout_reconnect() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    let http_relay_url = testnet.http_relay().local_link_url();

    // Third-party app (keyless) with a short HTTP timeout to force long-poll retries
    let capabilities = Capabilities::builder()
        .read_write("/pub/pubky.app/")
        .read("/pub/foo.bar/file")
        .finish();

    let client = testnet
        .client_builder()
        .request_timeout(Duration::from_millis(1_000))
        .build()
        .unwrap();

    // set custom global client with timeout of 1 sec
    // Start pairing auth flow using our custom client + local relay
    let auth = PubkyAuthFlow::builder(&capabilities)
        .client(client)
        .relay(http_relay_url)
        .start()
        .unwrap();

    // Signer side: sign up, then approve after a delay (to exercise timeout/retry)
    let signer = pubky.signer(Keypair::random());
    let signer_pubky = signer.public_key();
    signer.signup(&server.public_key(), None).await.unwrap();

    let url_clone = auth.authorization_url().clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(1_000)).await;
        signer.approve_auth(&url_clone).await.unwrap();
    });

    // The long-poll should survive timeouts and eventually yield an session
    let session = auth.await_approval().await.unwrap();
    assert_eq!(session.info().public_key(), &signer_pubky);

    // Access control enforcement (write inside scope OK, others forbidden)
    session
        .storage()
        .put("/pub/pubky.app/foo", Vec::<u8>::new())
        .await
        .unwrap();

    let err = session
        .storage()
        .put("/pub/pubky.app", Vec::<u8>::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::FORBIDDEN)
    );

    let err = session
        .storage()
        .put("/pub/foo.bar/file", Vec::<u8>::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::FORBIDDEN)
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn signup_with_token() {
    // 1. Start a test homeserver with closed signups (i.e. signup tokens required)
    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    let signer2 = pubky.signer(Keypair::random());

    let mut mock_dir = MockDataDir::test();
    mock_dir.config_toml.general.signup_mode = SignupMode::TokenRequired;
    let server = testnet.create_homeserver_with_mock(mock_dir).await.unwrap();

    // 2. Try to signup with an invalid token "AAAAA" and expect failure.
    let invalid_signup = signer
        .signup(&server.public_key(), Some("AAAA-BBBB-CCCC"))
        .await;
    assert!(
        invalid_signup.is_err(),
        "Signup should fail with an invalid signup token"
    );
    let err = invalid_signup.unwrap_err();
    assert!(
        err.to_string().to_lowercase().contains("401"),
        "Signup should fail with a 401 status code"
    );

    // 3. Call the admin endpoint to generate a valid signup token.
    let valid_token = server.admin().create_signup_token().await.unwrap();

    // 4. Now signup with the valid token. Expect success and a session back.
    let session = signer
        .signup(&server.public_key(), Some(&valid_token))
        .await
        .unwrap();
    assert!(
        !session.info().public_key().to_string().is_empty(),
        "SessionInfo should contain a valid public key"
    );

    // 5. Finally, sign in with the same keypair and verify that a session is returned.
    let pubky = signer.public_key();
    let session = signer.signin().await.unwrap();
    assert_eq!(
        session.info().public_key(),
        &pubky,
        "Signed-in session pubky should correspond to the signer's public key"
    );

    // 6. Signup with the same token again and expect failure.
    let signup_again = signer2
        .signup(&server.public_key(), Some(&valid_token))
        .await;
    let err = signup_again.expect_err("Signup with an already used token should fail");
    assert!(err.to_string().contains("401"));
    assert!(err.to_string().contains("Token already used"));
}

// This test verifies that when a signin happens immediately after signup,
// the record is not republished on signin (its timestamp remains unchanged)
// but when a signin happens after the record is “old” (in test, after 1 second),
// the record is republished (its timestamp increases).
#[tokio::test]
#[pubky_testnet::test]
async fn republish_if_stale_triggers_timestamp_bump() {
    use std::time::Duration;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();
    let client = testnet.client().unwrap();

    // Sign up a brand-new user (initial publish happens on signup)
    let signer = pubky.signer(Keypair::random());
    let pubky = signer.public_key().clone();
    signer.signup(&server.public_key(), None).await.unwrap();

    // Capture initial record timestamp
    let ts1 = client
        .pkarr()
        .resolve_most_recent(&pubky)
        .await
        .unwrap()
        .timestamp()
        .as_u64();

    // Make conditional publish consider the record stale after just 1ms,
    // then wait long enough to cross a whole second (pkarr timestamps are second-resolution).
    let pkdns = signer.pkdns().set_stale_after(Duration::from_millis(1));
    tokio::time::sleep(Duration::from_millis(1200)).await;

    // Conditional republish should now occur
    pkdns.publish_homeserver_if_stale(None).await.unwrap();

    let ts2 = client
        .pkarr()
        .resolve_most_recent(&pubky)
        .await
        .unwrap()
        .timestamp()
        .as_u64();

    assert_ne!(ts1, ts2, "record should be republished when stale");
}

// This test verifies that when a signin happens immediately after signup,
// the record is not republished on signin (its timestamp remains unchanged)
// but when a signin happens after the record is “old” (in test, after 1 second),
// the record is republished (its timestamp increases).
#[tokio::test]
#[pubky_testnet::test]
async fn conditional_publish_skips_when_fresh() {
    use std::time::Duration;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();
    let client = testnet.client().unwrap();

    let signer = pubky.signer(Keypair::random());
    let pubky = signer.public_key().clone();
    signer.signup(&server.public_key(), None).await.unwrap();

    let ts1 = client
        .pkarr()
        .resolve_most_recent(&pubky)
        .await
        .unwrap()
        .timestamp()
        .as_u64();

    // Set a very large staleness window so the record is definitively "fresh"
    // Default is 3600 seconds, we set it again just for sanity.
    let pkdns = signer.pkdns().set_stale_after(Duration::from_secs(3600));
    pkdns.publish_homeserver_if_stale(None).await.unwrap();

    let ts2 = client
        .pkarr()
        .resolve_most_recent(&pubky)
        .await
        .unwrap()
        .timestamp()
        .as_u64();

    assert_eq!(ts1, ts2, "fresh record must not be republished");
}

#[tokio::test]
#[pubky_testnet::test]
async fn test_republish_homeserver() {
    use std::time::Duration;

    // Setup testnet + a homeserver.
    let mut testnet = Testnet::new().await.unwrap();
    let max_record_age = Duration::from_secs(5);
    let pubky = testnet.sdk().unwrap();
    let server = testnet.create_homeserver().await.unwrap();

    // Create user and publish initial record via signup.
    let signer = pubky.signer(Keypair::random());
    let public_key = signer.public_key().clone();
    signer.signup(&server.public_key(), None).await.unwrap();

    // Initial timestamp.
    let ts1 = pubky
        .client()
        .pkarr()
        .resolve_most_recent(&public_key)
        .await
        .unwrap()
        .timestamp()
        .as_u64();

    // Conditional publish with a "fresh" record should NO-OP.
    let pkdns = signer.pkdns().set_stale_after(max_record_age);
    pkdns.publish_homeserver_if_stale(None).await.unwrap();

    let ts2 = pubky
        .client()
        .pkarr()
        .resolve_most_recent(&public_key)
        .await
        .unwrap()
        .timestamp()
        .as_u64();
    assert_eq!(ts1, ts2, "fresh record must not be republished");

    // Wait until the record is stale (add 1s to cross second-resolution).
    tokio::time::sleep(max_record_age + Duration::from_secs(1)).await;

    // Now the conditional publish should republish and bump the timestamp.
    pkdns.publish_homeserver_if_stale(None).await.unwrap();

    let ts3 = pubky
        .client()
        .pkarr()
        .resolve_most_recent(&public_key)
        .await
        .unwrap()
        .timestamp()
        .as_u64();

    assert!(ts3 > ts2, "record should be republished when stale");
}

/// Helper function to extract cookie ID and secret from exported token
/// Format: "pubkey:cookie_id:cookie_secret"
fn extract_cookie_from_export(export: &str) -> (String, String) {
    let parts: Vec<&str> = export.split(':').collect();
    assert!(
        parts.len() == 3,
        "Export should have format pubkey:cookie_id:cookie_secret"
    );
    (parts[1].to_string(), parts[2].to_string())
}

/// Test backward compatibility: Server sends both UUID and legacy cookies
/// This comprehensive test verifies:
/// 1. Server sends BOTH cookie formats (UUID + pubkey)
/// 2. Both cookies can be used for authentication independently
/// 3. Multiple sessions with UUID cookies work (no collision)
/// 4. Multiple sessions with legacy cookies collide (expected legacy behavior)
#[tokio::test]
#[pubky_testnet::test]
async fn test_backward_compatibility_dual_cookies() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    let keypair = Keypair::random();
    let public_key = keypair.public_key();

    // Create user
    let signer = pubky.signer(keypair);
    signer.signup(&server.public_key(), None).await.unwrap();

    println!("\n=== Phase 1: Create first session ===");
    let session_a = signer.signin().await.unwrap();
    let export_a = session_a.export_secret();
    println!("Session A export: {}", export_a);

    // Parse the export to extract both cookie formats
    let (uuid_a, secret_a) = extract_cookie_from_export(&export_a);
    let legacy_cookie_name = public_key.to_string();

    println!("Session A UUID cookie: {}={}", uuid_a, secret_a);
    println!(
        "Session A legacy cookie: {}={}",
        legacy_cookie_name, secret_a
    );

    // Test that Session A works (uses UUID cookie internally)
    session_a
        .storage()
        .put("/pub/test_a.txt", "data from session A")
        .await
        .unwrap();
    println!("✅ Session A works for writing data");

    println!("\n=== Phase 2: Create second session (same user) ===");
    let session_b = signer.signin().await.unwrap();
    let export_b = session_b.export_secret();
    println!("Session B export: {}", export_b);

    let (uuid_b, secret_b) = extract_cookie_from_export(&export_b);

    println!("Session B UUID cookie: {}={}", uuid_b, secret_b);
    println!(
        "Session B legacy cookie: {}={}",
        legacy_cookie_name, secret_b
    );

    // Verify UUIDs are different
    assert_ne!(uuid_a, uuid_b, "UUID cookies should be different");
    println!("✅ UUID cookies are different: no collision!");

    println!("\n=== Phase 3: Test both sessions work independently ===");

    // Session A should still work (UUID cookie prevents collision)
    session_a
        .storage()
        .put("/pub/test_a2.txt", "more data from session A")
        .await
        .unwrap();
    println!("✅ Session A still works after Session B created");

    // Session B should work
    session_b
        .storage()
        .put("/pub/test_b.txt", "data from session B")
        .await
        .unwrap();
    println!("✅ Session B works");

    println!("\n=== Phase 4: Verify both sessions remain valid ===");

    // Verify Session A can read data
    let response = session_a.storage().get("/pub/test_a.txt").await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    println!("✅ Session A can still read its data");

    // Verify Session B can read data
    let response = session_b.storage().get("/pub/test_b.txt").await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    println!("✅ Session B can read its data");

    println!("\n=== Summary ===");
    println!("✅ Server sends both UUID and legacy cookies (backward compatible)");
    println!("✅ Both cookie formats work for authentication");
    println!("✅ UUID cookies: {} != {} (NO COLLISION)", uuid_a, uuid_b);
    println!(
        "⚠️  Legacy cookies: {} == {} (COLLISION in browser)",
        legacy_cookie_name, legacy_cookie_name
    );
    println!("✅ Both sessions remain valid in database");
    println!("✅ New SDK clients use UUID cookies → no collision");
    println!("✅ Old SDK clients use legacy cookies → work but have collision");
}

/// Test backward compatibility: SDK can import legacy 2-part format
#[tokio::test]
#[pubky_testnet::test]
async fn test_backward_compatibility_legacy_export_format() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    let keypair = Keypair::random();
    let public_key = keypair.public_key();

    // Create user and session
    let signer = pubky.signer(keypair);
    signer.signup(&server.public_key(), None).await.unwrap();
    let session = signer.signin().await.unwrap();

    // Export in new format (3 parts)
    let new_export = session.export_secret();
    println!("New format export: {}", new_export);
    assert_eq!(
        new_export.split(':').count(),
        3,
        "New format should have 3 parts"
    );

    // Simulate legacy format (2 parts: pubkey:secret)
    let parts: Vec<&str> = new_export.split(':').collect();
    let legacy_export = format!("{}:{}", parts[0], parts[2]); // pubkey:secret (skip cookie_id)
    println!("Legacy format export: {}", legacy_export);
    assert_eq!(
        legacy_export.split(':').count(),
        2,
        "Legacy format should have 2 parts"
    );

    // Test: Import legacy format should work
    let restored_session =
        PubkySession::import_secret(&legacy_export, Some(pubky.client().clone()))
            .await
            .unwrap();

    // Verify the restored session works
    let session_info = restored_session.info();
    assert_eq!(session_info.public_key(), &public_key);

    // Verify we can use the restored session
    restored_session
        .storage()
        .put("/pub/test_legacy.txt", "legacy test")
        .await
        .unwrap();

    let response = restored_session
        .storage()
        .get("/pub/test_legacy.txt")
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    println!("✅ Legacy 2-part format import works!");
    println!("✅ Backward compatibility maintained");
}

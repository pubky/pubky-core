use pubky_testnet::pubky::deep_links::DeepLink;
#[allow(deprecated, reason = "E2E tests cover the deprecated cookie flow")]
use pubky_testnet::pubky::PubkyCookieAuthFlow;
use pubky_testnet::pubky::{
    AuthFlowKind, ClientId, Keypair, Method, PubkyHttpClient, PubkyJwtAuthFlow, PubkySession,
    StatusCode,
};
use pubky_testnet::pubky_common::capabilities::{Capabilities, Capability};
use pubky_testnet::{
    pubky_homeserver::{ConfigToml, SignupMode},
    EphemeralTestnet, Testnet,
};
use std::str::FromStr;
use std::time::Duration;

use super::build_full_testnet;
use pubky_testnet::pubky::errors::{Error, RequestError};

#[tokio::test]
#[pubky_testnet::test]
async fn basic_authn() {
    let testnet = build_full_testnet().await;
    let homeserver = testnet.homeserver_app();
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
    // This test requires the admin server to disable users
    let testnet = EphemeralTestnet::builder()
        .config(ConfigToml::default_test_config())
        .build()
        .await
        .unwrap();

    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Create a brand-new user and session
    let signer = pubky.signer(Keypair::random());
    let user_pubky = signer.public_key().clone();
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
    let admin_socket = server
        .admin_server()
        .expect("admin server should be enabled")
        .listen_socket();
    let admin_client = PubkyHttpClient::new().unwrap();

    // Disable the user
    let response = admin_client
        .request(
            Method::POST,
            &format!("http://{admin_socket}/users/{}/disable", user_pubky.z32()),
        )
        .header("X-Admin-Password", "admin")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

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
    assert_eq!(session2.info().public_key(), &user_pubky);
}

#[tokio::test]
#[pubky_testnet::test]
#[allow(deprecated, reason = "Test exercises the deprecated cookie auth flow")]
async fn authz() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let http_relay_url = testnet.http_relay().local_link_url();

    // Third-party app (keyless)
    let caps = Capabilities::builder()
        .read_write("/pub/pubky.app/")
        .read("/pub/foo.bar/file")
        .finish();

    // Third-party app (keyless)
    let auth = PubkyCookieAuthFlow::builder(&caps, AuthFlowKind::signin())
        .relay(http_relay_url)
        .client(pubky.client().clone())
        .start()
        .unwrap();

    let raw_deep_link = auth.authorization_url().to_string();
    let deep_link = DeepLink::from_str(&raw_deep_link).unwrap();
    let signin_deep_link = match deep_link {
        DeepLink::Signin(signin) => signin,
        _ => panic!("Expected a signin deep link"),
    };
    assert_eq!(signin_deep_link.capabilities(), &caps);
    assert_eq!(
        signin_deep_link.relay().as_str(),
        testnet.http_relay().local_link_url().as_str()
    );

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
#[allow(deprecated, reason = "Test exercises the deprecated cookie auth flow")]
async fn signup_authz() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let http_relay_url = testnet.http_relay().local_link_url();

    // Third-party app (keyless)
    let caps = Capabilities::builder()
        .read_write("/pub/pubky.app/")
        .read("/pub/foo.bar/file")
        .finish();

    // Third-party app (keyless)
    let auth = PubkyCookieAuthFlow::builder(
        &caps,
        AuthFlowKind::signup(server.public_key(), Some("1234567890".to_string())),
    )
    .relay(http_relay_url)
    .client(pubky.client().clone())
    .start()
    .unwrap();

    let raw_deep_link = auth.authorization_url().to_string();
    let deep_link = DeepLink::from_str(&raw_deep_link).unwrap();

    let signup_deep_link = match deep_link {
        DeepLink::Signup(signup) => signup,
        _ => panic!("Expected a signup deep link"),
    };
    assert_eq!(signup_deep_link.capabilities(), &caps);
    assert_eq!(
        signup_deep_link.relay().as_str(),
        testnet.http_relay().local_link_url().as_str()
    );
    assert_eq!(signup_deep_link.homeserver(), &server.public_key());
    assert_eq!(
        signup_deep_link.signup_token(),
        Some("1234567890".to_string())
    );

    // Signer authenticator
    let signer = pubky.signer(Keypair::random());
    signer
        .signup(signup_deep_link.homeserver(), None)
        .await
        .unwrap();
    signer
        .approve_auth(&auth.authorization_url())
        .await
        .unwrap();

    // Retrieve the session-bound agent (third party app)
    let user = auth.await_approval().await.unwrap();

    assert_eq!(user.info().public_key(), &signer.public_key());

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
    let testnet = build_full_testnet().await;
    let homeserver = testnet.homeserver_app();
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

    // Export session's secret and drop the session (simulate restart).
    // On native the cookie secret is always captured, so unwrap is safe.
    let secret_token = session.as_cookie().unwrap().export_secret().unwrap();
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
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
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
#[allow(deprecated, reason = "Test exercises the deprecated cookie auth flow")]
async fn authz_timeout_reconnect() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
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
    let auth = PubkyCookieAuthFlow::builder(&capabilities, AuthFlowKind::signin())
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
    let mut config = ConfigToml::default_test_config();
    config.general.signup_mode = SignupMode::TokenRequired;

    let testnet = EphemeralTestnet::builder()
        .config(config)
        .build()
        .await
        .unwrap();

    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    let signer2 = pubky.signer(Keypair::random());

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
    let valid_token = server
        .admin_server()
        .expect("admin server should be enabled")
        .create_signup_token()
        .await
        .unwrap();

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

#[tokio::test]
#[pubky_testnet::test]
async fn get_signup_token() {
    // 1. Start a test homeserver with closed signups (i.e. signup tokens required)
    let mut config = ConfigToml::default_test_config();
    config.general.signup_mode = SignupMode::TokenRequired;

    let testnet = EphemeralTestnet::builder()
        .config(config)
        .build()
        .await
        .unwrap();

    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let client = PubkyHttpClient::new().unwrap();
    let base_url = server.icann_http_url();

    // 2. GET /signup_tokens/invalid-format → 400
    let response = client
        .request(Method::GET, &format!("{}signup_tokens/invalid", base_url))
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Invalid token format should return 400"
    );

    // 3. GET /signup_tokens/AAAA-BBBB-CCCC (nonexistent) → 404
    let response = client
        .request(
            Method::GET,
            &format!("{}signup_tokens/AAAA-BBBB-CCCC", base_url),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "Nonexistent token should return 404"
    );

    // 4. Generate valid token via admin API
    let valid_token = server
        .admin_server()
        .expect("admin server should be enabled")
        .create_signup_token()
        .await
        .unwrap();

    // 5. GET /signup_tokens/<valid> → 200 with status: "valid"
    let response = client
        .request(
            Method::GET,
            &format!("{}signup_tokens/{}", base_url, valid_token),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Valid token should return 200"
    );
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|v| v.to_str().ok()),
        Some("no-store"),
        "Response should have Cache-Control: no-store header"
    );
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        body["status"], "valid",
        "Unused token should have status 'valid'"
    );
    assert!(
        body["created_at"].is_string(),
        "Response should have created_at field"
    );

    // 6. Use token with POST /signup
    let signer = pubky.signer(Keypair::random());
    let _session = signer
        .signup(&server.public_key(), Some(&valid_token))
        .await
        .unwrap();

    // 7. GET /signup_tokens/<used> → 200 with status: "used"
    let response = client
        .request(
            Method::GET,
            &format!("{}signup_tokens/{}", base_url, valid_token),
        )
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Used token should still return 200"
    );
    let body: serde_json::Value = response.json().await.unwrap();
    assert_eq!(
        body["status"], "used",
        "Used token should have status 'used'"
    );
    assert!(
        body["created_at"].is_string(),
        "Response should have created_at field"
    );
}

// This test verifies that when a signin happens immediately after signup,
// the record is not republished on signin (its timestamp remains unchanged)
// but when a signin happens after the record is "old" (in test, after 1 second),
// the record is republished (its timestamp increases).
#[tokio::test]
#[pubky_testnet::test]
async fn republish_if_stale_triggers_timestamp_bump() {
    use std::time::Duration;

    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
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

    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
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

// =====================================================================
// JWT (grant + Proof-of-Possession) auth flow tests
// =====================================================================
//
// These exercise the full client_id / cpk pipeline: PubkyJwtAuthFlow generates a client keypair, the signer
// (Ring) signs a `pubky-grant` JWS, the homeserver mints an Access JWT, and
// the SDK transparently attaches `Authorization: Bearer ...` on every
// subsequent request.

#[tokio::test]
#[pubky_testnet::test]
async fn authz_jwt_happy_path() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let http_relay_url = testnet.http_relay().local_link_url();

    // 1. Signer (Ring) creates the user via the legacy signup path.
    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();

    // 2. Third-party app starts an auth flow with a `client_id` — this
    //    uses PubkyJwtAuthFlow (grant + JWT mode) which emits a deep link
    //    with `cid` and `cpk` query params.
    let caps = Capabilities::builder()
        .read_write("/pub/pubky.app/")
        .read("/pub/foo.bar/file")
        .finish();
    let app_kp = Keypair::random();
    let auth = PubkyJwtAuthFlow::builder(
        &caps,
        AuthFlowKind::signin(),
        ClientId::new("test.app").unwrap(),
    )
    .relay(http_relay_url)
    .client(pubky.client().clone())
    .client_keypair(app_kp.clone())
    .start()
    .unwrap();

    // The deep link should advertise both `cid` and `cpk`.
    let url = auth.authorization_url();
    let query_string = url.query().unwrap_or_default();
    assert!(
        query_string.contains("cid=test.app"),
        "deep link must contain cid: {query_string}"
    );
    assert!(
        query_string.contains(&format!("cpk={}", app_kp.public_key().z32())),
        "deep link must contain cpk: {query_string}"
    );

    // 3. Signer approves — produces a `pubky-grant` JWS.
    signer
        .approve_auth(&auth.authorization_url())
        .await
        .unwrap();

    // 4. App receives the grant and exchanges it for a JWT session.
    let session = auth.await_approval().await.unwrap();
    assert_eq!(session.info().public_key(), &signer.public_key());

    // 5. JWT-backed storage operations work — bearer header is attached.
    session
        .storage()
        .put("/pub/pubky.app/foo", Vec::<u8>::new())
        .await
        .unwrap();

    // 6. Out-of-scope writes are rejected by the server's authorization layer.
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
async fn jwt_proactive_refresh_produces_fresh_token() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let http_relay_url = testnet.http_relay().local_link_url();

    // Sign up + obtain a JWT-backed session.
    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();

    let caps = Capabilities::builder().cap(Capability::root()).finish();
    let auth = PubkyJwtAuthFlow::builder(
        &caps,
        AuthFlowKind::signin(),
        ClientId::new("refresh.test").unwrap(),
    )
    .relay(http_relay_url)
    .client(pubky.client().clone())
    .start()
    .unwrap();
    signer
        .approve_auth(&auth.authorization_url())
        .await
        .unwrap();
    let session = auth.await_approval().await.unwrap();

    let token_before = session.as_jwt().unwrap().current_bearer().await;

    // Force a refresh and verify the JWT changed.
    session.as_jwt().unwrap().force_refresh().await.unwrap();
    let token_after = session.as_jwt().unwrap().current_bearer().await;

    assert_ne!(
        token_before, token_after,
        "force_refresh must mint a new bearer token"
    );

    // The refreshed session continues to work for storage ops.
    session
        .storage()
        .put("/pub/refresh.test/hello", b"world".to_vec())
        .await
        .unwrap();
}

#[tokio::test]
#[pubky_testnet::test]
async fn jwt_signout_kills_grant() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let http_relay_url = testnet.http_relay().local_link_url();

    // Sign up so the user exists.
    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();

    // Authorise app A with root caps so we can use it to inspect grants below.
    let session_a = jwt_signin_helper(
        &pubky,
        &signer,
        http_relay_url.clone(),
        "app.a",
        Capabilities::builder().cap(Capability::root()).finish(),
    )
    .await;

    // Authorise app B with a scoped capability — distinct grant on the server.
    let session_b = jwt_signin_helper(
        &pubky,
        &signer,
        http_relay_url,
        "app.b",
        Capabilities::builder().read_write("/pub/app.b/").finish(),
    )
    .await;

    // Both sessions should appear in the root session's grant list.
    let grants_before = session_a.as_jwt().unwrap().list_grants().await.unwrap();
    assert!(
        grants_before.len() >= 2,
        "expected at least 2 grants, got {}",
        grants_before.len()
    );

    // Sign out app B — this revokes its grant.
    session_b.signout().await.unwrap();

    // App A is unaffected.
    session_a
        .storage()
        .put("/pub/app.a/keepalive", Vec::<u8>::new())
        .await
        .unwrap();

    // The revoked grant no longer shows up in the active list.
    let grants_after = session_a.as_jwt().unwrap().list_grants().await.unwrap();
    assert!(
        grants_after.len() < grants_before.len(),
        "signout should drop the revoked grant from list_grants()"
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn jwt_list_and_revoke_grants_root_only() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let http_relay_url = testnet.http_relay().local_link_url();

    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();

    // Root session — used as the management surface.
    let root_session = jwt_signin_helper(
        &pubky,
        &signer,
        http_relay_url.clone(),
        "root.app",
        Capabilities::builder().cap(Capability::root()).finish(),
    )
    .await;

    // Scoped session — the one we'll revoke.
    let scoped_session = jwt_signin_helper(
        &pubky,
        &signer,
        http_relay_url,
        "scoped.app",
        Capabilities::builder()
            .read_write("/pub/scoped.app/")
            .finish(),
    )
    .await;

    // The scoped session can write inside its scope before revocation.
    scoped_session
        .storage()
        .put("/pub/scoped.app/before", Vec::<u8>::new())
        .await
        .unwrap();

    // Non-root sessions cannot enumerate grants — homeserver returns 403.
    let err = scoped_session
        .as_jwt()
        .unwrap()
        .list_grants()
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::FORBIDDEN),
        "non-root list_grants must be forbidden, got {err:?}"
    );

    // Root can list grants — both sessions appear.
    let grants = root_session.as_jwt().unwrap().list_grants().await.unwrap();
    assert!(
        grants.len() >= 2,
        "expected ≥2 grants, got {}",
        grants.len()
    );

    let scoped_gid = scoped_session.as_jwt().unwrap().grant_id().await;
    assert!(
        grants.iter().any(|g| g.grant_id == scoped_gid),
        "scoped grant id should be present in list"
    );

    // Root revokes the scoped grant.
    root_session
        .as_jwt()
        .unwrap()
        .revoke_grant(&scoped_gid)
        .await
        .unwrap();

    // Scoped session's bearer JWT is now invalid — the next request gets 401.
    let err = scoped_session
        .storage()
        .put("/pub/scoped.app/after", Vec::<u8>::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::UNAUTHORIZED),
        "revoked grant must yield 401, got {err:?}"
    );

    // Root keeps working.
    root_session
        .storage()
        .put("/pub/root.app/still-here", Vec::<u8>::new())
        .await
        .unwrap();
}

#[tokio::test]
#[pubky_testnet::test]
async fn jwt_signup_grant_flow() {
    use pubky_testnet::pubky_homeserver::SignupMode;
    use pubky_testnet::{pubky_homeserver::ConfigToml, EphemeralTestnet};

    // Open signups make this test self-contained.
    let mut config = ConfigToml::default_test_config();
    config.general.signup_mode = SignupMode::Open;
    let testnet = EphemeralTestnet::builder()
        .with_http_relay()
        .config(config)
        .build()
        .await
        .unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let http_relay_url = testnet.http_relay().local_link_url();

    // App initiates a signup-shaped flow with grant binding.
    let caps = Capabilities::builder()
        .read_write("/pub/signup.app/")
        .finish();
    let auth = PubkyJwtAuthFlow::builder(
        &caps,
        AuthFlowKind::signup(server.public_key(), None),
        ClientId::new("signup.app").unwrap(),
    )
    .relay(http_relay_url)
    .client(pubky.client().clone())
    .start()
    .unwrap();

    // Signer approves — same flow as signin, but the session call hits
    // /auth/jwt/signup which creates the user.
    let signer = pubky.signer(Keypair::random());
    signer
        .approve_auth(&auth.authorization_url())
        .await
        .unwrap();

    // For grant-based signup the SDK posts directly to the homeserver host, so the
    // user's `_pubky` PKARR record doesn't exist yet. The signer (which holds the
    // user keypair) publishes it now so subsequent storage requests can resolve
    // the user's homeserver.
    signer
        .pkdns()
        .publish_homeserver_force(Some(&server.public_key()))
        .await
        .unwrap();

    let session = auth.await_approval().await.unwrap();
    assert_eq!(session.info().public_key(), &signer.public_key());

    // The freshly created user can write inside the grant's scope.
    session
        .storage()
        .put("/pub/signup.app/welcome", b"hello".to_vec())
        .await
        .unwrap();
}

/// Helper: run a full grant-based signin flow for a previously signed-up
/// user, returning the resulting JWT session.
async fn jwt_signin_helper(
    pubky: &pubky_testnet::pubky::Pubky,
    signer: &pubky_testnet::pubky::PubkySigner,
    relay: url::Url,
    client_id: &'static str,
    caps: Capabilities,
) -> PubkySession {
    let auth = PubkyJwtAuthFlow::builder(
        &caps,
        AuthFlowKind::signin(),
        ClientId::new(client_id).unwrap(),
    )
    .relay(relay)
    .client(pubky.client().clone())
    .start()
    .unwrap();
    signer
        .approve_auth(&auth.authorization_url())
        .await
        .unwrap();
    auth.await_approval().await.unwrap()
}

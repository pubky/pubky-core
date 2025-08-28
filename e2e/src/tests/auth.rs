use pkarr::Keypair;
use pubky_testnet::pubky::{AuthFlow, KeyedAgent, KeylessAgent, PubkyAuth};
use pubky_testnet::pubky_common::capabilities::{Capabilities, Capability};
use pubky_testnet::{
    pubky_homeserver::{MockDataDir, SignupMode},
    EphemeralTestnet, Testnet,
};
use reqwest::StatusCode;
use std::time::Duration;

use pubky_testnet::pubky::errors::{Error, RequestError};

#[tokio::test]
async fn basic_authn() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();

    let user = KeyedAgent::random().unwrap(); // Lazy constructor uses our global testnet pubky client

    user.signup(&server.public_key(), None).await.unwrap();

    let session = user.session().await.unwrap().unwrap();

    assert!(session.capabilities().contains(&Capability::root()));

    user.signout().await.unwrap();

    {
        let session = user.session().await.unwrap();

        assert!(session.is_none());
    }

    user.signin().await.unwrap();

    {
        let session = user.session().await.unwrap().unwrap();

        assert_eq!(session.pubky(), &user.pubky().unwrap());
        assert!(session.capabilities().contains(&Capability::root()));
    }
}

// #[tokio::test]
// async fn disabled_user() {
//     let testnet = EphemeralTestnet::start().await.unwrap();
//     let server = testnet.homeserver();

//     let client = testnet.pubky_client().unwrap();

//     let keypair = Keypair::random();
//     let pubky = keypair.public_key();

//     // Create a new user
//     client
//         .signup(&keypair, &server.public_key(), None)
//         .await
//         .unwrap();

//     // Create a test file to make sure the user can write to their account
//     let file_url = format!("pubky://{pubky}/pub/pubky.app/foo");
//     client
//         .put(file_url.clone())
//         .body(vec![])
//         .send()
//         .await
//         .unwrap()
//         .error_for_status()
//         .unwrap();

//     // Make sure the user can read their own file
//     let response = client.get(file_url.clone()).send().await.unwrap();
//     assert_eq!(
//         response.status(),
//         StatusCode::OK,
//         "User should be able to read their own file"
//     );

//     let admin_socket = server.admin().listen_socket();
//     let admin_client = reqwest::Client::new();

//     // Disable the user
//     let response = admin_client
//         .post(format!("http://{admin_socket}/users/{pubky}/disable"))
//         .header("X-Admin-Password", "admin")
//         .send()
//         .await
//         .unwrap();
//     assert_eq!(response.status(), StatusCode::OK);

//     // Make sure the user can still read their own file
//     let response = client.get(file_url.clone()).send().await.unwrap();
//     assert_eq!(response.status(), StatusCode::OK);

//     // Make sure the user cannot write a new file
//     let response = client
//         .put(file_url.clone())
//         .body(vec![])
//         .send()
//         .await
//         .unwrap();
//     assert_eq!(response.status(), StatusCode::FORBIDDEN);

//     // Make sure the user can still sign in
//     client
//         .signin(&keypair)
//         .await
//         .expect("Signin should succeed");
// }

#[tokio::test]
async fn authz() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();

    let http_relay_url = testnet.http_relay().local_link_url();

    // Third-party app (keyless)
    let caps = Capabilities::builder()
        .rw("/pub/pubky.app/")
        .read("/pub/foo.bar/file")
        .finish();

    let auth_flow = AuthFlow::new(Some(http_relay_url), &caps).unwrap();
    let pubkyauth_url = auth_flow.pubkyauth_url().clone(); // needed by signer, show QR or deep-link

    // Start long-poll + signin now; this consumes the flow
    let agent_task = tokio::spawn(async move { auth_flow.into_agent().await });

    // Signer authenticator
    let signer = KeyedAgent::random().unwrap();
    signer.signup(&server.public_key(), None).await.unwrap();
    signer.send_auth_token(&pubkyauth_url).await.unwrap();

    // Retrieve the session-bound keyless agent
    let user = agent_task.await.unwrap().unwrap();

    assert_eq!(user.pubky().unwrap(), signer.pubky().unwrap());

    let session = user.session().await.unwrap().unwrap();
    assert_eq!(session.capabilities(), &caps.0);

    // Ensure the same user pubky has been authed on the keyless app from cold keypair
    assert_eq!(user.pubky(), signer.pubky());

    // Access control enforcement
    user.homeserver()
        .put("/pub/pubky.app/foo", Vec::<u8>::new())
        .await
        .unwrap();

    let err = user
        .homeserver()
        .put("/pub/pubky.app", Vec::<u8>::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::FORBIDDEN)
    );

    let err = user
        .homeserver()
        .put("/pub/foo.bar/file", Vec::<u8>::new())
        .await
        .unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::FORBIDDEN)
    );
}

// #[tokio::test]
// async fn multiple_users() {
//     let testnet = EphemeralTestnet::start().await.unwrap();
//     let server = testnet.homeserver();

//     let client = testnet.pubky_client().unwrap();

//     let first_keypair = Keypair::random();
//     let second_keypair = Keypair::random();

//     client
//         .signup(&first_keypair, &server.public_key(), None)
//         .await
//         .unwrap();

//     client
//         .signup(&second_keypair, &server.public_key(), None)
//         .await
//         .unwrap();

//     let session = client
//         .session(&first_keypair.public_key())
//         .await
//         .unwrap()
//         .unwrap();

//     assert_eq!(session.pubky(), &first_keypair.public_key());
//     assert!(session.capabilities().contains(&Capability::root()));

//     let session = client
//         .session(&second_keypair.public_key())
//         .await
//         .unwrap()
//         .unwrap();

//     assert_eq!(session.pubky(), &second_keypair.public_key());
//     assert!(session.capabilities().contains(&Capability::root()));
// }

// #[tokio::test]
// async fn authz_timeout_reconnect() {
//     let testnet = EphemeralTestnet::start().await.unwrap();
//     let server = testnet.homeserver();

//     let http_relay = testnet.http_relay();
//     let http_relay_url = http_relay.local_link_url();

//     let keypair = Keypair::random();
//     let pubky = keypair.public_key();

//     // Third party app side
//     let capabilities: Capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r".try_into().unwrap();

//     let client = testnet
//         .pubky_client_builder()
//         .request_timeout(Duration::from_millis(1000))
//         .build()
//         .unwrap();

//     let pubky_auth_request = client.auth_request(http_relay_url, &capabilities).unwrap();

//     // Authenticator side
//     {
//         let url = pubky_auth_request.url().clone();

//         let client = testnet.pubky_client().unwrap();
//         client
//             .signup(&keypair, &server.public_key(), None)
//             .await
//             .unwrap();

//         tokio::spawn(async move {
//             tokio::time::sleep(Duration::from_millis(1000)).await;
//             // loop {
//             client.send_auth_token(&keypair, &url).await.unwrap();
//             //     }
//         });
//     }

//     let public_key = pubky_auth_request.response().await.unwrap();

//     assert_eq!(&public_key, &pubky);

//     let session = client.session(&pubky).await.unwrap().unwrap();
//     assert_eq!(session.capabilities(), &capabilities.0);

//     // Test access control enforcement

//     client
//         .put(format!("pubky://{pubky}/pub/pubky.app/foo"))
//         .body(vec![])
//         .send()
//         .await
//         .unwrap()
//         .error_for_status()
//         .unwrap();

//     assert_eq!(
//         client
//             .put(format!("pubky://{pubky}/pub/pubky.app"))
//             .body(vec![])
//             .send()
//             .await
//             .unwrap()
//             .status(),
//         StatusCode::FORBIDDEN
//     );

//     assert_eq!(
//         client
//             .put(format!("pubky://{pubky}/pub/foo.bar/file"))
//             .body(vec![])
//             .send()
//             .await
//             .unwrap()
//             .status(),
//         StatusCode::FORBIDDEN
//     );
// }

#[tokio::test]
async fn test_signup_with_token() {
    // 1. Start a test homeserver with closed signups (i.e. signup tokens required)
    let mut testnet = Testnet::new().await.unwrap();
    let user = KeyedAgent::random().unwrap();
    let user2 = KeyedAgent::random().unwrap();

    let mut mock_dir = MockDataDir::test();
    mock_dir.config_toml.general.signup_mode = SignupMode::TokenRequired;
    let server = testnet.create_homeserver_with_mock(mock_dir).await.unwrap();

    // 2. Try to signup with an invalid token "AAAAA" and expect failure.
    let invalid_signup = user
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
    let session = user
        .signup(&server.public_key(), Some(&valid_token))
        .await
        .unwrap();
    assert!(
        !session.pubky().to_string().is_empty(),
        "Session should contain a valid public key"
    );

    // 5. Finally, sign in with the same keypair and verify that a session is returned.
    let signin_session = user.signin().await.unwrap();
    assert_eq!(
        signin_session.pubky(),
        &user.pubky().unwrap(),
        "Signed-in session should correspond to the same public key"
    );

    // 6. Signup with the same token again and expect failure.
    let signup_again = user2.signup(&server.public_key(), Some(&valid_token)).await;
    let err = signup_again.expect_err("Signup with an already used token should fail");
    assert!(err.to_string().contains("401"));
    assert!(err.to_string().contains("Token already used"));
}

// // This test verifies that when a signin happens immediately after signup,
// // the record is not republished on signin (its timestamp remains unchanged)
// // but when a signin happens after the record is “old” (in test, after 1 second),
// // the record is republished (its timestamp increases).
// #[tokio::test]
// async fn test_republish_on_signin_old_enough() {
//     // Setup the testnet and run a homeserver.
//     let testnet = EphemeralTestnet::start().await.unwrap();
//     // Create a client that will republish conditionally if a record is older than 1ms.
//     let client = testnet
//         .pubky_client_builder()
//         .max_record_age(Duration::from_millis(1))
//         .build()
//         .unwrap();

//     let server = testnet.homeserver();
//     let keypair = Keypair::random();

//     // Signup publishes a new record.
//     client
//         .signup(&keypair, &server.public_key(), None)
//         .await
//         .unwrap();
//     // Resolve the record and get its timestamp.
//     let record1 = client
//         .pkarr()
//         .resolve_most_recent(&keypair.public_key())
//         .await
//         .unwrap();
//     let ts1 = record1.timestamp().as_u64();

//     // Immediately sign in. This should update the record
//     // with PublishStrategy::IfOlderThan.
//     client.signin_and_publish(&keypair).await.unwrap();

//     let record2 = client
//         .pkarr()
//         .resolve_most_recent(&keypair.public_key())
//         .await
//         .unwrap();
//     let ts2 = record2.timestamp().as_u64();

//     // Because the signin happened after max_age(Duration::from_millis(1)),
//     // the record should have been republished.
//     assert_ne!(
//         ts1, ts2,
//         "Record was not republished after threshold exceeded"
//     );
// }

// // This test verifies that when a signin happens immediately after signup,
// // the record is not republished on signin (its timestamp remains unchanged)
// // but when a signin happens after the record is “old” (in test, after 1 second),
// // the record is republished (its timestamp increases).
// #[tokio::test]
// async fn test_republish_on_signin_not_old_enough() {
//     // Setup the testnet and run a homeserver.
//     let testnet = EphemeralTestnet::start().await.unwrap();
//     // Create a client that will republish conditionally if a record is older than 1hr.
//     let client = testnet.pubky_client().unwrap();

//     let server = testnet.homeserver();
//     let keypair = Keypair::random();

//     // Signup publishes a new record.
//     client
//         .signup(&keypair, &server.public_key(), None)
//         .await
//         .unwrap();
//     // Resolve the record and get its timestamp.
//     let record1 = client
//         .pkarr()
//         .resolve_most_recent(&keypair.public_key())
//         .await
//         .unwrap();
//     let ts1 = record1.timestamp().as_u64();

//     // Immediately sign in. This updates the record
//     // with PublishStrategy::IfOlderThan.
//     client.signin_and_publish(&keypair).await.unwrap();

//     let record2 = client
//         .pkarr()
//         .resolve_most_recent(&keypair.public_key())
//         .await
//         .unwrap();
//     let ts2 = record2.timestamp().as_u64();

//     // Because the record is fresh (less than 1 second old in our test configuration),
//     // the background task should not republish it. The timestamp should remain the same.
//     assert_eq!(
//         ts1, ts2,
//         "Record republished too early; timestamps should be equal"
//     );
// }

// #[tokio::test]
// async fn test_republish_homeserver() {
//     // Setup the testnet and run a homeserver.
//     let mut testnet = Testnet::new().await.unwrap();
//     let max_record_age = Duration::from_secs(5);

//     // Create a client that will republish conditionally if a record is older than 1 second
//     let client = testnet
//         .pubky_client_builder()
//         .max_record_age(max_record_age)
//         .build()
//         .unwrap();
//     let server = testnet.create_homeserver().await.unwrap();
//     let keypair = Keypair::random();

//     // Signup publishes a new record.
//     client
//         .signup(&keypair, &server.public_key(), None)
//         .await
//         .unwrap();
//     // Resolve the record and get its timestamp.
//     let record1 = client
//         .pkarr()
//         .resolve_most_recent(&keypair.public_key())
//         .await
//         .unwrap();
//     let ts1 = record1.timestamp().as_u64();

//     // Immediately call republish_homeserver.
//     // Since the record is fresh, republish should do nothing.
//     client
//         .republish_homeserver(&keypair, &server.public_key())
//         .await
//         .unwrap();
//     let record2 = client
//         .pkarr()
//         .resolve_most_recent(&keypair.public_key())
//         .await
//         .unwrap();
//     let ts2 = record2.timestamp().as_u64();
//     assert_eq!(
//         ts1, ts2,
//         "Record republished too early; timestamp should be equal"
//     );

//     // Wait long enough for the record to be considered 'old'.
//     tokio::time::sleep(max_record_age).await;
//     // Call republish_homeserver again; now the record should be updated.
//     client
//         .republish_homeserver(&keypair, &server.public_key())
//         .await
//         .unwrap();
//     let record3 = client
//         .pkarr()
//         .resolve_most_recent(&keypair.public_key())
//         .await
//         .unwrap();
//     let ts3 = record3.timestamp().as_u64();
//     assert!(
//         ts3 > ts2,
//         "Record was not republished after threshold exceeded"
//     );
// }

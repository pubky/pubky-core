use crate::Testnet;
use pkarr::Keypair;
use pubky_common::capabilities::{Capabilities, Capability};
use reqwest::StatusCode;
use std::time::Duration;

#[tokio::test]
async fn basic_authn() {
    let testnet = Testnet::run().await.unwrap();
    let server = testnet.run_homeserver().await.unwrap();

    let client = testnet.client_builder().build().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let session = client
        .session(&keypair.public_key())
        .await
        .unwrap()
        .unwrap();

    assert!(session.capabilities().contains(&Capability::root()));

    client.signout(&keypair.public_key()).await.unwrap();

    {
        let session = client.session(&keypair.public_key()).await.unwrap();

        assert!(session.is_none());
    }

    client.signin(&keypair).await.unwrap();

    {
        let session = client
            .session(&keypair.public_key())
            .await
            .unwrap()
            .unwrap();

        assert_eq!(session.pubky(), &keypair.public_key());
        assert!(session.capabilities().contains(&Capability::root()));
    }
}

#[tokio::test]
async fn authz() {
    let testnet = Testnet::run().await.unwrap();
    let server = testnet.run_homeserver().await.unwrap();

    let http_relay = testnet.run_http_relay().await.unwrap();
    let http_relay_url = http_relay.local_link_url();

    let keypair = Keypair::random();
    let pubky = keypair.public_key();

    // Third party app side
    let capabilities: Capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r".try_into().unwrap();

    let client = testnet.client_builder().build().unwrap();

    let pubky_auth_request = client.auth_request(http_relay_url, &capabilities).unwrap();

    // Authenticator side
    {
        let client = testnet.client_builder().build().unwrap();

        client
            .signup(&keypair, &server.public_key(), None)
            .await
            .unwrap();

        client
            .send_auth_token(&keypair, pubky_auth_request.url())
            .await
            .unwrap();
    }

    let public_key = pubky_auth_request.response().await.unwrap();

    assert_eq!(&public_key, &pubky);

    let session = client.session(&pubky).await.unwrap().unwrap();
    assert_eq!(session.capabilities(), &capabilities.0);

    // Test access control enforcement

    client
        .put(format!("pubky://{pubky}/pub/pubky.app/foo"))
        .body(vec![])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    assert_eq!(
        client
            .put(format!("pubky://{pubky}/pub/pubky.app"))
            .body(vec![])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    assert_eq!(
        client
            .put(format!("pubky://{pubky}/pub/foo.bar/file"))
            .body(vec![])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn multiple_users() {
    let testnet = Testnet::run().await.unwrap();
    let server = testnet.run_homeserver().await.unwrap();

    let client = testnet.client_builder().build().unwrap();

    let first_keypair = Keypair::random();
    let second_keypair = Keypair::random();

    client
        .signup(&first_keypair, &server.public_key(), None)
        .await
        .unwrap();

    client
        .signup(&second_keypair, &server.public_key(), None)
        .await
        .unwrap();

    let session = client
        .session(&first_keypair.public_key())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(session.pubky(), &first_keypair.public_key());
    assert!(session.capabilities().contains(&Capability::root()));

    let session = client
        .session(&second_keypair.public_key())
        .await
        .unwrap()
        .unwrap();

    assert_eq!(session.pubky(), &second_keypair.public_key());
    assert!(session.capabilities().contains(&Capability::root()));
}

#[tokio::test]
async fn authz_timeout_reconnect() {
    let testnet = Testnet::run().await.unwrap();
    let server = testnet.run_homeserver().await.unwrap();

    let http_relay = testnet.run_http_relay().await.unwrap();
    let http_relay_url = http_relay.local_link_url();

    let keypair = Keypair::random();
    let pubky = keypair.public_key();

    // Third party app side
    let capabilities: Capabilities = "/pub/pubky.app/:rw,/pub/foo.bar/file:r".try_into().unwrap();

    let client = testnet
        .client_builder()
        .request_timeout(Duration::from_millis(1000))
        .build()
        .unwrap();

    let pubky_auth_request = client.auth_request(http_relay_url, &capabilities).unwrap();

    // Authenticator side
    {
        let url = pubky_auth_request.url().clone();

        let client = testnet.client_builder().build().unwrap();
        client
            .signup(&keypair, &server.public_key(), None)
            .await
            .unwrap();

        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(400)).await;
            // loop {
            client.send_auth_token(&keypair, &url).await.unwrap();
            //     }
        });
    }

    let public_key = pubky_auth_request.response().await.unwrap();

    assert_eq!(&public_key, &pubky);

    let session = client.session(&pubky).await.unwrap().unwrap();
    assert_eq!(session.capabilities(), &capabilities.0);

    // Test access control enforcement

    client
        .put(format!("pubky://{pubky}/pub/pubky.app/foo"))
        .body(vec![])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    assert_eq!(
        client
            .put(format!("pubky://{pubky}/pub/pubky.app"))
            .body(vec![])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );

    assert_eq!(
        client
            .put(format!("pubky://{pubky}/pub/foo.bar/file"))
            .body(vec![])
            .send()
            .await
            .unwrap()
            .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn test_signup_with_token() {
    // 1. Start a test homeserver with closed signups (i.e. signup tokens required)
    let testnet = Testnet::run().await.unwrap();
    let server = testnet.run_homeserver_with_signup_tokens().await.unwrap();

    let admin_password = "admin";

    let client = testnet.client_builder().build().unwrap();
    let keypair = Keypair::random();

    // 2. Try to signup with an invalid token "AAAAA" and expect failure.
    let invalid_signup = client
        .signup(&keypair, &server.public_key(), Some("AAAA-BBBB-CCCC"))
        .await;
    assert!(
        invalid_signup.is_err(),
        "Signup should fail with an invalid signup token"
    );

    // 3. Call the admin endpoint to generate a valid signup token.
    //    The admin endpoint is protected via the header "X-Admin-Password"
    //    and the password we set up above.
    let admin_url = format!(
        "https://{}/admin/generate_signup_token",
        server.public_key()
    );

    // 3.1. Call the admin endpoint *with a WRONG admin password* to ensure we get 401 UNAUTHORIZED.
    let wrong_password_response = client
        .get(&admin_url)
        .header("X-Admin-Password", "wrong_admin_password")
        .send()
        .await
        .unwrap();
    assert_eq!(
        wrong_password_response.status(),
        StatusCode::UNAUTHORIZED,
        "Wrong admin password should return 401"
    );

    // 3.1 Now call the admin endpoint again, this time with the correct password.
    let admin_response = client
        .get(&admin_url)
        .header("X-Admin-Password", admin_password)
        .send()
        .await
        .unwrap();
    assert_eq!(
        admin_response.status(),
        StatusCode::OK,
        "Admin endpoint should return OK"
    );
    let valid_token = admin_response.text().await.unwrap(); // The token string.

    // 4. Now signup with the valid token. Expect success and a session back.
    let session = client
        .signup(&keypair, &server.public_key(), Some(&valid_token))
        .await
        .unwrap();
    assert!(
        !session.pubky().to_string().is_empty(),
        "Session should contain a valid public key"
    );

    // 5. Finally, sign in with the same keypair and verify that a session is returned.
    let signin_session = client.signin(&keypair).await.unwrap();
    assert_eq!(
        signin_session.pubky(),
        &keypair.public_key(),
        "Signed-in session should correspond to the same public key"
    );
}

// This test verifies that when a signin happens immediately after signup,
// the record is not republished on signin (its timestamp remains unchanged)
// but when a signin happens after the record is “old” (in test, after 1 second),
// the record is republished (its timestamp increases).
#[tokio::test]
async fn test_republish_on_signin() {
    // Setup the testnet and run a homeserver.
    let testnet = Testnet::run().await.unwrap();
    let server = testnet.run_homeserver().await.unwrap();
    // Create a client that will republish conditionally if a record is older than 1 second
    let client = testnet
        .client_builder()
        .max_record_age(Duration::from_secs(1))
        .build()
        .unwrap();
    let keypair = Keypair::random();

    // Signup publishes a new record.
    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();
    // Resolve the record and get its timestamp.
    let record1 = client
        .pkarr()
        .resolve_most_recent(&keypair.public_key())
        .await
        .unwrap();
    let ts1 = record1.timestamp().as_u64();

    // Immediately sign in. This spawns a background task to update the record
    // with PublishStrategy::IfOlderThan.
    client.signin(&keypair).await.unwrap();
    // Wait a short time to let the background task complete.
    tokio::time::sleep(Duration::from_millis(5)).await;
    let record2 = client
        .pkarr()
        .resolve_most_recent(&keypair.public_key())
        .await
        .unwrap();
    let ts2 = record2.timestamp().as_u64();

    // Because the record is fresh (less than 1 second old in our test configuration),
    // the background task should not republish it. The timestamp should remain the same.
    assert_eq!(
        ts1, ts2,
        "Record republished too early; timestamps should be equal"
    );

    // Wait long enough for the record to be considered 'old' (greater than 1 second).
    tokio::time::sleep(Duration::from_secs(1)).await;
    // Sign in again. Now the background task should trigger a republish.
    client.signin(&keypair).await.unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    let record3 = client
        .pkarr()
        .resolve_most_recent(&keypair.public_key())
        .await
        .unwrap();
    let ts3 = record3.timestamp().as_u64();

    // Now the republished record's timestamp should be greater than before.
    assert!(
        ts3 > ts2,
        "Record was not republished after threshold exceeded"
    );
}

#[tokio::test]
async fn test_republish_homeserver() {
    // Setup the testnet and run a homeserver.
    let testnet = Testnet::run().await.unwrap();
    let server = testnet.run_homeserver().await.unwrap();
    // Create a client that will republish conditionally if a record is older than 1 second
    let client = testnet
        .client_builder()
        .max_record_age(Duration::from_secs(1))
        .build()
        .unwrap();
    let keypair = Keypair::random();

    // Signup publishes a new record.
    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();
    // Resolve the record and get its timestamp.
    let record1 = client
        .pkarr()
        .resolve_most_recent(&keypair.public_key())
        .await
        .unwrap();
    let ts1 = record1.timestamp().as_u64();

    // Immediately call republish_homeserver.
    // Since the record is fresh, republish should do nothing.
    client
        .republish_homeserver(&keypair, &server.public_key())
        .await
        .unwrap();
    let record2 = client
        .pkarr()
        .resolve_most_recent(&keypair.public_key())
        .await
        .unwrap();
    let ts2 = record2.timestamp().as_u64();
    assert_eq!(
        ts1, ts2,
        "Record republished too early; timestamp should be equal"
    );

    // Wait long enough for the record to be considered 'old'.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    // Call republish_homeserver again; now the record should be updated.
    client
        .republish_homeserver(&keypair, &server.public_key())
        .await
        .unwrap();
    let record3 = client
        .pkarr()
        .resolve_most_recent(&keypair.public_key())
        .await
        .unwrap();
    let ts3 = record3.timestamp().as_u64();
    assert!(
        ts3 > ts2,
        "Record was not republished after threshold exceeded"
    );
}

use super::*;
use pubky_testnet::pubky::errors::{Error, RequestError};
use pubky_testnet::pubky::{ClientId, PubkySession, PublicKey};

/// Sign up a fresh user and return its public key plus an authenticated
/// (root-capability) grant session.
async fn signed_in_user(
    testnet: &pubky_testnet::EphemeralTestnet,
    client_id: &str,
) -> (PublicKey, PubkySession) {
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();
    let session = signer
        .signin(ClientId::new(client_id).unwrap())
        .await
        .unwrap();
    (signer.public_key(), session)
}

/// Extract the HTTP status from a typed server-rejection error, so tests assert
/// against `RequestError::Server { status, .. }` rather than a raw string.
fn server_status(err: &Error) -> Option<StatusCode> {
    match err {
        Error::Request(RequestError::Server { status, .. }) => Some(*status),
        _ => None,
    }
}

/// Test the SDK builder API: `event_stream_for()` and `add_users()`
/// This tests the high-level SDK interface rather than raw HTTP requests.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_builder_api() {
    use futures::StreamExt;
    use pubky_testnet::pubky::EventCursor;
    use tokio::time::{timeout, Duration};

    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Create three users
    let keypair1 = Keypair::random();
    let keypair2 = Keypair::random();
    let keypair3 = Keypair::random();

    let signer1 = pubky.signer(keypair1);
    let signer2 = pubky.signer(keypair2);
    let signer3 = pubky.signer(keypair3);

    let session1 = signer1
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();
    let session2 = signer2
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();
    let session3 = signer3
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();

    let pubky1 = signer1.public_key();
    let pubky2 = signer2.public_key();
    let pubky3 = signer3.public_key();

    // Create events for each user
    for i in 0..3 {
        session1
            .storage()
            .put(format!("/pub/user1_{i}.txt"), vec![i as u8])
            .await
            .unwrap();
    }
    for i in 0..2 {
        session2
            .storage()
            .put(format!("/pub/user2_{i}.txt"), vec![i as u8])
            .await
            .unwrap();
    }
    for i in 0..4 {
        session3
            .storage()
            .put(format!("/pub/user3_{i}.txt"), vec![i as u8])
            .await
            .unwrap();
    }

    // ==== Test 1: event_stream_for_user() - single user stream ====
    let mut stream = pubky
        .event_stream_for_user(&pubky1, None)
        .limit(10)
        .subscribe()
        .await
        .unwrap();

    let mut user1_events = Vec::new();
    while let Some(result) = stream.next().await {
        let event = result.unwrap();
        user1_events.push(event.resource.path.to_string());
        if user1_events.len() >= 3 {
            break;
        }
    }
    drop(stream);

    assert_eq!(
        user1_events.len(),
        3,
        "event_stream_for_user: Should get 3 events for user1"
    );
    assert!(
        user1_events.iter().all(|p| p.contains("user1_")),
        "event_stream_for_user: All events should be from user1"
    );

    // ==== Test 2: event_stream_for() with add_users() - multi-user stream ====
    let homeserver = server.public_key();

    let mut stream = pubky
        .event_stream_for(&homeserver)
        .add_users([(&pubky1, None), (&pubky2, None)])
        .unwrap()
        .limit(10)
        .subscribe()
        .await
        .unwrap();

    let mut multi_user_events = Vec::new();
    while let Some(result) = stream.next().await {
        let event = result.unwrap();
        multi_user_events.push((event.resource.owner.z32(), event.resource.path.to_string()));
        if multi_user_events.len() >= 5 {
            break;
        }
    }
    drop(stream);

    assert_eq!(
        multi_user_events.len(),
        5,
        "add_users: Should get 5 events total (3 from user1 + 2 from user2)"
    );

    let user1_count = multi_user_events
        .iter()
        .filter(|(u, _)| *u == pubky1.z32())
        .count();
    let user2_count = multi_user_events
        .iter()
        .filter(|(u, _)| *u == pubky2.z32())
        .count();
    let user3_count = multi_user_events
        .iter()
        .filter(|(u, _)| *u == pubky3.z32())
        .count();

    assert_eq!(user1_count, 3, "add_users: Should get 3 events from user1");
    assert_eq!(user2_count, 2, "add_users: Should get 2 events from user2");
    assert_eq!(
        user3_count, 0,
        "add_users: Should get 0 events from user3 (not subscribed)"
    );

    // ==== Test 3: add_users() with per-user cursors ====
    // First, get events and capture cursor at position 2 for user1
    let mut stream = pubky
        .event_stream_for(&homeserver)
        .add_users([(&pubky1, None)])
        .unwrap()
        .limit(3)
        .subscribe()
        .await
        .unwrap();

    let mut cursor_at_2: Option<EventCursor> = None;
    let mut count = 0;
    while let Some(result) = stream.next().await {
        let event = result.unwrap();
        count += 1;
        if count == 2 {
            cursor_at_2 = Some(event.cursor);
        }
        if count >= 3 {
            break;
        }
    }
    drop(stream);

    let cursor = cursor_at_2.expect("Should have captured cursor at position 2");

    // Now subscribe with cursor - should get only 1 remaining event from user1
    let mut stream = pubky
        .event_stream_for(&homeserver)
        .add_users([(&pubky1, Some(cursor)), (&pubky2, None)])
        .unwrap()
        .limit(10)
        .subscribe()
        .await
        .unwrap();

    let mut events_after_cursor = Vec::new();
    while let Some(result) = stream.next().await {
        let event = result.unwrap();
        events_after_cursor.push((event.resource.owner.z32(), event.resource.path.to_string()));
        if events_after_cursor.len() >= 3 {
            break;
        }
    }
    drop(stream);

    assert_eq!(
        events_after_cursor.len(),
        3,
        "Cursor: Should get 3 events (1 from user1 after cursor + 2 from user2)"
    );

    let user1_after = events_after_cursor
        .iter()
        .filter(|(u, _)| *u == pubky1.z32())
        .count();
    let user2_after = events_after_cursor
        .iter()
        .filter(|(u, _)| *u == pubky2.z32())
        .count();

    assert_eq!(
        user1_after, 1,
        "Cursor: Should get 1 event from user1 (after cursor)"
    );
    assert_eq!(
        user2_after, 2,
        "Cursor: Should get 2 events from user2 (no cursor, from beginning)"
    );

    // ==== Test 4: Builder options - reverse, path filter ====
    let mut stream = pubky
        .event_stream_for(&homeserver)
        .add_users([(&pubky1, None)])
        .unwrap()
        .reverse()
        .limit(3)
        .subscribe()
        .await
        .unwrap();

    let mut reverse_events = Vec::new();
    while let Some(result) = stream.next().await {
        let event = result.unwrap();
        reverse_events.push(event.resource.path.to_string());
        if reverse_events.len() >= 3 {
            break;
        }
    }
    drop(stream);

    assert_eq!(reverse_events.len(), 3, "Reverse: Should get 3 events");
    assert!(
        reverse_events[0].contains("user1_2"),
        "Reverse: First should be newest (user1_2), got: {}",
        reverse_events[0]
    );
    assert!(
        reverse_events[2].contains("user1_0"),
        "Reverse: Last should be oldest (user1_0), got: {}",
        reverse_events[2]
    );

    // ==== Test 5: Live mode with add_users() ====
    let mut stream = pubky
        .event_stream_for(&homeserver)
        .add_users([(&pubky1, None), (&pubky2, None)])
        .unwrap()
        .live()
        .subscribe()
        .await
        .unwrap();

    // Consume historical events first (5 total)
    for _ in 0..5 {
        let _ = stream.next().await;
    }

    // Spawn task to create a live event
    let session1_clone = session1.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        session1_clone
            .storage()
            .put("/pub/live_event.txt", vec![99])
            .await
            .unwrap();
    });

    // Should receive the live event
    let result = timeout(Duration::from_secs(5), stream.next()).await;
    assert!(result.is_ok(), "Live: Should receive event within timeout");

    let event = result
        .unwrap()
        .expect("Live: Stream should have event")
        .unwrap();
    assert!(
        event.resource.path.as_str().contains("live_event"),
        "Live: Should receive the live event, got: {}",
        event.resource.path
    );

    // ==== Test 6: Error case - add_users() with >50 users ====
    let many_keys: Vec<_> = (0..51).map(|_| Keypair::random().public_key()).collect();
    let many_refs: Vec<_> = many_keys.iter().map(|k| (k, None)).collect();

    let result = pubky.event_stream_for(&homeserver).add_users(many_refs);

    assert!(
        result.is_err(),
        "add_users: Should error when adding >50 users"
    );
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("50 users"),
        "add_users: Error should mention 50 user limit, got: {}",
        err
    );
}

/// An authenticated owner subscribing via the SDK builder with
/// `.session()` + a `/priv/` path receives that user's private events, scoped
/// to the requested filter.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_private_authorized_receives() {
    use futures::StreamExt;

    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let (user, session) = signed_in_user(&testnet, "sdk-owner.test").await;

    session.storage().put("/pub/a.txt", vec![1]).await.unwrap();
    session
        .storage()
        .put("/priv/app/secret.txt", vec![2])
        .await
        .unwrap();
    session
        .storage()
        .put("/priv/other/z.txt", vec![3])
        .await
        .unwrap();

    // Single-user, authenticated, filtered to `/priv/app/`.
    let mut stream = pubky
        .event_stream_for(&server.public_key())
        .add_users([(&user, None)])
        .unwrap()
        .session(&session)
        .path("/priv/app/")
        .limit(1)
        .subscribe()
        .await
        .unwrap();

    let event = stream
        .next()
        .await
        .expect("should receive the in-scope private event")
        .unwrap();
    assert_eq!(event.resource.owner.z32(), user.z32());
    assert!(
        event
            .resource
            .path
            .as_str()
            .contains("/priv/app/secret.txt"),
        "expected the in-scope private event, got: {}",
        event.resource.path
    );
}

/// A mixed `/pub/` + `/priv/app/` subscription returns the union
/// and never an unrequested private scope.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_private_union_excludes_unrequested() {
    use futures::StreamExt;

    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let (user, session) = signed_in_user(&testnet, "sdk-union.test").await;

    session.storage().put("/pub/a.txt", vec![1]).await.unwrap();
    session
        .storage()
        .put("/priv/app/secret.txt", vec![2])
        .await
        .unwrap();
    session
        .storage()
        .put("/priv/other/z.txt", vec![3])
        .await
        .unwrap();

    let mut stream = pubky
        .event_stream_for(&server.public_key())
        .add_users([(&user, None)])
        .unwrap()
        .session(&session)
        .path("/pub/")
        .path("/priv/app/")
        .limit(2)
        .subscribe()
        .await
        .unwrap();

    let mut paths = Vec::new();
    while let Some(result) = stream.next().await {
        let event = result.unwrap();
        let p = event.resource.path.to_string();
        assert!(
            !p.contains("/priv/other/"),
            "union leaked an unrequested private scope: {p}"
        );
        paths.push(p);
        if paths.len() >= 2 {
            break;
        }
    }
    assert!(paths.iter().any(|p| p.contains("/pub/a.txt")));
    assert!(paths.iter().any(|p| p.contains("/priv/app/secret.txt")));
}

/// A private-path subscription with no session is rejected by the
/// homeserver and surfaced as a typed `401 Unauthorized`.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_private_without_session_is_unauthorized() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let (user, _session) = signed_in_user(&testnet, "sdk-401.test").await;

    // No `.session()` → the homeserver rejects the private path.
    let result = pubky
        .event_stream_for(&server.public_key())
        .add_users([(&user, None)])
        .unwrap()
        .path("/priv/app/")
        .subscribe()
        .await;

    let err = match result {
        Ok(_) => panic!("private path without a session must be rejected"),
        Err(e) => e,
    };
    assert_eq!(
        server_status(&err),
        Some(StatusCode::UNAUTHORIZED),
        "expected a typed 401 Server error, got: {err}"
    );
}

/// A session may not read another user's private events; the homeserver
/// rejection surfaces as a typed `403 Forbidden`.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_private_wrong_user_is_forbidden() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let (_a, session_a) = signed_in_user(&testnet, "sdk-403-a.test").await;
    let (b, _session_b) = signed_in_user(&testnet, "sdk-403-b.test").await;

    // A's session requesting B's private events → 403 (server-enforced).
    let result = pubky
        .event_stream_for(&server.public_key())
        .add_users([(&b, None)])
        .unwrap()
        .session(&session_a)
        .path("/priv/app/")
        .subscribe()
        .await;

    let err = match result {
        Ok(_) => panic!("a session may not read another user's private events"),
        Err(e) => e,
    };
    assert_eq!(
        server_status(&err),
        Some(StatusCode::FORBIDDEN),
        "expected a typed 403 Server error, got: {err}"
    );
}

/// The session credential must never be sent to a homeserver that
/// isn't the session owner's. Pointing the builder at a foreign homeserver
/// fails early, client-side, before any request is issued.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_session_rejects_foreign_homeserver() {
    let testnet = build_full_testnet().await;
    let pubky = testnet.sdk().unwrap();
    let (user, session) = signed_in_user(&testnet, "sdk-leak.test").await;

    // A homeserver pubkey that is NOT the session owner's.
    let foreign_homeserver = Keypair::random().public_key();

    let result = pubky
        .event_stream_for(&foreign_homeserver)
        .add_users([(&user, None)])
        .unwrap()
        .session(&session)
        .path("/priv/app/")
        .subscribe()
        .await;

    let err = match result {
        Ok(_) => panic!("must refuse to send the session credential to a foreign homeserver"),
        Err(e) => e,
    };
    // Rejected client-side (no server was contacted), so there is no HTTP status.
    assert!(
        server_status(&err).is_none(),
        "should fail before contacting any server, got: {err}"
    );
    assert!(
        err.to_string().contains("homeserver"),
        "error should explain the homeserver mismatch, got: {err}"
    );
}

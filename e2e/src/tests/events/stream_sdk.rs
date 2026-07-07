use super::*;
use futures::StreamExt;
use pubky_testnet::pubky::errors::{Error, RequestError};
use pubky_testnet::pubky::{ClientId, EventCursor, PubkySession, PublicKey};
use tokio::time::{timeout, Duration};

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

/// An authenticated owner receives their own private events through the SDK
/// builder: a single `/priv/app/` filter yields only the in-scope event, and a
/// mixed `/pub/` + `/priv/app/` subscription returns the union without leaking
/// an unrequested private scope.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_private_authorized_scoping() {
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

    // A single `/priv/app/` filter yields only the in-scope private event.
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
    drop(stream);

    // A mixed `/pub/` + `/priv/app/` subscription returns the union and never
    // the unrequested `/priv/other/` scope.
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

    let err = result
        .err()
        .expect("private path without a session must be rejected");
    assert_eq!(
        server_status(&err),
        Some(StatusCode::UNAUTHORIZED),
        "expected a typed 401 Server error, got: {err}"
    );
}

/// A session must not read another user's private events. The SDK scopes a
/// private subscription to the credential's own user, so A's session is *not*
/// attached to B's private stream.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_wrong_user_private_stream_is_not_attached() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let (_a, session_a) = signed_in_user(&testnet, "sdk-401-a.test").await;
    let (b, _session_b) = signed_in_user(&testnet, "sdk-401-b.test").await;

    // A's session requesting B's private events: the SDK refuses to attach A's
    // credential to a private stream scoped to B, so the request is anonymous.
    let result = pubky
        .event_stream_for(&server.public_key())
        .add_users([(&b, None)])
        .unwrap()
        .session(&session_a)
        .path("/priv/app/")
        .subscribe()
        .await;

    let err = result
        .err()
        .expect("a session may not read another user's private events");
    assert_eq!(
        server_status(&err),
        Some(StatusCode::UNAUTHORIZED),
        "expected a typed 401 Server error (credential not attached), got: {err}"
    );
}

/// A session must not be attached to a multi-user private subscription
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_multi_user_private_stream_is_not_attached() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let (a, session_a) = signed_in_user(&testnet, "sdk-multi-a.test").await;
    let (b, _session_b) = signed_in_user(&testnet, "sdk-multi-b.test").await;

    // A's session on a subscription naming both A and B with a private path: the
    // SDK won't attach a credential to a multi-user private stream.
    let result = pubky
        .event_stream_for(&server.public_key())
        .add_users([(&a, None), (&b, None)])
        .unwrap()
        .session(&session_a)
        .path("/priv/app/")
        .subscribe()
        .await;

    let err = result
        .err()
        .expect("a multi-user private subscription must not be authenticated");
    assert_eq!(
        server_status(&err),
        Some(StatusCode::UNAUTHORIZED),
        "expected a typed 401 Server error (credential not attached), got: {err}"
    );
}

/// Attaching a session must not hijack the subscription's target.
/// With two homeservers, subscribing to a user on HS2 while holding a session on
/// HS1 still targets HS2 — the session is simply not attached (owner ≠ target).
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_session_does_not_override_target_homeserver() {
    let mut testnet = build_full_testnet().await;
    let hs1 = testnet.homeserver_app().public_key();
    let hs2 = testnet
        .create_random_homeserver()
        .await
        .unwrap()
        .public_key();
    assert_ne!(hs1, hs2, "test needs two distinct homeservers");

    let pubky = testnet.sdk().unwrap();

    // `me` holds a session on HS1.
    let me = pubky.signer(Keypair::random());
    me.signup(&hs1, None).await.unwrap();
    let my_session = me
        .signin(ClientId::new("sdk-override.test").unwrap())
        .await
        .unwrap();

    // `other` lives on HS2 and writes a public event there.
    let other_signer = pubky.signer(Keypair::random());
    other_signer.signup(&hs2, None).await.unwrap();
    let other = other_signer.public_key();
    let other_session = other_signer
        .signin(ClientId::new("sdk-override-other.test").unwrap())
        .await
        .unwrap();
    other_session
        .storage()
        .put("/pub/hello.txt", vec![1])
        .await
        .unwrap();

    // Subscribe to `other`'s public events with MY (HS1) session attached. The
    // builder must resolve `other`'s homeserver (HS2), not mine (HS1), and stream
    // `other`'s event; the mismatched session is silently not attached.
    let mut stream = pubky
        .event_stream_for_user(&other, None)
        .session(&my_session)
        .path("/pub/")
        .limit(1)
        .subscribe()
        .await
        .unwrap();

    let event = stream
        .next()
        .await
        .expect("should receive other's public event from their own homeserver")
        .unwrap();
    assert_eq!(event.resource.owner.z32(), other.z32());
    assert!(
        event.resource.path.as_str().contains("/pub/hello.txt"),
        "expected other's public event, got: {}",
        event.resource.path
    );
}

/// A cookie-backed session authenticates its own single-user private stream
/// (bound to its homeserver at signup); the path filter still excludes an
/// out-of-scope private event.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_cookie_backed_private_authorized() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    let session = signer
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();
    let user = signer.public_key();

    session
        .storage()
        .put("/priv/app/secret.txt", vec![42])
        .await
        .unwrap();
    // An out-of-scope private write that the `/priv/app/` filter must exclude.
    session
        .storage()
        .put("/priv/other/z.txt", vec![7])
        .await
        .unwrap();

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
        .expect("cookie-backed session should receive its in-scope private event")
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
    assert!(
        !event.resource.path.as_str().contains("/priv/other/"),
        "out-of-scope private event leaked: {}",
        event.resource.path
    );
}

/// A cookie bound to HS1 must not grant private access when targeting HS2: the
/// subscribe fails `401`. Outcome-only — HS2 returns `401` whether or not the
/// cookie was sent (it holds no matching session); non-attachment itself is
/// proven by the `can_attach_to` unit tests in the cookie credential.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_cookie_bound_homeserver_is_enforced() {
    let mut testnet = build_full_testnet().await;
    let hs1 = testnet.homeserver_app().public_key();
    let hs2 = testnet
        .create_random_homeserver()
        .await
        .unwrap()
        .public_key();
    assert_ne!(hs1, hs2, "test needs two distinct homeservers");

    let pubky = testnet.sdk().unwrap();

    let me = pubky.signer(Keypair::random());
    let my_session = me.signup_cookie(&hs1, None).await.unwrap();
    let my_user = me.public_key();

    let result = pubky
        .event_stream_for(&hs2)
        .add_users([(&my_user, None)])
        .unwrap()
        .session(&my_session)
        .path("/priv/app/")
        .subscribe()
        .await;

    let err = result
        .err()
        .expect("a cookie bound to HS1 must not authenticate a private stream on HS2");
    assert_eq!(
        server_status(&err),
        Some(StatusCode::UNAUTHORIZED),
        "expected a typed 401 Server error, got: {err}"
    );
}

/// A cookie session for A must not grant private access to a stream scoped to B:
/// the subscribe fails `401`. Outcome-only — B's stream returns `401` whether or
/// not A's cookie was sent; the gate's non-attachment (B ≠ credential user) is
/// proven by the `should_attach_credential` unit tests in the event stream.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_cookie_wrong_user_private_is_not_attached() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer_a = pubky.signer(Keypair::random());
    let session_a = signer_a
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();

    let signer_b = pubky.signer(Keypair::random());
    signer_b
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();
    let b = signer_b.public_key();

    let result = pubky
        .event_stream_for(&server.public_key())
        .add_users([(&b, None)])
        .unwrap()
        .session(&session_a)
        .path("/priv/app/")
        .subscribe()
        .await;

    let err = result
        .err()
        .expect("A's cookie must not authenticate a private stream scoped to B");
    assert_eq!(
        server_status(&err),
        Some(StatusCode::UNAUTHORIZED),
        "expected a typed 401 Server error, got: {err}"
    );
}

/// A public event stream works normally with a cookie session attached
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_public_stream_with_cookie_session_works() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    let session = signer
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();
    let user = signer.public_key();

    session
        .storage()
        .put("/pub/hello.txt", vec![1])
        .await
        .unwrap();

    let mut stream = pubky
        .event_stream_for(&server.public_key())
        .add_users([(&user, None)])
        .unwrap()
        .session(&session)
        .path("/pub/")
        .limit(1)
        .subscribe()
        .await
        .unwrap();

    let event = stream
        .next()
        .await
        .expect("public stream should deliver the public event")
        .unwrap();
    assert_eq!(event.resource.owner.z32(), user.z32());
    assert!(
        event.resource.path.as_str().contains("/pub/hello.txt"),
        "expected the public event, got: {}",
        event.resource.path
    );
}

/// A cookie-backed live private stream receives in-scope private events and the
/// path filter excludes out-of-scope ones.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_sdk_cookie_live_private_receives_in_scope() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    let session = signer
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();
    let user = signer.public_key();

    let mut stream = pubky
        .event_stream_for(&server.public_key())
        .add_users([(&user, None)])
        .unwrap()
        .session(&session)
        .path("/priv/app/")
        .live()
        .subscribe()
        .await
        .unwrap();

    // After subscribing, write an out-of-scope then an in-scope private event.
    let writer = session.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(100)).await;
        writer
            .storage()
            .put("/priv/other/skip.txt", vec![9])
            .await
            .unwrap();
        writer
            .storage()
            .put("/priv/app/live.txt", vec![1])
            .await
            .unwrap();
    });

    let event = timeout(Duration::from_secs(5), stream.next())
        .await
        .expect("should receive a live event within the timeout")
        .expect("stream should yield an event")
        .unwrap();
    assert!(
        event.resource.path.as_str().contains("/priv/app/live.txt"),
        "expected the in-scope live private event (out-of-scope excluded), got: {}",
        event.resource.path
    );
}

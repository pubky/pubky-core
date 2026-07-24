//! End-to-end coverage for private-path subscriptions on `/events-stream`
//! repeated `path=` params, per-path authorization, and the
//! guarantee that private events never leak to unauthorized callers

use super::*;
use pubky_testnet::pubky::ClientId;

/// Sign up a fresh user and return an authenticated grant session plus the
/// bearer the homeserver minted for it (for raw, credentialed requests).
async fn signed_in_user(
    testnet: &pubky_testnet::EphemeralTestnet,
    client_id: &str,
) -> (
    pubky_testnet::pubky::PublicKey,
    pubky_testnet::pubky::PubkySession,
    String,
) {
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();
    let session = signer
        .signin(ClientId::new(client_id).unwrap())
        .await
        .unwrap();
    let bearer = session.as_grant().unwrap().current_bearer().await;
    (signer.public_key(), session, bearer)
}

/// Anonymous, unfiltered stream is public-only: a `/priv/` write must never
/// appear, even though the same user has public events.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_excludes_private_for_anonymous() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let server_host = server.public_key().z32();
    let (user, session, _bearer) = signed_in_user(&testnet, "leak.test").await;

    // Interleave public and private writes.
    session.storage().put("/pub/a.txt", vec![1]).await.unwrap();
    session
        .storage()
        .put("/priv/app/secret.txt", vec![2])
        .await
        .unwrap();
    session.storage().put("/pub/b.txt", vec![3]).await.unwrap();
    session
        .storage()
        .put("/priv/app/other.txt", vec![4])
        .await
        .unwrap();

    // Anonymous, no `path` → implicit public-only (`/pub/`).
    let url = format!("https://{}/events-stream?user={}", server_host, user.z32());
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut paths = Vec::new();
    while let Some(Ok(event)) = stream.next().await {
        let line = event.data.lines().next().unwrap().to_string();
        assert!(
            !line.contains("/priv/"),
            "anonymous stream leaked a private path: {line}"
        );
        paths.push(line);
        if paths.len() >= 2 {
            break;
        }
    }
    assert_eq!(paths.len(), 2, "should see exactly the two public events");
    assert!(paths.iter().all(|p| p.contains("/pub/")));
}

/// An anonymous subscription that requests a private path is rejected with 401
/// (authentication required) before any event is streamed.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_private_path_requires_auth() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let server_host = server.public_key().z32();

    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();
    let user = signer.public_key();

    let url = format!(
        "https://{}/events-stream?user={}&path=/priv/app/",
        server_host,
        user.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(
        response
            .headers()
            .get("vary")
            .and_then(|value| value.to_str().ok()),
        Some("pubky-host, Authorization, Cookie")
    );
}

/// An authorized owner (root capability) receives their own private events,
/// scoped to the requested filter, and a mixed `/pub/` + `/priv/app/`
/// subscription returns the union, excluding private scopes not requested.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_authorized_owner_receives_private() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let server_host = server.public_key().z32();
    let (user, session, bearer) = signed_in_user(&testnet, "owner.test").await;

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

    // `path=/priv/app/` → exactly the in scope private event.
    let url = format!(
        "https://{}/events-stream?user={}&path=/priv/app/&limit=1",
        server_host,
        user.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .header("Authorization", format!("Bearer {bearer}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("cache-control")
            .and_then(|value| value.to_str().ok()),
        Some("no-store")
    );
    assert_eq!(
        response
            .headers()
            .get("vary")
            .and_then(|value| value.to_str().ok()),
        Some("pubky-host, Authorization, Cookie")
    );
    let mut stream = response.bytes_stream().eventsource();
    let event = stream.next().await.unwrap().unwrap();
    assert!(event
        .data
        .lines()
        .next()
        .unwrap()
        .contains("/priv/app/secret.txt"));

    // Mixed union `path=/pub/&path=/priv/app/` → pub/a + priv/app/secret, never
    // the unrequested `/priv/other/` scope.
    let url = format!(
        "https://{}/events-stream?user={}&path=/pub/&path=/priv/app/",
        server_host,
        user.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .header("Authorization", format!("Bearer {bearer}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream().eventsource();
    let mut paths = Vec::new();
    while let Some(Ok(event)) = stream.next().await {
        let line = event.data.lines().next().unwrap().to_string();
        assert!(
            !line.contains("/priv/other/"),
            "union leaked an unrequested private scope: {line}"
        );
        paths.push(line);
        if paths.len() >= 2 {
            break;
        }
    }
    assert!(paths.iter().any(|p| p.contains("/pub/a.txt")));
    assert!(paths.iter().any(|p| p.contains("/priv/app/secret.txt")));
}

/// Private-path subscriptions are 403 when the session does not match the
/// requested user, or when more than one user is requested alongside a private
/// path.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_private_path_forbidden_cases() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let server_host = server.public_key().z32();

    let (a, _session_a, bearer_a) = signed_in_user(&testnet, "tenant-a.test").await;

    // A second, distinct user that also exists on the homeserver.
    let signer_b = pubky.signer(Keypair::random());
    signer_b.signup(&server.public_key(), None).await.unwrap();
    let b = signer_b.public_key();

    // A's session requesting B's private events → 403.
    let url = format!(
        "https://{}/events-stream?user={}&path=/priv/app/",
        server_host,
        b.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .header("Authorization", format!("Bearer {bearer_a}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);

    // Multiple users alongside a private path → 403, even authenticated as A.
    let url = format!(
        "https://{}/events-stream?user={}&user={}&path=/priv/app/",
        server_host,
        a.z32(),
        b.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .header("Authorization", format!("Bearer {bearer_a}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
}

/// An anonymous live subscriber must never receive a private
/// event broadcast after subscription, while still receiving public events.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_live_excludes_private_for_anonymous() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let server_host = server.public_key().z32();
    let (user, session, _bearer) = signed_in_user(&testnet, "live-leak.test").await;

    // Anonymous live subscription, no path → public-only.
    let url = format!(
        "https://{}/events-stream?user={}&live=true",
        server_host,
        user.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream().eventsource();

    // Let the (empty) historical phase drain and transition to live.
    sleep(Duration::from_millis(300)).await;

    // Private write first, then a public one.
    session
        .storage()
        .put("/priv/app/hidden.txt", vec![1])
        .await
        .unwrap();
    session
        .storage()
        .put("/pub/visible.txt", vec![2])
        .await
        .unwrap();

    // The public event must arrive; no private path may appear before it.
    let mut saw_public = false;
    while let Ok(Some(Ok(event))) = timeout(Duration::from_secs(5), stream.next()).await {
        let line = event.data.lines().next().unwrap().to_string();
        assert!(
            !line.contains("/priv/"),
            "live anonymous stream leaked a private path: {line}"
        );
        if line.contains("/pub/visible.txt") {
            saw_public = true;
            break;
        }
    }
    assert!(saw_public, "should have received the public live event");

    // And nothing private sneaks in immediately after.
    if let Ok(Some(Ok(event))) = timeout(Duration::from_secs(2), stream.next()).await {
        let line = event.data.lines().next().unwrap().to_string();
        assert!(
            !line.contains("/priv/"),
            "live anonymous stream leaked a private path after the public event: {line}"
        );
    }
}

/// Live positive: an authorized owner subscribed to `/priv/app/` receives a
/// later in-scope private event, and never an out-of-scope private one.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_live_authorized_receives_private() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;
    use std::time::Duration;
    use tokio::time::{sleep, timeout};

    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let server_host = server.public_key().z32();
    let (user, session, bearer) = signed_in_user(&testnet, "live-owner.test").await;

    let url = format!(
        "https://{}/events-stream?user={}&path=/priv/app/&live=true",
        server_host,
        user.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .header("Authorization", format!("Bearer {bearer}"))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream().eventsource();

    sleep(Duration::from_millis(300)).await;

    // An out-of-scope private write (must not arrive) and an in-scope one.
    session
        .storage()
        .put("/priv/other/x.txt", vec![1])
        .await
        .unwrap();
    session
        .storage()
        .put("/priv/app/live.txt", vec![2])
        .await
        .unwrap();

    let mut saw_in_scope = false;
    while let Ok(Some(Ok(event))) = timeout(Duration::from_secs(5), stream.next()).await {
        let line = event.data.lines().next().unwrap().to_string();
        assert!(
            !line.contains("/priv/other/"),
            "authorized live subscriber received an out-of-scope private event: {line}"
        );
        if line.contains("/priv/app/live.txt") {
            saw_in_scope = true;
            break;
        }
    }
    assert!(
        saw_in_scope,
        "authorized live subscriber should receive the in-scope private event"
    );
}

/// Sign up a fresh cookie user; return its pubkey, the (kept-alive) session, and
/// the raw `name=value` Cookie header for its session secret.
async fn cookie_user(
    testnet: &pubky_testnet::EphemeralTestnet,
) -> (
    pubky_testnet::pubky::PublicKey,
    pubky_testnet::pubky::PubkySession,
    String,
) {
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let signer = pubky.signer(Keypair::random());
    let session = signer
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();
    // `export_secret()` returns `<pubkey_z32>:<secret>`; the wire cookie is
    // `<pubkey_z32>=<secret>`.
    let token = session.as_cookie().unwrap().export_secret().unwrap();
    let (name, value) = token.split_once(':').unwrap();
    (signer.public_key(), session, format!("{name}={value}"))
}

/// Raw `/events-stream?user=A&path=/priv/app/` with A's session cookie is
/// authenticated (200): the server resolves the cookie by the single `user=`
/// tenant even though the endpoint is homeserver-addressed.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_cookie_same_tenant_is_authorized() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let server_host = server.public_key().z32();

    let (a, session, cookie) = cookie_user(&testnet).await;
    session
        .storage()
        .put("/priv/app/secret.txt", vec![1])
        .await
        .unwrap();

    let url = format!(
        "https://{}/events-stream?user={}&path=/priv/app/&limit=1",
        server_host,
        a.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .header("Cookie", cookie)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}

/// A valid cookie value for A, presented under user B's cookie name for
/// `user=B`, does not authenticate: the session belongs to A, not B → 401.
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_cookie_cross_user_secret_is_unauthenticated() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let server_host = server.public_key().z32();

    let (_a, _session, a_cookie) = cookie_user(&testnet).await;
    let a_secret = a_cookie.split_once('=').unwrap().1.to_string();
    let b = Keypair::random().public_key();

    let url = format!(
        "https://{}/events-stream?user={}&path=/priv/app/",
        server_host,
        b.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .header("Cookie", format!("{}={}", b.z32(), a_secret))
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

/// A valid cookie plus an invalid `Authorization: Bearer` is rejected for both
/// homeserver- and user-addressed requests: a presented bearer disables the
/// cookie fallback (conservative precedence).
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_cookie_not_used_when_bearer_present() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();
    let server_host = server.public_key().z32();

    let (a, session, cookie) = cookie_user(&testnet).await;
    session
        .storage()
        .put("/priv/app/secret.txt", vec![1])
        .await
        .unwrap();

    let url = format!(
        "https://{}/events-stream?user={}&path=/priv/app/",
        server_host,
        a.z32()
    );
    let response = pubky
        .client()
        .request(Method::GET, &url)
        .header("Cookie", cookie.clone())
        .header("Authorization", "Bearer not-a-real-token")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

    let response = pubky
        .client()
        .request(Method::GET, &url)
        .header("pubky-host", a.z32())
        .header("Cookie", cookie)
        .header("Authorization", "Bearer not-a-real-token")
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

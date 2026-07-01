//! Authorization matrix for private (`/priv/`) resources: each actor tier
//! (anonymous, under-scoped owner, other tenant, authorized owner) against
//! every storage action.
//!
//! `/events-stream` authorization is covered in `events::stream_private`.

use super::build_full_testnet;
use pubky_testnet::pubky::{
    AuthFlowKind, ClientId, IntoPubkyResource, Keypair, Method, PubkyGrantAuthFlow,
    PubkyHttpClient, PubkySession, PubkySigner, StatusCode,
};
use pubky_testnet::pubky_common::capabilities::Capabilities;
use pubky_testnet::pubky_common::crypto::PublicKey;
use pubky_testnet::EphemeralTestnet;

const SECRET: &str = "/priv/app/secret.txt";
const ABSENT: &str = "/priv/app/absent.txt";
const DIR: &str = "/priv/app/";

/// Approve a grant with `caps` for `signer` and return the resulting session.
async fn grant_session(
    testnet: &EphemeralTestnet,
    signer: &PubkySigner,
    caps: Capabilities,
) -> PubkySession {
    let pubky = testnet.sdk().unwrap();
    let auth = PubkyGrantAuthFlow::builder(
        &caps,
        AuthFlowKind::signin(),
        ClientId::new("test.app").unwrap(),
    )
    .relay(testnet.http_relay().local_link_url())
    .client(pubky.client().clone())
    .start()
    .unwrap();
    signer
        .approve_auth(&auth.authorization_url())
        .await
        .unwrap();
    auth.await_approval().await.unwrap()
}

/// Transport URL for `path` in `owner`'s namespace.
fn owner_url(owner: &PublicKey, path: &str) -> String {
    format!("{}/{}", owner, path.trim_start_matches('/'))
        .into_pubky_resource()
        .unwrap()
        .to_transport_url()
        .unwrap()
        .to_string()
}

/// Send a raw request and return its status.
async fn req_status(
    client: &PubkyHttpClient,
    method: Method,
    url: &str,
    bearer: Option<&str>,
    body: Option<Vec<u8>>,
) -> StatusCode {
    let mut rb = client.request(method, &url);
    if let Some(bearer) = bearer {
        rb = rb.header("Authorization", format!("Bearer {bearer}"));
    }
    if let Some(body) = body {
        rb = rb.body(body);
    }
    rb.send().await.unwrap().status()
}

/// Every verb against a private resource, expecting the same denied `status`
/// for both an existing and an absent path (no existence oracle).
async fn assert_all_verbs_denied(
    client: &PubkyHttpClient,
    owner: &PublicKey,
    bearer: Option<&str>,
    status: StatusCode,
) {
    let secret = owner_url(owner, SECRET);
    let absent = owner_url(owner, ABSENT);
    let dir = owner_url(owner, DIR);

    assert_eq!(
        req_status(client, Method::PUT, &secret, bearer, Some(vec![0])).await,
        status
    );
    assert_eq!(
        req_status(client, Method::DELETE, &secret, bearer, None).await,
        status
    );
    assert_eq!(
        req_status(client, Method::GET, &dir, bearer, None).await,
        status
    );

    for method in [Method::GET, Method::HEAD] {
        let existing = req_status(client, method.clone(), &secret, bearer, None).await;
        let missing = req_status(client, method, &absent, bearer, None).await;
        assert_eq!(existing, status);
        assert_eq!(
            existing, missing,
            "existing and absent must return the same status"
        );
    }
}

#[tokio::test]
#[pubky_testnet::test]
async fn anonymous_priv_access_is_unauthorized() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();
    let owner = signer.public_key();

    // Seed the private file so the existence-oracle check has a real target.
    let covering = grant_session(
        &testnet,
        &signer,
        Capabilities::builder().read_write(DIR).finish(),
    )
    .await;
    covering.storage().put(SECRET, vec![1, 2, 3]).await.unwrap();

    assert_all_verbs_denied(pubky.client(), &owner, None, StatusCode::UNAUTHORIZED).await;
}

#[tokio::test]
#[pubky_testnet::test]
async fn under_scoped_owner_priv_access_is_forbidden() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();
    let owner = signer.public_key();

    let covering = grant_session(
        &testnet,
        &signer,
        Capabilities::builder().read_write(DIR).finish(),
    )
    .await;
    covering.storage().put(SECRET, vec![1, 2, 3]).await.unwrap();

    // Same owner, session scoped to a sibling subtree that does not cover `/priv/app/`.
    let under = grant_session(
        &testnet,
        &signer,
        Capabilities::builder().read_write("/priv/other/").finish(),
    )
    .await;
    let token = under.as_grant().unwrap().current_bearer().await;

    assert_all_verbs_denied(pubky.client(), &owner, Some(&token), StatusCode::FORBIDDEN).await;
}

#[tokio::test]
#[pubky_testnet::test]
async fn cross_tenant_priv_access_is_forbidden() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let owner_signer = pubky.signer(Keypair::random());
    owner_signer
        .signup(&server.public_key(), None)
        .await
        .unwrap();
    let owner = owner_signer.public_key();
    let covering = grant_session(
        &testnet,
        &owner_signer,
        Capabilities::builder().read_write(DIR).finish(),
    )
    .await;
    covering.storage().put(SECRET, vec![1, 2, 3]).await.unwrap();

    // A different tenant, even with an identically scoped cap in their own namespace.
    let tenant_signer = pubky.signer(Keypair::random());
    tenant_signer
        .signup(&server.public_key(), None)
        .await
        .unwrap();
    let tenant = grant_session(
        &testnet,
        &tenant_signer,
        Capabilities::builder().read_write(DIR).finish(),
    )
    .await;
    let token = tenant.as_grant().unwrap().current_bearer().await;

    assert_all_verbs_denied(pubky.client(), &owner, Some(&token), StatusCode::FORBIDDEN).await;
}

#[tokio::test]
#[pubky_testnet::test]
async fn owner_with_cap_priv_access_is_authorized() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();
    let owner = signer.public_key();
    let session = grant_session(
        &testnet,
        &signer,
        Capabilities::builder().read_write(DIR).finish(),
    )
    .await;
    let token = session.as_grant().unwrap().current_bearer().await;
    let bearer = Some(token.as_str());

    let client = pubky.client();
    let secret = owner_url(&owner, SECRET);
    let absent = owner_url(&owner, ABSENT);
    let dir = owner_url(&owner, DIR);

    // Write, read, list, delete — all authorized.
    assert_eq!(
        req_status(client, Method::PUT, &secret, bearer, Some(vec![1, 2, 3])).await,
        StatusCode::CREATED
    );
    assert_eq!(
        req_status(client, Method::GET, &secret, bearer, None).await,
        StatusCode::OK
    );
    assert_eq!(
        req_status(client, Method::HEAD, &secret, bearer, None).await,
        StatusCode::OK
    );
    assert_eq!(
        req_status(client, Method::GET, &dir, bearer, None).await,
        StatusCode::OK
    );
    assert_eq!(
        req_status(client, Method::DELETE, &secret, bearer, None).await,
        StatusCode::NO_CONTENT
    );

    // The authorized tier distinguishes absent (404) from the denied tiers' uniform status.
    assert_eq!(
        req_status(client, Method::GET, &absent, bearer, None).await,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        req_status(client, Method::HEAD, &absent, bearer, None).await,
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn anonymous_events_feed_excludes_private() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    signer.signup(&server.public_key(), None).await.unwrap();
    let session = grant_session(
        &testnet,
        &signer,
        Capabilities::builder()
            .read_write("/pub/")
            .read_write(DIR)
            .finish(),
    )
    .await;

    // Interleave public and private writes.
    session.storage().put("/pub/a.txt", vec![1]).await.unwrap();
    session.storage().put(SECRET, vec![2]).await.unwrap();
    session.storage().put("/pub/b.txt", vec![3]).await.unwrap();

    // The anonymous public feed must never surface a private path.
    let feed_url = format!("https://{}/events/?limit=100", server.public_key().z32());
    let text = pubky
        .client()
        .request(Method::GET, &feed_url)
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();

    assert!(
        !text.contains("/priv/"),
        "public feed leaked a private path:\n{text}"
    );
    assert!(
        text.contains("/pub/a.txt") && text.contains("/pub/b.txt"),
        "public events should be present:\n{text}"
    );
}

use super::*;

#[tokio::test]
#[pubky_testnet::test]
async fn unauthorized_put_delete() {
    let testnet = build_full_testnet().await;
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Owner user
    let owner = pubky.signer(Keypair::random());
    let owner_session = owner
        .signup_cookie(&server.public_key(), None)
        .await
        .unwrap();

    let path = "/pub/foo.txt";

    // Someone tries to write to owner's namespace -> 401 Unauthorized
    let owner_url = format!(
        "{}/{}",
        owner_session.info().public_key(),
        path.trim_start_matches('/')
    );

    let owner_transport_url = owner_url
        .clone()
        .into_pubky_resource()
        .unwrap()
        .to_transport_url()
        .unwrap();

    let response = pubky
        .client()
        .request(Method::PUT, &owner_transport_url)
        .body(vec![0, 1, 2, 3, 4])
        .send()
        .await
        .unwrap();

    assert!(matches!(response.status(), StatusCode::UNAUTHORIZED));

    // Owner writes successfully
    let resp = owner_session
        .storage()
        .put(path, vec![0, 1, 2, 3, 4])
        .await
        .unwrap();
    assert!(resp.status().is_success());

    // Other tries to delete owner's file → 401 Unauthorized
    let response = pubky
        .client()
        .request(Method::DELETE, &owner_transport_url)
        .send()
        .await
        .unwrap();

    assert!(matches!(response.status(), StatusCode::UNAUTHORIZED));

    // Owner can read contents
    let body = owner_session
        .storage()
        .get(path)
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(body, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));
}

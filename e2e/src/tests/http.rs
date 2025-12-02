use pubky_testnet::pubky::Method;
use pubky_testnet::EphemeralTestnet;

#[tokio::test]
#[pubky_testnet::test]
async fn http_get_pubky() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();

    let client = testnet.client().unwrap();

    let pubky_url = format!("https://{}/", server.public_key().z32());
    let response = client
        .request(Method::GET, &pubky_url)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200)
}

#[tokio::test]
#[pubky_testnet::test]
async fn http_get_icann() {
    let testnet = EphemeralTestnet::start().await.unwrap();

    let client = testnet.client().unwrap();

    let icann_url = "https://google.com/".to_string();

    let response = client
        .request(Method::GET, &icann_url)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}

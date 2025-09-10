use pubky_testnet::EphemeralTestnet;
use reqwest::Method;

#[tokio::test]
async fn http_get_pubky() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();

    let client = testnet.client().unwrap();

    let response = client
        .request(Method::GET, format!("https://{}/", server.public_key()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200)
}

#[tokio::test]
async fn http_get_icann() {
    let testnet = EphemeralTestnet::start().await.unwrap();

    let client = testnet.client().unwrap();

    let response = client
        .request(Method::GET, "https://google.com/")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}

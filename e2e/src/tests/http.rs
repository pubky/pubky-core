use pubky_testnet::SimpleTestnet;

#[tokio::test]
async fn http_get_pubky() {
    let testnet = SimpleTestnet::run().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

    let response = client
        .get(format!("https://{}/", server.public_key()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200)
}

#[tokio::test]
async fn http_get_icann() {
    let testnet = SimpleTestnet::run().await.unwrap();

    let client = testnet.pubky_client_builder().build().unwrap();

    let response = client
        .request(Default::default(), "https://example.com/")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}

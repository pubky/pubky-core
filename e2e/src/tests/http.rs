use pubky_testnet::Testnet;

#[tokio::test]
async fn http_get_pubky() {
    let testnet = Testnet::run().await.unwrap();
    let homeserver = testnet.run_homeserver_suite().await.unwrap();

    let client = testnet.client_builder().build().unwrap();

    let response = client
        .get(format!("https://{}/", homeserver.public_key()))
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200)
}

#[tokio::test]
async fn http_get_icann() {
    let testnet = Testnet::run().await.unwrap();

    let client = testnet.client_builder().build().unwrap();

    let response = client
        .request(Default::default(), "https://example.com/")
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
}

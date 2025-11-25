use pubky_testnet::{
    pubky::{Keypair, Method, StatusCode},
    EphemeralTestnet,
};

#[tokio::test]
#[pubky_testnet::test]
async fn metrics_comprehensive() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;
    use tokio::time::Duration;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let metrics_server = server
        .metrics_server()
        .expect("metrics server should be enabled in tests");
    let metrics_socket = metrics_server.listen_socket();
    let metrics_url = format!("http://{}/metrics", metrics_socket);

    // 1. Test basic endpoint accessibility and Prometheus format
    let response = pubky
        .client()
        .request(Method::GET, &metrics_url)
        .send()
        .await
        .unwrap();

    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Metrics endpoint should be accessible"
    );

    let body = response.text().await.unwrap();
    assert!(!body.is_empty(), "Metrics output should not be empty");
    assert!(
        body.contains("# HELP") || body.contains("# TYPE"),
        "Metrics should be in Prometheus format with HELP/TYPE comments"
    );

    let expected_metrics = [
        "events_db_query_duration_ms",
        "event_stream_db_query_duration_ms",
        "event_stream_broadcast_lagged_count",
        "event_stream_active_connections",
        "event_stream_connection_duration_seconds",
    ];

    for metric in expected_metrics {
        assert!(
            body.contains(metric),
            "Metrics should contain {}: {}",
            metric,
            body
        );
    }

    // 2. Test metric recording with concurrent connections
    let keypair1 = Keypair::random();
    let keypair2 = Keypair::random();
    let signer1 = pubky.signer(keypair1);
    let signer2 = pubky.signer(keypair2);

    let session1 = signer1.signup(&server.public_key(), None).await.unwrap();
    let session2 = signer2.signup(&server.public_key(), None).await.unwrap();

    let user_pubky1 = signer1.public_key();
    let user_pubky2 = signer2.public_key();

    // Create events to generate metrics
    session1
        .storage()
        .put("/pub/test1.txt", vec![1])
        .await
        .unwrap();
    session2
        .storage()
        .put("/pub/test2.txt", vec![2])
        .await
        .unwrap();

    // Call /events endpoint to generate DB query metrics
    let events_url = format!(
        "https://{}/events?user={}",
        server.public_key(),
        user_pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &events_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    // 3. Test concurrent stream connections
    let stream_url1 = format!(
        "https://{}/events-stream?user={}&live=true",
        server.public_key(),
        user_pubky1
    );
    let stream_url2 = format!(
        "https://{}/events-stream?user={}&live=true",
        server.public_key(),
        user_pubky2
    );

    let response1 = pubky
        .client()
        .request(Method::GET, &stream_url1)
        .send()
        .await
        .unwrap();
    let response2 = pubky
        .client()
        .request(Method::GET, &stream_url2)
        .send()
        .await
        .unwrap();

    let mut stream1 = response1.bytes_stream().eventsource();
    let mut stream2 = response2.bytes_stream().eventsource();

    // Read initial events from both streams
    stream1.next().await;
    stream2.next().await;

    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify 2 active connections
    let response = pubky
        .client()
        .request(Method::GET, &metrics_url)
        .send()
        .await
        .unwrap();
    let metrics = response.text().await.unwrap();

    assert!(
        metrics.contains("event_stream_active_connections 2"),
        "Should have 2 active connections: {}",
        metrics
    );

    // Verify DB query metrics recorded
    assert!(
        metrics.contains("events_db_query_duration_ms_count"),
        "Should have events_db_query_duration_ms_count metric"
    );
    assert!(
        metrics.contains("event_stream_db_query_duration_ms_count"),
        "Should have event_stream_db_query_duration_ms_count metric"
    );

    // Close one stream
    drop(stream1);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify 1 active connection
    let response = pubky
        .client()
        .request(Method::GET, &metrics_url)
        .send()
        .await
        .unwrap();
    let metrics = response.text().await.unwrap();

    assert!(
        metrics.contains("event_stream_active_connections 1"),
        "Should have 1 active connection after closing one: {}",
        metrics
    );

    // Close second stream
    drop(stream2);
    tokio::time::sleep(Duration::from_millis(100)).await;

    // Verify 0 active connections and connection duration recorded
    let response = pubky
        .client()
        .request(Method::GET, &metrics_url)
        .send()
        .await
        .unwrap();
    let final_metrics = response.text().await.unwrap();

    assert!(
        final_metrics.contains("event_stream_active_connections 0"),
        "Should have 0 active connections after closing all: {}",
        final_metrics
    );

    assert!(
        final_metrics.contains("event_stream_connection_duration_seconds_count"),
        "Should have event_stream_connection_duration_seconds_count metric"
    );
}

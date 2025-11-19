use pubky_testnet::{
    pubky::{Keypair, Method, StatusCode},
    EphemeralTestnet,
};

/// Comprehensive test for single-user event streaming modes:
/// - Historical event pagination (>100 events across internal batches)
/// - Finite limit enforcement
/// - Live event streaming
/// - Phase transition (historical -> live)
/// - Batch mode connection closing
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_basic_modes() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;
    use tokio::time::{timeout, Duration};

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Create one user with 250 events - reuse for all subtests
    let keypair = Keypair::random();
    let signer = pubky.signer(keypair);
    let session = signer.signup(&server.public_key(), None).await.unwrap();
    let user_pubky = signer.public_key();

    // Create 250 events (internal batch size is 100, tests pagination)
    for i in 0..250 {
        let path = format!("/pub/file_{i}.txt");
        session.storage().put(path, vec![i as u8]).await.unwrap();
    }
    // Add some DELETE events
    for i in 240..245 {
        let path = format!("/pub/file_{i}.txt");
        session.storage().delete(path).await.unwrap();
    }

    // ==== Test 1: Historical auto-pagination (>100 events) ====
    let stream_url = format!(
        "https://{}/events-stream?user={}&limit=255",
        server.public_key(),
        user_pubky
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut event_count = 0;
    let mut put_count = 0;
    let mut del_count = 0;
    let mut last_cursor = String::new();
    let mut cursor_250 = String::new();

    while event_count < 255 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                put_count += 1;
            } else if event.event == "DEL" {
                del_count += 1;
            }
            if let Some(cursor_line) = event.data.lines().find(|l| l.starts_with("cursor: ")) {
                last_cursor = cursor_line.strip_prefix("cursor: ").unwrap().to_string();
                // Capture cursor at event 250 for Test 3
                if event_count == 249 {
                    cursor_250 = last_cursor.clone();
                }
            }
            event_count += 1;
        } else {
            break;
        }
    }

    assert_eq!(
        event_count, 255,
        "Historical: Should receive all 255 events"
    );
    assert_eq!(put_count, 250, "Historical: Should have 250 PUT events");
    assert_eq!(del_count, 5, "Historical: Should have 5 DEL events");
    assert!(!last_cursor.is_empty(), "Historical: Should have cursor");
    assert!(
        stream.next().await.is_none(),
        "Historical: Should close after limit"
    );

    // ==== Test 2: Finite limit enforcement ====
    let stream_url = format!(
        "https://{}/events-stream?user={}&limit=50",
        server.public_key(),
        user_pubky
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut event_count = 0;

    while event_count < 50 {
        if let Some(Ok(_event)) = stream.next().await {
            event_count += 1;
        } else {
            break;
        }
    }

    assert_eq!(event_count, 50, "Limit: Should receive exactly 50 events");
    assert!(
        stream.next().await.is_none(),
        "Limit: Should close after limit"
    );

    // ==== Test 3: Live mode starting from cursor 250 ====

    // Connect with live=true from cursor 250
    // Reuse cursor_250 captured from Test 1 (event 250)
    let stream_url = format!(
        "https://{}/events-stream?user={}:{}&live=true",
        server.public_key(),
        user_pubky,
        cursor_250
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();

    // Create live event
    let session_clone = session.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let path = "/pub/live_test.txt";
        session_clone.storage().put(path, vec![42]).await.unwrap();
    });

    let result = timeout(Duration::from_secs(5), async {
        // Should get 5 DEL events from history, then the live event
        let mut event_count = 0;
        while let Some(Ok(event)) = stream.next().await {
            event_count += 1;
            if event.event == "PUT" && event.data.contains("live_test.txt") {
                return Some(event_count);
            }
        }
        None
    })
    .await;

    assert!(result.is_ok(), "Live: Should receive event within timeout");
    let total = result.unwrap();
    assert!(total.is_some(), "Live: Should receive the live event");
    assert_eq!(total.unwrap(), 6, "Live: Should get 5 DEL + 1 live PUT");

    // ==== Test 4: Live mode with limit - transitions from historical to live until limit reached ====
    let stream_url = format!(
        "https://{}/events-stream?user={}:{}&live=true&limit=10",
        server.public_key(),
        user_pubky,
        cursor_250
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();

    // Spawn a task to create live events
    let session_clone = session.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(300)).await;
        for i in 0..10 {
            let path = format!("/pub/live_extra_{i}.txt");
            session_clone
                .storage()
                .put(path, vec![i as u8])
                .await
                .unwrap();
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });

    let mut total_count = 0;
    let result = timeout(Duration::from_secs(5), async {
        while let Some(Ok(_event)) = stream.next().await {
            total_count += 1;
            if total_count >= 10 {
                break;
            }
        }
        total_count
    })
    .await;

    assert!(result.is_ok(), "Live+Limit: Should complete within timeout");
    assert_eq!(
        result.unwrap(),
        10,
        "Live+Limit: Should stop at exactly 10 total events (6 historical + 4 live)"
    );

    // Verify stream is closed after limit reached
    let next_result = timeout(Duration::from_millis(500), stream.next()).await;
    assert!(
        next_result.is_ok() && next_result.unwrap().is_none(),
        "Live+Limit: Connection should close after reaching limit"
    );

    // ==== Test 5: Batch mode (live=false) - connection closes after historical events ====
    let stream_url = format!(
        "https://{}/events-stream?user={}:{}&limit=5",
        server.public_key(),
        user_pubky,
        cursor_250
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut batch_event_count = 0;

    // Collect historical events
    while let Some(Ok(_event)) = stream.next().await {
        batch_event_count += 1;
        if batch_event_count >= 5 {
            break;
        }
    }

    assert_eq!(
        batch_event_count, 5,
        "Batch: Should receive 5 DEL events from history"
    );

    // Schedule a new event after connection established
    tokio::time::sleep(Duration::from_millis(100)).await;
    let path = "/pub/new_event.txt";
    session.storage().put(path, vec![99]).await.unwrap();

    // In batch mode, stream should close after historical events
    // Try to read with timeout - should get None (connection closed)
    let next_result = timeout(Duration::from_millis(500), stream.next()).await;
    assert!(
        next_result.is_ok() && next_result.unwrap().is_none(),
        "Batch: Connection should be closed, not waiting for live events"
    );

    // ==== Test 6: Content hash verification ====
    // Verify PUT events include content_hash, DEL events do not
    // We only need to check one PUT and one DEL event to verify the format
    // Fetch from cursor_250 which we know has 5 DEL events after it
    let stream_url = format!(
        "https://{}/events-stream?user={}:{}&limit=6",
        server.public_key(),
        user_pubky,
        cursor_250
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut found_del = false;

    while let Some(Ok(event)) = stream.next().await {
        let data_lines: Vec<&str> = event.data.lines().collect();
        let has_content_hash = data_lines
            .iter()
            .any(|line| line.starts_with("content_hash: "));

        if event.event == "DEL" {
            assert!(
                !has_content_hash,
                "ContentHash: DEL event should NOT have content_hash field"
            );
            found_del = true;
            break; // We've verified both a PUT (from earlier in stream) and now a DEL
        } else if event.event == "PUT" {
            assert!(
                has_content_hash,
                "ContentHash: PUT event should have content_hash field"
            );
            // Verify format: should be 64 hex characters (blake3 hash)
            if let Some(hash_line) = data_lines
                .iter()
                .find(|line| line.starts_with("content_hash: "))
            {
                let hash_value = hash_line.strip_prefix("content_hash: ").unwrap();
                assert_eq!(
                    hash_value.len(),
                    64,
                    "ContentHash: Should be 64 hex characters"
                );
                assert!(
                    hash_value.chars().all(|c| c.is_ascii_hexdigit()),
                    "ContentHash: Should contain only hex digits"
                );
            }
        }
    }

    assert!(
        found_del,
        "ContentHash: Should have found at least one DEL event to verify"
    );

    // ==== Test 6b: Verify content_hash format matches HTTP headers ====
    // This ensures the SSE event stream and HTTP GET endpoints use the same hash format

    // Create a new file with known content
    let test_data = b"hello world for hash test";
    session
        .storage()
        .put("/pub/hash_test.txt", test_data.to_vec())
        .await
        .unwrap();

    // Get the content_hash from HTTP GET headers (ETag)
    let get_url = format!("https://{}/pub/hash_test.txt", server.public_key());
    let get_response = pubky
        .client()
        .request(Method::GET, &get_url)
        .header("pubky-host", user_pubky.to_string())
        .send()
        .await
        .unwrap();
    assert_eq!(get_response.status(), StatusCode::OK);

    let etag_header = get_response
        .headers()
        .get("etag")
        .expect("Should have ETag header")
        .to_str()
        .unwrap();
    // ETag is wrapped in quotes: "hash_value"
    let etag_hash = etag_header.trim_matches('"');

    // Get the content_hash from event stream
    let stream_url = format!(
        "https://{}/events-stream?user={}&path=/pub/hash_test.txt",
        server.public_key(),
        user_pubky
    );
    let stream_response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(stream_response.status(), StatusCode::OK);

    let mut stream = stream_response.bytes_stream().eventsource();
    let mut event_hash = String::new();

    if let Some(Ok(event)) = stream.next().await {
        let data_lines: Vec<&str> = event.data.lines().collect();
        if let Some(hash_line) = data_lines.iter().find(|l| l.starts_with("content_hash: ")) {
            event_hash = hash_line
                .strip_prefix("content_hash: ")
                .unwrap()
                .to_string();
        }
    }

    assert_eq!(
        etag_hash, event_hash,
        "ContentHash: Event stream hash should match HTTP ETag header. ETag: {}, Event: {}",
        etag_hash, event_hash
    );

    // ==== Test 7: Empty user behavior ====
    // Create new user with no events
    let empty_keypair = Keypair::random();
    let empty_signer = pubky.signer(empty_keypair);
    let empty_session = empty_signer
        .signup(&server.public_key(), None)
        .await
        .unwrap();
    let empty_user_pubky = empty_signer.public_key();

    // Test 7a: Batch mode should close immediately
    let stream_url = format!(
        "https://{}/events-stream?user={}",
        server.public_key(),
        empty_user_pubky
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    assert!(
        stream.next().await.is_none(),
        "Empty user batch mode: Should close immediately with no events"
    );

    // Test 7b: Live mode should stay open and receive new events
    let stream_url = format!(
        "https://{}/events-stream?user={}&live=true",
        server.public_key(),
        empty_user_pubky
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();

    // Create a live event for the empty user
    let session_clone = empty_session.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let path = "/pub/first_event.txt";
        session_clone.storage().put(path, vec![42]).await.unwrap();
    });

    let result = timeout(Duration::from_secs(5), async {
        if let Some(Ok(event)) = stream.next().await {
            return Some((event.event.clone(), event.data.contains("first_event.txt")));
        }
        None
    })
    .await;

    assert!(
        result.is_ok(),
        "Empty user live mode: Should receive event within timeout"
    );
    let event_data = result.unwrap();
    assert!(
        event_data.is_some(),
        "Empty user live mode: Should receive the live event"
    );
    let (event_type, has_path) = event_data.unwrap();
    assert_eq!(
        event_type, "PUT",
        "Empty user live mode: Should be PUT event"
    );
    assert!(
        has_path,
        "Empty user live mode: Should contain first_event.txt path"
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_multiple_users() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let keypair1 = Keypair::random();
    let keypair2 = Keypair::random();
    let keypair3 = Keypair::random();

    // Create three users
    let signer1 = pubky.signer(keypair1);
    let signer2 = pubky.signer(keypair2);
    let signer3 = pubky.signer(keypair3);

    let session1 = signer1.signup(&server.public_key(), None).await.unwrap();
    let session2 = signer2.signup(&server.public_key(), None).await.unwrap();
    let session3 = signer3.signup(&server.public_key(), None).await.unwrap();

    let pubky1 = signer1.public_key();
    let pubky2 = signer2.public_key();
    let pubky3 = signer3.public_key();

    // Create different events for each user
    for i in 0..3 {
        let path = format!("/pub/test1_{i}.txt");
        session1.storage().put(path, vec![i as u8]).await.unwrap();
    }
    for i in 0..2 {
        let path = format!("/pub/test2_{i}.txt");
        session2.storage().put(path, vec![i as u8]).await.unwrap();
    }
    for i in 0..4 {
        let path = format!("/pub/test3_{i}.txt");
        session3.storage().put(path, vec![i as u8]).await.unwrap();
    }

    // Stream events for user1 and user2 (should get 5 events total)
    let stream_url = format!(
        "https://{}/events-stream?user={}&user={}",
        server.public_key(),
        pubky1,
        pubky2
    );

    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();

    let status = response.status();
    if status != StatusCode::OK {
        let body = response.text().await.unwrap();
        panic!("Expected 200 OK, got {}: {}", status, body);
    }

    let mut stream = response.bytes_stream().eventsource();
    let mut events = Vec::new();

    while events.len() < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                let lines: Vec<&str> = event.data.lines().collect();
                let path = lines[0].strip_prefix("pubky://").unwrap();
                events.push(path.to_string());
            }
        } else {
            break;
        }
    }

    // Verify we got events from both users
    assert_eq!(events.len(), 5, "Should receive 5 events total");
    let user1_events = events
        .iter()
        .filter(|e| e.contains(&pubky1.to_string()))
        .count();
    let user2_events = events
        .iter()
        .filter(|e| e.contains(&pubky2.to_string()))
        .count();

    assert_eq!(user1_events, 3, "Should receive 3 events from user1");
    assert_eq!(user2_events, 2, "Should receive 2 events from user2");

    // Verify no events from user3
    let user3_events = events
        .iter()
        .filter(|e| e.contains(&pubky3.to_string()))
        .count();
    assert_eq!(user3_events, 0, "Should not receive events from user3");

    // Now test that returned cursor values are correct with per-user cursors
    // Get the first 2 events and track cursor per user
    let stream_url_for_cursor = format!(
        "https://{}/events-stream?user={}&user={}&limit=2",
        server.public_key(),
        pubky1,
        pubky2
    );

    let response = pubky
        .client()
        .request(Method::GET, &stream_url_for_cursor)
        .send()
        .await
        .unwrap();

    let mut stream = response.bytes_stream().eventsource();
    let mut user1_cursor = String::new();
    let mut user2_cursor = String::new();
    let mut first_two_events = Vec::new();

    while first_two_events.len() < 2 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                let lines: Vec<&str> = event.data.lines().collect();
                let path = lines[0].strip_prefix("pubky://").unwrap();
                first_two_events.push(path.to_string());

                // Extract cursor and associate with the user
                if let Some(cursor_line) = lines.iter().find(|l| l.starts_with("cursor: ")) {
                    let cursor = cursor_line.strip_prefix("cursor: ").unwrap().to_string();

                    // Determine which user this event belongs to
                    if lines[0].contains(&pubky1.to_string()) {
                        user1_cursor = cursor;
                    } else if lines[0].contains(&pubky2.to_string()) {
                        user2_cursor = cursor;
                    }
                }
            }
        } else {
            break;
        }
    }

    drop(stream);

    assert_eq!(first_two_events.len(), 2, "Should get first 2 events");

    // Now request the remaining events using per-user cursors
    // This should properly handle the case where each user has a different cursor position
    // Build the URL conditionally based on whether we have cursors
    let mut url_parts = vec![format!("https://{}/events-stream?", server.public_key())];

    if !user1_cursor.is_empty() {
        url_parts.push(format!("user={}:{}", pubky1, user1_cursor));
    } else {
        url_parts.push(format!("user={}", pubky1));
    }

    if !user2_cursor.is_empty() {
        url_parts.push(format!("&user={}:{}", pubky2, user2_cursor));
    } else {
        url_parts.push(format!("&user={}", pubky2));
    }

    let stream_url_with_cursor = url_parts.join("");

    let response = pubky
        .client()
        .request(Method::GET, &stream_url_with_cursor)
        .send()
        .await
        .unwrap();

    let mut stream = response.bytes_stream().eventsource();
    let mut remaining_events = Vec::new();

    while remaining_events.len() < 3 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                let lines: Vec<&str> = event.data.lines().collect();
                let path = lines[0].strip_prefix("pubky://").unwrap();
                remaining_events.push(path.to_string());
            }
        } else {
            break;
        }
    }

    // We should get exactly 3 remaining events (1 from user1, 2 from user2)
    assert_eq!(
        remaining_events.len(),
        3,
        "Should receive all remaining events after cursor. Got: {:?}",
        remaining_events
    );

    let user1_remaining = remaining_events
        .iter()
        .filter(|e| e.contains(&pubky1.to_string()))
        .count();
    let user2_remaining = remaining_events
        .iter()
        .filter(|e| e.contains(&pubky2.to_string()))
        .count();

    // With per-user cursors, each user's position is tracked independently:
    // - First 2 events were: test1_0, test1_1 (both from user1)
    // - User1 cursor is at test1_1, so we should get: test1_2 (1 event)
    // - User2 cursor is empty (none of user2's events were in first batch), so we get: test2_0, test2_1 (2 events)
    // Total remaining: 3 events
    assert_eq!(
        user1_remaining, 1,
        "Should get 1 remaining event from user1 (test1_2). Got events: {:?}",
        remaining_events
    );
    assert_eq!(
        user2_remaining, 2,
        "Should get 2 remaining events from user2 (test2_0, test2_1). Got events: {:?}",
        remaining_events
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_validation_errors() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let keypair1 = Keypair::random();
    let keypair2 = Keypair::random();

    // Sign up user1, leave user2 not registered
    let signer1 = pubky.signer(keypair1);
    let session1 = signer1.signup(&server.public_key(), None).await.unwrap();

    let pubky1 = signer1.public_key();
    let pubky2 = keypair2.public_key(); // Not registered
    let invalid_pubkey = "invalid_key_not_zbase32";

    // Test 1: No user parameter
    let stream_url = format!("https://{}/events-stream", server.public_key());
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "No user parameter"
    );
    let body = response.text().await.unwrap();
    assert!(body.contains("user parameter is required"));

    // Test 2: Too many users (>50)
    let mut query_params = vec![];
    for _i in 0..51 {
        let keypair = Keypair::random();
        query_params.push(format!("user={}", keypair.public_key()));
    }
    let stream_url = format!(
        "https://{}/events-stream?{}",
        server.public_key(),
        query_params.join("&")
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST, "Too many users");
    let body = response.text().await.unwrap();
    assert!(body.contains("Too many users") || body.contains("Maximum allowed: 50"));

    // Test 3: Invalid public key format
    let stream_url = format!(
        "https://{}/events-stream?user={}",
        server.public_key(),
        invalid_pubkey
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Invalid key format"
    );
    let body = response.text().await.unwrap();
    assert!(body.contains("Invalid public key"));

    // Test 4: Valid key but user not registered
    let stream_url = format!(
        "https://{}/events-stream?user={}",
        server.public_key(),
        pubky2
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "User not registered"
    );

    // Test 5: Mix of valid registered and unregistered user
    let stream_url = format!(
        "https://{}/events-stream?user={}&user={}",
        server.public_key(),
        pubky1,
        pubky2
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::NOT_FOUND,
        "Mixed valid/unregistered"
    );

    // Test 6: Mix of valid user and invalid key format
    let stream_url = format!(
        "https://{}/events-stream?user={}&user={}",
        server.public_key(),
        pubky1,
        invalid_pubkey
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Mixed valid/invalid"
    );
    let body = response.text().await.unwrap();
    assert!(body.contains("Invalid public key"));

    // Test 7: Multiple invalid keys
    let stream_url = format!(
        "https://{}/events-stream?user={}&user={}",
        server.public_key(),
        invalid_pubkey,
        "another_invalid_key"
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Multiple invalid keys"
    );

    // Test 8: Incompatible live=true with reverse=true
    let stream_url = format!(
        "https://{}/events-stream?user={}&live=true&reverse=true",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "live+reverse incompatible"
    );
    let body = response.text().await.unwrap();
    assert!(body.contains("Cannot use live mode with reverse ordering"));

    // Test 9: Invalid cursor formats
    // Create an event first to get a valid cursor for comparison
    session1
        .storage()
        .put("/pub/test.txt", vec![1])
        .await
        .unwrap();

    // Test 9a: Malformed cursor (non-numeric)
    let stream_url = format!(
        "https://{}/events-stream?user={}:abc123xyz",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Malformed cursor should fail"
    );
    let body = response.text().await.unwrap();
    assert!(body.contains("Invalid cursor"));

    // Test 9b: Negative cursor (technically valid i64, but no events will have negative IDs)
    let stream_url = format!(
        "https://{}/events-stream?user={}:-100&limit=10",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Negative cursor is technically valid but returns all events (since -100 < any event ID)"
    );
    let mut stream = response.bytes_stream().eventsource();
    // Since cursor is -100, all events (which have positive IDs) are "after" it
    // Should get at least 1 event (the test.txt we created)
    assert!(
        stream.next().await.is_some(),
        "Negative cursor should return events since all event IDs are positive"
    );

    // Test 9c: Very large cursor beyond any events (should succeed but return no events)
    let stream_url = format!(
        "https://{}/events-stream?user={}:999999999&limit=10",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::OK,
        "Large cursor should succeed"
    );
    let mut stream = response.bytes_stream().eventsource();
    // Should immediately close with no events
    assert!(
        stream.next().await.is_none(),
        "Large cursor beyond events should return no events"
    );

    // Test 10: Path parameter normalization
    // Test 10a: Path without leading slash - should automatically add "/" prefix
    let stream_url = format!(
        "https://{}/events-stream?user={}&path=pub/test.txt&limit=1",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    // Path without leading slash should be normalized to "/pub/test.txt" and return events
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream().eventsource();
    // Should get at least 1 event (the test.txt we created earlier)
    assert!(
        stream.next().await.is_some(),
        "Path without leading slash should be automatically normalized with / prefix"
    );

    // Test 10b: Empty path parameter (should be treated as no filter)
    let stream_url = format!(
        "https://{}/events-stream?user={}&path=&limit=1",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);
    let mut stream = response.bytes_stream().eventsource();
    // Should get at least 1 event (the test.txt we created)
    assert!(
        stream.next().await.is_some(),
        "Empty path should be treated as no filter"
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_reverse() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let keypair = Keypair::random();
    let signer = pubky.signer(keypair);
    let session = signer.signup(&server.public_key(), None).await.unwrap();

    let user_pubky = signer.public_key();

    // Create 10 events with identifiable content
    for i in 0..10 {
        let path = format!("/pub/file_{i}.txt");
        session.storage().put(path, vec![i as u8]).await.unwrap();
    }

    // Test forward order (reverse=false) - should get oldest first
    let stream_url_forward = format!(
        "https://{}/events-stream?user={}&limit=10",
        server.public_key(),
        user_pubky
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url_forward)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut forward_files = Vec::new();
    let mut event_count = 0;

    while event_count < 10 {
        if let Some(Ok(event)) = stream.next().await {
            for line in event.data.lines() {
                if line.contains("/pub/file_") {
                    if let Some(filename) = line.split("/pub/").nth(1) {
                        forward_files.push(filename.to_string());
                    }
                }
            }
            event_count += 1;
        } else {
            println!("Forward stream ended at event {}", event_count);
            break;
        }
    }

    assert_eq!(
        forward_files.len(),
        10,
        "Should receive 10 events in forward order"
    );
    assert_eq!(
        forward_files[0], "file_0.txt",
        "First event should be file_0"
    );
    assert_eq!(
        forward_files[9], "file_9.txt",
        "Last event should be file_9"
    );

    // Test reverse order (reverse=true) - should get newest first
    let stream_url_reverse = format!(
        "https://{}/events-stream?user={}&reverse=true&limit=10",
        server.public_key(),
        user_pubky
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url_reverse)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut reverse_files = Vec::new();
    let mut event_count = 0;

    while event_count < 10 {
        if let Some(Ok(event)) = stream.next().await {
            for line in event.data.lines() {
                if line.contains("/pub/file_") {
                    if let Some(filename) = line.split("/pub/").nth(1) {
                        reverse_files.push(filename.to_string());
                    }
                }
            }
            event_count += 1;
        } else {
            println!("Reverse stream ended at event {}", event_count);
            break;
        }
    }

    assert_eq!(
        reverse_files.len(),
        10,
        "Should receive 10 events in reverse order"
    );
    assert_eq!(
        reverse_files[0], "file_9.txt",
        "First event should be file_9 (newest)"
    );
    assert_eq!(
        reverse_files[9], "file_0.txt",
        "Last event should be file_0 (oldest)"
    );

    // Verify reverse order is exactly the reverse of forward order
    let mut forward_reversed = forward_files.clone();
    forward_reversed.reverse();
    assert_eq!(
        reverse_files, forward_reversed,
        "Reverse order should be exactly the reverse of forward order"
    );

    // Verify that stream closed after all events (phase 2 not entered)
    // The stream from reverse test should already be exhausted
    assert_eq!(event_count, 10, "Should have received exactly 10 events");
}

/// Comprehensive test for directory filtering (`path` parameter):
/// - Basic filtering by different directory paths
/// - Filter with cursor pagination
/// - Filter with multiple users
/// - Filter with reverse ordering
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_path_filter() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Create 2 users upfront with diverse directory structures
    let keypair1 = Keypair::random();
    let keypair2 = Keypair::random();
    let signer1 = pubky.signer(keypair1);
    let signer2 = pubky.signer(keypair2);
    let session1 = signer1.signup(&server.public_key(), None).await.unwrap();
    let session2 = signer2.signup(&server.public_key(), None).await.unwrap();
    let pubky1 = signer1.public_key();
    let pubky2 = signer2.public_key();

    // User 1: Create events in different directories (used for basic filtering & reverse tests)
    for i in 0..5 {
        let path = format!("/pub/files/doc_{i}.txt");
        session1.storage().put(path, vec![i as u8]).await.unwrap();
    }
    for i in 0..3 {
        let path = format!("/pub/photos/pic_{i}.jpg");
        session1.storage().put(path, vec![i as u8]).await.unwrap();
    }
    for i in 0..2 {
        let path = format!("/pub/root_{i}.txt");
        session1.storage().put(path, vec![i as u8]).await.unwrap();
    }
    // Add DELETE events
    session1
        .storage()
        .delete("/pub/files/doc_0.txt")
        .await
        .unwrap();

    // User 2: Create events for cursor pagination and multi-user tests
    for i in 0..10 {
        let path = format!("/pub/files/item_{i}.txt");
        session2.storage().put(path, vec![i as u8]).await.unwrap();
    }
    for i in 0..5 {
        let path = format!("/pub/photos/pic_{i}.jpg");
        session2.storage().put(path, vec![i as u8]).await.unwrap();
    }
    for i in 0..3 {
        let path = format!("/pub/data/test_{i}.txt");
        session2.storage().put(path, vec![i as u8]).await.unwrap();
    }

    // ==== Test 1: Basic filtering ====

    // Filter user1 by /pub/files/ - expect 5 PUT + 1 DEL events
    let stream_url = format!(
        "https://{}/events-stream?user={}&path=/pub/files/",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut files_events = Vec::new();
    let mut put_count = 0;
    let mut del_count = 0;
    while files_events.len() < 6 {
        if let Some(Ok(event)) = stream.next().await {
            let path = event.data.lines().next().unwrap();
            assert!(
                path.contains("/pub/files/"),
                "Filter: Expected /pub/files/, got: {}",
                path
            );
            if event.event == "PUT" {
                put_count += 1;
            } else if event.event == "DEL" {
                del_count += 1;
            }
            files_events.push(path.to_string());
        } else {
            break;
        }
    }
    assert_eq!(
        files_events.len(),
        6,
        "Filter: Should get 6 events from /pub/files/ (5 PUT + 1 DEL)"
    );
    assert_eq!(put_count, 5, "Filter: Should have 5 PUT events");
    assert_eq!(del_count, 1, "Filter: Should have 1 DEL event");

    // Filter user1 by broader /pub/ - expect 11 events total (10 PUT + 1 DEL)
    let stream_url = format!(
        "https://{}/events-stream?user={}&path=/pub/",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut pub_events = Vec::new();
    while pub_events.len() < 11 {
        if let Some(Ok(event)) = stream.next().await {
            pub_events.push(event.data.lines().next().unwrap().to_string());
        } else {
            break;
        }
    }
    assert_eq!(
        pub_events.len(),
        11,
        "Filter: Should get 11 events from /pub/ (10 PUT + 1 DEL)"
    );

    // ==== Test 2: Filter with cursor pagination ====

    // Get first 5 with cursor
    let stream_url = format!(
        "https://{}/events-stream?user={}&path=/pub/files/&limit=5",
        server.public_key(),
        pubky2
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut first_batch = Vec::new();
    let mut cursor = String::new();

    while first_batch.len() < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                let lines: Vec<&str> = event.data.lines().collect();
                assert!(
                    lines[0].contains("/pub/files/"),
                    "Cursor: Expected filtered path"
                );
                first_batch.push(lines[0].to_string());
                if let Some(c) = lines.iter().find(|l| l.starts_with("cursor: ")) {
                    cursor = c.strip_prefix("cursor: ").unwrap().to_string();
                }
            }
        } else {
            break;
        }
    }
    drop(stream);
    assert_eq!(first_batch.len(), 5, "Cursor: Should get first 5 events");

    // Get remaining 5 with cursor
    let stream_url = format!(
        "https://{}/events-stream?user={}:{}&path=/pub/files/",
        server.public_key(),
        pubky2,
        cursor
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut second_batch = Vec::new();

    while second_batch.len() < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                let path = event.data.lines().next().unwrap();
                assert!(
                    !first_batch.contains(&path.to_string()),
                    "Cursor: Duplicate event"
                );
                second_batch.push(path.to_string());
            }
        } else {
            break;
        }
    }
    assert_eq!(
        second_batch.len(),
        5,
        "Cursor: Should get remaining 5 events"
    );

    // ==== Test 3: Filter with multiple users ====
    // Filter both users by files directory - user1 has 6 events (5 PUT + 1 DEL), user2 has 10
    let stream_url = format!(
        "https://{}/events-stream?user={}&user={}&path=/pub/files/",
        server.public_key(),
        pubky1,
        pubky2
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut multi_events = Vec::new();

    while multi_events.len() < 16 {
        if let Some(Ok(event)) = stream.next().await {
            let path = event.data.lines().next().unwrap();
            assert!(
                path.contains("/pub/files/"),
                "Multi-user: Expected filtered path"
            );
            multi_events.push(path.to_string());
        } else {
            break;
        }
    }

    assert_eq!(
        multi_events.len(),
        16,
        "Multi-user: Should get 16 filtered events (user1: 6, user2: 10)"
    );
    let user1_count = multi_events
        .iter()
        .filter(|e| e.contains(&pubky1.to_string()))
        .count();
    let user2_count = multi_events
        .iter()
        .filter(|e| e.contains(&pubky2.to_string()))
        .count();
    assert_eq!(user1_count, 6, "Multi-user: Should get 6 from user1");
    assert_eq!(user2_count, 10, "Multi-user: Should get 10 from user2");

    // ==== Test 4: Filter with reverse ordering ====
    // Use user1's /pub/files/ which has 5 PUT + 1 DEL = 6 events

    // Forward order
    let stream_url = format!(
        "https://{}/events-stream?user={}&path=/pub/files/&limit=6",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut forward = Vec::new();
    while forward.len() < 6 {
        if let Some(Ok(event)) = stream.next().await {
            if let Some(fname) = event
                .data
                .lines()
                .next()
                .and_then(|p| p.split("/pub/files/").nth(1))
            {
                forward.push(format!("{}:{}", event.event, fname));
            }
        } else {
            break;
        }
    }

    // Reverse order
    let stream_url = format!(
        "https://{}/events-stream?user={}&path=/pub/files/&reverse=true&limit=6",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut reverse = Vec::new();
    while reverse.len() < 6 {
        if let Some(Ok(event)) = stream.next().await {
            if let Some(fname) = event
                .data
                .lines()
                .next()
                .and_then(|p| p.split("/pub/files/").nth(1))
            {
                reverse.push(format!("{}:{}", event.event, fname));
            }
        } else {
            break;
        }
    }

    assert_eq!(forward.len(), 6, "Reverse: Forward should have 6 events");
    assert_eq!(reverse.len(), 6, "Reverse: Reverse should have 6 events");
    assert_eq!(
        forward[0], "PUT:doc_0.txt",
        "Reverse: Forward first should be PUT doc_0"
    );
    assert_eq!(
        reverse[0], "DEL:doc_0.txt",
        "Reverse: Reverse first should be DEL doc_0 (newest)"
    );

    // Verify reverse order is exactly the reverse of forward order
    let mut fwd_rev = forward.clone();
    fwd_rev.reverse();
    assert_eq!(reverse, fwd_rev, "Reverse: Should be exact reverse");

    // ==== Test 5: Filter with special LIKE characters (_, %) ====
    // Test that underscore in path doesn't act as wildcard
    // Create paths with underscores and similar names
    session1
        .storage()
        .put("/pub/my_folder/file.txt", vec![1])
        .await
        .unwrap();
    session1
        .storage()
        .put("/pub/myfolder/file.txt", vec![2]) // Similar but no underscore
        .await
        .unwrap();

    // Filter by /pub/my_folder/ - should only get files from my_folder, not myfolder
    let stream_url = format!(
        "https://{}/events-stream?user={}&path=/pub/my_folder/",
        server.public_key(),
        pubky1
    );
    let response = pubky
        .client()
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut wildcard_test_events = Vec::new();
    // Try to read up to 2 events to ensure we don't get myfolder
    while let Some(Ok(event)) = stream.next().await {
        let path = event.data.lines().next().unwrap();
        wildcard_test_events.push(path.to_string());
        if wildcard_test_events.len() >= 2 {
            break;
        }
    }

    assert_eq!(
        wildcard_test_events.len(),
        1,
        "Wildcard: Should get exactly 1 event from /pub/my_folder/, not from /pub/myfolder/"
    );
    assert!(
        wildcard_test_events[0].contains("/pub/my_folder/"),
        "Wildcard: Path should contain /pub/my_folder/. Got: {}",
        wildcard_test_events[0]
    );
    assert!(
        !wildcard_test_events[0].contains("/pub/myfolder/"),
        "Wildcard: Should not match /pub/myfolder/. Got: {}",
        wildcard_test_events[0]
    );
}

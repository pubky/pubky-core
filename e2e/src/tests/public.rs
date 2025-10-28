use pkarr::Keypair;
use pubky_testnet::{pubky_homeserver::MockDataDir, EphemeralTestnet, Testnet};
use rand::rng;
use rand::seq::SliceRandom;
use reqwest::{Method, StatusCode};

#[tokio::test]
#[pubky_testnet::test]
async fn put_get_delete() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
    let url = url.as_str();

    client
        .put(url)
        .body(vec![0, 1, 2, 3, 4])
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Use Pubky native method to get data from homeserver
    let response = client.get(url).send().await.unwrap();

    let content_header = response.headers().get("content-type").unwrap();
    // Tests if MIME type was inferred correctly from the file path (magic bytes do not work)
    assert_eq!(content_header, "text/plain");

    let byte_value = response.bytes().await.unwrap();
    assert_eq!(byte_value, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

    // Use regular web method to get data from homeserver (with query pubky-host)
    let regular_url = format!(
        "{}pub/foo.txt?pubky-host={}",
        server.icann_http_url(),
        keypair.public_key()
    );

    // We set `non.pubky.host` header as otherwise he client will use by default
    // the homeserver pubky as host and this request will resolve the `/pub/foo.txt` of
    // the wrong tenant user
    let response = client
        .get(regular_url)
        .header("Host", "non.pubky.host")
        .send()
        .await
        .unwrap();

    let content_header = response.headers().get("content-type").unwrap();
    // Tests if MIME type was inferred correctly from the file path (magic bytes do not work)
    assert_eq!(content_header, "text/plain");

    let byte_value = response.bytes().await.unwrap();
    assert_eq!(byte_value, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

    client
        .delete(url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let response = client.get(url).send().await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
#[pubky_testnet::test]
async fn put_quota_applied() {
    // Start a test homeserver with 1 MB user data limit
    let mut testnet = Testnet::new().await.unwrap();
    let client = testnet.pubky_client().unwrap();

    let mut mock_dir = MockDataDir::test();
    mock_dir.config_toml.general.user_storage_quota_mb = 1; // 1 MB
    let server = testnet
        .create_homeserver_suite_with_mock(mock_dir)
        .await
        .unwrap();

    let keypair = Keypair::random();

    // Signup
    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let url = format!("pubky://{}/pub/data", keypair.public_key());

    // First 600 KB → OK
    let data: Vec<u8> = vec![0; 600_000];
    let resp = client.put(&url).body(data.clone()).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Overwriting the data 600 KB → should 201
    let resp = client.put(&url).body(data.clone()).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Writing now 600 KB more on a different path (totals 1.2 MB) → should 507
    let url_2 = format!("pubky://{}/pub/data2", keypair.public_key());
    let resp = client.put(&url_2).body(data).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::INSUFFICIENT_STORAGE);

    // Overwriting the data 600 KB with 1100KB → should 507
    let data_2: Vec<u8> = vec![0; 1_100_000];
    let resp = client.put(&url).body(data_2).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::INSUFFICIENT_STORAGE);

    // Delete the original data of 600 KB → should 204 and user usage go down to 0 bytes
    let resp = client.delete(&url).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Write exactly 1025 KB → should 507 because it exactly exceeds quota
    let data_3: Vec<u8> = vec![0; 1025 * 1024 - 256];
    let resp = client.put(&url).body(data_3).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::INSUFFICIENT_STORAGE);

    // Write exactly 1 MB → should 201 because it exactly fits within quota
    let data_3: Vec<u8> = vec![0; 1024 * 1024 - 256];
    let resp = client.put(&url).body(data_3).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
#[pubky_testnet::test]
async fn unauthorized_put_delete() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let public_key = keypair.public_key();

    let url = format!("pubky://{public_key}/pub/foo.txt");
    let url = url.as_str();

    let other_client = testnet.pubky_client().unwrap();
    {
        let other = Keypair::random();

        // TODO: remove extra client after switching to subdomains.
        other_client
            .signup(&other, &server.public_key(), None)
            .await
            .unwrap();

        assert_eq!(
            other_client
                .put(url)
                .body(vec![0, 1, 2, 3, 4])
                .send()
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }

    client
        .put(url)
        .body(vec![0, 1, 2, 3, 4])
        .send()
        .await
        .unwrap();

    {
        let other = Keypair::random();

        // TODO: remove extra client after switching to subdomains.
        other_client
            .signup(&other, &server.public_key(), None)
            .await
            .unwrap();

        assert_eq!(
            other_client.delete(url).send().await.unwrap().status(),
            StatusCode::UNAUTHORIZED
        );
    }

    let response = client.get(url).send().await.unwrap().bytes().await.unwrap();

    assert_eq!(response, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));
}

#[tokio::test]
#[pubky_testnet::test]
async fn list_deep() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let pubky = keypair.public_key();
    // Write files to the server
    let mut urls = vec![
        format!("pubky://{pubky}/pub/a.wrong/a.txt"),
        format!("pubky://{pubky}/pub/example.com/a.txt"),
        format!("pubky://{pubky}/pub/example.com/b.txt"),
        format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
        format!("pubky://{pubky}/pub/example.wrong/a.txt"),
        format!("pubky://{pubky}/pub/example.com/c.txt"),
        format!("pubky://{pubky}/pub/example.com/d.txt"),
        format!("pubky://{pubky}/pub/z.wrong/a.txt"),
    ];
    urls.shuffle(&mut rng()); // Shuffle randomly to test the order of the list
    for url in urls {
        client.put(url).body(vec![0]).send().await.unwrap();
    }

    // List all files with no cursor, no limit
    let url = format!("pubky://{pubky}/pub/example.com/extra");
    {
        let list = client.list(&url).unwrap().send().await.unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/a.txt"),
                format!("pubky://{pubky}/pub/example.com/b.txt"),
                format!("pubky://{pubky}/pub/example.com/c.txt"),
                format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                format!("pubky://{pubky}/pub/example.com/d.txt"),
            ],
            "normal list with no limit or cursor"
        );
    }

    // List files with limit of 2
    {
        let list = client.list(&url).unwrap().limit(2).send().await.unwrap();
        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/a.txt"),
                format!("pubky://{pubky}/pub/example.com/b.txt"),
            ],
            "normal list with limit but no cursor"
        );
    }

    // List files with limit of 2 and a file cursor
    {
        let list = client
            .list(&url)
            .unwrap()
            .limit(2)
            .cursor(format!("pubky://{pubky}/pub/example.com/a.txt").as_str())
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/b.txt"),
                format!("pubky://{pubky}/pub/example.com/c.txt"),
            ],
            "normal list with limit and a file cursor"
        );
    }

    {
        let list = client
            .list(&url)
            .unwrap()
            .limit(2)
            .cursor(&format!("pubky://{pubky}/pub/example.com/a.txt"))
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/b.txt"),
                format!("pubky://{pubky}/pub/example.com/c.txt"),
            ],
            "normal list with limit and a full url cursor"
        );
    }
}

#[tokio::test]
#[pubky_testnet::test]
async fn list_shallow() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let pubky = keypair.public_key();

    // Write files to the server
    let mut urls = vec![
        format!("pubky://{pubky}/pub/a.com/a.txt"),
        format!("pubky://{pubky}/pub/example.com/a.txt"),
        format!("pubky://{pubky}/pub/example.com/b.txt"),
        format!("pubky://{pubky}/pub/example.com/c.txt"),
        format!("pubky://{pubky}/pub/example.com/d.txt"),
        format!("pubky://{pubky}/pub/example.con/d.txt"),
        format!("pubky://{pubky}/pub/example.con"),
        format!("pubky://{pubky}/pub/file"),
        format!("pubky://{pubky}/pub/file2"),
        format!("pubky://{pubky}/pub/z.com/a.txt"),
    ];
    urls.shuffle(&mut rng()); // Shuffle randomly to test the order of the list
    for url in urls {
        client.put(url).body(vec![0]).send().await.unwrap();
    }

    // List all files with no cursor, no limit
    let url = format!("pubky://{pubky}/pub/");
    {
        let list = client
            .list(&url)
            .unwrap()
            .shallow(true)
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/a.com/"),
                format!("pubky://{pubky}/pub/example.com/"),
                format!("pubky://{pubky}/pub/example.con"),
                format!("pubky://{pubky}/pub/example.con/"),
                format!("pubky://{pubky}/pub/file"),
                format!("pubky://{pubky}/pub/file2"),
                format!("pubky://{pubky}/pub/z.com/"),
            ],
            "normal list shallow"
        );
    }

    // List files with limit of 2
    {
        let list = client
            .list(&url)
            .unwrap()
            .shallow(true)
            .limit(2)
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/a.com/"),
                format!("pubky://{pubky}/pub/example.com/"),
            ],
            "normal list shallow with limit but no cursor"
        );
    }

    // List files with limit of 2 and a file cursor
    let list1 = client
        .list(&url)
        .unwrap()
        .shallow(true)
        .limit(2)
        .cursor(format!("pubky://{pubky}/pub/example.com/").as_str())
        .send()
        .await
        .unwrap();

    assert_eq!(
        list1,
        vec![
            format!("pubky://{pubky}/pub/example.con"),
            format!("pubky://{pubky}/pub/example.con/"),
        ],
        "normal list shallow with limit and a file cursor"
    );
    // Do the same again but without the pubky:// prefix
    let list2 = client
        .list(&url)
        .unwrap()
        .shallow(true)
        .limit(2)
        .cursor(format!("{pubky}/pub/example.com/a.txt").as_str())
        .send()
        .await
        .unwrap();

    assert_eq!(
        list2, list1,
        "normal list shallow with limit and a file cursor without the pubky:// prefix"
    );

    // List files with limit of 3 and a directory cursor
    {
        let list = client
            .list(&url)
            .unwrap()
            .shallow(true)
            .limit(3)
            .cursor(format!("pubky://{pubky}/pub/example.com/").as_str())
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.con"),
                format!("pubky://{pubky}/pub/example.con/"),
                format!("pubky://{pubky}/pub/file"),
            ],
            "normal list shallow with limit and a directory cursor"
        );
    }
}

#[tokio::test]
#[pubky_testnet::test]
async fn list_events() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let pubky = keypair.public_key();

    let urls = vec![
        format!("pubky://{pubky}/pub/a.com/a.txt"),
        format!("pubky://{pubky}/pub/example.com/a.txt"),
        format!("pubky://{pubky}/pub/example.com/b.txt"),
        format!("pubky://{pubky}/pub/example.com/c.txt"),
        format!("pubky://{pubky}/pub/example.com/d.txt"),
        format!("pubky://{pubky}/pub/example.con/d.txt"),
        format!("pubky://{pubky}/pub/example.con"),
        format!("pubky://{pubky}/pub/file"),
        format!("pubky://{pubky}/pub/file2"),
        format!("pubky://{pubky}/pub/z.com/a.txt"),
    ];
    for url in urls {
        client.put(&url).body(vec![0]).send().await.unwrap();
        client.delete(url).send().await.unwrap();
    }

    let feed_url = format!("https://{}/events/", server.public_key());

    let client = testnet.pubky_client().unwrap();

    let cursor;

    {
        let response = client
            .request(Method::GET, format!("{feed_url}?limit=10"))
            .send()
            .await
            .unwrap();

        let text = response.text().await.unwrap();
        let lines = text.split('\n').collect::<Vec<_>>();

        cursor = lines.last().unwrap().split(" ").last().unwrap().to_string();

        assert_eq!(
            lines,
            vec![
                format!("PUT pubky://{pubky}/pub/a.com/a.txt"),
                format!("DEL pubky://{pubky}/pub/a.com/a.txt"),
                format!("PUT pubky://{pubky}/pub/example.com/a.txt"),
                format!("DEL pubky://{pubky}/pub/example.com/a.txt"),
                format!("PUT pubky://{pubky}/pub/example.com/b.txt"),
                format!("DEL pubky://{pubky}/pub/example.com/b.txt"),
                format!("PUT pubky://{pubky}/pub/example.com/c.txt"),
                format!("DEL pubky://{pubky}/pub/example.com/c.txt"),
                format!("PUT pubky://{pubky}/pub/example.com/d.txt"),
                format!("DEL pubky://{pubky}/pub/example.com/d.txt"),
                format!("cursor: {cursor}",)
            ]
        );
    }

    {
        let response = client
            .request(Method::GET, format!("{feed_url}?limit=10&cursor={cursor}"))
            .send()
            .await
            .unwrap();

        let text = response.text().await.unwrap();
        let lines = text.split('\n').collect::<Vec<_>>();

        assert_eq!(
            lines,
            vec![
                format!("PUT pubky://{pubky}/pub/example.con/d.txt"),
                format!("DEL pubky://{pubky}/pub/example.con/d.txt"),
                format!("PUT pubky://{pubky}/pub/example.con"),
                format!("DEL pubky://{pubky}/pub/example.con"),
                format!("PUT pubky://{pubky}/pub/file"),
                format!("DEL pubky://{pubky}/pub/file"),
                format!("PUT pubky://{pubky}/pub/file2"),
                format!("DEL pubky://{pubky}/pub/file2"),
                format!("PUT pubky://{pubky}/pub/z.com/a.txt"),
                format!("DEL pubky://{pubky}/pub/z.com/a.txt"),
                lines.last().unwrap().to_string()
            ]
        )
    }
}

#[tokio::test]
#[pubky_testnet::test]
async fn read_after_event() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let pubky = keypair.public_key();

    let url = format!("pubky://{pubky}/pub/a.com/a.txt");

    client.put(&url).body(vec![0]).send().await.unwrap();

    let feed_url = format!("https://{}/events/", server.public_key());

    let client = testnet.pubky_client().unwrap();

    {
        let response = client
            .request(Method::GET, format!("{feed_url}?limit=10"))
            .send()
            .await
            .unwrap();

        let text = response.text().await.unwrap();
        let lines = text.split('\n').collect::<Vec<_>>();

        let cursor = lines.last().unwrap().split(" ").last().unwrap().to_string();

        assert_eq!(
            lines,
            vec![
                format!("PUT pubky://{pubky}/pub/a.com/a.txt"),
                format!("cursor: {cursor}",)
            ]
        );
    }

    let response = client.get(url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let body = response.bytes().await.unwrap();

    assert_eq!(body.as_ref(), &[0]);
}

#[tokio::test]
#[pubky_testnet::test]
async fn dont_delete_shared_blobs() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let homeserver = testnet.homeserver_suite();

    let client = testnet.pubky_client().unwrap();

    let homeserver_pubky = homeserver.public_key();

    let user_1 = Keypair::random();
    let user_2 = Keypair::random();

    client
        .signup(&user_1, &homeserver_pubky, None)
        .await
        .unwrap();
    client
        .signup(&user_2, &homeserver_pubky, None)
        .await
        .unwrap();

    let user_1_id = user_1.public_key();
    let user_2_id = user_2.public_key();

    let url_1 = format!("pubky://{user_1_id}/pub/pubky.app/file/file_1");
    let url_2 = format!("pubky://{user_2_id}/pub/pubky.app/file/file_1");

    let file = vec![1];
    client.put(&url_1).body(file.clone()).send().await.unwrap();
    client.put(&url_2).body(file.clone()).send().await.unwrap();

    // Delete file 1
    client
        .delete(url_1)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let blob = client
        .get(url_2)
        .send()
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();

    assert_eq!(blob, file);

    let feed_url = format!("https://{}/events/", homeserver.public_key());

    let response = client
        .request(Method::GET, feed_url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let text = response.text().await.unwrap();
    let lines = text.split('\n').collect::<Vec<_>>();

    assert_eq!(
        lines,
        vec![
            format!("PUT pubky://{user_1_id}/pub/pubky.app/file/file_1",),
            format!("PUT pubky://{user_2_id}/pub/pubky.app/file/file_1",),
            format!("DEL pubky://{user_1_id}/pub/pubky.app/file/file_1",),
            lines.last().unwrap().to_string()
        ]
    );
}

/// Comprehensive test for single-user event streaming modes:
/// - Historical event pagination (>100 events across internal batches)
/// - Live event streaming (no historical events)
/// - Phase transition (historical -> live)
/// - Finite limit enforcement
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_basic_modes() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;
    use tokio::time::{timeout, Duration};

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();
    let client = testnet.pubky_client().unwrap();

    // ==== Test 1: Historical auto-pagination (>100 events) ====
    let keypair1 = Keypair::random();
    client
        .signup(&keypair1, &server.public_key(), None)
        .await
        .unwrap();
    let pubky1 = keypair1.public_key();

    // Create 250 events (internal batch size is 100, tests pagination)
    for i in 0..250 {
        let url = format!("pubky://{pubky1}/pub/file_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    let stream_url = format!(
        "https://{}/events-stream?user={}&limit=250",
        server.public_key(),
        pubky1
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut event_count = 0;
    let mut last_cursor = String::new();

    while event_count < 250 {
        if let Some(Ok(event)) = stream.next().await {
            assert_eq!(event.event, "PUT");
            if let Some(cursor_line) = event.data.lines().find(|l| l.starts_with("cursor: ")) {
                last_cursor = cursor_line.strip_prefix("cursor: ").unwrap().to_string();
            }
            event_count += 1;
        } else {
            break;
        }
    }

    assert_eq!(event_count, 250, "Historical: Should receive all 250 events");
    assert!(!last_cursor.is_empty(), "Historical: Should have cursor");
    assert!(stream.next().await.is_none(), "Historical: Should close after limit");

    // ==== Test 2: Live mode (no historical events) ====
    let keypair2 = Keypair::random();
    client
        .signup(&keypair2, &server.public_key(), None)
        .await
        .unwrap();
    let pubky2 = keypair2.public_key();

    let stream_url = format!(
        "https://{}/events-stream?user={}",
        server.public_key(),
        pubky2
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();

    // Create live event
    let pubky2_clone = pubky2.clone();
    let client_clone = client.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(200)).await;
        let url = format!("pubky://{pubky2_clone}/pub/live_test.txt");
        client_clone.put(&url).body(vec![42]).send().await.unwrap();
    });

    let result = timeout(Duration::from_secs(5), async {
        while let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" && event.data.contains("live_test.txt") {
                return Some(event);
            }
        }
        None
    }).await;

    assert!(result.is_ok(), "Live: Should receive event within timeout");
    assert!(result.unwrap().is_some(), "Live: Should receive the live event");

    // ==== Test 3: Phase transition (historical -> live) ====
    let keypair3 = Keypair::random();
    client
        .signup(&keypair3, &server.public_key(), None)
        .await
        .unwrap();
    let pubky3 = keypair3.public_key();

    // Create historical events
    for i in 0..5 {
        let url = format!("pubky://{pubky3}/pub/historical_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }
    tokio::time::sleep(Duration::from_millis(100)).await;

    let stream_url = format!(
        "https://{}/events-stream?user={}",
        server.public_key(),
        pubky3
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    let mut stream = response.bytes_stream().eventsource();

    // Schedule live event
    let pubky3_clone = pubky3.clone();
    let client_clone = client.clone();
    tokio::spawn(async move {
        tokio::time::sleep(Duration::from_millis(500)).await;
        let url = format!("pubky://{pubky3_clone}/pub/live_after_historical.txt");
        client_clone.put(&url).body(vec![99]).send().await.unwrap();
    });

    let mut historical_count = 0;
    let mut received_live = false;

    let result = timeout(Duration::from_secs(10), async {
        while let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                if event.data.contains("historical_") {
                    historical_count += 1;
                } else if event.data.contains("live_after_historical") {
                    received_live = true;
                    break;
                }
            }
        }
    }).await;

    assert!(result.is_ok(), "Transition: Should complete within timeout");
    assert_eq!(historical_count, 5, "Transition: Should get 5 historical events");
    assert!(received_live, "Transition: Should get live event after historical");

    // ==== Test 4: Finite limit enforcement ====
    let keypair4 = Keypair::random();
    client
        .signup(&keypair4, &server.public_key(), None)
        .await
        .unwrap();
    let pubky4 = keypair4.public_key();

    for i in 0..100 {
        let url = format!("pubky://{pubky4}/pub/file_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    let stream_url = format!(
        "https://{}/events-stream?user={}&limit=50",
        server.public_key(),
        pubky4
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut event_count = 0;

    while event_count < 50 {
        if let Some(Ok(event)) = stream.next().await {
            assert_eq!(event.event, "PUT");
            event_count += 1;
        } else {
            break;
        }
    }

    assert_eq!(event_count, 50, "Limit: Should receive exactly 50 events");
    assert!(stream.next().await.is_none(), "Limit: Should close after limit");
}

#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_cursor_pagination() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();
    let client = testnet.pubky_client().unwrap();

    let keypair = Keypair::random();
    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let pubky = keypair.public_key();

    // Create 10 events
    for i in 0..10 {
        let url = format!("pubky://{pubky}/pub/file_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    // First, get events without cursor to obtain a cursor from the 5th event
    let stream_url = format!(
        "https://{}/events-stream?user={}&limit=5",
        server.public_key(),
        pubky
    );
    let response = client
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();

    let mut stream = response.bytes_stream().eventsource();
    let mut cursor = String::new();
    let mut count = 0;

    // Get first 5 events and extract cursor
    while count < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                if let Some(cursor_line) = event.data.lines().find(|l| l.starts_with("cursor: ")) {
                    cursor = cursor_line.strip_prefix("cursor: ").unwrap().to_string();
                }
                count += 1;
            }
        } else {
            break;
        }
    }

    drop(stream);

    // Now connect with cursor - should only get events 6-10
    let stream_url = format!(
        "https://{}/events-stream?user={}:{}",
        server.public_key(),
        pubky,
        cursor
    );
    let response = client
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();

    let mut stream = response.bytes_stream().eventsource();
    let mut remaining_count = 0;

    // Collect remaining events
    while remaining_count < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                // Verify these are the later events
                let event_num = event
                    .data
                    .lines()
                    .next()
                    .and_then(|line| line.split("file_").nth(1))
                    .and_then(|s| s.split('.').next())
                    .and_then(|s| s.parse::<usize>().ok());

                if let Some(num) = event_num {
                    assert!(
                        num >= 5,
                        "Should only receive events after cursor (file_5 or later)"
                    );
                }
                remaining_count += 1;
            }
        } else {
            break;
        }
    }

    assert_eq!(
        remaining_count, 5,
        "Should receive exactly 5 events after cursor"
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_multiple_users() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();
    let client = testnet.pubky_client().unwrap();

    let keypair1 = Keypair::random();
    let keypair2 = Keypair::random();
    let keypair3 = Keypair::random();

    // Create three users
    client
        .signup(&keypair1, &server.public_key(), None)
        .await
        .unwrap();
    client
        .signup(&keypair2, &server.public_key(), None)
        .await
        .unwrap();
    client
        .signup(&keypair3, &server.public_key(), None)
        .await
        .unwrap();

    let pubky1 = keypair1.public_key();
    let pubky2 = keypair2.public_key();
    let pubky3 = keypair3.public_key();

    // Create different events for each user
    for i in 0..3 {
        let url = format!("pubky://{pubky1}/pub/test1_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }
    for i in 0..2 {
        let url = format!("pubky://{pubky2}/pub/test2_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }
    for i in 0..4 {
        let url = format!("pubky://{pubky3}/pub/test3_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    // Stream events for user1 and user2 (should get 5 events total)
    let stream_url = format!(
        "https://{}/events-stream?user={}&user={}",
        server.public_key(),
        pubky1,
        pubky2
    );

    let response = client
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

    let response = client
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

    let response = client
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
    // This will FAIL if the cursor implementation is broken for multiple users
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
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();
    let client = testnet.pubky_client().unwrap();

    let keypair1 = Keypair::random();
    let keypair2 = Keypair::random();

    // Sign up user1, leave user2 not registered
    client
        .signup(&keypair1, &server.public_key(), None)
        .await
        .unwrap();

    let pubky1 = keypair1.public_key();
    let pubky2 = keypair2.public_key(); // Not registered
    let invalid_pubkey = "invalid_key_not_zbase32";

    // Test 1: No user parameter
    let stream_url = format!("https://{}/events-stream", server.public_key());
    let response = client
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
    let response = client
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
    let response = client
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
    assert!(body.contains("Invalid user public key"));

    // Test 4: Valid key but user not registered
    let stream_url = format!(
        "https://{}/events-stream?user={}",
        server.public_key(),
        pubky2
    );
    let response = client
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
    let response = client
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
    let response = client
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
    assert!(body.contains("Invalid user public key"));

    // Test 7: Multiple invalid keys
    let stream_url = format!(
        "https://{}/events-stream?user={}&user={}",
        server.public_key(),
        invalid_pubkey,
        "another_invalid_key"
    );
    let response = client
        .request(Method::GET, &stream_url)
        .send()
        .await
        .unwrap();
    assert_eq!(
        response.status(),
        StatusCode::BAD_REQUEST,
        "Multiple invalid keys"
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_reverse() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();
    let client = testnet.pubky_client().unwrap();

    let keypair = Keypair::random();
    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let pubky = keypair.public_key();

    // Create 10 events with identifiable content
    for i in 0..10 {
        let url = format!("pubky://{pubky}/pub/file_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    // Test forward order (reverse=false) - should get oldest first
    let stream_url_forward = format!(
        "https://{}/events-stream?user={}&limit=10",
        server.public_key(),
        pubky
    );
    let response = client
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
        pubky
    );
    let response = client
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

    // Test that stream closes after all events are fetched with reverse=true
    // (i.e., phase 2 of SSE is not entered)
    let stream_url_close_test = format!(
        "https://{}/events-stream?user={}&reverse=true&limit=10",
        server.public_key(),
        pubky
    );
    let response = client
        .request(Method::GET, &stream_url_close_test)
        .send()
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut event_count = 0;

    // Collect all events
    while let Some(result) = stream.next().await {
        if result.is_ok() {
            event_count += 1;
        }
    }

    assert_eq!(event_count, 10, "Should receive exactly 10 events");

    // Try to read one more event - stream should be closed
    let next_event = stream.next().await;
    assert!(next_event.is_none(), "Stream should close after all events are fetched with reverse=true (phase 2 should not be entered)");
}

/// Comprehensive test for directory filtering (`filter_dir` parameter):
/// - Basic filtering by different directory paths
/// - Filter with cursor pagination
/// - Filter with multiple users
/// - Filter with reverse ordering
#[tokio::test]
#[pubky_testnet::test]
async fn events_stream_filter_dir() {
    use eventsource_stream::Eventsource;
    use futures::StreamExt;

    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();
    let client = testnet.pubky_client().unwrap();

    // ==== Test 1: Basic filtering ====
    let keypair1 = Keypair::random();
    client.signup(&keypair1, &server.public_key(), None).await.unwrap();
    let pubky1 = keypair1.public_key();

    // Create events in different directories
    for i in 0..3 {
        let url = format!("pubky://{pubky1}/pub/files/doc_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }
    for i in 0..2 {
        let url = format!("pubky://{pubky1}/pub/photos/pic_{i}.jpg");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }
    for i in 0..2 {
        let url = format!("pubky://{pubky1}/pub/root_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    // Filter by /pub/files/ - expect 3 events
    let stream_url = format!(
        "https://{}/events-stream?user={}&filter_dir=/pub/files/",
        server.public_key(), pubky1
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);

    let mut stream = response.bytes_stream().eventsource();
    let mut files_events = Vec::new();
    while files_events.len() < 3 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                let path = event.data.lines().next().unwrap();
                assert!(path.contains("/pub/files/"), "Filter: Expected /pub/files/, got: {}", path);
                files_events.push(path.to_string());
            }
        } else { break; }
    }
    assert_eq!(files_events.len(), 3, "Filter: Should get 3 events from /pub/files/");

    // Filter by broader /pub/ - expect 7 events total
    let stream_url = format!(
        "https://{}/events-stream?user={}&filter_dir=/pub/",
        server.public_key(), pubky1
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut pub_events = Vec::new();
    while pub_events.len() < 7 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                pub_events.push(event.data.lines().next().unwrap().to_string());
            }
        } else { break; }
    }
    assert_eq!(pub_events.len(), 7, "Filter: Should get 7 events from /pub/");

    // ==== Test 2: Filter with cursor pagination ====
    let keypair2 = Keypair::random();
    client.signup(&keypair2, &server.public_key(), None).await.unwrap();
    let pubky2 = keypair2.public_key();

    for i in 0..10 {
        let url = format!("pubky://{pubky2}/pub/files/doc_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }
    for i in 0..5 {
        let url = format!("pubky://{pubky2}/pub/photos/pic_{i}.jpg");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    // Get first 5 with cursor
    let stream_url = format!(
        "https://{}/events-stream?user={}&filter_dir=/pub/files/&limit=5",
        server.public_key(), pubky2
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut first_batch = Vec::new();
    let mut cursor = String::new();

    while first_batch.len() < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                let lines: Vec<&str> = event.data.lines().collect();
                assert!(lines[0].contains("/pub/files/"), "Cursor: Expected filtered path");
                first_batch.push(lines[0].to_string());
                if let Some(c) = lines.iter().find(|l| l.starts_with("cursor: ")) {
                    cursor = c.strip_prefix("cursor: ").unwrap().to_string();
                }
            }
        } else { break; }
    }
    drop(stream);
    assert_eq!(first_batch.len(), 5, "Cursor: Should get first 5 events");

    // Get remaining 5 with cursor
    let stream_url = format!(
        "https://{}/events-stream?user={}:{}&filter_dir=/pub/files/",
        server.public_key(), pubky2, cursor
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut second_batch = Vec::new();

    while second_batch.len() < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                let path = event.data.lines().next().unwrap();
                assert!(!first_batch.contains(&path.to_string()), "Cursor: Duplicate event");
                second_batch.push(path.to_string());
            }
        } else { break; }
    }
    assert_eq!(second_batch.len(), 5, "Cursor: Should get remaining 5 events");

    // ==== Test 3: Filter with multiple users ====
    let keypair3 = Keypair::random();
    client.signup(&keypair3, &server.public_key(), None).await.unwrap();
    let pubky3 = keypair3.public_key();

    // User 2: 3 in files, 2 in photos
    for i in 0..3 {
        let url = format!("pubky://{pubky2}/pub/data/files/item_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }
    for i in 0..2 {
        let url = format!("pubky://{pubky2}/pub/data/photos/pic_{i}.jpg");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    // User 3: 2 in files, 3 in photos
    for i in 0..2 {
        let url = format!("pubky://{pubky3}/pub/data/files/item_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }
    for i in 0..3 {
        let url = format!("pubky://{pubky3}/pub/data/photos/pic_{i}.jpg");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    // Filter both users by files directory
    let stream_url = format!(
        "https://{}/events-stream?user={}&user={}&filter_dir=/pub/data/files/",
        server.public_key(), pubky2, pubky3
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut multi_events = Vec::new();

    while multi_events.len() < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if event.event == "PUT" {
                let path = event.data.lines().next().unwrap();
                assert!(path.contains("/pub/data/files/"), "Multi-user: Expected filtered path");
                multi_events.push(path.to_string());
            }
        } else { break; }
    }

    assert_eq!(multi_events.len(), 5, "Multi-user: Should get 5 filtered events");
    let user2_count = multi_events.iter().filter(|e| e.contains(&pubky2.to_string())).count();
    let user3_count = multi_events.iter().filter(|e| e.contains(&pubky3.to_string())).count();
    assert_eq!(user2_count, 3, "Multi-user: Should get 3 from user2");
    assert_eq!(user3_count, 2, "Multi-user: Should get 2 from user3");

    // ==== Test 4: Filter with reverse ordering ====
    let keypair4 = Keypair::random();
    client.signup(&keypair4, &server.public_key(), None).await.unwrap();
    let pubky4 = keypair4.public_key();

    for i in 0..5 {
        let url = format!("pubky://{pubky4}/pub/files/doc_{i}.txt");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }
    for i in 0..3 {
        let url = format!("pubky://{pubky4}/pub/photos/pic_{i}.jpg");
        client.put(&url).body(vec![i as u8]).send().await.unwrap();
    }

    // Forward order
    let stream_url = format!(
        "https://{}/events-stream?user={}&filter_dir=/pub/files/&limit=5",
        server.public_key(), pubky4
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut forward = Vec::new();
    while forward.len() < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if let Some(fname) = event.data.lines().next().and_then(|p| p.split("/pub/files/").nth(1)) {
                forward.push(fname.to_string());
            }
        } else { break; }
    }

    // Reverse order
    let stream_url = format!(
        "https://{}/events-stream?user={}&filter_dir=/pub/files/&reverse=true&limit=5",
        server.public_key(), pubky4
    );
    let response = client.request(Method::GET, &stream_url).send().await.unwrap();
    let mut stream = response.bytes_stream().eventsource();
    let mut reverse = Vec::new();
    while reverse.len() < 5 {
        if let Some(Ok(event)) = stream.next().await {
            if let Some(fname) = event.data.lines().next().and_then(|p| p.split("/pub/files/").nth(1)) {
                reverse.push(fname.to_string());
            }
        } else { break; }
    }

    assert_eq!(forward[0], "doc_0.txt", "Reverse: Forward first should be doc_0");
    assert_eq!(reverse[0], "doc_4.txt", "Reverse: Reverse first should be doc_4");
    let mut fwd_rev = forward.clone();
    fwd_rev.reverse();
    assert_eq!(reverse, fwd_rev, "Reverse: Should be exact reverse");
}

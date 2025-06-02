use bytes::Bytes;
use pkarr::Keypair;
use pubky_testnet::{pubky_homeserver::MockDataDir, EphemeralTestnet, Testnet};
use reqwest::{Method, StatusCode};

#[tokio::test]
async fn put_get_delete() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

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
async fn put_magic_bytes_file() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let url = format!("pubky://{}/pub/foopng", keypair.public_key());
    let url = url.as_str();

    let png_bytes = vec![0x89_u8, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];

    client
        .put(url)
        .body(png_bytes.clone())
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let response = client.get(url).send().await.unwrap();

    let content_header = response.headers().get("content-type").unwrap();
    assert_eq!(content_header, "image/png");

    let byte_value = response.bytes().await.unwrap();
    assert_eq!(byte_value, bytes::Bytes::from(png_bytes));
}

#[tokio::test]
async fn put_quota_applied() {
    // Start a test homeserver with 1 MB user data limit
    let mut testnet = Testnet::new().await.unwrap();
    let client = testnet.pubky_client_builder().build().unwrap();

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
    let data_3: Vec<u8> = vec![0; 1025 * 1024];
    let resp = client.put(&url).body(data_3).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::INSUFFICIENT_STORAGE);

    // Write exactly 1 MB → should 201 because it exactly fits within quota
    let data_3: Vec<u8> = vec![0; 1024 * 1024];
    let resp = client.put(&url).body(data_3).send().await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn unauthorized_put_delete() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let public_key = keypair.public_key();

    let url = format!("pubky://{public_key}/pub/foo.txt");
    let url = url.as_str();

    let other_client = testnet.pubky_client_builder().build().unwrap();
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
async fn list() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let pubky = keypair.public_key();

    let urls = vec![
        format!("pubky://{pubky}/pub/a.wrong/a.txt"),
        format!("pubky://{pubky}/pub/example.com/a.txt"),
        format!("pubky://{pubky}/pub/example.com/b.txt"),
        format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
        format!("pubky://{pubky}/pub/example.wrong/a.txt"),
        format!("pubky://{pubky}/pub/example.com/c.txt"),
        format!("pubky://{pubky}/pub/example.com/d.txt"),
        format!("pubky://{pubky}/pub/z.wrong/a.txt"),
    ];

    for url in urls {
        client.put(url).body(vec![0]).send().await.unwrap();
    }

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

    {
        let list = client
            .list(&url)
            .unwrap()
            .limit(2)
            .cursor("a.txt")
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
            .cursor("cc-nested/")
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                format!("pubky://{pubky}/pub/example.com/d.txt"),
            ],
            "normal list with limit and a directory cursor"
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

    {
        let list = client
            .list(&url)
            .unwrap()
            .limit(2)
            .cursor("/a.txt")
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/b.txt"),
                format!("pubky://{pubky}/pub/example.com/c.txt"),
            ],
            "normal list with limit and a leading / cursor"
        );
    }

    {
        let list = client
            .list(&url)
            .unwrap()
            .reverse(true)
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/d.txt"),
                format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                format!("pubky://{pubky}/pub/example.com/c.txt"),
                format!("pubky://{pubky}/pub/example.com/b.txt"),
                format!("pubky://{pubky}/pub/example.com/a.txt"),
            ],
            "reverse list with no limit or cursor"
        );
    }

    {
        let list = client
            .list(&url)
            .unwrap()
            .reverse(true)
            .limit(2)
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/d.txt"),
                format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
            ],
            "reverse list with limit but no cursor"
        );
    }

    {
        let list = client
            .list(&url)
            .unwrap()
            .reverse(true)
            .limit(2)
            .cursor("d.txt")
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/cc-nested/z.txt"),
                format!("pubky://{pubky}/pub/example.com/c.txt"),
            ],
            "reverse list with limit and cursor"
        );
    }
}

#[tokio::test]
async fn list_shallow() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

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
        client.put(url).body(vec![0]).send().await.unwrap();
    }

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

    {
        let list = client
            .list(&url)
            .unwrap()
            .shallow(true)
            .limit(2)
            .cursor("example.com/a.txt")
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.com/"),
                format!("pubky://{pubky}/pub/example.con"),
            ],
            "normal list shallow with limit and a file cursor"
        );
    }

    {
        let list = client
            .list(&url)
            .unwrap()
            .shallow(true)
            .limit(3)
            .cursor("example.com/")
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

    {
        let list = client
            .list(&url)
            .unwrap()
            .reverse(true)
            .shallow(true)
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/z.com/"),
                format!("pubky://{pubky}/pub/file2"),
                format!("pubky://{pubky}/pub/file"),
                format!("pubky://{pubky}/pub/example.con/"),
                format!("pubky://{pubky}/pub/example.con"),
                format!("pubky://{pubky}/pub/example.com/"),
                format!("pubky://{pubky}/pub/a.com/"),
            ],
            "reverse list shallow"
        );
    }

    {
        let list = client
            .list(&url)
            .unwrap()
            .reverse(true)
            .shallow(true)
            .limit(2)
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/z.com/"),
                format!("pubky://{pubky}/pub/file2"),
            ],
            "reverse list shallow with limit but no cursor"
        );
    }

    {
        let list = client
            .list(&url)
            .unwrap()
            .shallow(true)
            .reverse(true)
            .limit(2)
            .cursor("file2")
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/file"),
                format!("pubky://{pubky}/pub/example.con/"),
            ],
            "reverse list shallow with limit and a file cursor"
        );
    }

    {
        let list = client
            .list(&url)
            .unwrap()
            .shallow(true)
            .reverse(true)
            .limit(2)
            .cursor("example.con/")
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("pubky://{pubky}/pub/example.con"),
                format!("pubky://{pubky}/pub/example.com/"),
            ],
            "reverse list shallow with limit and a directory cursor"
        );
    }
}

#[tokio::test]
async fn list_events() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

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

    let client = testnet.pubky_client_builder().build().unwrap();

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
async fn read_after_event() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let pubky = keypair.public_key();

    let url = format!("pubky://{pubky}/pub/a.com/a.txt");

    client.put(&url).body(vec![0]).send().await.unwrap();

    let feed_url = format!("https://{}/events/", server.public_key());

    let client = testnet.pubky_client_builder().build().unwrap();

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
async fn dont_delete_shared_blobs() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let homeserver = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

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

#[tokio::test]
async fn stream() {
    // TODO: test better streaming API
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_suite();

    let client = testnet.pubky_client_builder().build().unwrap();

    let keypair = Keypair::random();

    client
        .signup(&keypair, &server.public_key(), None)
        .await
        .unwrap();

    let url = format!("pubky://{}/pub/foo.txt", keypair.public_key());
    let url = url.as_str();

    let bytes = Bytes::from(vec![0; 1024 * 1024]);

    client.put(url).body(bytes.clone()).send().await.unwrap();

    let response = client.get(url).send().await.unwrap().bytes().await.unwrap();

    assert_eq!(response, bytes);

    client.delete(url).send().await.unwrap();

    let response = client.get(url).send().await.unwrap();

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}

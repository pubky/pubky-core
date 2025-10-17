use bytes::Bytes;
use pubky_testnet::{
    pubky::{errors::RequestError, Error, IntoPubkyResource, Keypair, Method, StatusCode},
    pubky_homeserver::MockDataDir,
    EphemeralTestnet, Testnet,
};

#[tokio::test]
async fn put_get_delete() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());

    let session = signer.signup(&server.public_key(), None).await.unwrap();
    let public_key = session.info().public_key();

    // relative URL is always based over own user homeserver
    let path = "/pub/foo.txt";

    session
        .storage()
        .put(path, vec![0, 1, 2, 3, 4])
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Use Pubky native method to get data from homeserver
    let response = pubky
        .public_storage()
        .get(format!("pubky{public_key}/{path}"))
        .await
        .unwrap();

    let content_header = response.headers().get("content-type").unwrap();
    // Tests if MIME type was inferred correctly from the file path (magic bytes do not work)
    assert_eq!(content_header, "text/plain");

    let byte_value = response.bytes().await.unwrap();
    assert_eq!(byte_value, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

    // Use regular web method to get data from homeserver (with query pubky-host)
    let regular_url = format!(
        "{}pub/foo.txt?pubky-host={}",
        server.icann_http_url(),
        session.info().public_key()
    );

    // We set `non.pubky.host` header as otherwise he client will use by default
    // the homeserver pubky as host and this request will resolve the `/pub/foo.txt` of
    // the wrong tenant user
    let response = session
        .client()
        .request(Method::GET, &regular_url)
        .header("Host", "non.pubky.host")
        .send()
        .await
        .unwrap();

    let content_header = response.headers().get("content-type").unwrap();
    // Tests if MIME type was inferred correctly from the file path (magic bytes do not work)
    assert_eq!(content_header, "text/plain");

    let byte_value = response.bytes().await.unwrap();
    assert_eq!(byte_value, bytes::Bytes::from(vec![0, 1, 2, 3, 4]));

    session
        .storage()
        .delete(path)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    // Should not exist, PubkyError of 404 type
    assert!(session.storage().get(path).await.is_err());
}

use serde::{Deserialize, Serialize};

#[tokio::test]
async fn put_then_get_json_roundtrip() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());

    let session = signer.signup(&server.public_key(), None).await.unwrap();
    let public_key = session.info().public_key();

    #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
    struct Payload {
        msg: String,
        n: u32,
        flag: bool,
    }

    let path = "/pub/data/roundtrip.json";
    let expected = Payload {
        msg: "hello".to_string(),
        n: 42,
        flag: true,
    };

    // Ignore the result; the write still succeeds and is asserted via the subsequent GET.
    let _ = session.storage().put_json(path, &expected).await;

    // Read back as strongly-typed JSON and assert equality.
    let got: Payload = pubky
        .public_storage()
        .get_json(format!("pubky{}/{path}", public_key))
        .await
        .unwrap();
    assert_eq!(got, expected);

    // Sanity-check MIME is JSON when fetching raw.
    let resp = session.storage().get(path).await.unwrap();
    let ct = resp
        .headers()
        .get("content-type")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(ct.starts_with("application/json"));

    // Cleanup
    session
        .storage()
        .delete(path)
        .await
        .unwrap()
        .error_for_status()
        .unwrap();
}

#[tokio::test]
async fn put_quota_applied() {
    // Start a test homeserver with 1 MB user data limit
    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();

    let mut mock_dir = MockDataDir::test();
    mock_dir.config_toml.general.user_storage_quota_mb = 1; // 1 MB
    let server = testnet.create_homeserver_with_mock(mock_dir).await.unwrap();

    // Create a user/session
    let signer = pubky.signer(Keypair::random());
    let session = signer.signup(&server.public_key(), None).await.unwrap();

    let p1 = "/pub/data";
    let p2 = "/pub/data2";

    // First 600 KB → OK (201)
    let data_600k: Vec<u8> = vec![0; 600_000];
    let resp = session.storage().put(p1, data_600k.clone()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Overwrite same 600 KB → still 201
    let resp = session.storage().put(p1, data_600k.clone()).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);

    // Write 600 KB more at a different path (total 1.2 MB) → 507
    let err = session
        .storage()
        .put(p2, data_600k.clone())
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::Request(RequestError::Server { status, .. })
            if status == StatusCode::INSUFFICIENT_STORAGE
    ));

    // Overwrite /pub/data with 1.1 MB → 507
    let data_1100k: Vec<u8> = vec![0; 1_100_000];
    let err = session.storage().put(p1, data_1100k).await.unwrap_err();
    assert!(matches!(
        err,
        Error::Request(RequestError::Server { status, .. })
            if status == StatusCode::INSUFFICIENT_STORAGE
    ));

    // Delete the original 600 KB → 204
    let resp = session.storage().delete(p1).await.unwrap();
    assert_eq!(resp.status(), StatusCode::NO_CONTENT);

    // Write exactly 1025 KB → 507 (exceeds 1 MB quota)
    let data_1025k_minus_256: Vec<u8> = vec![0; 1025 * 1024 - 256];
    let err = session
        .storage()
        .put(p1, data_1025k_minus_256)
        .await
        .unwrap_err();
    assert!(matches!(
        err,
        Error::Request(RequestError::Server { status, .. })
            if status == StatusCode::INSUFFICIENT_STORAGE
    ));

    // Write exactly 1 MB (minus the same 256 fudge) → 201 (fits quota)
    let data_1mb_minus_256: Vec<u8> = vec![0; 1024 * 1024 - 256];
    let resp = session.storage().put(p1, data_1mb_minus_256).await.unwrap();
    assert_eq!(resp.status(), StatusCode::CREATED);
}

#[tokio::test]
async fn unauthorized_put_delete() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    // Owner user
    let owner = pubky.signer(Keypair::random());
    let owner_session = owner.signup(&server.public_key(), None).await.unwrap();

    let path = "/pub/foo.txt";

    // Someone tries to write to owner's namespace -> 401 Unauthorized
    let owner_url = format!(
        "pubky{}/{}",
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

#[tokio::test]
async fn list() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    let public_key = signer.public_key();

    let session = signer.signup(&server.public_key(), None).await.unwrap();

    let paths = vec![
        "/pub/a.wrong/a.txt",
        "/pub/example.com/a.txt",
        "/pub/example.com/b.txt",
        "/pub/example.com/cc-nested/z.txt",
        "/pub/example.wrong/a.txt",
        "/pub/example.com/c.txt",
        "/pub/example.com/d.txt",
        "/pub/z.wrong/a.txt",
    ];

    for path in paths {
        session.storage().put(path, vec![0]).await.unwrap();
    }

    let path = "/pub/example.com/";

    {
        let list = session.storage().list(path).unwrap().send().await.unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/a.txt"),
                format!("pubky{public_key}/pub/example.com/b.txt"),
                format!("pubky{public_key}/pub/example.com/c.txt"),
                format!("pubky{public_key}/pub/example.com/cc-nested/z.txt"),
                format!("pubky{public_key}/pub/example.com/d.txt"),
            ],
            "normal list with no limit or cursor"
        );
    }

    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .limit(2)
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/a.txt"),
                format!("pubky{public_key}/pub/example.com/b.txt"),
            ],
            "normal list with limit but no cursor"
        );
    }

    {
        let list = pubky
            .public_storage()
            .list(format!("pubky{public_key}{path}"))
            .unwrap()
            .limit(2)
            .cursor("a.txt")
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/b.txt"),
                format!("pubky{public_key}/pub/example.com/c.txt"),
            ],
            "normal list with limit and a file cursor"
        );
    }

    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .limit(2)
            .cursor("cc-nested/")
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/cc-nested/z.txt"),
                format!("pubky{public_key}/pub/example.com/d.txt"),
            ],
            "normal list with limit and a directory cursor"
        );
    }

    {
        let list = pubky
            .public_storage()
            .list(format!("pubky://{public_key}{path}"))
            .unwrap()
            .limit(2)
            .cursor(&format!("pubky://{public_key}/pub/example.com/a.txt"))
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/b.txt"),
                format!("pubky{public_key}/pub/example.com/c.txt"),
            ],
            "normal list with limit and a full url cursor"
        );
    }

    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .limit(2)
            .cursor("/a.txt")
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/b.txt"),
                format!("pubky{public_key}/pub/example.com/c.txt"),
            ],
            "normal list with limit and a leading / cursor"
        );
    }

    {
        let list = pubky
            .public_storage()
            .list(format!("pubky://{public_key}{path}"))
            .unwrap()
            .reverse(true)
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/d.txt"),
                format!("pubky{public_key}/pub/example.com/cc-nested/z.txt"),
                format!("pubky{public_key}/pub/example.com/c.txt"),
                format!("pubky{public_key}/pub/example.com/b.txt"),
                format!("pubky{public_key}/pub/example.com/a.txt"),
            ],
            "reverse list with no limit or cursor"
        );
    }

    {
        let list = pubky
            .public_storage()
            .list(format!("pubky{public_key}{path}"))
            .unwrap()
            .reverse(true)
            .limit(2)
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/d.txt"),
                format!("pubky{public_key}/pub/example.com/cc-nested/z.txt"),
            ],
            "reverse list with limit but no cursor"
        );
    }

    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .reverse(true)
            .limit(2)
            .cursor("d.txt")
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/cc-nested/z.txt"),
                format!("pubky{public_key}/pub/example.com/c.txt"),
            ],
            "reverse list with limit and cursor"
        );
    }
}

#[tokio::test]
async fn list_shallow() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    // Create a user/session
    let signer = pubky.signer(Keypair::random());
    let public_key = signer.public_key();
    let session = signer.signup(&server.public_key(), None).await.unwrap();

    // Seed data: first-level dirs/files under /pub plus nested content.
    let paths = vec![
        "/pub/a.com/a.txt",
        "/pub/example.com/a.txt",
        "/pub/example.com/b.txt",
        "/pub/example.com/c.txt",
        "/pub/example.com/d.txt",
        "/pub/example.xyz/d.txt",
        "/pub/example.xyz", // a file at top-level named "example.xyz"
        "/pub/file",
        "/pub/file2",
        "/pub/z.com/a.txt",
    ];
    for p in paths {
        session.storage().put(p, vec![0]).await.unwrap();
    }

    let path = "/pub/";

    // shallow (no limit, no cursor)
    {
        let list = pubky
            .public_storage()
            .list(format!("pubky{public_key}/{path}"))
            .unwrap()
            .shallow(true)
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/a.com/"),
                format!("pubky{public_key}/pub/example.com/"),
                format!("pubky{public_key}/pub/example.xyz"),
                format!("pubky{public_key}/pub/example.xyz/"),
                format!("pubky{public_key}/pub/file"),
                format!("pubky{public_key}/pub/file2"),
                format!("pubky{public_key}/pub/z.com/"),
            ],
            "normal list shallow"
        );
    }

    // shallow + limit(2)
    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .shallow(true)
            .limit(2)
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/a.com/"),
                format!("pubky{public_key}/pub/example.com/"),
            ],
            "normal list shallow with limit but no cursor"
        );
    }

    // shallow + limit(2) + file cursor
    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .shallow(true)
            .limit(2)
            .cursor("example.com/a.txt")
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.com/"),
                format!("pubky{public_key}/pub/example.xyz"),
            ],
            "normal list shallow with limit and a file cursor"
        );
    }

    // shallow + limit(3) + directory cursor
    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .shallow(true)
            .limit(3)
            .cursor("example.com/")
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.xyz"),
                format!("pubky{public_key}/pub/example.xyz/"),
                format!("pubky{public_key}/pub/file"),
            ],
            "normal list shallow with limit and a directory cursor"
        );
    }

    // shallow + reverse
    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .reverse(true)
            .shallow(true)
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/z.com/"),
                format!("pubky{public_key}/pub/file2"),
                format!("pubky{public_key}/pub/file"),
                format!("pubky{public_key}/pub/example.xyz/"),
                format!("pubky{public_key}/pub/example.xyz"),
                format!("pubky{public_key}/pub/example.com/"),
                format!("pubky{public_key}/pub/a.com/"),
            ],
            "reverse list shallow"
        );
    }

    // shallow + reverse + limit(2)
    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .reverse(true)
            .shallow(true)
            .limit(2)
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/z.com/"),
                format!("pubky{public_key}/pub/file2"),
            ],
            "reverse list shallow with limit but no cursor"
        );
    }

    // shallow + reverse + limit(2) + file cursor
    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .shallow(true)
            .reverse(true)
            .limit(2)
            .cursor("file2")
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/file"),
                format!("pubky{public_key}/pub/example.xyz/"),
            ],
            "reverse list shallow with limit and a file cursor"
        );
    }

    // shallow + reverse + limit(2) + directory cursor
    {
        let list = session
            .storage()
            .list(path)
            .unwrap()
            .shallow(true)
            .reverse(true)
            .limit(2)
            .cursor("example.xyz/")
            .send()
            .await
            .unwrap();
        let list: Vec<String> = list.into_iter().map(|u| u.to_string()).collect();

        assert_eq!(
            list,
            vec![
                format!("pubky{public_key}/pub/example.xyz"),
                format!("pubky{public_key}/pub/example.com/"),
            ],
            "reverse list shallow with limit and a directory cursor"
        );
    }
}

#[tokio::test]
async fn list_events() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    // Create a user/session
    let signer = pubky.signer(Keypair::random());
    let public_key = signer.public_key();
    let session = signer.signup(&server.public_key(), None).await.unwrap();

    // Write + delete a bunch of files to populate the event feed
    let paths = vec![
        "/pub/a.com/a.txt",
        "/pub/example.com/a.txt",
        "/pub/example.com/b.txt",
        "/pub/example.com/c.txt",
        "/pub/example.com/d.txt",
        "/pub/example.xyz/d.txt",
        "/pub/example.xyz", // file (not dir)
        "/pub/file",
        "/pub/file2",
        "/pub/z.com/a.txt",
    ];
    for p in &paths {
        session.storage().put(p.to_string(), vec![0]).await.unwrap();
        session.storage().delete(p.to_string()).await.unwrap();
    }

    // Feed is exposed under the public-key host
    let feed_url = format!("https://{}/events/", server.public_key());

    // Page 1
    let cursor: String = {
        let page1_url = format!("{feed_url}?limit=10");
        let resp = session
            .client()
            .request(Method::GET, &page1_url)
            .send()
            .await
            .unwrap();

        let text = resp.text().await.unwrap();
        let lines = text.split('\n').collect::<Vec<_>>();

        // last line is "cursor: <id>"
        let cursor = lines.last().unwrap().split(' ').last().unwrap().to_string();

        assert_eq!(
            lines,
            vec![
                format!("PUT pubky://{public_key}/pub/a.com/a.txt"),
                format!("DEL pubky://{public_key}/pub/a.com/a.txt"),
                format!("PUT pubky://{public_key}/pub/example.com/a.txt"),
                format!("DEL pubky://{public_key}/pub/example.com/a.txt"),
                format!("PUT pubky://{public_key}/pub/example.com/b.txt"),
                format!("DEL pubky://{public_key}/pub/example.com/b.txt"),
                format!("PUT pubky://{public_key}/pub/example.com/c.txt"),
                format!("DEL pubky://{public_key}/pub/example.com/c.txt"),
                format!("PUT pubky://{public_key}/pub/example.com/d.txt"),
                format!("DEL pubky://{public_key}/pub/example.com/d.txt"),
                format!("cursor: {cursor}"),
            ]
        );

        cursor
    };

    // Page 2 (using cursor)
    {
        let page2_url = format!("{feed_url}?limit=10&cursor={cursor}");
        let resp = session
            .client()
            .request(Method::GET, &page2_url)
            .send()
            .await
            .unwrap();

        let text = resp.text().await.unwrap();
        let lines = text.split('\n').collect::<Vec<_>>();

        assert_eq!(
            lines,
            vec![
                format!("PUT pubky://{public_key}/pub/example.xyz/d.txt"),
                format!("DEL pubky://{public_key}/pub/example.xyz/d.txt"),
                format!("PUT pubky://{public_key}/pub/example.xyz"),
                format!("DEL pubky://{public_key}/pub/example.xyz"),
                format!("PUT pubky://{public_key}/pub/file"),
                format!("DEL pubky://{public_key}/pub/file"),
                format!("PUT pubky://{public_key}/pub/file2"),
                format!("DEL pubky://{public_key}/pub/file2"),
                format!("PUT pubky://{public_key}/pub/z.com/a.txt"),
                format!("DEL pubky://{public_key}/pub/z.com/a.txt"),
                lines.last().unwrap().to_string(),
            ]
        );
    }
}

#[tokio::test]
async fn read_after_event() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    // User + session
    let signer = pubky.signer(Keypair::random());
    let public_key = signer.public_key();
    let session = signer.signup(&server.public_key(), None).await.unwrap();

    // Write one file
    let url = format!("pubky://{public_key}/pub/a.com/a.txt");
    session
        .storage()
        .put("/pub/a.com/a.txt", vec![0])
        .await
        .unwrap();

    // Events page 1
    let feed_url = format!("https://{}/events/", server.public_key());
    {
        let page_url = format!("{feed_url}?limit=10");
        let resp = pubky
            .client()
            .request(Method::GET, &page_url)
            .send()
            .await
            .unwrap();

        let text = resp.text().await.unwrap();
        let lines = text.split('\n').collect::<Vec<_>>();
        let cursor = lines.last().unwrap().split(' ').last().unwrap().to_string();

        assert_eq!(
            lines,
            vec![format!("PUT {url}"), format!("cursor: {cursor}")]
        );
    }

    // Now the file should exist
    pubky.public_storage().exists(url.clone()).await.unwrap();
    // Provide metadata
    pubky.public_storage().stats(url.clone()).await.unwrap();
    // And be fetchable
    let resp = pubky.public_storage().get(url).await.unwrap();
    assert_eq!(resp.status(), StatusCode::OK);
    let body = resp.bytes().await.unwrap();
    assert_eq!(body.as_ref(), &[0]);
}

#[tokio::test]
async fn dont_delete_shared_blobs() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let homeserver = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    // Two independent users
    let u1 = pubky.signer(Keypair::random());
    let u2 = pubky.signer(Keypair::random());

    let a1 = u1.signup(&homeserver.public_key(), None).await.unwrap();
    let a2 = u2.signup(&homeserver.public_key(), None).await.unwrap();

    let user_1_id = u1.public_key();
    let user_2_id = u2.public_key();

    let p1 = "/pub/pubky.app/file/file_1";
    let p2 = "/pub/pubky.app/file/file_1";

    let file = vec![1];

    // Both write identical content to their own paths
    a1.storage().put(p1, file.clone()).await.unwrap();
    a2.storage().put(p2, file.clone()).await.unwrap();

    // Delete user 1's file
    a1.storage().delete(p1).await.unwrap();

    // User 2's file must still exist and match
    let blob = a2.storage().get(p2).await.unwrap().bytes().await.unwrap();
    assert_eq!(blob, file);

    // Event feed should show PUT u1, PUT u2, DEL u1 (order preserved)
    let feed_url = format!("https://{}/events/", homeserver.public_key());
    let resp = pubky
        .client()
        .request(Method::GET, &feed_url)
        .send()
        .await
        .unwrap()
        .error_for_status()
        .unwrap();

    let text = resp.text().await.unwrap();
    let lines = text.split('\n').collect::<Vec<_>>();

    assert_eq!(
        lines,
        vec![
            format!("PUT pubky://{user_1_id}/pub/pubky.app/file/file_1"),
            format!("PUT pubky://{user_2_id}/pub/pubky.app/file/file_1"),
            format!("DEL pubky://{user_1_id}/pub/pubky.app/file/file_1"),
            lines.last().unwrap().to_string(),
        ]
    );
}

#[tokio::test]
async fn stream() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver();
    let pubky = testnet.sdk().unwrap();

    let signer = pubky.signer(Keypair::random());
    let session = signer.signup(&server.public_key(), None).await.unwrap();

    let path = "/pub/foo.txt";
    let bytes = Bytes::from(vec![0; 1024 * 1024]); // 1 MiB

    // Upload large body
    session.storage().put(path, bytes.clone()).await.unwrap();

    // Read back and compare
    let got = session
        .storage()
        .get(path)
        .await
        .unwrap()
        .bytes()
        .await
        .unwrap();
    assert_eq!(got, bytes);

    // Delete and verify 404 on subsequent GET
    session.storage().delete(path).await.unwrap();
    let err = session.storage().get(path).await.unwrap_err();
    assert!(
        matches!(err, Error::Request(RequestError::Server { status, .. }) if status == StatusCode::NOT_FOUND)
    );
}

use bytes::Bytes;
use pubky_testnet::{
    pubky::{errors::RequestError, Error, IntoPubkyResource, Keypair, Method, StatusCode},
    pubky_homeserver::MockDataDir,
    EphemeralTestnet, Testnet,
};
use rand::rng;
use rand::seq::SliceRandom;

#[tokio::test]
#[pubky_testnet::test]
async fn put_get_delete() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
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
        .get(format!("{public_key}/{path}"))
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
        session.info().public_key().z32()
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
#[pubky_testnet::test]
async fn put_then_get_json_roundtrip() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
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
        .get_json(format!("{public_key}/{path}"))
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
#[pubky_testnet::test]
async fn put_quota_applied() {
    // Start a test homeserver with 1 MB user data limit
    let mut testnet = Testnet::new().await.unwrap();
    let pubky = testnet.sdk().unwrap();

    let mut mock_dir = MockDataDir::test();
    mock_dir.config_toml.general.user_storage_quota_mb = 1; // 1 MB
    let server = testnet
        .create_homeserver_app_with_mock(mock_dir)
        .await
        .unwrap();

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
#[pubky_testnet::test]
async fn unauthorized_put_delete() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Owner user
    let owner = pubky.signer(Keypair::random());
    let owner_session = owner.signup(&server.public_key(), None).await.unwrap();

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

#[tokio::test]
#[pubky_testnet::test]
async fn list_deep() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Owner user
    let owner = pubky.signer(Keypair::random());
    let owner_session = owner.signup(&server.public_key(), None).await.unwrap();
    let public_key = owner_session.info().public_key();
    // Write files to the server
    let mut paths = vec![
        format!("/pub/a.wrong/a.txt"),
        format!("/pub/example.com/a.txt"),
        format!("/pub/example.com/b.txt"),
        format!("/pub/example.com/cc-nested/z.txt"),
        format!("/pub/example.wrong/a.txt"),
        format!("/pub/example.com/c.txt"),
        format!("/pub/example.com/d.txt"),
        format!("/pub/z.wrong/a.txt"),
    ];
    paths.shuffle(&mut rng()); // Shuffle randomly to test the order of the list
    for url in paths {
        owner_session.storage().put(url, vec![0]).await.unwrap();
    }

    // List all files with no cursor, no limit
    let url = format!("/pub/example.com/");
    {
        let list = owner_session
            .storage()
            .list(&url)
            .unwrap()
            .send()
            .await
            .unwrap();
        assert_eq!(
            list,
            vec![
                format!("{public_key}/pub/example.com/a.txt")
                    .parse()
                    .unwrap(),
                format!("{public_key}/pub/example.com/b.txt")
                    .parse()
                    .unwrap(),
                format!("{public_key}/pub/example.com/c.txt")
                    .parse()
                    .unwrap(),
                format!("{public_key}/pub/example.com/cc-nested/z.txt")
                    .parse()
                    .unwrap(),
                format!("{public_key}/pub/example.com/d.txt")
                    .parse()
                    .unwrap(),
            ],
            "normal list with no limit or cursor"
        );
    }

    // List files with limit of 2
    {
        let list = owner_session
            .storage()
            .list(&url)
            .unwrap()
            .limit(2)
            .send()
            .await
            .unwrap();
        assert_eq!(
            list,
            vec![
                format!("{public_key}/pub/example.com/a.txt")
                    .parse()
                    .unwrap(),
                format!("{public_key}/pub/example.com/b.txt")
                    .parse()
                    .unwrap(),
            ],
            "normal list with limit but no cursor"
        );
    }

    // List files with limit of 2 and a file cursor
    {
        let list = owner_session
            .storage()
            .list(&url)
            .unwrap()
            .limit(2)
            .cursor(format!("{}/pub/example.com/a.txt", public_key.z32()).as_str())
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("{public_key}/pub/example.com/b.txt")
                    .parse()
                    .unwrap(),
                format!("{public_key}/pub/example.com/c.txt")
                    .parse()
                    .unwrap(),
            ],
            "normal list with limit and a file cursor"
        );
    }

    {
        let list = owner_session
            .storage()
            .list(&url)
            .unwrap()
            .limit(2)
            .cursor(&format!("{}/pub/example.com/a.txt", public_key.z32()))
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("{public_key}/pub/example.com/b.txt")
                    .parse()
                    .unwrap(),
                format!("{public_key}/pub/example.com/c.txt")
                    .parse()
                    .unwrap(),
            ],
            "normal list with limit and a full url cursor"
        );
    }
}

#[tokio::test]
#[pubky_testnet::test]
async fn list_shallow() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Owner user
    let owner = pubky.signer(Keypair::random());
    let owner_session = owner.signup(&server.public_key(), None).await.unwrap();
    let public_key = owner_session.info().public_key();

    // Write files to the server
    let mut urls = vec![
        format!("/pub/a.com/a.txt"),
        format!("/pub/example.com/a.txt"),
        format!("/pub/example.com/b.txt"),
        format!("/pub/example.com/c.txt"),
        format!("/pub/example.com/d.txt"),
        format!("/pub/example.con/d.txt"),
        format!("/pub/example.con"),
        format!("/pub/file"),
        format!("/pub/file2"),
        format!("/pub/z.com/a.txt"),
    ];
    urls.shuffle(&mut rng()); // Shuffle randomly to test the order of the list
    for url in urls {
        owner_session.storage().put(url, vec![0]).await.unwrap();
    }

    // List all files with no cursor, no limit
    let url = format!("/pub/");
    {
        let list = owner_session
            .storage()
            .list(&url)
            .unwrap()
            .shallow(true)
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("{public_key}/pub/a.com/").parse().unwrap(),
                format!("{public_key}/pub/example.com/").parse().unwrap(),
                format!("{public_key}/pub/example.con").parse().unwrap(),
                format!("{public_key}/pub/example.con/").parse().unwrap(),
                format!("{public_key}/pub/file").parse().unwrap(),
                format!("{public_key}/pub/file2").parse().unwrap(),
                format!("{public_key}/pub/z.com/").parse().unwrap(),
            ],
            "normal list shallow"
        );
    }

    // List files with limit of 2
    {
        let list = owner_session
            .storage()
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
                format!("{public_key}/pub/a.com/").parse().unwrap(),
                format!("{public_key}/pub/example.com/").parse().unwrap(),
            ],
            "normal list shallow with limit but no cursor"
        );
    }

    // List files with limit of 2 and a file cursor
    let list1 = owner_session
        .storage()
        .list(&url)
        .unwrap()
        .shallow(true)
        .limit(2)
        .cursor(format!("{}/pub/example.com/", public_key.z32()).as_str())
        .send()
        .await
        .unwrap();

    assert_eq!(
        list1,
        vec![
            format!("{public_key}/pub/example.con").parse().unwrap(),
            format!("{public_key}/pub/example.con/").parse().unwrap(),
        ],
        "normal list shallow with limit and a file cursor"
    );
    // Do the same again but without the pubky:// prefix
    let list2 = owner_session
        .storage()
        .list(&url)
        .unwrap()
        .shallow(true)
        .limit(2)
        .cursor(format!("{}/pub/example.com/a.txt", public_key.z32()).as_str())
        .send()
        .await
        .unwrap();

    assert_eq!(
        list2, list1,
        "normal list shallow with limit and a file cursor without the pubky:// prefix"
    );

    // List files with limit of 3 and a directory cursor
    {
        let list = owner_session
            .storage()
            .list(&url)
            .unwrap()
            .shallow(true)
            .limit(3)
            .cursor(format!("{}/pub/example.com/", public_key.z32()).as_str())
            .send()
            .await
            .unwrap();

        assert_eq!(
            list,
            vec![
                format!("{public_key}/pub/example.con").parse().unwrap(),
                format!("{public_key}/pub/example.con/").parse().unwrap(),
                format!("{public_key}/pub/file").parse().unwrap(),
            ],
            "normal list shallow with limit and a directory cursor"
        );
    }
}

#[tokio::test]
#[pubky_testnet::test]
async fn list_events() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // Create a user/session
    let signer = pubky.signer(Keypair::random());
    let public_key = signer.public_key();
    let public_key_z32 = public_key.z32();
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
    let feed_url = format!("https://{}/events/", server.public_key().z32());

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
                format!("PUT pubky://{public_key_z32}/pub/a.com/a.txt"),
                format!("DEL pubky://{public_key_z32}/pub/a.com/a.txt"),
                format!("PUT pubky://{public_key_z32}/pub/example.com/a.txt"),
                format!("DEL pubky://{public_key_z32}/pub/example.com/a.txt"),
                format!("PUT pubky://{public_key_z32}/pub/example.com/b.txt"),
                format!("DEL pubky://{public_key_z32}/pub/example.com/b.txt"),
                format!("PUT pubky://{public_key_z32}/pub/example.com/c.txt"),
                format!("DEL pubky://{public_key_z32}/pub/example.com/c.txt"),
                format!("PUT pubky://{public_key_z32}/pub/example.com/d.txt"),
                format!("DEL pubky://{public_key_z32}/pub/example.com/d.txt"),
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
                format!("PUT pubky://{public_key_z32}/pub/example.xyz/d.txt"),
                format!("DEL pubky://{public_key_z32}/pub/example.xyz/d.txt"),
                format!("PUT pubky://{public_key_z32}/pub/example.xyz"),
                format!("DEL pubky://{public_key_z32}/pub/example.xyz"),
                format!("PUT pubky://{public_key_z32}/pub/file"),
                format!("DEL pubky://{public_key_z32}/pub/file"),
                format!("PUT pubky://{public_key_z32}/pub/file2"),
                format!("DEL pubky://{public_key_z32}/pub/file2"),
                format!("PUT pubky://{public_key_z32}/pub/z.com/a.txt"),
                format!("DEL pubky://{public_key_z32}/pub/z.com/a.txt"),
                lines.last().unwrap().to_string(),
            ]
        );
    }
}

#[tokio::test]
#[pubky_testnet::test]
async fn read_after_event() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    // User + session
    let signer = pubky.signer(Keypair::random());
    let public_key = signer.public_key();
    let public_key_z32 = public_key.z32();
    let session = signer.signup(&server.public_key(), None).await.unwrap();

    // Write one file
    let url = format!("pubky://{public_key_z32}/pub/a.com/a.txt");
    session
        .storage()
        .put("/pub/a.com/a.txt", vec![0])
        .await
        .unwrap();

    // Events page 1
    let feed_url = format!("https://{}/events/", server.public_key().z32());
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
#[pubky_testnet::test]
async fn dont_delete_shared_blobs() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let homeserver = testnet.homeserver_app();
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
    let feed_url = format!("https://{}/events/", homeserver.public_key().z32());
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
            format!("PUT pubky://{}/pub/pubky.app/file/file_1", user_1_id.z32()),
            format!("PUT pubky://{}/pub/pubky.app/file/file_1", user_2_id.z32()),
            format!("DEL pubky://{}/pub/pubky.app/file/file_1", user_1_id.z32()),
            lines.last().unwrap().to_string(),
        ]
    );
}

#[tokio::test]
#[pubky_testnet::test]
async fn stream() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
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

/// Test that two users can write to the same path and the content is correctly separated.
/// Mix file and reading between the two users.
#[tokio::test]
#[pubky_testnet::test]
async fn write_same_path_separate_users() {
    let testnet = EphemeralTestnet::start().await.unwrap();
    let server = testnet.homeserver_app();
    let pubky = testnet.sdk().unwrap();

    let signer_a = pubky.signer(Keypair::random());
    let session_a = signer_a.signup(&server.public_key(), None).await.unwrap();
    let signer_b = pubky.signer(Keypair::random());
    let session_b = signer_b.signup(&server.public_key(), None).await.unwrap();

    let path = "/pub/foo.txt";
    let content1 = Bytes::from(b"content1".to_vec());

    // Write to user A content1
    session_a
        .storage()
        .put(path, content1.clone())
        .await
        .unwrap();
    // Read back and compare
    let response = session_a.storage().get(path).await.unwrap();
    let content_length = response
        .headers()
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(content_length, content1.len() as u64);
    let read_bytes_a = response.bytes().await.unwrap();
    assert_eq!(read_bytes_a, content1);

    // Write to user B content1
    session_b
        .storage()
        .put(path, content1.clone())
        .await
        .unwrap();
    // Read back and compare
    let response = session_b.storage().get(path).await.unwrap();
    let content_length = response
        .headers()
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(content_length, content1.len() as u64);
    let read_bytes_b = response.bytes().await.unwrap();
    assert_eq!(read_bytes_b, content1);

    let content2 = Bytes::from(b"content2_long".to_vec());

    // Write to user A content2
    session_a
        .storage()
        .put(path, content2.clone())
        .await
        .unwrap();
    // Read back and compare
    let response = session_a.storage().get(path).await.unwrap();
    let content_length = response
        .headers()
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(content_length, content2.len() as u64);
    let read_bytes_a = response.bytes().await.unwrap();
    assert_eq!(read_bytes_a, content2);

    // Read user B content 1 again. Make sure it's the original content1.
    let response = session_b.storage().get(path).await.unwrap();
    let content_length = response
        .headers()
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(content_length, content1.len() as u64);
    let read_bytes_b = response.bytes().await.unwrap();
    assert_eq!(read_bytes_b, content1);

    // Delete user A content2
    session_a.storage().delete(path).await.unwrap();
    // Read user B content 2 again. Make sure it's the original content1.
    let response = session_b.storage().get(path).await.unwrap();
    let content_length = response
        .headers()
        .get("content-length")
        .unwrap()
        .to_str()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(content_length, content1.len() as u64);
    let read_bytes_b = response.bytes().await.unwrap();
    assert_eq!(read_bytes_b, content1);
}

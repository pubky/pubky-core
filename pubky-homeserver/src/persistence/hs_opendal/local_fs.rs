#[cfg(test)]
mod tests {
    #[tokio::test]
    async fn test_local_fs() {
        let builder = opendal::services::Fs::default()
            .root("/tmp/pubky-homeserver");
        let op = opendal::Operator::new(builder).unwrap().finish();
        let _ = op.write("test.txt", "Hello, world!").await.unwrap();
        let content = op.read("test.txt").await.unwrap();
        let content_str = String::from_utf8(content.to_vec()).unwrap();
        assert_eq!(content_str, "Hello, world!");
    }


    #[tokio::test]
    async fn test_google_bucket() {
        let gcs_builder = opendal::services::Gcs::default()
        .bucket("homeserver-test").credential_path("/Users/severinbuhler/git/pubky/pubky-core/pubky-stag-gcs-account.json")
        .root("/")
        .default_storage_class("STANDARD");

        let op = opendal::Operator::new(gcs_builder).unwrap().finish();
        let _ = op.write("test.txt", "Hello, world!").await.unwrap();
        let content = op.read("test.txt").await.unwrap();
        let content_str = String::from_utf8(content.to_vec()).unwrap();
        assert_eq!(content_str, "Hello, world!");
    }

    #[tokio::test]
    async fn test_in_memory() {
        let builder = opendal::services::Memory::default();
        let op = opendal::Operator::new(builder).unwrap().finish();
        let _ = op.write("test.txt", "Hello, world!").await.unwrap();
        let content = op.read("test.txt").await.unwrap();
        let content_str = String::from_utf8(content.to_vec()).unwrap();
        assert_eq!(content_str, "Hello, world!");
    }
}
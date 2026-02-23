//! Embedded PostgreSQL support for running tests without external Postgres.
//!
//! This module provides an embedded PostgreSQL instance that can be used
//! for integration tests without requiring a separate Postgres installation.

use postgresql_embedded::{PostgreSQL, Settings, VersionReq};
use pubky_homeserver::ConnectionString;
use rand::Rng;
use std::time::Duration;

/// An embedded PostgreSQL instance for testing.
///
/// This wraps `postgresql_embedded::PostgreSQL` and manages its lifecycle,
/// including creating a test database and providing a connection string.
///
/// The embedded Postgres is automatically stopped when this struct is dropped.
pub struct EmbeddedPostgres {
    pg: PostgreSQL,
    database_name: String,
}

impl EmbeddedPostgres {
    /// Start a new embedded PostgreSQL instance.
    ///
    /// This will:
    /// 1. Download PostgreSQL binaries if not already cached (~50-100MB, cached for subsequent runs)
    /// 2. Start the PostgreSQL server
    /// 3. Create a test database with a unique name
    pub async fn start() -> anyhow::Result<Self> {
        let settings = Settings {
            version: VersionReq::parse("=18.1.0")?,
            installation_dir: dirs::cache_dir()
                .unwrap_or_else(std::env::temp_dir)
                .join("pubky-testnet")
                .join("postgresql"),
            timeout: Some(Duration::from_secs(120)),
            ..Default::default()
        };

        let mut pg = PostgreSQL::new(settings);
        pg.setup().await?;
        pg.start().await?;

        // Create a unique database name for this test run
        // Use both timestamp and random suffix to avoid collisions in parallel tests
        let database_name = format!(
            "pubky_test_{}_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
            rand::rng().random::<u32>()
        );

        // Create the test database
        pg.create_database(&database_name).await?;

        Ok(Self { pg, database_name })
    }

    /// Get the connection string for this embedded PostgreSQL instance.
    pub fn connection_string(&self) -> anyhow::Result<ConnectionString> {
        let settings = self.pg.settings();
        let url = format!(
            "postgres://{}:{}@{}:{}/{}",
            settings.username, settings.password, settings.host, settings.port, self.database_name
        );
        ConnectionString::new(&url).map_err(|e| anyhow::anyhow!("Invalid connection string: {}", e))
    }
}

impl Drop for EmbeddedPostgres {
    fn drop(&mut self) {
        // PostgreSQL::drop will handle stopping the server
        // The database will be cleaned up when Postgres stops
        tracing::debug!(
            "Stopping embedded PostgreSQL (database: {})",
            self.database_name
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::EphemeralTestnet;
    use pubky::Keypair;

    /// Comprehensive test for embedded postgres: startup, user operations, HTTP relay, and cleanup.
    /// Consolidated into one test to reduce the number of postgres instances started.
    #[tokio::test]
    async fn test_embedded_postgres_full_lifecycle() {
        // Start testnet with embedded postgres and HTTP relay
        let testnet = EphemeralTestnet::builder()
            .with_embedded_postgres()
            .with_http_relay()
            .build()
            .await
            .expect("Failed to start testnet with embedded postgres");

        // Verify the homeserver is running
        assert!(!testnet.homeserver_app().public_key().to_string().is_empty());

        // Verify HTTP relay is running
        let _ = testnet.http_relay();

        // Test user operations
        let pubky = testnet.sdk().expect("Failed to create SDK");
        let keypair = Keypair::random();
        let signer = pubky.signer(keypair);

        let session = signer
            .signup(&testnet.homeserver_app().public_key(), None)
            .await
            .expect("Failed to signup user");

        // Store and retrieve data
        let path = "/pub/test.txt";
        let data = b"Hello from embedded postgres test!";
        session
            .storage()
            .put(path, data.as_slice())
            .await
            .expect("Failed to store data");

        let response = session
            .storage()
            .get(path)
            .await
            .expect("Failed to get data");
        let bytes = response.bytes().await.expect("Failed to read bytes");
        assert_eq!(bytes.as_ref(), data);

        // Drop first testnet and verify cleanup by creating another
        drop(testnet);

        let testnet2 = EphemeralTestnet::builder()
            .with_embedded_postgres()
            .build()
            .await
            .expect("Failed to start second testnet - cleanup may have failed");

        assert!(!testnet2
            .homeserver_app()
            .public_key()
            .to_string()
            .is_empty());
    }

    /// Test that specifying both embedded postgres and a custom connection string fails.
    /// This test is fast as it fails before starting any postgres instance.
    #[tokio::test]
    async fn test_embedded_postgres_and_custom_connection_string_fails() {
        use pubky_homeserver::ConnectionString;

        let connection = ConnectionString::new("postgres://localhost:5432/test").unwrap();

        let result = EphemeralTestnet::builder()
            .postgres(connection)
            .with_embedded_postgres()
            .build()
            .await;

        match result {
            Ok(_) => panic!("Should fail when both postgres options are set"),
            Err(err) => {
                assert!(
                    err.to_string().contains(
                        "Cannot use both embedded postgres and a custom connection string"
                    ),
                    "Expected error about conflicting postgres options, got: {}",
                    err
                );
            }
        }
    }
}

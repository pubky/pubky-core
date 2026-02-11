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
    /// Test that we can create users and store data with embedded postgres.
    #[tokio::test]
    async fn test_embedded_postgres_user_operations() {
        let testnet = EphemeralTestnet::builder()
            .with_embedded_postgres()
            .build()
            .await
            .expect("Failed to start testnet with embedded postgres");

        let pubky = testnet.sdk().expect("Failed to create SDK");

        // Create a new keypair for the user
        let keypair = Keypair::random();
        let signer = pubky.signer(keypair);

        // Sign up the user
        let session = signer
            .signup(&testnet.homeserver_app().public_key(), None)
            .await
            .expect("Failed to signup user");

        // Store some data
        let path = "/pub/test.txt";
        let data = b"Hello from embedded postgres test!";
        session
            .storage()
            .put(path, data.as_slice())
            .await
            .expect("Failed to store data");

        // Read it back
        let response = session
            .storage()
            .get(path)
            .await
            .expect("Failed to get data");

        let bytes = response.bytes().await.expect("Failed to read bytes");
        assert_eq!(bytes.as_ref(), data);
    }

    /// Test that cleanup works properly when the testnet is dropped.
    #[tokio::test]
    async fn test_embedded_postgres_cleanup() {
        // Create and drop a testnet
        {
            let testnet = EphemeralTestnet::builder()
                .with_embedded_postgres()
                .build()
                .await
                .expect("Failed to start testnet with embedded postgres");

            // Use it briefly
            let _ = testnet.homeserver_app().public_key();
        }

        // Create another testnet - this should work if cleanup was successful
        {
            let testnet = EphemeralTestnet::builder()
                .with_embedded_postgres()
                .build()
                .await
                .expect("Failed to start second testnet with embedded postgres");

            let _ = testnet.homeserver_app().public_key();
        }
    }

    /// Test that embedded postgres works with HTTP relay enabled.
    #[tokio::test]
    async fn test_embedded_postgres_with_http_relay() {
        let testnet = EphemeralTestnet::builder()
            .with_embedded_postgres()
            .with_http_relay()
            .build()
            .await
            .expect("Failed to start testnet with embedded postgres and http relay");

        // Verify both are running
        let _ = testnet.homeserver_app().public_key();
        let _ = testnet.http_relay();
    }

    /// Test that specifying both embedded postgres and a custom connection string fails.
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

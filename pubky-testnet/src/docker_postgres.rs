//! Docker-based PostgreSQL for running tests without external Postgres.
//!
//! This module provides a containerized PostgreSQL instance (via testcontainers)
//! that can be used for integration tests without requiring a separate Postgres
//! installation. Containers are cleaned up on drop and on SIGINT/SIGTERM
//! (via the testcontainers watchdog). Note: `kill -9` will still leave
//! containers orphaned.

use pubky_homeserver::ConnectionString;
use testcontainers::{runners::AsyncRunner, ContainerAsync};
use testcontainers_modules::postgres::Postgres;
use tokio::sync::OnceCell;

/// Shared Docker postgres instance, initialized once per process.
static SHARED_PG: OnceCell<DockerPostgres> = OnceCell::const_new();

/// A containerized PostgreSQL instance for testing.
///
/// This wraps a testcontainers `Postgres` container and manages its lifecycle.
/// The container is automatically stopped and removed when this struct is dropped.
///
/// # Sharing Across Tests (Recommended)
///
/// Each `DockerPostgres::start()` starts a **separate** PostgreSQL container.
/// Use [`DockerPostgres::shared()`] to start **one** instance and share it across tests:
///
/// ```ignore
/// use pubky_testnet::docker_postgres::DockerPostgres;
/// use pubky_testnet::EphemeralTestnet;
///
/// #[tokio::test]
/// async fn my_test() {
///     let pg = DockerPostgres::shared().await;
///     let testnet = EphemeralTestnet::builder()
///         .postgres(pg.connection_string().unwrap())
///         .build()
///         .await
///         .unwrap();
///     // Each testnet gets its own ephemeral database — tests remain isolated.
/// }
/// ```
pub struct DockerPostgres {
    _container: ContainerAsync<Postgres>,
    host: String,
    port: u16,
}

/// Deprecated alias for [`DockerPostgres`].
#[deprecated(since = "0.9.0", note = "Renamed to `DockerPostgres`")]
pub type EmbeddedPostgres = DockerPostgres;

impl DockerPostgres {
    /// Return a shared Docker PostgreSQL instance, starting it on first call.
    ///
    /// This is the recommended way to share a single PostgreSQL container across
    /// multiple tests. Docker handles all cleanup automatically.
    ///
    /// # Panics
    ///
    /// Panics if the container fails to start (e.g., Docker is not running).
    pub async fn shared() -> &'static DockerPostgres {
        SHARED_PG
            .get_or_init(|| async {
                DockerPostgres::start()
                    .await
                    .expect("Failed to start shared Docker postgres. Is Docker running?")
            })
            .await
    }

    /// Start a new Docker PostgreSQL container.
    ///
    /// Requires Docker to be running on the host.
    pub async fn start() -> anyhow::Result<Self> {
        let container = Postgres::default().start().await.map_err(|e| {
            anyhow::anyhow!("Failed to start Postgres container. Is Docker running? Error: {e}")
        })?;

        let host = container.get_host().await?.to_string();
        let port = container.get_host_port_ipv4(5432).await?;

        Ok(Self {
            _container: container,
            host,
            port,
        })
    }

    /// Get the connection string for this Docker PostgreSQL instance.
    pub fn connection_string(&self) -> anyhow::Result<ConnectionString> {
        let url = format!(
            "postgres://postgres:postgres@{}:{}/postgres",
            self.host, self.port
        );
        ConnectionString::new(&url).map_err(|e| anyhow::anyhow!("Invalid connection string: {e}"))
    }

    /// Get the Docker container ID (for diagnostics / verification).
    pub fn container_id(&self) -> &str {
        self._container.id()
    }
}

#[cfg(test)]
mod tests {
    use super::DockerPostgres;
    use crate::EphemeralTestnet;
    use pubky::Keypair;

    /// Basic integration test: start a testnet with docker postgres, signup a user, store and retrieve data.
    #[tokio::test]
    async fn test_docker_postgres_with_testnet() {
        let testnet = EphemeralTestnet::builder()
            .with_docker_postgres()
            .build()
            .await
            .expect("Failed to start testnet with docker postgres");

        // Verify the homeserver is running
        assert!(!testnet.homeserver_app().public_key().to_string().is_empty());

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
        let data = b"Hello from docker postgres test!";
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
    }

    /// Verify that dropping a DockerPostgres actually removes the Docker container.
    #[tokio::test]
    async fn test_container_cleaned_up_on_drop() {
        let pg = DockerPostgres::start()
            .await
            .expect("Failed to start docker postgres");
        let container_id = pg.container_id().to_string();

        // Verify the container is running.
        let output = std::process::Command::new("docker")
            .args(["inspect", "--format", "{{.State.Running}}", &container_id])
            .output()
            .expect("docker inspect failed");
        let running = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(running, "true", "Container should be running before drop");

        // Drop it — testcontainers should stop and remove the container.
        drop(pg);

        // Give Docker a moment to clean up.
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;

        // Verify the container no longer exists.
        let output = std::process::Command::new("docker")
            .args(["inspect", &container_id])
            .output()
            .expect("docker inspect failed");
        assert!(
            !output.status.success(),
            "Container {container_id} should not exist after drop"
        );
    }
}

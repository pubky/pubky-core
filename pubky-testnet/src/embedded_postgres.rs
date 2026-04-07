//! Embedded PostgreSQL support for running tests without external Postgres.
//!
//! This module provides an embedded PostgreSQL instance that can be used
//! for integration tests without requiring a separate Postgres installation.

use postgresql_embedded::{PostgreSQL, Settings, VersionReq};
use pubky_homeserver::ConnectionString;
use rand::Rng;
use std::sync::atomic::{AtomicI32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use tokio::sync::OnceCell;

/// Shared embedded postgres instance, initialized once per process.
static SHARED_PG: OnceCell<EmbeddedPostgres> = OnceCell::const_new();

/// A clone of the shared `PostgreSQL` handle, used solely for atexit cleanup.
/// Wrapped in `Mutex<Option<…>>` so the atexit handler can `take()` it,
/// which triggers `PostgreSQL::Drop` → `pg_ctl stop`.
static SHARED_PG_HANDLE: OnceLock<Mutex<Option<PostgreSQL>>> = OnceLock::new();

/// PID of the shared PostgreSQL child process, used by signal handlers.
/// Signal handlers cannot safely lock a mutex or spawn processes, so they
/// send SIGTERM to this PID directly. Set to 0 when no instance is running.
static SHARED_PG_PID: AtomicI32 = AtomicI32::new(0);

extern "C" {
    fn atexit(func: extern "C" fn()) -> std::ffi::c_int;
}

/// Atexit handler that stops the shared embedded postgres instance.
///
/// Rust does not run `Drop` for `static` values at process exit.
/// This handler takes the cloned `PostgreSQL` out of the static and drops it,
/// which triggers `PostgreSQL::Drop` — the library's own shutdown logic
/// (`pg_ctl stop`, temp directory cleanup).
extern "C" fn stop_shared_postgres() {
    // Clear PID so signal handlers don't race with this cleanup.
    SHARED_PG_PID.store(0, Ordering::Relaxed);

    let Some(mutex) = SHARED_PG_HANDLE.get() else {
        return;
    };
    let Ok(mut guard) = mutex.lock() else {
        return;
    };
    // Dropping the `PostgreSQL` value triggers `pg_ctl stop` synchronously.
    drop(guard.take());
}

/// Signal handler for SIGINT/SIGTERM that kills the shared postgres process.
///
/// Uses only async-signal-safe functions (`kill`, `signal`, `raise`).
/// After killing postgres, restores the default handler and re-raises so the
/// process terminates with the expected signal status.
extern "C" fn signal_handler(sig: libc::c_int) {
    let pid = SHARED_PG_PID.load(Ordering::Relaxed);
    if pid > 0 {
        unsafe {
            libc::kill(pid, libc::SIGTERM);
        }
    }
    // Restore the default handler and re-raise to get normal exit behavior.
    unsafe {
        libc::signal(sig, libc::SIG_DFL);
        libc::raise(sig);
    }
}

/// An embedded PostgreSQL instance for testing.
///
/// This wraps `postgresql_embedded::PostgreSQL` and manages its lifecycle,
/// including creating a test database and providing a connection string.
///
/// The embedded Postgres is automatically stopped when this struct is dropped.
///
/// # Sharing Across Tests (Recommended)
///
/// Each `EmbeddedPostgres::start()` downloads and starts a **separate** PostgreSQL server.
/// Use [`EmbeddedPostgres::shared()`] to start **one** instance and share it across tests:
///
/// ```ignore
/// use pubky_testnet::embedded_postgres::EmbeddedPostgres;
/// use pubky_testnet::EphemeralTestnet;
///
/// #[tokio::test]
/// async fn my_test() {
///     let pg = EmbeddedPostgres::shared().await;
///     let testnet = EphemeralTestnet::builder()
///         .postgres(pg.connection_string().unwrap())
///         .build()
///         .await
///         .unwrap();
///     // Each testnet gets its own ephemeral database — tests remain isolated.
/// }
/// ```
pub struct EmbeddedPostgres {
    pg: PostgreSQL,
    database_name: String,
}

impl EmbeddedPostgres {
    /// Return a shared embedded PostgreSQL instance, starting it on first call.
    ///
    /// This is the recommended way to share a single PostgreSQL server across
    /// multiple tests. Cleanup handlers are registered to prevent orphaned
    /// PostgreSQL child processes:
    ///
    /// - **atexit**: performs full `pg_ctl stop` and temp directory cleanup on
    ///   normal process exit.
    /// - **SIGINT / SIGTERM**: sends `SIGTERM` to the postgres PID so it shuts
    ///   down even if the test runner is interrupted (e.g. Ctrl+C).
    ///
    /// Note: `SIGKILL` cannot be caught, so a `kill -9` on the test process
    /// will still leave postgres orphaned.
    ///
    /// # Panics
    ///
    /// Panics if the embedded PostgreSQL instance fails to start.
    pub async fn shared() -> &'static EmbeddedPostgres {
        SHARED_PG
            .get_or_init(|| async {
                let pg = EmbeddedPostgres::start()
                    .await
                    .expect("Failed to start shared embedded postgres");

                // Store a clone of the PostgreSQL handle for the atexit handler.
                // PostgreSQL is Clone (just settings/paths) — cheap to clone.
                // Note: dropping the clone runs `pg_ctl stop` against the same
                // data_dir, which is the desired cleanup behavior.
                let _ = SHARED_PG_HANDLE.set(Mutex::new(Some(pg.pg.clone())));

                // Read the postgres PID for signal-handler cleanup.
                let pid_file = pg.data_dir().join("postmaster.pid");
                if let Ok(contents) = std::fs::read_to_string(&pid_file) {
                    if let Some(pid) = contents
                        .lines()
                        .next()
                        .and_then(|line| line.trim().parse::<i32>().ok())
                    {
                        SHARED_PG_PID.store(pid, Ordering::Relaxed);
                    }
                }

                // Register cleanup handlers to prevent orphaned postgres processes.
                // - atexit: runs on normal exit, performs full `pg_ctl stop` + temp dir cleanup
                // - SIGINT/SIGTERM: sends SIGTERM to postgres PID (async-signal-safe)
                unsafe {
                    atexit(stop_shared_postgres);
                    libc::signal(libc::SIGINT, signal_handler as libc::sighandler_t);
                    libc::signal(libc::SIGTERM, signal_handler as libc::sighandler_t);
                }

                pg
            })
            .await
    }

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

    /// Get the data directory of the embedded PostgreSQL instance.
    pub fn data_dir(&self) -> &std::path::Path {
        &self.pg.settings().data_dir
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

    /// Test that concurrent calls to `shared()` all return the same instance.
    #[tokio::test]
    async fn test_shared_returns_same_instance_concurrently() {
        use super::EmbeddedPostgres;

        // Spawn multiple concurrent calls to shared().
        // Cast to usize to make the future Send-safe (raw pointers aren't Send).
        let handles: Vec<_> = (0..5)
            .map(|_| {
                tokio::spawn(async {
                    EmbeddedPostgres::shared().await as *const EmbeddedPostgres as usize
                })
            })
            .collect();

        let mut addrs = Vec::new();
        for handle in handles {
            addrs.push(handle.await.expect("task panicked"));
        }

        // All addresses should be identical — same &'static instance.
        let first = addrs[0];
        for (i, addr) in addrs.iter().enumerate().skip(1) {
            assert_eq!(
                first, *addr,
                "shared() call {i} returned a different instance"
            );
        }
    }

    /// Test that `SHARED_PG_PID` gracefully stays at 0 when the PID file is
    /// missing or unparseable, rather than panicking.
    #[test]
    fn test_pid_parsing_graceful_on_missing_file() {
        use std::sync::atomic::Ordering;

        // Ensure the static is at its default (0) or was previously set.
        // We can't reset the OnceCell, but we can verify the parsing logic
        // in isolation by testing the same code path directly.
        let tmp = std::env::temp_dir().join("nonexistent_postmaster.pid");
        // File doesn't exist — read_to_string should fail, PID stays unchanged.
        let before = super::SHARED_PG_PID.load(Ordering::Relaxed);
        if let Ok(contents) = std::fs::read_to_string(&tmp) {
            if let Some(pid) = contents
                .lines()
                .next()
                .and_then(|line| line.trim().parse::<i32>().ok())
            {
                super::SHARED_PG_PID.store(pid, Ordering::Relaxed);
            }
        }
        let after = super::SHARED_PG_PID.load(Ordering::Relaxed);
        assert_eq!(before, after, "PID should not change when file is missing");
    }

    /// Test that PID parsing handles a malformed PID file without panicking.
    #[test]
    fn test_pid_parsing_graceful_on_bad_content() {
        use std::io::Write;
        use std::sync::atomic::Ordering;

        let tmp = std::env::temp_dir().join("bad_postmaster.pid");
        {
            let mut f = std::fs::File::create(&tmp).expect("create temp file");
            writeln!(f, "not_a_number").expect("write temp file");
        }

        let before = super::SHARED_PG_PID.load(Ordering::Relaxed);
        if let Ok(contents) = std::fs::read_to_string(&tmp) {
            if let Some(pid) = contents
                .lines()
                .next()
                .and_then(|line| line.trim().parse::<i32>().ok())
            {
                super::SHARED_PG_PID.store(pid, Ordering::Relaxed);
            }
        }
        let after = super::SHARED_PG_PID.load(Ordering::Relaxed);
        assert_eq!(
            before, after,
            "PID should not change when file content is not a valid PID"
        );

        let _ = std::fs::remove_file(&tmp);
    }
}

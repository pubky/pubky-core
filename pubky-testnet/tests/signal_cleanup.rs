//! Integration test verifying that `EmbeddedPostgres::shared()` cleans up
//! the PostgreSQL child process when the test process receives SIGINT.
//!
//! How it works:
//! 1. The parent spawns itself as a child process with `__SIGINT_CHILD=1`.
//! 2. The child calls `EmbeddedPostgres::shared()`, prints the postgres PID,
//!    then sleeps indefinitely (simulating a long-running test).
//! 3. The parent reads the PID, sends SIGINT to the child, then verifies
//!    that the postgres process is no longer running.

#[cfg(feature = "embedded-postgres")]
#[tokio::test]
async fn sigint_cleans_up_shared_postgres() {
    use std::io::BufRead;

    if std::env::var("__SIGINT_CHILD").is_ok() {
        // === Child process ===
        let pg = pubky_testnet::embedded_postgres::EmbeddedPostgres::shared().await;
        let data_dir = pg.data_dir();
        let pid_file = data_dir.join("postmaster.pid");

        let contents =
            std::fs::read_to_string(&pid_file).expect("postmaster.pid should exist after start");
        let pid = contents
            .lines()
            .next()
            .expect("postmaster.pid should have a first line")
            .trim();

        // Print the PID so the parent can read it, then sleep forever.
        // The parent will send SIGINT to terminate us.
        println!("PG_PID={pid}");

        // Sleep indefinitely — parent will SIGINT us.
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        }
    }

    // === Parent process ===
    let exe = std::env::current_exe().expect("can't find test binary path");

    let mut child = std::process::Command::new(&exe)
        .arg("sigint_cleans_up_shared_postgres")
        .arg("--exact")
        .arg("--nocapture")
        .env("__SIGINT_CHILD", "1")
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .expect("failed to spawn child process");

    let stdout = child.stdout.take().expect("stdout was piped");
    let reader = std::io::BufReader::new(stdout);

    // Read lines until we find PG_PID=...
    // This blocks until the child prints the PID (i.e., postgres is running).
    let mut pg_pid: Option<i32> = None;
    for line in reader.lines() {
        let line = line.expect("failed to read child stdout");
        if let Some(pid_str) = line.strip_prefix("PG_PID=") {
            pg_pid = Some(
                pid_str
                    .parse()
                    .unwrap_or_else(|_| panic!("invalid PID: {pid_str}")),
            );
            break;
        }
    }

    let pg_pid = pg_pid.expect("child did not print PG_PID=...");

    // Verify postgres is actually running before we send the signal.
    assert!(
        unsafe { libc::kill(pg_pid, 0) == 0 },
        "PostgreSQL process (PID {pg_pid}) should be running before SIGINT"
    );

    // Send SIGINT to the child process (simulates Ctrl+C).
    let child_pid = child.id() as i32;
    unsafe {
        libc::kill(child_pid, libc::SIGINT);
    }

    // Wait for the child to exit.
    let status = child.wait().expect("failed to wait for child");
    // The child should have been killed by SIGINT (exit via signal, not success).
    assert!(
        !status.success(),
        "child should have exited due to SIGINT, got: {status}"
    );

    // Poll until the postgres process exits (up to 30 seconds).
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        let still_alive = unsafe { libc::kill(pg_pid, 0) == 0 };
        if !still_alive {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "PostgreSQL process (PID {pg_pid}) still running after 30s — signal cleanup failed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    eprintln!("Verified: PostgreSQL process (PID {pg_pid}) was cleaned up after SIGINT");
}

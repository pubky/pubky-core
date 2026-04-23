//! Integration test verifying that `EmbeddedPostgres::shared()` cleans up
//! the PostgreSQL child process on exit (via the atexit handler).
//!
//! How it works:
//! 1. The test spawns itself as a child process with `__ATEXIT_CHILD=1`.
//! 2. The child calls `EmbeddedPostgres::shared()`, prints the postgres PID
//!    (read from `postmaster.pid`), and exits normally.
//! 3. The parent reads the PID from the child's stdout, waits briefly, then
//!    checks that the postgres process is no longer running.

#[cfg(feature = "embedded-postgres")]
#[tokio::test]
async fn atexit_cleans_up_shared_postgres() {
    if std::env::var("__ATEXIT_CHILD").is_ok() {
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

        // Print the PID so the parent can read it.
        println!("PG_PID={pid}");

        // Exit normally — the atexit handler should stop postgres.
        return;
    }

    // === Parent process ===
    let exe = std::env::current_exe().expect("can't find test binary path");

    let output = std::process::Command::new(&exe)
        .arg("atexit_cleans_up_shared_postgres")
        .arg("--exact")
        .arg("--nocapture")
        .env("__ATEXIT_CHILD", "1")
        .output()
        .expect("failed to spawn child process");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        output.status.success(),
        "child process failed.\nstdout: {stdout}\nstderr: {stderr}"
    );

    // Extract the PID from the child's stdout.
    let pid_str = stdout
        .lines()
        .find_map(|line| line.strip_prefix("PG_PID="))
        .unwrap_or_else(|| panic!("child did not print PG_PID=...\nstdout: {stdout}"));
    let pid: i32 = pid_str
        .parse()
        .unwrap_or_else(|_| panic!("invalid PID: {pid_str}"));

    // Poll until the postgres process exits (up to 30 seconds).
    // Avoids a fixed sleep that could flake on slow CI.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    loop {
        // kill(pid, 0) returns 0 if the process exists, -1 if it doesn't.
        let still_alive = unsafe { libc::kill(pid, 0) == 0 };
        if !still_alive {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "PostgreSQL process (PID {pid}) still running after 30s — atexit cleanup failed"
        );
        tokio::time::sleep(std::time::Duration::from_millis(250)).await;
    }

    eprintln!("Verified: PostgreSQL process (PID {pid}) was cleaned up by atexit handler");
}

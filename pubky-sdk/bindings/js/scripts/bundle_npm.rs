use std::env;
use std::io;
use std::process::{Command, ExitStatus, Stdio};

// If the process hangs, try `cargo clean` to remove all locks.

fn main() {
    println!("Building wasm for pubky...");

    build_wasm("nodejs").unwrap();
    patch().unwrap();
}

fn build_wasm(target: &str) -> io::Result<ExitStatus> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    let status = Command::new("wasm-pack")
        .args([
            "build",
            &manifest_dir,
            "--release",
            "--target",
            target,
            "--out-dir",
            &format!("pkg/{}", target),
            "--out-name",
            "pubky",
        ])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        eprintln!("wasm-pack {target} failed with status: {status}");
    }

    Ok(status)
}

fn patch() -> io::Result<ExitStatus> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    println!("{manifest_dir}/scripts/patch.mjs");
    let status = Command::new("node")
        .args([format!("{manifest_dir}/scripts/patch.mjs")])
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        eprintln!("patch.mjs failed with status: {status}");
    }

    Ok(status)
}

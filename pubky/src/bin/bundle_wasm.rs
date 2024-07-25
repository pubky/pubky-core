use std::io;
use std::process::{Command, ExitStatus};

// If the process hangs, try `cargo clean` to remove all locks.

fn main() {
    println!("cargo:rerun-if-changed=client/");

    build_wasm("nodejs").unwrap();
    patch().unwrap();
}

fn build_wasm(target: &str) -> io::Result<ExitStatus> {
    let output = Command::new("wasm-pack")
        .args([
            "build",
            "--release",
            "--target",
            target,
            "--out-dir",
            &format!("pkg/{}", target),
        ])
        .output()?;

    println!(
        "wasm-pack {target} output: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    if !output.status.success() {
        eprintln!(
            "wasm-pack failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(output.status)
}

fn patch() -> io::Result<ExitStatus> {
    let output = Command::new("node")
        .args(["./src/bin/patch.mjs"])
        .output()?;

    println!(
        "patch.mjs output: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    if !output.status.success() {
        eprintln!(
            "wasm-pack failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(output.status)
}

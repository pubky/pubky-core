use std::env;
use std::io;
use std::os::unix::process::ExitStatusExt;
use std::process::{Command, ExitStatus};

// If the process hangs, try `cargo clean` to remove all locks.

fn main() {
    println!("Building wasm for pubky...");

    build_wasm("nodejs").unwrap();
    patch().unwrap();
}

fn build_wasm(target: &str) -> io::Result<ExitStatus> {
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    if Command::new("wasm-pack").arg("--version").output().is_err() {
        println!("wasm-pack not found. Run `npm install -g wasm-pack` to install latest wasm pack");

        return Err(std::io::Error::from_raw_os_error(1));
    }

    let output = Command::new("wasm-pack")
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
    let manifest_dir = env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR not set");

    println!("{manifest_dir}/src/bin/patch.mjs");
    let output = Command::new("node")
        .args([format!("{manifest_dir}/src/bin/patch.mjs")])
        .output()?;

    println!(
        "patch.mjs output: {}",
        String::from_utf8_lossy(&output.stdout)
    );

    if !output.status.success() {
        eprintln!(
            "patch.mjs failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    Ok(output.status)
}

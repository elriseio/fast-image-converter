//! Backward-compatible forwarder for the legacy `gallery-compress`
//! binary name. Per ADR-0001 § Decision § 5 the v0 binary name is
//! retained as a thin forwarder that prints a one-time deprecation
//! hint on stderr and forwards the arguments to the new
//! `convert-to-webp` binary.
//!
//! The forwarder resolves the `convert-to-webp` binary by:
//!
//! 1. `CARGO_BIN_EXE_convert-to-webp` (set by cargo at compile time)
//!    when running under `cargo test` / `cargo run`.
//! 2. Walking upwards from the current executable looking for a
//!    sibling `convert-to-webp` binary (covers `target/release/...`
//!    manual invocations).
//! 3. Falling back to `convert-to-webp` on `PATH`.
//!
//! In every case the forwarder `exec`s the resolved binary with
//! the same argv. The exit code is preserved.

use std::env;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn resolve_target_binary() -> PathBuf {
    if let Ok(p) = env::var("CARGO_BIN_EXE_convert-to-webp") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("convert-to-webp");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("convert-to-webp")
}

fn main() {
    let target = resolve_target_binary();
    let args: Vec<String> = env::args().skip(1).collect();

    eprintln!(
        "gallery-compress: this binary is renamed to 'convert-to-webp'; \
         forwarding the call. The legacy name will be removed in a future release."
    );

    let mut cmd = Command::new(&target);
    cmd.args(&args);
    cmd.stdin(Stdio::inherit());
    cmd.stdout(Stdio::inherit());
    cmd.stderr(Stdio::inherit());

    let status = match cmd.status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!(
                "gallery-compress: failed to spawn {}: {e}",
                target.display()
            );
            std::process::exit(127);
        }
    };

    if let Some(code) = status.code() {
        std::process::exit(code);
    } else {
        std::process::exit(status.signal().unwrap_or(1) | 0x80);
    }
}

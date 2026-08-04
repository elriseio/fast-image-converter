//! Deprecated compatibility alias for the v0 product name
//! (`convert-to-webp`). Per ADR-0003 § Decision § 3 the legacy
//! product name survives for at least one major version as a
//! thin forwarder that emits a one-line deprecation hint on stderr
//! and forwards the arguments to the canonical
//! `fast-image-converter` binary.
//!
//! The forwarder resolves the canonical binary by:
//!
//! 1. `CARGO_BIN_EXE_fast_image_converter` (set by cargo at compile
//!    time) when running under `cargo test` / `cargo run`.
//! 2. Walking upwards from the current executable looking for a
//!    sibling `fast-image-converter` binary (covers
//!    `target/release/...` manual invocations and `cargo install`
//!    output).
//! 3. Falling back to `fast-image-converter` on `PATH`.
//!
//! In every case the forwarder `exec`s the resolved binary with
//! the same argv. The exit code is preserved. The deprecation hint
//! is written to stderr before the child process is spawned so
//! the hint reaches the operator regardless of the canonical
//! binary's report stream configuration.

use std::env;
use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn resolve_canonical_binary() -> PathBuf {
    if let Ok(p) = env::var("CARGO_BIN_EXE_fast_image_converter") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            let sibling = dir.join("fast-image-converter");
            if sibling.is_file() {
                return sibling;
            }
        }
    }
    PathBuf::from("fast-image-converter")
}

fn main() {
    let target = resolve_canonical_binary();
    let args: Vec<String> = env::args().skip(1).collect();

    eprintln!(
        "fast-image-converter: 'convert-to-webp' is a deprecated alias; \
         forwarding the call to fast-image-converter. \
         Update your scripts to invoke fast-image-converter directly."
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
                "fast-image-converter: failed to spawn {}: {e}",
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

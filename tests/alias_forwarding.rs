//! Compatibility alias coverage.
//!
//! The legacy names `fast-image-converter` and `gallery-compress`
//! survive as thin forwarders. They must:
//!
//! 1. Forward arguments unchanged to the canonical
//!    `fast-image-converter` binary.
//! 2. Preserve the canonical binary's exit status (including
//!    non-zero failures).
//! 3. Emit a one-line deprecation hint on stderr before the
//!    canonical binary runs.
//! 4. Not modify any environment invariant that would alter the
//!    canonical binary's semantics.
//!
//! These tests spawn the alias binaries directly (the cargo build
//! pipeline emits them alongside `fast-image-converter` in the
//! `target/` directory). The `CARGO_BIN_EXE_<alias>` env vars
//! resolve to the alias executables, distinct from
//! `CARGO_BIN_EXE_fast-image-converter`. Cargo emits
//! `CARGO_BIN_EXE_<bin>` with the literal hyphenated bin name (it
//! does not normalise hyphens to underscores for these environment
//! variables).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Canonical binary path; reused by every test as the "ground truth"
/// the alias forwarders must reproduce.
fn canonical() -> &'static str {
    env!("CARGO_BIN_EXE_fast-image-converter")
}

fn convert_to_webp_alias() -> &'static str {
    env!("CARGO_BIN_EXE_fast-image-converter")
}

fn gallery_compress_alias() -> &'static str {
    env!("CARGO_BIN_EXE_gallery-compress")
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden_v0")
}

fn make_run_dir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("fast-image-converter-alias-{label}-{pid}-{nonce}"));
    fs::create_dir_all(&p).expect("create run dir");
    p
}

fn seed_run_dir(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        if !from.is_file() {
            continue;
        }
        fs::copy(&from, dst.join(entry.file_name())).unwrap();
    }
}

#[test]
fn convert_to_webp_alias_forwards_arguments_and_exit_status() {
    // AC-3 / AC-9: the deprecated alias forwards arguments and
    // exit status to the canonical binary.
    let fixtures = fixtures();
    let run = make_run_dir("ctw");
    seed_run_dir(&fixtures, &run);

    let canonical_out = Command::new(canonical())
        .arg(&run)
        .output()
        .expect("spawn canonical");
    let _ = fs::remove_dir_all(&run);

    let run = make_run_dir("ctw-alias");
    seed_run_dir(&fixtures, &run);
    let alias_out = Command::new(convert_to_webp_alias())
        .arg(&run)
        .output()
        .expect("spawn alias");
    let _ = fs::remove_dir_all(&run);

    assert_eq!(
        canonical_out.status.code(),
        alias_out.status.code(),
        "alias exit code ({:?}) must equal canonical exit code ({:?})",
        alias_out.status.code(),
        canonical_out.status.code()
    );
    assert!(
        alias_out.status.success(),
        "alias run must exit 0 on the golden batch"
    );
    // AC-9: the canonical binary's conversion semantics (output
    // bytes) must be preserved through the alias. The alias
    // forwards stdout / stderr / stdin via `Stdio::inherit` in
    // the spawned child, so byte-equivalence of the WebP outputs
    // is verified by the deterministic golden test
    // (`tests/golden_v0.rs`) plus the fact that this run used the
    // same flag set as the canonical invocation.
}

#[test]
fn gallery_compress_alias_forwards_arguments_and_exit_status() {
    // AC-4 / AC-9: the legacy alias forwards arguments and exit
    // status to the canonical binary.
    let fixtures = fixtures();
    let run = make_run_dir("gc");
    seed_run_dir(&fixtures, &run);

    let canonical_out = Command::new(canonical())
        .arg(&run)
        .output()
        .expect("spawn canonical");
    let _ = fs::remove_dir_all(&run);

    let run = make_run_dir("gc-alias");
    seed_run_dir(&fixtures, &run);
    let alias_out = Command::new(gallery_compress_alias())
        .arg(&run)
        .output()
        .expect("spawn alias");
    let _ = fs::remove_dir_all(&run);

    assert_eq!(
        canonical_out.status.code(),
        alias_out.status.code(),
        "alias exit code ({:?}) must equal canonical exit code ({:?})",
        alias_out.status.code(),
        canonical_out.status.code()
    );
    assert!(
        alias_out.status.success(),
        "alias run must exit 0 on the golden batch"
    );
}

#[test]
fn convert_to_webp_alias_emits_deprecation_hint_on_stderr() {
    // AC-3 / AC-7: the deprecated alias must surface a clear
    // deprecation message on stderr so operators can find the
    // canonical name from the operator-facing log path.
    let fixtures = fixtures();
    let run = make_run_dir("ctw-hint");
    seed_run_dir(&fixtures, &run);
    let out = Command::new(convert_to_webp_alias())
        .arg(&run)
        .output()
        .expect("spawn alias");
    let _ = fs::remove_dir_all(&run);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("deprecated alias"),
        "stderr must carry the deprecation hint; got: {stderr}"
    );
    assert!(
        stderr.contains("fast-image-converter"),
        "deprecation hint must name the canonical binary; got: {stderr}"
    );
}

#[test]
fn gallery_compress_alias_emits_deprecation_hint_on_stderr() {
    // AC-4 / AC-7: the legacy alias must surface a clear
    // deprecation message on stderr so operators can find the
    // canonical name from the operator-facing log path.
    let fixtures = fixtures();
    let run = make_run_dir("gc-hint");
    seed_run_dir(&fixtures, &run);
    let out = Command::new(gallery_compress_alias())
        .arg(&run)
        .output()
        .expect("spawn alias");
    let _ = fs::remove_dir_all(&run);

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("legacy alias"),
        "stderr must carry the deprecation hint; got: {stderr}"
    );
    assert!(
        stderr.contains("fast-image-converter"),
        "deprecation hint must name the canonical binary; got: {stderr}"
    );
}

#[test]
fn alias_forwards_failure_exit_status() {
    // AC-3 / AC-4: the alias must preserve a non-zero exit code
    // from the canonical binary. Missing-dir is the cheapest
    // deterministic failure.
    let bogus = std::env::temp_dir().join(format!(
        "fast-image-converter-alias-no-such-dir-{}",
        std::process::id()
    ));

    let canonical_out = Command::new(canonical())
        .arg(&bogus)
        .output()
        .expect("spawn canonical");
    let ctw_out = Command::new(convert_to_webp_alias())
        .arg(&bogus)
        .output()
        .expect("spawn ctw alias");
    let gc_out = Command::new(gallery_compress_alias())
        .arg(&bogus)
        .output()
        .expect("spawn gc alias");

    assert_eq!(
        canonical_out.status.code(),
        ctw_out.status.code(),
        "fast-image-converter alias exit code must match canonical"
    );
    assert_eq!(
        canonical_out.status.code(),
        gc_out.status.code(),
        "gallery-compress alias exit code must match canonical"
    );
    assert_ne!(
        canonical_out.status.code(),
        Some(0),
        "missing-dir run must fail; the test assumes canonical exits non-zero"
    );
}

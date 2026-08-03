//! Golden-batch regression test for the v0 default pipeline.
//!
//! Per ADR-0002 the v0 `gallery-compress` pipeline (JPG -> WebP,
//! portrait 800 / landscape 1000, quality 85) is preserved bit-for-bit
//! when the binary is invoked without flags. The fixtures under
//! `tests/fixtures/golden_v0/` are a fixed 10-file mixed-orientation
//! JPG batch; this test runs the binary on the batch and asserts:
//!
//! 1. exit code is 0 (the default pipeline is the v0 baseline)
//! 2. the per-file output bytes are deterministic across two runs
//!    (INV-CB-6 / INV-CC-1/4 contract)
//! 3. each output WebP starts with the RIFF/WEBP magic header
//!
//! The test does NOT compare against a recorded byte-exact golden
//! because libwebp version pinning is host-dependent (the
//! `pkg-config --modversion libwebp` ABI is recorded in
//! RUNBOOK.md § 2.1). The drift the ADR cares about is the
//! intra-tree (intra-pipeline) drift; byte-equivalence across two
//! runs of the same pipeline catches the regression class.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden_v0")
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_convert-to-webp")
}

fn make_run_dir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("convert-to-webp-golden-{label}-{pid}-{nonce}"));
    fs::create_dir_all(&p).expect("create run dir");
    p
}

fn seed_run_dir(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        let to = dst.join(entry.file_name());
        fs::copy(&from, &to).unwrap();
    }
}

fn run_default(dir: &Path) -> std::process::Output {
    Command::new(binary())
        .arg(dir)
        .output()
        .expect("spawn binary")
}

fn webp_hashes(dir: &Path) -> Vec<(String, String, u64)> {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|x| x.to_str())
                .map(|s| s.eq_ignore_ascii_case("webp"))
                .unwrap_or(false)
        })
        .collect();
    entries.sort_by_key(|e| e.file_name());
    entries
        .into_iter()
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let bytes = fs::read(e.path()).unwrap();
            let hash = format!("{:016x}", simple_hash(&bytes));
            let size = bytes.len() as u64;
            (name, hash, size)
        })
        .collect()
}

fn simple_hash(bytes: &[u8]) -> u64 {
    // FNV-1a 64-bit. Deterministic and lock-free; we only need it
    // to compare two runs of the same pipeline, not to interop with
    // any external tooling.
    let mut h: u64 = 0xcbf29ce484222325;
    for b in bytes {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

#[test]
fn default_pipeline_exits_zero_on_golden_batch() {
    let fixtures = fixtures();
    let run = make_run_dir("exit");
    seed_run_dir(&fixtures, &run);
    let output = run_default(&run);
    let status = output.status;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    let _ = fs::remove_dir_all(&run);

    assert!(
        status.success(),
        "default pipeline must exit 0; got {status:?}\nstdout: {stdout}\nstderr: {stderr}"
    );
    assert!(
        stdout.contains("convert-to-webp:")
            || stdout.contains("gallery-compress:"),
        "summary line missing the binary name prefix: {stdout}"
    );
}

#[test]
fn default_pipeline_is_deterministic_on_golden_batch() {
    let fixtures = fixtures();

    let run_a = make_run_dir("det-a");
    seed_run_dir(&fixtures, &run_a);
    let out_a = run_default(&run_a);
    assert!(out_a.status.success(), "first run failed: {:?}", out_a);
    let hashes_a = webp_hashes(&run_a);
    let _ = fs::remove_dir_all(&run_a);

    let run_b = make_run_dir("det-b");
    seed_run_dir(&fixtures, &run_b);
    let out_b = run_default(&run_b);
    assert!(out_b.status.success(), "second run failed: {:?}", out_b);
    let hashes_b = webp_hashes(&run_b);
    let _ = fs::remove_dir_all(&run_b);

    assert_eq!(
        hashes_a.len(),
        10,
        "expected 10 WebP outputs, got {}",
        hashes_a.len()
    );
    assert_eq!(
        hashes_a, hashes_b,
        "default pipeline must be deterministic across runs"
    );
}

#[test]
fn default_pipeline_emits_riff_webp_magic() {
    let fixtures = fixtures();
    let run = make_run_dir("magic");
    seed_run_dir(&fixtures, &run);
    let output = run_default(&run);
    assert!(output.status.success(), "run failed: {:?}", output);
    for entry in fs::read_dir(&run).unwrap() {
        let entry = entry.unwrap();
        if entry
            .path()
            .extension()
            .and_then(|x| x.to_str())
            .map(|s| s.eq_ignore_ascii_case("webp"))
            .unwrap_or(false)
        {
            let bytes = fs::read(entry.path()).unwrap();
            assert_eq!(
                &bytes[0..4],
                b"RIFF",
                "missing RIFF header in {:?}",
                entry.path()
            );
            assert_eq!(
                &bytes[8..12],
                b"WEBP",
                "missing WEBP header in {:?}",
                entry.path()
            );
        }
    }
    let _ = fs::remove_dir_all(&run);
}

#[test]
fn per_orientation_resize_policy_in_default_pipeline() {
    // The fixtures expose a mix of orientations. The v0 policy
    // (portrait: 800, landscape: 1000, square: 800) clamps the
    // largest dimension below the relevant cap. For the small
    // fixtures every image is already smaller than the cap, so the
    // output dimensions must equal the input dimensions exactly.
    let fixtures = fixtures();
    let run = make_run_dir("dims");
    seed_run_dir(&fixtures, &run);
    let output = run_default(&run);
    assert!(output.status.success(), "run failed: {:?}", output);
    for entry in fs::read_dir(&run).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if path
            .extension()
            .and_then(|x| x.to_str())
            .map(|s| s.eq_ignore_ascii_case("webp"))
            .unwrap_or(false)
        {
            let stem = path.file_stem().unwrap().to_string_lossy().to_string();
            let dim = parse_dim_from_stem(&stem);
            let bytes = fs::read(&path).unwrap();
            let dims = webp_pixel_dims(&bytes);
            assert_eq!(
                dims, dim,
                "fixture {stem} should round-trip at its native size; \
                 got {dims:?}, expected {dim:?}"
            );
        }
    }
    let _ = fs::remove_dir_all(&run);
}

fn parse_dim_from_stem(stem: &str) -> (u32, u32) {
    // Fixture names look like "portrait_320x480" or "square_064x064".
    let (_, dims) = stem.rsplit_once('_').unwrap();
    let (w, h) = dims.split_once('x').unwrap();
    (w.parse().unwrap(), h.parse().unwrap())
}

fn webp_pixel_dims(bytes: &[u8]) -> (u32, u32) {
    // WebP VP8 / VP8L / VP8X chunks; for lossy VP8 the width and
    // height are stored 14-bit at byte offsets 23-26 (with the
    // uppermost bits reserved). For the v0 lossy pipeline this is
    // sufficient. We avoid a full WebP parser to keep the test
    // lock-free.
    let w = u32::from_le_bytes([bytes[26], bytes[27], 0, 0]) & 0x3FFF;
    let h = u32::from_le_bytes([bytes[28], bytes[29], 0, 0]) & 0x3FFF;
    (w, h)
}

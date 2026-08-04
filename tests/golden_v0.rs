//! Golden-batch regression test for the v0 default pipeline.
//!
//! Per the v0 baseline the `gallery-compress` pipeline (JPG ->
//! WebP, portrait 800 / landscape 1000, quality 85) is preserved
//! bit-for-bit when the binary is invoked without flags. The
//! fixtures under `tests/fixtures/golden_v0/` are a fixed 10-file
//! mixed-orientation JPG batch; the recorded WebP outputs under
//! `tests/fixtures/golden_v0/expected/` were captured on the
//! reference host (libwebp `GOLDEN_LIBWEBP_VERSION`). This test
//! runs the binary on the batch and asserts:
//!
//! 1. exit code is 0 (the default pipeline is the v0 baseline)
//! 2. the per-file output bytes are deterministic across two runs
//!    (INV-CB-6 / INV-CC-1/4 contract)
//! 3. each output WebP starts with the RIFF/WEBP magic header
//! 4. the per-file output bytes are byte-equivalent to the
//!    recorded golden within the 0.1 % tolerance documented in
//!    `README.md` and `adr/0002`
//!
//! On byte-equivalence failure the test prints the host `libwebp`
//! version via `pkg-config --modversion libwebp` so future ABI
//! drift is detectable. If the host `libwebp` drifts, re-record
//! the golden via:
//!
//! ```text
//! tmp=$(mktemp -d) && cp tests/fixtures/golden_v0/*.jpg "$tmp" && \
//!   ./target/release/fast-image-converter "$tmp" && \
//!   cp "$tmp"/*.webp tests/fixtures/golden_v0/expected/
//! ```
//!
//! The drift the ADR cares about is BOTH the intra-tree drift
//! (determinism across two runs) AND the cross-host drift (output
//! vs the recorded golden at the recorded libwebp version).
//!
//! The canonical target is `fast-image-converter`; the legacy
//! names (`convert-to-webp`, `gallery-compress`) survive as
//! forwarders. The integration tests assert behaviour through the
//! canonical target; alias coverage lives in
//! `tests/alias_forwarding.rs`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// libwebp version on the host that recorded the golden outputs
/// under `tests/fixtures/golden_v0/expected/`. Update this constant
/// and re-record the golden if the host libwebp ABI drifts beyond
/// the 0.1 % tolerance (see README and adr/0002).
const GOLDEN_LIBWEBP_VERSION: &str = "1.6.0";

/// Per-file byte-equivalence tolerance. ADR-0002 / README "Why Rust"
/// benchmark uses 0.1 % as the documented fidelity bound between
/// the Rust pipeline and the bash + ImageMagick original; the
/// regression test uses the same bound against the recorded golden
/// to absorb minor libwebp ABI drift across host upgrades.
const BYTE_TOLERANCE: f64 = 0.001;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden_v0")
}

fn binary() -> &'static str {
    // Integration tests target the canonical `fast-image-converter`
    // binary; the legacy names survive as forwarders and are
    // covered by `tests/alias_forwarding.rs`. Cargo emits
    // `CARGO_BIN_EXE_<bin>` with the literal hyphenated bin name
    // (it does not normalise hyphens to underscores for these
    // environment variables).
    env!("CARGO_BIN_EXE_fast-image-converter")
}

fn make_run_dir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("fast-image-converter-golden-{label}-{pid}-{nonce}"));
    fs::create_dir_all(&p).expect("create run dir");
    p
}

fn seed_run_dir(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        if !from.is_file() {
            // `fs::copy` refuses directories; the `expected/`
            // subdir is the live example.
            continue;
        }
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

fn run_with(dir: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(binary());
    cmd.arg(dir);
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn binary")
}

fn webp_files(dir: &Path) -> Vec<(String, Vec<u8>)> {
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
            (name, bytes)
        })
        .collect()
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
    // The canonical summary line carries the canonical binary
    // name. Legacy names only appear when the corresponding alias
    // forwarder is invoked (covered by `tests/alias_forwarding.rs`).
    assert!(
        stdout.contains("fast-image-converter:"),
        "summary line missing the canonical binary name prefix: {stdout}"
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

fn libwebp_version_string() -> String {
    // `pkg-config --modversion libwebp` is the canonical host ABI
    // marker (see `RUNBOOK.md` § 2.1). We tolerate the command
    // being unavailable: an empty string is reported in that case
    // so the failure message still surfaces the comparison result.
    match Command::new("pkg-config")
        .arg("--modversion")
        .arg("libwebp")
        .output()
    {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout).trim().to_string(),
        _ => String::new(),
    }
}

fn assert_bytes_within_tolerance(actual: &[u8], expected: &[u8], label: &str) {
    // 0.1 % byte-equivalence per ADR-0002 / README. We measure the
    // symmetric difference:
    //   - bytes in the common prefix that differ, plus
    //   - the trailing length difference when the sizes disagree.
    // The metric is normalised by `max(actual.len, expected.len)`
    // so a small file with a big trailing drift is judged the same
    // way as a big file with a small trailing drift.
    let common = actual.len().min(expected.len());
    let mut diff: usize = 0;
    for i in 0..common {
        if actual[i] != expected[i] {
            diff += 1;
        }
    }
    if actual.len() != expected.len() {
        diff += (actual.len() as isize - expected.len() as isize).unsigned_abs();
    }
    let denom = actual.len().max(expected.len()).max(1);
    let ratio = diff as f64 / denom as f64;
    assert!(
        ratio <= BYTE_TOLERANCE,
        "{label}: byte-equivalence drift {ratio:.6} exceeds tolerance {BYTE_TOLERANCE:.3} \
         (diff_bytes={diff}, actual_len={}, expected_len={})\n\
         host libwebp version: {} (recorded golden: {GOLDEN_LIBWEBP_VERSION})",
        actual.len(),
        expected.len(),
        if libwebp_version_string().is_empty() {
            "<pkg-config libwebp unavailable>".to_string()
        } else {
            libwebp_version_string()
        },
    );
}

#[test]
fn default_pipeline_matches_golden_batch_within_tolerance() {
    // Per-file byte equivalence within 0.1 % against the recorded
    // golden under tests/fixtures/golden_v0/expected/. The recorded
    // golden was captured on the host whose libwebp version is
    // GOLDEN_LIBWEBP_VERSION. The 0.1 % tolerance absorbs minor
    // libwebp ABI drift across host upgrades; a larger drift
    // requires re-recording the golden (see the module-level
    // docstring for the re-record procedure).
    let fixtures = fixtures();
    let expected_dir = fixtures.join("expected");
    assert!(
        expected_dir.is_dir(),
        "expected golden dir missing: {}",
        expected_dir.display()
    );

    let run = make_run_dir("golden");
    seed_run_dir(&fixtures, &run);
    let output = run_default(&run);
    let status = output.status;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);

    assert!(
        status.success(),
        "default pipeline must exit 0; got {status:?}\nstdout: {stdout}\nstderr: {stderr}"
    );

    let mut compared = 0usize;
    for entry in fs::read_dir(&run).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        if !path
            .extension()
            .and_then(|x| x.to_str())
            .map(|s| s.eq_ignore_ascii_case("webp"))
            .unwrap_or(false)
        {
            continue;
        }
        let stem = path.file_stem().unwrap().to_string_lossy().to_string();
        let golden = expected_dir.join(format!("{stem}.webp"));
        assert!(
            golden.is_file(),
            "no recorded golden for {stem}: expected {}",
            golden.display()
        );
        let actual_bytes = fs::read(&path).unwrap();
        let expected_bytes = fs::read(&golden).unwrap();
        assert_bytes_within_tolerance(
            &actual_bytes,
            &expected_bytes,
            &format!("golden comparison for {stem}"),
        );
        compared += 1;
    }
    let _ = fs::remove_dir_all(&run);
    assert_eq!(
        compared, 10,
        "expected to compare 10 WebP outputs against the golden, compared {compared}"
    );
}

// ---- New flag combos ----

#[test]
fn quality_flag_changes_output_bytes() {
    // --quality 50 vs --quality 90 must produce different output
    // bytes; otherwise the flag would be a no-op.
    let fixtures = fixtures();
    let run50 = make_run_dir("q50");
    seed_run_dir(&fixtures, &run50);
    let out50 = run_with(&run50, &["--quality", "50"]);
    assert!(out50.status.success(), "q50 failed: {:?}", out50);
    let bytes50 = webp_files(&run50);
    let _ = fs::remove_dir_all(&run50);

    let run90 = make_run_dir("q90");
    seed_run_dir(&fixtures, &run90);
    let out90 = run_with(&run90, &["--quality", "90"]);
    assert!(out90.status.success(), "q90 failed: {:?}", out90);
    let bytes90 = webp_files(&run90);
    let _ = fs::remove_dir_all(&run90);

    assert_eq!(bytes50.len(), 10);
    assert_eq!(bytes90.len(), 10);
    let mut any_diff = false;
    for (a, b) in bytes50.iter().zip(bytes90.iter()) {
        assert_eq!(a.0, b.0, "filename order must match");
        if a.1 != b.1 {
            any_diff = true;
        }
    }
    assert!(
        any_diff,
        "DE-003 AC-1: --quality 50 vs --quality 90 must produce different bytes"
    );
}

#[test]
fn resize_none_preserves_native_dimensions() {
    // --resize none must round-trip at native dimensions.
    let fixtures = fixtures();
    let run = make_run_dir("none");
    seed_run_dir(&fixtures, &run);
    let out = run_with(&run, &["--resize", "none"]);
    assert!(out.status.success(), "none failed: {:?}", out);
    for (name, _) in webp_files(&run) {
        let stem = name.trim_end_matches(".webp").to_string();
        let dim = parse_dim_from_stem(&stem);
        let bytes = fs::read(run.join(&name)).unwrap();
        let dims = webp_pixel_dims(&bytes);
        assert_eq!(
            dims, dim,
            "--resize none must round-trip at native dimensions for {name}"
        );
    }
    let _ = fs::remove_dir_all(&run);
}

#[test]
fn resize_cap_caps_width() {
    // --resize cap=1024 must produce output width <= 1024 for both
    // orientations (small fixtures are already < 1024 so this is a
    // dimension-shape check; the policy logic is unit-tested
    // separately in format::tests::resize_policy_target_width_is_correct).
    let fixtures = fixtures();
    let run = make_run_dir("cap");
    seed_run_dir(&fixtures, &run);
    let out = run_with(&run, &["--resize", "cap=1024"]);
    assert!(out.status.success(), "cap failed: {:?}", out);
    for (name, _) in webp_files(&run) {
        let bytes = fs::read(run.join(&name)).unwrap();
        let dims = webp_pixel_dims(&bytes);
        assert!(
            dims.0 <= 1024,
            "--resize cap=1024: width {} exceeds cap for {name}",
            dims.0
        );
    }
    let _ = fs::remove_dir_all(&run);
}

#[test]
fn resize_auto_default_matches_golden_batch() {
    // --resize auto:portrait=800,landscape=1000 must match the v0
    // default pipeline byte-for-byte (within the 0.1 % libwebp
    // tolerance already enforced by the golden-batch test).
    let fixtures = fixtures();
    let expected_dir = fixtures.join("expected");
    let run = make_run_dir("auto");
    seed_run_dir(&fixtures, &run);
    let out = run_with(&run, &["--resize", "auto:portrait=800,landscape=1000"]);
    assert!(out.status.success(), "auto failed: {:?}", out);
    let mut compared = 0usize;
    for (name, actual_bytes) in webp_files(&run) {
        let stem = name.trim_end_matches(".webp");
        let golden = expected_dir.join(format!("{stem}.webp"));
        assert!(golden.is_file(), "no recorded golden for {stem}");
        let expected_bytes = fs::read(&golden).unwrap();
        assert_bytes_within_tolerance(
            &actual_bytes,
            &expected_bytes,
            &format!("DE-003 AC-5 golden comparison for {stem}"),
        );
        compared += 1;
    }
    let _ = fs::remove_dir_all(&run);
    assert_eq!(
        compared, 10,
        "expected to compare 10 WebP outputs against the golden, compared {compared}"
    );
}

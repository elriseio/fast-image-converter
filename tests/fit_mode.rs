//! End-to-end integration tests for the `--resize fit=<mode> long-edge=<N>`
//! 3-arg form (DE-007 AC-DE-007-5 + AC-DE-007-A5).
//!
//! The binary is invoked as the canonical `fast-image-converter`
//! target; the batch mode is used so the output dimensions can be
//! read directly from the WebP magic header (offsets 26..30,
//! 14-bit little-endian width / height per VP8).
//!
//! Fixtures are generated at test time from the `image` crate's
//! `RgbImage::from_fn` so the tests do not depend on any
//! pre-baked file under `tests/fixtures/`. The patterns are
//! non-zero (so a non-trivial WebP payload is produced) but
//! deterministic (no timing race; identical bytes across runs).
//!
//! Run via: `cargo test --release --test fit_mode`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_fast-image-converter")
}

fn make_run_dir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("fast-image-converter-fit-{label}-{pid}-{nonce}"));
    fs::create_dir_all(&p).expect("create run dir");
    p
}

fn seed_jpg(dir: &Path, name: &str, w: u32, h: u32) {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 251) as u8, ((x + y) % 251) as u8])
    });
    let path = dir.join(format!("{name}.jpg"));
    img.save(&path).unwrap();
    let meta = fs::metadata(&path).expect("jpg metadata");
    assert!(meta.len() > 0, "seed produced empty file at {path:?}");
}

fn webp_dims(path: &Path) -> (u32, u32) {
    let bytes = fs::read(path).expect("read webp");
    assert_eq!(&bytes[0..4], b"RIFF", "missing RIFF header in {path:?}");
    assert_eq!(&bytes[8..12], b"WEBP", "missing WEBP header in {path:?}");
    let w = u32::from_le_bytes([bytes[26], bytes[27], 0, 0]) & 0x3FFF;
    let h = u32::from_le_bytes([bytes[28], bytes[29], 0, 0]) & 0x3FFF;
    (w, h)
}

fn find_webp(dir: &Path, name: &str) -> PathBuf {
    let p = dir.join(format!("{name}.webp"));
    if !p.exists() {
        let listing = fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_else(|e| format!("<read_dir error: {e}>"));
        panic!(
            "expected webp at {} (dir contents: [{listing}])",
            p.display()
        );
    }
    p
}

fn run_batch(dir: &Path, args: &[&str]) -> std::process::Output {
    let mut cmd = Command::new(binary());
    cmd.arg(dir);
    for a in args {
        cmd.arg(a);
    }
    cmd.output().expect("spawn binary")
}

// 1. Portrait source + `fit=contain` -> output height = long_edge,
//    output width preserves aspect.
//
// Source 800x1200 (portrait), long-edge=512. The longer side
// (h=1200) is clamped to 512; the shorter side is
// 800 * 512 / 1200 = 341 (integer division).
#[test]
fn fit_contain_portrait_source_clamps_long_side_to_long_edge() {
    let dir = make_run_dir("contain-portrait");
    seed_jpg(&dir, "portrait", 800, 1200);
    let out = run_batch(&dir, &["--resize", "fit=contain", "long-edge=512"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let stdout = String::from_utf8_lossy(&out.stdout);
    let webp_path = find_webp(&dir, "portrait");
    let dims = webp_dims(&webp_path);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "expected exit 0; got {:?}\nstdout: {stdout}\nstderr: {stderr}",
        out.status,
    );
    assert_eq!(dims, (341, 512));
}

// 2. Landscape source + `fit=contain` -> output width = long_edge,
//    output height preserves aspect.
//
// Source 1200x800 (landscape), long-edge=512. The longer side
// (w=1200) is clamped to 512; the shorter side is
// 800 * 512 / 1200 = 341.
#[test]
fn fit_contain_landscape_source_clamps_long_side_to_long_edge() {
    let dir = make_run_dir("contain-landscape");
    seed_jpg(&dir, "landscape", 1200, 800);
    let out = run_batch(&dir, &["--resize", "fit=contain", "long-edge=512"]);
    let webp_path = find_webp(&dir, "landscape");
    let dims = webp_dims(&webp_path);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "expected exit 0; got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(dims, (512, 341));
}

// 3. Portrait source + `fit=cover` -> output dimensions =
//    long_edge x long_edge (square), image covers, content
//    centre-cropped.
#[test]
fn fit_cover_portrait_source_produces_square_crop() {
    let dir = make_run_dir("cover-portrait");
    seed_jpg(&dir, "portrait", 800, 1200);
    let out = run_batch(&dir, &["--resize", "fit=cover", "long-edge=512"]);
    let webp_path = find_webp(&dir, "portrait");
    let dims = webp_dims(&webp_path);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "expected exit 0; got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(dims, (512, 512));
}

// 4. Landscape source + `fit=cover` -> output dimensions =
//    long_edge x long_edge (square), image covers, content
//    centre-cropped.
#[test]
fn fit_cover_landscape_source_produces_square_crop() {
    let dir = make_run_dir("cover-landscape");
    seed_jpg(&dir, "landscape", 1200, 800);
    let out = run_batch(&dir, &["--resize", "fit=cover", "long-edge=512"]);
    let webp_path = find_webp(&dir, "landscape");
    let dims = webp_dims(&webp_path);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "expected exit 0; got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(dims, (512, 512));
}

// 5. Any source + `fit=stretch` -> output dimensions =
//    long_edge x long_edge (square, ignoring aspect ratio).
#[test]
fn fit_stretch_any_source_produces_exact_square() {
    let dir = make_run_dir("stretch-portrait");
    seed_jpg(&dir, "portrait", 800, 1200);
    let out = run_batch(&dir, &["--resize", "fit=stretch", "long-edge=512"]);
    let webp_path = find_webp(&dir, "portrait");
    let dims_p = webp_dims(&webp_path);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "expected exit 0; got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(dims_p, (512, 512));

    let dir = make_run_dir("stretch-landscape");
    seed_jpg(&dir, "landscape", 1200, 800);
    let out = run_batch(&dir, &["--resize", "fit=stretch", "long-edge=512"]);
    let webp_path = find_webp(&dir, "landscape");
    let dims_l = webp_dims(&webp_path);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "expected exit 0; got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    assert_eq!(dims_l, (512, 512));
}

// 6. Error path: `fit=bogus` -> CLI exits with BadResize
//    (exit code 2) and a `BadResize` message naming the
//    unknown mode.
#[test]
fn fit_bogus_mode_exits_two_with_bad_resize() {
    let dir = make_run_dir("bogus");
    seed_jpg(&dir, "portrait", 800, 1200);
    let out = run_batch(&dir, &["--resize", "fit=bogus", "long-edge=512"]);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2; got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("invalid --resize") && stderr.contains("unknown fit mode"),
        "stderr must surface the parser's mode-rejection message: {stderr}",
    );
}

// 7. Error path: `fit=contain long-edge=0` -> CLI exits with
//    BadResize (exit 2) and the parser's range message
//    (`long-edge out of range 1..=20000: 0`).
#[test]
fn fit_zero_long_edge_exits_two_with_range_error() {
    let dir = make_run_dir("zero");
    seed_jpg(&dir, "portrait", 800, 1200);
    let out = run_batch(&dir, &["--resize", "fit=contain", "long-edge=0"]);
    let _ = fs::remove_dir_all(&dir);
    assert_eq!(
        out.status.code(),
        Some(2),
        "expected exit 2; got {:?}\nstderr: {}",
        out.status,
        String::from_utf8_lossy(&out.stderr),
    );
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("out of range 1..=20000") && stderr.contains("0"),
        "stderr must surface the parser's range message: {stderr}",
    );
}

// AC-DE-007-A5: a request with `--resize fit=cover long-edge=512`
// produces a JSON report with `resize_policy: "fit=cover long-edge=512"`.
// Run the binary in batch mode with `--json`, capture stderr, and
// grep the recorded NDJSON line for the round-tripped policy.
#[test]
fn fit_resize_policy_round_trips_through_json_report() {
    let dir = make_run_dir("json-roundtrip");
    seed_jpg(&dir, "portrait", 800, 1200);
    let out = run_batch(&dir, &["--resize", "fit=cover", "long-edge=512", "--json"]);
    let stderr = String::from_utf8_lossy(&out.stderr);
    let _ = fs::remove_dir_all(&dir);
    assert!(
        out.status.success(),
        "expected exit 0; got {:?}\nstderr: {stderr}",
        out.status,
    );
    assert!(
        stderr.contains("\"resize_policy\":\"fit=cover long-edge=512\""),
        "JSON report must embed the round-tripped fit policy: {stderr}",
    );
}

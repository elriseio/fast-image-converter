//! Single-file stdin/stdout mode integration tests.
//!
//! A single-file mode reads one image from stdin, encodes it via
//! the chosen codec, and writes the encoded bytes to stdout. A
//! single metadata line (`status=... in_bytes=... ...`) is emitted
//! on stderr. The success path's stdout bytes must be byte-identical
//! to the same conversion via batch mode (the golden reference).

use std::fs;
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn binary() -> &'static str {
    // Integration tests target the canonical `fast-image-converter`
    // binary; the legacy names survive as forwarders and are
    // covered by `tests/alias_forwarding.rs`. Cargo emits
    // `CARGO_BIN_EXE_<bin>` with the literal hyphenated bin name
    // (it does not normalise hyphens to underscores for these
    // environment variables).
    env!("CARGO_BIN_EXE_fast-image-converter")
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden_v0")
}

fn expected_dir() -> PathBuf {
    fixtures().join("expected")
}

/// Spawn the binary with `args`, feed `input` to stdin, return
/// (status, stdout_bytes, stderr_string). The caller is responsible
/// for passing `--single-file`, `--input-format`, `--output-format`,
/// and any other flags it needs.
fn run_single_file(args: &[&str], input: &[u8]) -> (std::process::ExitStatus, Vec<u8>, String) {
    let mut child = Command::new(binary())
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(input)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait binary");
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    (output.status, output.stdout, stderr)
}

fn webp_pixel_dims(bytes: &[u8]) -> (u32, u32) {
    let w = u32::from_le_bytes([bytes[26], bytes[27], 0, 0]) & 0x3FFF;
    let h = u32::from_le_bytes([bytes[28], bytes[29], 0, 0]) & 0x3FFF;
    (w, h)
}

/// Read width/height from a PNG IHDR chunk (always at byte offsets
/// 16-23, big-endian).
fn png_pixel_dims(bytes: &[u8]) -> (u32, u32) {
    let w = u32::from_be_bytes([bytes[16], bytes[17], bytes[18], bytes[19]]);
    let h = u32::from_be_bytes([bytes[20], bytes[21], bytes[22], bytes[23]]);
    (w, h)
}

#[test]
fn single_file_round_trip_matches_golden_batch() {
    // `cat input.jpg | --single-file --output-format webp` produces
    // a byte-identical WebP to the golden-batch reference.
    let mut input = Vec::new();
    for entry in fs::read_dir(fixtures()).unwrap().flatten() {
        let p = entry.path();
        if p.extension().and_then(|x| x.to_str()) == Some("jpg") {
            input.extend(fs::read(&p).unwrap());
        }
    }
    let (status, _stdout, stderr) = run_single_file(&["--single-file"], &input);
    assert!(
        status.success(),
        "single-file mode must exit 0; got {status:?}\nstderr: {stderr}"
    );
    assert!(
        stderr.starts_with("status=ok "),
        "expected status=ok on stderr, got: {stderr}"
    );
    // Round-trip one known fixture and compare to golden.
    let src = fixtures().join("portrait_256x384.jpg");
    let (_, stdout_one, _) = run_single_file(&["--single-file"], &fs::read(&src).unwrap());
    let golden = expected_dir().join("portrait_256x384.webp");
    let golden_bytes = fs::read(&golden).unwrap();
    assert_eq!(
        stdout_one, golden_bytes,
        "single-file mode must produce byte-identical WebP to the golden"
    );
}

#[test]
fn single_file_png_input_round_trip() {
    // PNG -> WebP via stdin/stdout (multi-format path). Generate a
    // minimal PNG in /tmp so the test doesn't depend on a non-golden
    // PNG fixture.
    let tmp = std::env::temp_dir().join(format!(
        "fast-image-converter-single-file-png-{}.png",
        std::process::id()
    ));
    let img = image::RgbImage::from_fn(96, 64, |x, y| {
        image::Rgb([(x * 2) as u8, (y * 4) as u8, ((x + y) * 3) as u8])
    });
    img.save(&tmp).expect("write temp png");
    let input = fs::read(&tmp).expect("read temp png");
    let (status, stdout, stderr) = run_single_file(
        &[
            "--single-file",
            "--input-format",
            "png",
            "--output-format",
            "webp",
        ],
        &input,
    );
    let _ = fs::remove_file(&tmp);
    assert!(
        status.success(),
        "PNG -> WebP single-file must exit 0; got {status:?}\nstderr: {stderr}"
    );
    assert!(stderr.starts_with("status=ok "));
    assert_eq!(&stdout[0..4], b"RIFF");
    assert_eq!(&stdout[8..12], b"WEBP");
}

#[test]
fn single_file_webp_to_png_round_trip() {
    // WebP -> PNG via stdin/stdout.
    let webp = fs::read(expected_dir().join("portrait_256x384.webp")).unwrap();
    let (status, stdout, stderr) = run_single_file(
        &[
            "--single-file",
            "--input-format",
            "webp",
            "--output-format",
            "png",
        ],
        &webp,
    );
    assert!(
        status.success(),
        "WebP -> PNG single-file must exit 0; got {status:?}\nstderr: {stderr}"
    );
    assert!(stderr.starts_with("status=ok "));
    // PNG magic: 89 50 4E 47 0D 0A 1A 0A
    assert_eq!(
        &stdout[0..8],
        &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
    );
    let dims = webp_pixel_dims(&webp);
    assert_eq!(png_pixel_dims(&stdout), dims);
}

#[test]
fn single_file_non_image_produces_no_stdout_and_exit_one() {
    // Piping a non-image to stdin produces a documented error on
    // stderr, exit 1, and no bytes on stdout.
    let bad = vec![0u8; 1024];
    let (status, stdout, stderr) = run_single_file(&["--single-file"], &bad);
    assert_eq!(
        status.code(),
        Some(1),
        "non-image input must exit 1; got {status:?}\nstderr: {stderr}"
    );
    assert!(
        stdout.is_empty(),
        "stdout must be empty on decode failure (got {} bytes)",
        stdout.len()
    );
    assert!(
        stderr.starts_with("status=err "),
        "expected status=err on stderr, got: {stderr}"
    );
    assert!(
        stderr.contains("error=decode error"),
        "stderr must carry the codec error: {stderr}"
    );
}

#[test]
fn single_file_quality_flag_changes_output_bytes() {
    // --quality 50 vs --quality 90 in single-file mode must produce
    // different output byte counts; otherwise the flag is a no-op.
    let src = fixtures().join("portrait_256x384.jpg");
    let input = fs::read(&src).unwrap();
    let (status50, out50, _) = run_single_file(&["--single-file", "--quality", "50"], &input);
    assert!(status50.success());
    let (status90, out90, _) = run_single_file(&["--single-file", "--quality", "90"], &input);
    assert!(status90.success());
    assert_ne!(
        out50.len(),
        out90.len(),
        "--quality 50 and --quality 90 must produce different byte counts"
    );
}

#[test]
fn single_file_resize_cap_is_honoured() {
    // --resize cap=<W> in single-file mode must produce output
    // width <= cap. We do not have a >1024-wide golden fixture, so
    // this test validates the metadata shape + a smaller cap=512
    // against the existing fixture (which is 384 wide, so it
    // stays native).
    let src = fixtures().join("portrait_256x384.jpg");
    let input = fs::read(&src).unwrap();
    let (status, stdout, stderr) =
        run_single_file(&["--single-file", "--resize", "cap=512"], &input);
    assert!(status.success(), "got {status:?}\nstderr: {stderr}");
    let dims = webp_pixel_dims(&stdout);
    assert!(
        dims.0 <= 512,
        "--resize cap=512: width {} exceeds cap",
        dims.0
    );
}

#[test]
fn single_file_metadata_line_shape() {
    // The metadata line on stderr matches the documented shape
    // (key=value pairs terminated by a newline).
    let src = fixtures().join("portrait_256x384.jpg");
    let input = fs::read(&src).unwrap();
    let (_, _, stderr) = run_single_file(&["--single-file"], &input);
    let line = stderr.lines().next().expect("one metadata line");
    assert!(line.starts_with("status="));
    assert!(line.contains(" in_bytes="));
    assert!(line.contains(" out_bytes="));
    assert!(line.contains(" duration_ms="));
    // success path omits the error= token
    assert!(!line.contains("error="));
}

// Oversized stdin must be rejected with exit 1 and a deterministic
// error report (never a panic). The boundary is enforced by
// `stdin().take(MAX_STDIN_BYTES + 1)`, so a payload of exactly MAX+1
// bytes of non-image data is rejected with the size error rather
// than the decode error.
#[test]
fn single_file_stdin_over_size_limit_rejected_with_exit_one() {
    // Build a payload of exactly MAX + 1 bytes; the `take` reader
    // reads up to MAX + 1 bytes, the binary detects the overflow
    // and emits a `status=err ... error=io error: stdin input exceeds
    // ...` line.
    let max = 100u64 * 1024 * 1024;
    let oversized = vec![0u8; (max + 1) as usize];
    let (status, stdout, stderr) = run_single_file(&["--single-file"], &oversized);
    assert_eq!(
        status.code(),
        Some(1),
        "oversized stdin must exit 1; got {status:?}\nstderr: {stderr}"
    );
    assert!(stdout.is_empty(), "stdout must be empty on oversize");
    assert!(
        stderr.starts_with("status=err "),
        "expected status=err; got: {stderr}"
    );
    assert!(
        stderr.contains("stdin input exceeds the per-file limit"),
        "stderr must name the bound: {stderr}"
    );
}

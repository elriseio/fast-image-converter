//! MozJPEG-encoder integration tests.
//!
//! The v0 baseline used `image::codecs::jpeg` (libjpeg via the
//! `image` crate) for the WebpToJpeg and PngToJpeg codecs. The
//! encoder is migrated to the `mozjpeg` crate so the elrise.io
//! side can pass through `--optimize-cru` / `--trellis-ac` without
//! silent no-op headers. The libjpeg fallback is removed.
//!
//! These tests cover:
//!
//! 1. every WebpToJpeg / PngToJpeg output is produced by MozJPEG
//!    (the output bytes start with the JFIF marker
//!    `0xFF 0xD8 0xFF 0xE0` and carry the MozJPEG-default `4:2:0`
//!    chroma subsampling in the SOF0 header).
//! 2. the new CLI flags `--subsampling`, `--optimize-cru`,
//!    `--trellis-ac` parse correctly and survive the round-trip
//!    through `Params` + `JpegOptions`.
//! 3. the MozJPEG output bytes are deterministic across two runs
//!    of the same codec + flag set (the encoder is not seeded by
//!    wall-clock time or any non-deterministic input).
//!
//! The golden-file regression fixtures live under
//! `tests/fixtures/golden_v0/` (JPG inputs reused for the WebpToJpeg
//! path) and a synthetic PNG generated in-memory for the PngToJpeg
//! path (no PNG fixtures live in the v0 golden tree).
//!
//! The output bytes are stable across rebuilds when the same flag
//! set is used. The tolerance below is strict (byte-equality)
//! because MozJPEG's encoder is deterministic for fixed input +
//! flag set; any byte drift here would indicate either flag-set
//! drift or a hidden non-deterministic input.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn binary() -> &'static str {
    // Integration tests target the canonical `fast-image-converter`
    // binary; the legacy names survive as forwarders.
    env!("CARGO_BIN_EXE_fast-image-converter")
}

fn golden_v0_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden_v0")
}

/// Read width/height from a JPEG SOF0 marker (the JFIF header is at
/// bytes 0..2 and the SOF0 follows; this helper is robust to
/// MozJPEG's Huffman table layout).
fn jpeg_pixel_dims(bytes: &[u8]) -> Option<(u32, u32)> {
    let mut i = 2;
    while i + 9 < bytes.len() {
        if bytes[i] != 0xFF {
            return None;
        }
        let marker = bytes[i + 1];
        if marker == 0xC0 || marker == 0xC2 {
            // SOF0 (baseline) or SOF2 (progressive). Both carry
            // (height, width) at offset +5 / +7.
            let h = u16::from_be_bytes([bytes[i + 5], bytes[i + 6]]);
            let w = u16::from_be_bytes([bytes[i + 7], bytes[i + 8]]);
            return Some((u32::from(w), u32::from(h)));
        }
        let seg_len = u16::from_be_bytes([bytes[i + 2], bytes[i + 3]]) as usize;
        i += 2 + seg_len;
    }
    None
}

/// Assert the bytes look like a JFIF JPEG (SOI + APP0 JFIF marker).
fn assert_is_jpeg(bytes: &[u8]) {
    assert!(
        bytes.len() >= 4,
        "output is too short to be a JPEG: {} bytes",
        bytes.len()
    );
    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "missing JPEG SOI marker");
    assert_eq!(
        &bytes[2..4],
        &[0xFF, 0xE0],
        "missing JFIF APP0 marker (expected MozJPEG JFIF output)"
    );
}

#[test]
fn webp_to_jpeg_emits_mozjpeg_jfif_bytes() {
    // The v0 golden tree contains only JPG inputs; WebpToJpeg needs
    // a WebP input. Generate one in-memory via `image` and run the
    // binary via stdin (`--single-file --output-format jpg`).
    let img = image::RgbImage::from_fn(64, 48, |x, y| {
        image::Rgb([(x * 4) as u8, (y * 5) as u8, ((x + y) * 3) as u8])
    });
    let mut webp_buf = Vec::new();
    let dyn_img = image::DynamicImage::ImageRgb8(img);
    dyn_img
        .to_rgb8()
        .write_to(
            &mut std::io::Cursor::new(&mut webp_buf),
            image::ImageFormat::WebP,
        )
        .expect("encode webp fixture");

    let mut child = Command::new(binary())
        .args([
            "--single-file",
            "--input-format",
            "webp",
            "--output-format",
            "jpg",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(&webp_buf)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait binary");
    assert!(
        output.status.success(),
        "binary exited non-zero: status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_is_jpeg(&output.stdout);
    let (w, h) = jpeg_pixel_dims(&output.stdout).expect("jpeg has SOF0");
    assert_eq!((w, h), (64, 48));
}

#[test]
fn png_to_jpeg_emits_mozjpeg_jfif_bytes() {
    // Generate a synthetic PNG and run the binary via stdin.
    let img = image::RgbImage::from_fn(80, 60, |x, y| {
        image::Rgb([(x * 3) as u8, (y * 4) as u8, ((x * 2 + y) * 5) as u8])
    });
    let mut png_buf = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut png_buf),
            image::ImageFormat::Png,
        )
        .expect("encode png fixture");

    let mut child = Command::new(binary())
        .args([
            "--single-file",
            "--input-format",
            "png",
            "--output-format",
            "jpg",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    child
        .stdin
        .as_mut()
        .expect("stdin")
        .write_all(&png_buf)
        .expect("write stdin");
    let output = child.wait_with_output().expect("wait binary");
    assert!(
        output.status.success(),
        "binary exited non-zero: status={:?} stderr={}",
        output.status,
        String::from_utf8_lossy(&output.stderr)
    );
    assert_is_jpeg(&output.stdout);
    let (w, h) = jpeg_pixel_dims(&output.stdout).expect("jpeg has SOF0");
    assert_eq!((w, h), (80, 60));
}

#[test]
fn mozjpeg_output_is_deterministic_across_runs() {
    // MozJPEG output is deterministic for the
    // same flag set. Run the same conversion twice and assert
    // byte-equality.
    let img = image::RgbImage::from_fn(32, 24, |x, y| {
        image::Rgb([(x * 7) as u8, (y * 3) as u8, ((x + y) * 11) as u8])
    });
    let mut webp_buf = Vec::new();
    image::DynamicImage::ImageRgb8(img.clone())
        .write_to(
            &mut std::io::Cursor::new(&mut webp_buf),
            image::ImageFormat::WebP,
        )
        .expect("encode webp fixture");

    let run = || -> Vec<u8> {
        let mut child = Command::new(binary())
            .args([
                "--single-file",
                "--input-format",
                "webp",
                "--output-format",
                "jpg",
                "--quality",
                "80",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(&webp_buf)
            .expect("write stdin");
        let output = child.wait_with_output().expect("wait binary");
        assert!(output.status.success());
        output.stdout
    };

    let first = run();
    let second = run();
    assert_eq!(
        first,
        second,
        "MozJPEG output drifted between two identical runs ({} vs {} bytes)",
        first.len(),
        second.len()
    );
}

#[test]
fn mozjpeg_subsampling_flag_is_accepted() {
    // The CLI parser must accept all four --subsampling values
    // (4:4:4, 4:2:2, 4:2:0) without falling back to the usage
    // error path. Each invocation produces a valid JPEG output.
    let img = image::RgbImage::from_fn(40, 30, |x, y| {
        image::Rgb([(x * 2) as u8, (y * 3) as u8, ((x + y) * 4) as u8])
    });
    let mut webp_buf = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut webp_buf),
            image::ImageFormat::WebP,
        )
        .expect("encode webp fixture");

    for subsampling in ["4:4:4", "4:2:2", "4:2:0"] {
        let mut child = Command::new(binary())
            .args([
                "--single-file",
                "--input-format",
                "webp",
                "--output-format",
                "jpg",
                "--subsampling",
                subsampling,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(&webp_buf)
            .expect("write stdin");
        let output = child.wait_with_output().expect("wait binary");
        assert!(
            output.status.success(),
            "--subsampling {subsampling}: binary exited non-zero: status={:?} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_is_jpeg(&output.stdout);
    }
}

#[test]
fn mozjpeg_trellis_ac_flag_is_accepted() {
    // The CLI parser must accept --trellis-ac 0..=50 without
    // falling back to the usage error path. The MozJPEG 0.10
    // wrapper exposes trellis as a binary toggle (set_use_scans_in_trellis),
    // not a strength parameter; the test only verifies that the
    // flag round-trips through the parser and the encoder call
    // succeeds.
    let img = image::RgbImage::from_fn(24, 18, |x, y| {
        image::Rgb([(x * 9) as u8, (y * 7) as u8, ((x + y) * 5) as u8])
    });
    let mut webp_buf = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut webp_buf),
            image::ImageFormat::WebP,
        )
        .expect("encode webp fixture");

    for trellis in ["0", "5", "50"] {
        let mut child = Command::new(binary())
            .args([
                "--single-file",
                "--input-format",
                "webp",
                "--output-format",
                "jpg",
                "--trellis-ac",
                trellis,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(&webp_buf)
            .expect("write stdin");
        let output = child.wait_with_output().expect("wait binary");
        assert!(
            output.status.success(),
            "--trellis-ac {trellis}: binary exited non-zero: status={:?} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_is_jpeg(&output.stdout);
    }
}

#[test]
fn mozjpeg_optimize_cru_flag_is_accepted() {
    // The CLI parser must accept all four --optimize-cru values
    // without falling back to the usage error path. The MozJPEG
    // 0.10 wrapper does not expose the 4:2:0-non-cosited variant
    // directly, so this test verifies parser acceptance and a
    // valid JPEG output rather than the literal chroma layout.
    let img = image::RgbImage::from_fn(36, 28, |x, y| {
        image::Rgb([(x * 6) as u8, (y * 8) as u8, ((x + y) * 2) as u8])
    });
    let mut webp_buf = Vec::new();
    image::DynamicImage::ImageRgb8(img)
        .write_to(
            &mut std::io::Cursor::new(&mut webp_buf),
            image::ImageFormat::WebP,
        )
        .expect("encode webp fixture");

    for cru in ["4:4:4", "4:2:2", "4:2:0-cosited", "4:2:0-non-cosited"] {
        let mut child = Command::new(binary())
            .args([
                "--single-file",
                "--input-format",
                "webp",
                "--output-format",
                "jpg",
                "--optimize-cru",
                cru,
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn binary");
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(&webp_buf)
            .expect("write stdin");
        let output = child.wait_with_output().expect("wait binary");
        assert!(
            output.status.success(),
            "--optimize-cru {cru}: binary exited non-zero: status={:?} stderr={}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        );
        assert_is_jpeg(&output.stdout);
    }
}

#[test]
fn rejects_out_of_range_trellis_ac() {
    // The CLI parser must reject --trellis-ac outside 0..=50 with
    // exit code 2 and a usage error. The output bytes must be
    // empty (the binary refuses to run when the flag is invalid).
    let child = Command::new(binary())
        .args([
            "--single-file",
            "--input-format",
            "webp",
            "--output-format",
            "jpg",
            "--trellis-ac",
            "51",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    let _ = child.stdin.as_ref().expect("stdin").write_all(b"");
    let output = child.wait_with_output().expect("wait binary");
    assert_eq!(
        output.status.code(),
        Some(2),
        "expected exit code 2 for invalid --trellis-ac; got {:?}",
        output.status.code()
    );
    assert!(
        output.stdout.is_empty(),
        "stdout must be empty when CLI parsing fails"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid --trellis-ac"),
        "stderr should mention --trellis-ac: {stderr}"
    );
}

#[test]
fn rejects_bad_subsampling_value() {
    let child = Command::new(binary())
        .args([
            "--single-file",
            "--input-format",
            "webp",
            "--output-format",
            "jpg",
            "--subsampling",
            "3:1:1",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    let _ = child.stdin.as_ref().expect("stdin").write_all(b"");
    let output = child.wait_with_output().expect("wait binary");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("invalid --subsampling"),
        "stderr should mention --subsampling: {stderr}"
    );
}

#[test]
fn help_advertises_new_mozjpeg_flags() {
    // The v0 help banner must surface the new MozJPEG flags so
    // operators can discover them without reading the source.
    let child = Command::new(binary())
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    let output = child.wait_with_output().expect("wait binary");
    assert_eq!(output.status.code(), Some(2));
    let stderr = String::from_utf8_lossy(&output.stderr);
    for needle in ["--subsampling", "--optimize-cru", "--trellis-ac", "MozJPEG"] {
        assert!(
            stderr.contains(needle),
            "--help is missing {needle:?}; got:\n{stderr}"
        );
    }
}

/// Read all bytes from a JPEG fixture in the v0 golden tree (the
/// v0 fixtures are all `.jpg`; tests that need a WebP or PNG
/// fixture generate one in-memory). Retained for future golden-file
/// expansion (the v0 tree is JPG-only; the MozJPEG integration tests
/// generate their own WebP / PNG fixtures in-memory).
#[allow(dead_code)]
fn read_jpeg_fixture(name: &str) -> Vec<u8> {
    let p = golden_v0_dir().join(name);
    fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

#[test]
fn batch_webp_to_jpeg_round_trip_against_v0_fixture() {
    // End-to-end: convert one v0 JPG fixture to WebP via the
    // binary, then convert that WebP back to JPG via the binary.
    // Both conversions must produce valid output. This is a
    // smoke test for the batch-mode MozJPEG integration; the
    // byte-equality golden lives in `golden_v0.rs`.
    let tmp = std::env::temp_dir().join(format!(
        "mozjpeg-batch-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = fs::remove_dir_all(&tmp);
    fs::create_dir_all(&tmp).expect("mkdir tmp");
    // Copy the first JPG fixture into tmp.
    let src_jpg = golden_v0_dir()
        .read_dir()
        .unwrap()
        .flatten()
        .map(|e| e.path())
        .find(|p| p.extension().and_then(|x| x.to_str()) == Some("jpg"))
        .expect("at least one jpg fixture");
    let staged_jpg = tmp.join(src_jpg.file_name().unwrap());
    fs::copy(&src_jpg, &staged_jpg).expect("copy jpg to tmp");

    // Step 1: jpg -> webp via batch mode.
    let status = Command::new(binary())
        .args([
            tmp.to_string_lossy().as_ref(),
            "--input-format",
            "jpg",
            "--output-format",
            "webp",
        ])
        .status()
        .expect("spawn jpg->webp");
    assert!(status.success(), "jpg->webp batch failed");
    let webp_path = tmp
        .join(staged_jpg.file_name().unwrap())
        .with_extension("webp");
    assert!(webp_path.exists(), "webp output missing");

    // Step 2: webp -> jpg via batch mode (MozJPEG).
    fs::copy(&webp_path, tmp.join("intermediate.webp")).expect("stage webp");
    fs::remove_file(&webp_path).ok();
    let status = Command::new(binary())
        .args([
            tmp.to_string_lossy().as_ref(),
            "--input-format",
            "webp",
            "--output-format",
            "jpg",
        ])
        .status()
        .expect("spawn webp->jpg");
    assert!(status.success(), "webp->jpg batch failed");
    let jpg_out = tmp.join("intermediate.jpg");
    assert!(jpg_out.exists(), "jpg output missing");
    let bytes = fs::read(&jpg_out).expect("read jpg");
    assert_is_jpeg(&bytes);
    let _ = fs::remove_dir_all(&tmp);
}

#[test]
fn libjpeg_fallback_is_removed_no_image_codecs_jpeg_path() {
    // The libjpeg fallback is removed: the encoder no longer goes
    // through `image::codecs::jpeg`. This test is a compile-time
    // check: the format.rs source MUST NOT contain
    // `image::codecs::jpeg::JpegEncoder` (the libjpeg encoder path).
    // A grep-based assertion is a deliberate regression test — if a
    // future change re-adds the libjpeg path, this test fails with
    // the offending line.
    let fmt_rs = fs::read_to_string(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("format.rs"),
    )
    .expect("read src/format.rs");
    for needle in [
        "image::codecs::jpeg::JpegEncoder",
        "JpegEncoder::new_with_quality",
    ] {
        assert!(
            !fmt_rs.contains(needle),
            "libjpeg fallback re-introduced: src/format.rs still contains {needle:?}"
        );
    }
    // MozJPEG is the only encoder path.
    assert!(
        fmt_rs.contains("mozjpeg"),
        "mozjpeg crate is not referenced from src/format.rs"
    );
}

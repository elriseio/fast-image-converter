use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use rayon::prelude::*;

mod format;
mod params;
mod report;
use format::{
    CodecImpl, Format, JpegToPng, JpegToWebp, PngToJpeg, PngToWebp, WebpToJpeg,
    WebpToPng,
};
use params::parse_resize;

const BINARY_NAME: &str = "convert-to-webp";

#[derive(Debug, Clone, PartialEq, Eq)]
enum CliError {
    Usage,
    BadInputFormat(String),
    BadOutputFormat(String),
    BadQuality(String),
    BadResize(String),
    BadReportFd(String),
    AmbiguousMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Mode {
    /// Directory mode: positional arg is the source directory; outputs
    /// are written next to each input file; the v0 baseline.
    Batch { dir: PathBuf },
    /// Single-file mode: bytes are read from stdin, encoded bytes are
    /// written to stdout, and a single metadata line is emitted on
    /// stderr. See DE-004.
    SingleFile,
}

#[derive(Debug)]
struct Cli {
    mode: Mode,
    input_format: Option<Format>,
    output_format: Option<Format>,
    quality: Option<u8>,
    resize: Option<crate::format::ResizePolicy>,
    keep_source: bool,
    /// Emit the per-file metadata line as a structured NDJSON
    /// record instead of the v0 key=value shape. Per DE-005.
    json: bool,
    /// File descriptor the per-file report stream is written to.
    /// Defaults to 2 (stderr). Override with `--report-fd <N>`;
    /// N=1 is forbidden (would collide with the encoded bytes in
    /// single-file mode) and non-writable fds are rejected with
    /// usage + exit 2. Per DE-005 AC-7.
    report_fd: i32,
}

fn parse_quality(s: &str) -> Result<u8, String> {
    let n: i64 = s
        .parse()
        .map_err(|_| format!("not an integer: {s}"))?;
    if !(1..=100).contains(&n) {
        return Err(format!("{n} out of range 1..100"));
    }
    Ok(n as u8)
}

fn parse_cli(args: &[String]) -> Result<Cli, CliError> {
    let mut positional: Option<String> = None;
    let mut input_format: Option<Format> = None;
    let mut output_format: Option<Format> = None;
    let mut quality: Option<u8> = None;
    let mut resize: Option<crate::format::ResizePolicy> = None;
    let mut keep_source = false;
    let mut single_file = false;
    let mut json = false;
    let mut report_fd: i32 = 2;

    let mut i = 1;
    while i < args.len() {
        let arg = &args[i];
        match arg.as_str() {
            "--input-format" => {
                let v = args.get(i + 1).ok_or(CliError::Usage)?;
                input_format = Some(Format::parse(v).ok_or_else(|| {
                    CliError::BadInputFormat((*v).clone())
                })?);
                i += 2;
            }
            "--output-format" => {
                let v = args.get(i + 1).ok_or(CliError::Usage)?;
                output_format = Some(Format::parse(v).ok_or_else(|| {
                    CliError::BadOutputFormat((*v).clone())
                })?);
                i += 2;
            }
            "--quality" => {
                let v = args.get(i + 1).ok_or(CliError::Usage)?;
                quality = Some(parse_quality(v).map_err(CliError::BadQuality)?);
                i += 2;
            }
            "--resize" => {
                let v = args.get(i + 1).ok_or(CliError::Usage)?;
                resize = Some(parse_resize(v).map_err(CliError::BadResize)?);
                i += 2;
            }
            "--keep-source" => {
                keep_source = true;
                i += 1;
            }
            "--single-file" | "-1" => {
                single_file = true;
                i += 1;
            }
            "--json" => {
                json = true;
                i += 1;
            }
            "--report-fd" => {
                let v = args.get(i + 1).ok_or(CliError::Usage)?;
                let n: i64 = v.parse().map_err(|_| {
                    CliError::BadReportFd(format!("{v:?}: not an integer"))
                })?;
                if n < 0 || n > i32::MAX as i64 {
                    return Err(CliError::BadReportFd(format!(
                        "{v:?}: out of range 0..{}",
                        i32::MAX
                    )));
                }
                report_fd = n as i32;
                i += 2;
            }
            "-h" | "--help" => return Err(CliError::Usage),
            other if other.starts_with("--") => {
                eprintln!(
                    "{BINARY_NAME}: unknown flag: {other}\n\
                     Try '{BINARY_NAME} --help' for usage."
                );
                return Err(CliError::Usage);
            }
            _ => {
                if positional.is_some() {
                    eprintln!("{BINARY_NAME}: unexpected extra arg: {arg}");
                    return Err(CliError::Usage);
                }
                positional = Some(arg.clone());
                i += 1;
            }
        }
    }

    let mode = if single_file {
        if positional.is_some() {
            return Err(CliError::AmbiguousMode);
        }
        Mode::SingleFile
    } else {
        let dir = positional.ok_or(CliError::Usage)?;
        Mode::Batch {
            dir: PathBuf::from(dir),
        }
    };

    Ok(Cli {
        mode,
        input_format,
        output_format,
        quality,
        resize,
        keep_source,
        json,
        report_fd,
    })
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let cli = match parse_cli(&args) {
        Ok(c) => c,
        Err(e) => {
            match e {
                CliError::BadInputFormat(v) => eprintln!(
                    "{BINARY_NAME}: invalid --input-format: {v} (expected jpg, png, or webp)"
                ),
                CliError::BadOutputFormat(v) => eprintln!(
                    "{BINARY_NAME}: invalid --output-format: {v} (expected jpg, png, or webp)"
                ),
                CliError::BadQuality(v) => eprintln!(
                    "{BINARY_NAME}: invalid --quality: {v} (expected integer in 1..100)"
                ),
                CliError::BadResize(v) => eprintln!(
                    "{BINARY_NAME}: invalid --resize: {v} \
                     (expected 'none', 'cap=<W>', or 'auto:portrait=<W>,landscape=<H>')"
                ),
                CliError::BadReportFd(v) => eprintln!(
                    "{BINARY_NAME}: invalid --report-fd: {v} (expected integer 0, 2, or a writable fd)"
                ),
                CliError::AmbiguousMode => eprintln!(
                    "{BINARY_NAME}: --single-file does not accept a positional argument"
                ),
                CliError::Usage => {}
            }
            print_usage();
            return ExitCode::from(2);
        }
    };

    // Per DE-005 AC-7: N=1 is forbidden; values other than 1, 2,
    // or a writable integer fd are rejected with usage + exit 2.
    if let Err(msg) = validate_report_fd(cli.report_fd) {
        eprintln!("{BINARY_NAME}: {msg}");
        print_usage();
        return ExitCode::from(2);
    }

    let dir = match &cli.mode {
        Mode::Batch { dir } => dir.clone(),
        Mode::SingleFile => {
            return run_single_file(&cli);
        }
    };

    let dir: PathBuf = if dir.to_string_lossy().contains('/') {
        dir.clone()
    } else {
        // Bare arg (e.g. a year like "2025"): require GALLERY_BASE.
        // Per DE-006 / AR-002 the binary no longer carries a
        // hard-coded absolute host path; the operator MUST set the
        // environment variable explicitly or pass an absolute path.
        let gallery_base = match env::var("GALLERY_BASE") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                eprintln!(
                    "{BINARY_NAME}: bare argument {:?} requires GALLERY_BASE to be set \
                     (or pass an absolute path).",
                    dir
                );
                print_usage();
                return ExitCode::from(2);
            }
        };
        PathBuf::from(&gallery_base).join(dir)
    };

    if !dir.is_dir() {
        eprintln!("{BINARY_NAME}: not a directory: {}", dir.display());
        return ExitCode::from(1);
    }

    // Default = v0 behaviour: jpg -> webp. Both flags are explicit
    // overrides; the absent pair remains the v0 default.
    let input_format = cli.input_format.unwrap_or(Format::Jpg);
    let output_format = cli.output_format.unwrap_or(Format::Webp);

    let codec: CodecImpl = match (input_format, output_format) {
        (Format::Jpg, Format::Webp) => CodecImpl::JpegToWebp(JpegToWebp),
        (Format::Png, Format::Webp) => CodecImpl::PngToWebp(PngToWebp),
        (Format::Webp, Format::Png) => CodecImpl::WebpToPng(WebpToPng),
        (Format::Webp, Format::Jpg) => CodecImpl::WebpToJpeg(WebpToJpeg),
        (Format::Jpg, Format::Png) => CodecImpl::JpegToPng(JpegToPng),
        (Format::Png, Format::Jpg) => CodecImpl::PngToJpeg(PngToJpeg),
        (Format::Jpg, Format::Jpg)
        | (Format::Png, Format::Png)
        | (Format::Webp, Format::Webp) => {
            eprintln!(
                "{BINARY_NAME}: same input/output format ({input_format:?}) \
                 is a no-op; refusing to overwrite the source."
            );
            return ExitCode::from(2);
        }
    };

    // Build params: defaults match the v0 baseline; CLI flags override.
    let mut params = crate::params::Params::default();
    if let Some(q) = cli.quality {
        params.quality = q;
    }
    if let Some(r) = cli.resize {
        params.resize = r;
    }
    params.keep_source = cli.keep_source;

    let candidates: Vec<PathBuf> = match fs::read_dir(&dir) {
        Ok(rd) => rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && has_accepted_extension(p, codec.accepted_extensions())
            })
            .collect(),
        Err(e) => {
            eprintln!("{BINARY_NAME}: cannot read {}: {}", dir.display(), e);
            return ExitCode::from(1);
        }
    };

    if candidates.is_empty() {
        let accepted = codec.accepted_extensions().join(", .");
        println!(
            "{BINARY_NAME}: no candidate files (.{accepted}) in {}",
            dir.display()
        );
        return ExitCode::from(0);
    }

    let n = candidates.len();
    let count = AtomicU64::new(0);
    let total_in = AtomicU64::new(0);
    let total_out = AtomicU64::new(0);
    let failed = AtomicU64::new(0);

    candidates.par_iter().for_each(|src| {
        let started = Instant::now();
        let dst = src.with_extension(codec.output_extension());
        let conv = codec.convert_one_with(src, &dst, &params);
        let duration_ms = started.elapsed().as_millis() as u64;
        let (bytes_in, bytes_out) = match &conv {
            Ok(r) => (r.in_bytes, r.out_bytes),
            Err(_) => (0, 0),
        };
        // Per DE-005 § 2 the JSON mode emits one NDJSON record per
        // candidate in completion order (independent lines, no
        // enclosing array). Emitted from inside the parallel
        // iterator so each candidate's line lands as soon as it
        // finishes; eprintln! acquires the stderr lock per call so
        // the JSON lines do not interleave even though rayon runs
        // the closure concurrently.
        match conv {
            Ok(report) => {
                if !params.keep_source {
                    if let Err(e) = fs::remove_file(src) {
                        // Decode / encode / write succeeded but
                        // the post-conversion source-delete failed
                        // (codec-bounds.md INV-CB-3 / § 3 Outputs:
                        // 'source-delete' is part of success).
                        // Record this candidate as a failure.
                        failed.fetch_add(1, Ordering::Relaxed);
                        if cli.json {
                            emit_batch_record_io_failure(
                                &cli,
                                input_format,
                                output_format,
                                &params,
                                src,
                                &report,
                                &format!("cannot delete source: {e}"),
                                duration_ms,
                            );
                        } else {
                            eprintln!(
                                "{BINARY_NAME}: cannot delete {}: {e}",
                                src.display()
                            );
                        }
                        return;
                    }
                }
                count.fetch_add(1, Ordering::Relaxed);
                total_in.fetch_add(bytes_in, Ordering::Relaxed);
                total_out.fetch_add(bytes_out, Ordering::Relaxed);
                if cli.json {
                    emit_batch_record_success(
                        &cli,
                        input_format,
                        output_format,
                        &params,
                        src,
                        &report,
                        duration_ms,
                    );
                }
            }
            Err(e) => {
                failed.fetch_add(1, Ordering::Relaxed);
                let kind = codec_error_kind(&e);
                let raw = codec_error_inner_message(&e);
                if cli.json {
                    emit_batch_record_failure(
                        &cli,
                        input_format,
                        output_format,
                        &params,
                        src,
                        bytes_in,
                        kind,
                        raw,
                        duration_ms,
                    );
                } else {
                    eprintln!("{BINARY_NAME}: {}: {}", src.display(), e);
                }
            }
        }
    });

    let count_val = count.load(Ordering::Relaxed);
    let total_in_val = total_in.load(Ordering::Relaxed);
    let total_out_val = total_out.load(Ordering::Relaxed);
    let failed_val = failed.load(Ordering::Relaxed);

    println!(
        "{BINARY_NAME}: {} files in {}: {} -> {}",
        count_val,
        dir.display(),
        human_bytes(total_in_val),
        human_bytes(total_out_val)
    );
    eprintln!("(processed {} candidates, {} failed)", n, failed_val);

    if failed_val > 0 {
        ExitCode::from(1)
    } else {
        ExitCode::from(0)
    }
}

fn has_accepted_extension(p: &Path, accepted: &'static [&'static str]) -> bool {
    let ext = match p.extension().and_then(|x| x.to_str()) {
        Some(e) => e.to_ascii_lowercase(),
        None => return false,
    };
    accepted.iter().any(|a| a.eq_ignore_ascii_case(&ext))
}

/// Single-file mode: read all of stdin, decode via the chosen codec,
/// encode with the supplied params, and write the encoded bytes to
/// stdout. The metadata line on stderr lands in DE-004 commit 3; this
/// helper only establishes the read/write plumbing and exit-code
/// contract.
fn run_single_file(cli: &Cli) -> ExitCode {
    use std::io::{Read, Write};
    use std::time::Instant;

    // Default = v0 behaviour: jpg -> webp. Both flags are explicit
    // overrides; the absent pair remains the v0 default.
    let input_format = cli.input_format.unwrap_or(Format::Jpg);
    let output_format = cli.output_format.unwrap_or(Format::Webp);

    let codec: CodecImpl = match (input_format, output_format) {
        (Format::Jpg, Format::Webp) => CodecImpl::JpegToWebp(JpegToWebp),
        (Format::Png, Format::Webp) => CodecImpl::PngToWebp(PngToWebp),
        (Format::Webp, Format::Png) => CodecImpl::WebpToPng(WebpToPng),
        (Format::Webp, Format::Jpg) => CodecImpl::WebpToJpeg(WebpToJpeg),
        (Format::Jpg, Format::Png) => CodecImpl::JpegToPng(JpegToPng),
        (Format::Png, Format::Jpg) => CodecImpl::PngToJpeg(PngToJpeg),
        (Format::Jpg, Format::Jpg)
        | (Format::Png, Format::Png)
        | (Format::Webp, Format::Webp) => {
            let params = crate::params::Params::default();
            emit_single_file_failure_report(
                cli,
                input_format,
                output_format,
                &params,
                0,
                None,
                0,
                None,
                crate::report::ErrorKind::Io,
                "same input/output format is a no-op",
                0,
            );
            return ExitCode::from(2);
        }
    };

    let mut params = crate::params::Params::default();
    if let Some(q) = cli.quality {
        params.quality = q;
    }
    if let Some(r) = cli.resize {
        params.resize = r;
    }
    // keep_source is silently ignored in single-file mode (no source
    // filesystem path to preserve; DE-004 §2 Scope).

    let started = Instant::now();
    let mut in_bytes_buf = Vec::new();
    if let Err(e) = std::io::stdin().read_to_end(&mut in_bytes_buf) {
        emit_single_file_failure_report(
            cli,
            input_format,
            output_format,
            &params,
            0,
            None,
            0,
            None,
            crate::report::ErrorKind::Io,
            &format!("cannot read stdin: {e}"),
            started.elapsed().as_millis() as u64,
        );
        return ExitCode::from(1);
    }

    let img = match codec.decode_bytes(&in_bytes_buf) {
        Ok(img) => img,
        Err(e) => {
            let raw = codec_error_inner_message(&e);
            emit_single_file_failure_report(
                cli,
                input_format,
                output_format,
                &params,
                in_bytes_buf.len() as u64,
                None,
                0,
                None,
                crate::report::ErrorKind::Decode,
                raw,
                started.elapsed().as_millis() as u64,
            );
            return ExitCode::from(1);
        }
    };

    let resized = crate::format::apply_resize(&img, params.resize);

    let encoded = match codec.encode_to_vec(&resized, params.quality) {
        Ok(b) => b,
        Err(e) => {
            let raw = codec_error_inner_message(&e);
            emit_single_file_failure_report(
                cli,
                input_format,
                output_format,
                &params,
                in_bytes_buf.len() as u64,
                Some((img.width(), img.height())),
                0,
                None,
                crate::report::ErrorKind::Encode,
                raw,
                started.elapsed().as_millis() as u64,
            );
            return ExitCode::from(1);
        }
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if let Err(e) = out.write_all(&encoded) {
        emit_single_file_failure_report(
            cli,
            input_format,
            output_format,
            &params,
            in_bytes_buf.len() as u64,
            Some((img.width(), img.height())),
            0,
            None,
            crate::report::ErrorKind::Io,
            &format!("cannot write stdout: {e}"),
            started.elapsed().as_millis() as u64,
        );
        return ExitCode::from(1);
    }
    if let Err(e) = out.flush() {
        emit_single_file_failure_report(
            cli,
            input_format,
            output_format,
            &params,
            in_bytes_buf.len() as u64,
            Some((img.width(), img.height())),
            encoded.len() as u64,
            Some((resized.width(), resized.height())),
            crate::report::ErrorKind::Io,
            &format!("cannot flush stdout: {e}"),
            started.elapsed().as_millis() as u64,
        );
        return ExitCode::from(1);
    }

    emit_single_file_success_report(
        cli,
        input_format,
        output_format,
        &params,
        in_bytes_buf.len() as u64,
        (img.width(), img.height()),
        encoded.len() as u64,
        (resized.width(), resized.height()),
        started.elapsed().as_millis() as u64,
    );
    ExitCode::from(0)
}

/// Emit the single-line metadata record on stderr in single-file
/// mode. Shape per DE-004 § 5:
///
///   status=<ok|err> in_bytes=<N> out_bytes=<N> duration_ms=<N> error=<message>
///
/// `error=` is omitted on success and present (with the codec-
/// reported message) on failure.
fn emit_single_file_metadata(
    status: &str,
    in_bytes: u64,
    out_bytes: u64,
    duration_ms: u64,
    error: Option<&str>,
) {
    match error {
        Some(msg) => eprintln!(
            "status={status} in_bytes={in_bytes} out_bytes={out_bytes} \
             duration_ms={duration_ms} error={msg}"
        ),
        None => eprintln!(
            "status={status} in_bytes={in_bytes} out_bytes={out_bytes} \
             duration_ms={duration_ms}"
        ),
    }
}

/// Emit the success-path single-file report on stderr. When
/// `--json` is set, emits a structured NDJSON record per DE-005
/// § 2 (one JSON object per line). Otherwise falls back to the
/// v0 key=value shape emitted by `emit_single_file_metadata`.
///
/// Failure paths in single-file mode still use
/// `emit_single_file_metadata` directly; they are migrated to a
/// JSON-capable dispatcher in commit 3 (per DE-005 § 4).
fn emit_single_file_success_report(
    cli: &Cli,
    input_format: Format,
    output_format: Format,
    params: &crate::params::Params,
    in_bytes: u64,
    input_dims: (u32, u32),
    out_bytes: u64,
    output_dims: (u32, u32),
    duration_ms: u64,
) {
    if !cli.json {
        if cli.report_fd == 2 {
            emit_single_file_metadata(
                "ok",
                in_bytes,
                out_bytes,
                duration_ms,
                None,
            );
        } else {
            let line = format!(
                "status=ok in_bytes={in_bytes} out_bytes={out_bytes} duration_ms={duration_ms}"
            );
            emit_report_line(cli.report_fd, &line);
        }
        return;
    }
    use crate::report::{
        CodecMeta, ImageFormat, ImageInfo, Mode, Report, Status,
    };
    let report = Report {
        mode: Mode::SingleFile,
        status: Status::Ok,
        input: Some(ImageInfo {
            format: ImageFormat::from(input_format),
            bytes: in_bytes,
            width: Some(input_dims.0),
            height: Some(input_dims.1),
        }),
        output: Some(ImageInfo {
            format: ImageFormat::from(output_format),
            bytes: out_bytes,
            width: Some(output_dims.0),
            height: Some(output_dims.1),
        }),
        codec: CodecMeta {
            quality: params.quality,
            resize_policy: resize_policy_to_string(params.resize),
        },
        host: host_meta(),
        duration_ms,
        error: None,
    };
    emit_report_line(cli.report_fd, &report.to_json());
}

/// Emit a failure-path single-file report on stderr. When `--json`
/// is set, emits a structured NDJSON record per DE-005 § 2 with
/// `status: "err"` and the documented `error` block. Otherwise
/// falls back to the v0 key=value shape with the kind prefix
/// (`decode error:`, `encode error:`, or `io error:`).
///
/// `input_dims` / `output_dims` are `None` when the corresponding
/// data is not known (decode failure → no output dims; encode
/// failure → output dims are zero; pre-decode failure → both
/// `None`). `in_bytes` / `out_bytes` of 0 produce `null` blocks
/// in the JSON record (per AC-4: "output fields are present but
/// zeroed where not meaningful").
fn emit_single_file_failure_report(
    cli: &Cli,
    input_format: Format,
    output_format: Format,
    params: &crate::params::Params,
    in_bytes: u64,
    input_dims: Option<(u32, u32)>,
    out_bytes: u64,
    output_dims: Option<(u32, u32)>,
    error_kind: crate::report::ErrorKind,
    error_msg: &str,
    duration_ms: u64,
) {
    if !cli.json {
        let kind_str = match error_kind {
            crate::report::ErrorKind::Decode => "decode error",
            crate::report::ErrorKind::Encode => "encode error",
            crate::report::ErrorKind::Io => "io error",
        };
        let full_msg = format!("{kind_str}: {error_msg}");
        if cli.report_fd == 2 {
            emit_single_file_metadata(
                "err",
                in_bytes,
                out_bytes,
                duration_ms,
                Some(&full_msg),
            );
        } else {
            let line = format!(
                "status=err in_bytes={in_bytes} out_bytes={out_bytes} \
                 duration_ms={duration_ms} error={full_msg}"
            );
            emit_report_line(cli.report_fd, &line);
        }
        return;
    }
    use crate::report::{
        CodecMeta, ImageFormat, ImageInfo, Mode, Report, ReportError, Status,
    };
    let input = if in_bytes > 0 {
        Some(ImageInfo {
            format: ImageFormat::from(input_format),
            bytes: in_bytes,
            width: input_dims.map(|d| d.0),
            height: input_dims.map(|d| d.1),
        })
    } else {
        None
    };
    let output = if out_bytes > 0 {
        Some(ImageInfo {
            format: ImageFormat::from(output_format),
            bytes: out_bytes,
            width: output_dims.map(|d| d.0),
            height: output_dims.map(|d| d.1),
        })
    } else {
        None
    };
    let report = Report {
        mode: Mode::SingleFile,
        status: Status::Err,
        input,
        output,
        codec: CodecMeta {
            quality: params.quality,
            resize_policy: resize_policy_to_string(params.resize),
        },
        host: host_meta(),
        duration_ms,
        error: Some(ReportError {
            kind: error_kind,
            message: error_msg.to_string(),
        }),
    };
    emit_report_line(cli.report_fd, &report.to_json());
}

/// Format a `ResizePolicy` back to its CLI string form so the
/// JSON report is round-trippable against `parse_resize`. Used by
/// the `--json` mode; the v0 key=value shape does not embed the
/// policy.
fn resize_policy_to_string(p: crate::format::ResizePolicy) -> String {
    use crate::format::ResizePolicy;
    match p {
        ResizePolicy::None => "none".to_string(),
        ResizePolicy::MaxWidth(w) => format!("cap={w}"),
        ResizePolicy::PortraitLandscape {
            portrait,
            landscape,
        } => format!("auto:portrait={portrait},landscape={landscape}"),
    }
}

/// Build-time host metadata. Both fields are baked into the
/// binary at compile time by `build.rs`:
/// - `CONVERT_TO_WEBP_LIBWEBP_VERSION`: pkg-config --modversion;
///   required (the `webp` crate's build script already depends
///   on pkg-config finding libwebp).
/// - `CONVERT_TO_WEBP_BUILD_COMMIT_SHA`: git rev-parse HEAD;
///   optional (absent on tarball builds or when git is missing).
fn host_meta() -> crate::report::HostMeta {
    crate::report::HostMeta {
        libwebp_version: env!("CONVERT_TO_WEBP_LIBWEBP_VERSION"),
        build_commit_sha: option_env!("CONVERT_TO_WEBP_BUILD_COMMIT_SHA"),
    }
}

/// Validate the `--report-fd` argument per DE-005 AC-7. Returns
/// `Ok(())` if `fd` is an acceptable report stream, `Err(msg)`
/// otherwise. The accepted set is:
///
/// - `fd == 2` (the conventional stderr fd); accepted without
///   further checks (the runtime may have closed stderr; if so,
///   writes fail but the binary stays consistent).
/// - `fd` is a positive integer that, when queried via
///   `fcntl(F_GETFL)`, reports an access mode of `O_WRONLY` or
///   `O_RDWR`. Read-only fds are rejected.
/// - `fd == 0` (stdin) is accepted only if it is open for
///   writing (rare, but the validation is correct).
///
/// `fd == 1` (stdout) is **forbidden** regardless of access mode:
/// in single-file mode stdout carries the encoded bytes and the
/// report stream would collide with the payload.
fn validate_report_fd(fd: i32) -> Result<(), String> {
    if fd == 1 {
        return Err(
            "--report-fd 1 is forbidden (would collide with the encoded bytes \
             in single-file mode)"
                .to_string(),
        );
    }
    if fd == 2 {
        return Ok(());
    }
    if fd < 0 {
        return Err(format!("--report-fd must be a non-negative integer; got {fd}"));
    }
    // fcntl(fd, F_GETFL): returns -1 with errno=EBADF if the fd
    // is not open, or the file status flags otherwise.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 {
        return Err(format!(
            "--report-fd {fd}: fd is not open or not a valid file descriptor"
        ));
    }
    // O_ACCMODE is the access-mode mask (O_RDONLY / O_WRONLY /
    // O_RDWR). A read-only fd is rejected.
    let accmode = flags & libc::O_ACCMODE as i32;
    if accmode != libc::O_WRONLY && accmode != libc::O_RDWR {
        return Err(format!(
            "--report-fd {fd}: fd is not open for writing"
        ));
    }
    Ok(())
}

/// Emit one line of report output to `fd`, terminated with `\n`.
/// For `fd == 2` (the default) this delegates to `eprintln!`
/// which acquires the stderr lock and writes the full line
/// atomically. For any other fd, a process-wide mutex serialises
/// non-stderr writes so the rayon-driven batch path cannot
/// interleave bytes from different JSON lines on a pipe (POSIX
/// guarantees atomic writes up to `PIPE_BUF` only on regular
/// files / pipes; the mutex is the portable line-atomicity
/// guarantee we provide).
fn emit_report_line(fd: i32, line: &str) {
    if fd == 2 {
        eprintln!("{}", line);
        return;
    }
    let _guard = report_fd_lock()
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let bytes = line.as_bytes();
    let mut written = 0;
    while written < bytes.len() {
        let n = unsafe {
            libc::write(
                fd,
                bytes[written..].as_ptr() as *const _,
                bytes.len() - written,
            )
        };
        if n <= 0 {
            return;
        }
        written += n as usize;
    }
    let newline: [u8; 1] = [b'\n'];
    unsafe {
        libc::write(fd, newline.as_ptr() as *const _, 1);
    }
}

fn report_fd_lock() -> &'static std::sync::Mutex<()> {
    use std::sync::Mutex;
    use std::sync::OnceLock;
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

/// Return the inner message of a `CodecError` without the
/// `"decode error: "` / `"encode error: "` / `"io error: "`
/// prefix that `Display` adds. The JSON report's `error.message`
/// field carries the raw codec message; the v0 key=value path
/// adds the kind-prefix back via `emit_single_file_failure_report`.
fn codec_error_inner_message(e: &crate::format::CodecError) -> &str {
    use crate::format::CodecError;
    match e {
        CodecError::Decode(m) | CodecError::Encode(m) | CodecError::Io(m) => m,
    }
}

/// Map a `CodecError` to the JSON `error.kind` enum. Used by the
/// batch-mode JSON emitter and by `emit_batch_record_failure`.
fn codec_error_kind(e: &crate::format::CodecError) -> crate::report::ErrorKind {
    use crate::format::CodecError;
    match e {
        CodecError::Decode(_) => crate::report::ErrorKind::Decode,
        CodecError::Encode(_) => crate::report::ErrorKind::Encode,
        CodecError::Io(_) => crate::report::ErrorKind::Io,
    }
}

/// Emit one NDJSON line for a successful batch-mode conversion.
/// Called from inside the rayon parallel iterator; per DE-005 § 2
/// the line is emitted as soon as the candidate finishes, in
/// completion order (no enclosing array).
fn emit_batch_record_success(
    cli: &Cli,
    input_format: Format,
    output_format: Format,
    params: &crate::params::Params,
    src: &Path,
    report: &crate::format::ConversionReport,
    duration_ms: u64,
) {
    use crate::report::{
        CodecMeta, ImageFormat, ImageInfo, Mode, Report, Status,
    };
    let r = Report {
        mode: Mode::Batch,
        status: Status::Ok,
        input: Some(ImageInfo {
            format: ImageFormat::from(input_format),
            bytes: report.in_bytes,
            width: Some(report.input_width),
            height: Some(report.input_height),
        }),
        output: Some(ImageInfo {
            format: ImageFormat::from(output_format),
            bytes: report.out_bytes,
            width: Some(report.output_width),
            height: Some(report.output_height),
        }),
        codec: CodecMeta {
            quality: params.quality,
            resize_policy: resize_policy_to_string(params.resize),
        },
        host: host_meta(),
        duration_ms,
        error: None,
    };
    // `src` is intentionally not embedded in the per-file record;
    // the caller's NDJSON line index identifies the file in
    // completion order (per DE-005 § 2).
    let _ = src;
    emit_report_line(cli.report_fd, &r.to_json());
}

/// Emit one NDJSON line for a batch-mode codec failure (decode /
/// encode / write). `in_bytes` is the source byte count when
/// known (0 when decode never ran); output dims are absent.
fn emit_batch_record_failure(
    cli: &Cli,
    input_format: Format,
    _output_format: Format,
    params: &crate::params::Params,
    src: &Path,
    in_bytes: u64,
    error_kind: crate::report::ErrorKind,
    error_msg: &str,
    duration_ms: u64,
) {
    use crate::report::{
        CodecMeta, ImageFormat, ImageInfo, Mode, Report, ReportError, Status,
    };
    let input = if in_bytes > 0 {
        Some(ImageInfo {
            format: ImageFormat::from(input_format),
            bytes: in_bytes,
            width: None,
            height: None,
        })
    } else {
        None
    };
    let r = Report {
        mode: Mode::Batch,
        status: Status::Err,
        input,
        output: None,
        codec: CodecMeta {
            quality: params.quality,
            resize_policy: resize_policy_to_string(params.resize),
        },
        host: host_meta(),
        duration_ms,
        error: Some(ReportError {
            kind: error_kind,
            message: error_msg.to_string(),
        }),
    };
    let _ = src;
    emit_report_line(cli.report_fd, &r.to_json());
}

/// Emit one NDJSON line for a post-conversion source-delete
/// failure (decode / encode / write succeeded but `fs::remove_file`
/// failed). The struct already carries input / output bytes and
/// dimensions because the codec returned Ok; the error.kind is
/// Io and the message names the delete step.
fn emit_batch_record_io_failure(
    cli: &Cli,
    input_format: Format,
    output_format: Format,
    params: &crate::params::Params,
    src: &Path,
    conv: &crate::format::ConversionReport,
    error_msg: &str,
    duration_ms: u64,
) {
    use crate::report::{
        CodecMeta, ImageFormat, ImageInfo, Mode, Report, ReportError, Status,
    };
    let r = Report {
        mode: Mode::Batch,
        status: Status::Err,
        input: Some(ImageInfo {
            format: ImageFormat::from(input_format),
            bytes: conv.in_bytes,
            width: Some(conv.input_width),
            height: Some(conv.input_height),
        }),
        output: Some(ImageInfo {
            format: ImageFormat::from(output_format),
            bytes: conv.out_bytes,
            width: Some(conv.output_width),
            height: Some(conv.output_height),
        }),
        codec: CodecMeta {
            quality: params.quality,
            resize_policy: resize_policy_to_string(params.resize),
        },
        host: host_meta(),
        duration_ms,
        error: Some(ReportError {
            kind: crate::report::ErrorKind::Io,
            message: error_msg.to_string(),
        }),
    };
    let _ = src;
    emit_report_line(cli.report_fd, &r.to_json());
}

fn print_usage() {
    eprintln!(
        "Usage: convert-to-webp <dir> [--input-format <fmt>] [--output-format <fmt>]\n\
         \x20                      [--quality <1..100>] [--resize <policy>] [--keep-source]\n\
         \x20                      [--json] [--report-fd <N>]\n\
         \x20      convert-to-webp --single-file [--input-format <fmt>] [--output-format <fmt>]\n\
         \x20                      [--quality <1..100>] [--resize <policy>]\n\
         \x20                      [--json] [--report-fd <N>]\n\
         \n\
         Arguments:\n\
         \x20 <dir>                  directory containing the input images (batch mode)\n\
         \n\
         Flags:\n\
         \x20 --input-format <fmt>   one of: jpg, png, webp (default: jpg)\n\
         \x20 --output-format <fmt>  one of: jpg, png, webp (default: webp)\n\
         \x20 --quality <n>          encode quality in 1..100 (default: 85; honoured by WebP and JPEG outputs)\n\
         \x20 --resize <policy>      'none' | 'cap=<W>' | 'auto:portrait=<W>,landscape=<H>' (default: auto:portrait=800,landscape=1000)\n\
         \x20 --keep-source          leave the source file in place after a successful conversion (batch mode only)\n\
         \x20 --single-file, -1      read one image from stdin, write the encoded image to stdout\n\
         \x20 --json                 emit the per-file report as a structured NDJSON record (DE-005) instead of the v0 key=value line\n\
         \x20 --report-fd <N>        override the report stream fd (default 2; N=1 is forbidden)\n\
         \x20 -h, --help             show this help\n\
         \n\
         Examples:\n\
         \x20 convert-to-webp /tmp/my-images\n\
         \x20 convert-to-webp /tmp/my-images --input-format png --output-format webp\n\
         \x20 convert-to-webp /tmp/my-images --input-format webp --output-format png\n\
         \x20 convert-to-webp /tmp/my-images --input-format webp --output-format jpg\n\
         \x20 convert-to-webp /tmp/my-images --quality 75 --resize cap=1024 --keep-source\n\
         \x20 cat input.jpg | convert-to-webp --single-file --output-format webp > output.webp\n\
         \x20 cat input.jpg | convert-to-webp --single-file --output-format webp --json > out.webp 2> report.jsonl\n\
         \n\
         Env:\n\
         \x20 GALLERY_BASE  optional; when set, used as the parent directory for a bare\n\
         \x20                positional argument (e.g. a year like \"2025\"). Has no built-in\n\
         \x20                default; pass an absolute path or set GALLERY_BASE."
    );
}

fn human_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "K", "M", "G", "T"];
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{}{}", n, UNITS[0])
    } else {
        format!("{:.1}{}", v, UNITS[i])
    }
}

#[cfg(test)]
mod cli_tests {
    use super::*;

    fn v(s: &[&str]) -> Vec<String> {
        s.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn parses_default_invocation() {
        let cli = parse_cli(&v(&["gallery-compress", "/tmp/x"])).unwrap();
        assert_eq!(cli.mode, Mode::Batch { dir: PathBuf::from("/tmp/x") });
        assert_eq!(cli.input_format, None);
        assert_eq!(cli.output_format, None);
    }

    #[test]
    fn parses_explicit_flags() {
        let cli = parse_cli(&v(&[
            "gallery-compress",
            "/tmp/x",
            "--input-format",
            "png",
            "--output-format",
            "webp",
        ]))
        .unwrap();
        assert_eq!(cli.input_format, Some(Format::Png));
        assert_eq!(cli.output_format, Some(Format::Webp));
    }

    #[test]
    fn rejects_bad_input_format() {
        let err = parse_cli(&v(&[
            "gallery-compress",
            "/tmp/x",
            "--input-format",
            "tiff",
        ]))
        .unwrap_err();
        assert!(matches!(err, CliError::BadInputFormat(s) if s == "tiff"));
    }

    #[test]
    fn rejects_bad_output_format() {
        let err = parse_cli(&v(&[
            "gallery-compress",
            "/tmp/x",
            "--output-format",
            "gif",
        ]))
        .unwrap_err();
        assert!(matches!(err, CliError::BadOutputFormat(s) if s == "gif"));
    }

    #[test]
    fn rejects_unknown_flag() {
        let err = parse_cli(&v(&["gallery-compress", "--foo"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn rejects_help() {
        let err = parse_cli(&v(&["gallery-compress", "--help"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn rejects_missing_positional() {
        let err = parse_cli(&v(&["gallery-compress"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn rejects_extra_positional() {
        let err = parse_cli(&v(&["gallery-compress", "/tmp/a", "/tmp/b"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn parses_single_file_mode() {
        let cli = parse_cli(&v(&["gallery-compress", "--single-file"])).unwrap();
        assert_eq!(cli.mode, Mode::SingleFile);
    }

    #[test]
    fn rejects_single_file_with_positional() {
        let err = parse_cli(&v(&["gallery-compress", "--single-file", "/tmp/x"]))
            .unwrap_err();
        assert_eq!(err, CliError::AmbiguousMode);
    }

    #[test]
    fn format_parse_accepts_three_values() {
        assert_eq!(Format::parse("jpg"), Some(Format::Jpg));
        assert_eq!(Format::parse("jpeg"), Some(Format::Jpg));
        assert_eq!(Format::parse("JPEG"), Some(Format::Jpg));
        assert_eq!(Format::parse("png"), Some(Format::Png));
        assert_eq!(Format::parse("PNG"), Some(Format::Png));
        assert_eq!(Format::parse("webp"), Some(Format::Webp));
        assert_eq!(Format::parse("WebP"), Some(Format::Webp));
        assert_eq!(Format::parse("tiff"), None);
    }
}

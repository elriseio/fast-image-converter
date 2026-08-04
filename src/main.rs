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
    CodecImpl, Format, JpegToPng, JpegToWebp, PngToJpeg, PngToWebp, WebpToJpeg, WebpToPng,
};
use params::parse_resize;

// Runtime error prefixes and the usage banner use the canonical
// product name. Compatibility aliases (`convert-to-webp`,
// `gallery-compress`) live in `src/bin/*.rs` and forward into this
// binary; the aliases themselves emit their own one-line
// deprecation hint on stderr before spawning the canonical binary.
const BINARY_NAME: &str = "fast-image-converter";

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
    /// stderr.
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
    /// record instead of the v0 key=value shape.
    json: bool,
    /// File descriptor the per-file report stream is written to.
    /// Defaults to 2 (stderr). Override with `--report-fd <N>`;
    /// N=1 is forbidden (would collide with the encoded bytes in
    /// single-file mode) and non-writable fds are rejected with
    /// usage + exit 2.
    report_fd: i32,
}

/// Bounded context shared by every per-file report emitter. Bundles
/// the fields that are identical across the success and failure
/// paths (CLI flags, formats, params, wall-time) so the emitter
/// signatures stay below clippy's seven-argument limit. The struct
/// is `Copy`-friendly because every field is small and trivially
/// cloneable; callers that already hold `&Cli` pass a single
/// reference instead of four loose parameters.
#[derive(Copy, Clone)]
struct ReportContext<'a> {
    cli: &'a Cli,
    input_format: Format,
    output_format: Format,
    params: &'a crate::params::Params,
    duration_ms: u64,
}

fn parse_quality(s: &str) -> Result<u8, String> {
    let n: i64 = s.parse().map_err(|_| format!("not an integer: {s}"))?;
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
                input_format =
                    Some(Format::parse(v).ok_or_else(|| CliError::BadInputFormat((*v).clone()))?);
                i += 2;
            }
            "--output-format" => {
                let v = args.get(i + 1).ok_or(CliError::Usage)?;
                output_format =
                    Some(Format::parse(v).ok_or_else(|| CliError::BadOutputFormat((*v).clone()))?);
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
                let n: i64 = v
                    .parse()
                    .map_err(|_| CliError::BadReportFd(format!("{v:?}: not an integer")))?;
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

    // N=1 is forbidden; values other than 1, 2, or a writable
    // integer fd are rejected with usage + exit 2.
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
        // The binary does not carry a hard-coded absolute host path;
        // the operator MUST set the environment variable explicitly
        // or pass an absolute path.
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
        (Format::Jpg, Format::Jpg) | (Format::Png, Format::Png) | (Format::Webp, Format::Webp) => {
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
            .filter(|p| p.is_file() && has_accepted_extension(p, codec.accepted_extensions()))
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

    // Bounded report context template. The closure re-derives a
    // per-candidate ctx because `duration_ms` varies per file;
    // everything else is captured from the surrounding scope.
    let ctx_template = ReportContext {
        cli: &cli,
        input_format,
        output_format,
        params: &params,
        duration_ms: 0,
    };

    candidates.par_iter().for_each(|src| {
        let started = Instant::now();
        let dst = src.with_extension(codec.output_extension());
        // Reject oversized source files BEFORE decode so the rayon
        // worker never reads past the per-file byte limit. The check
        // uses `fs::metadata` (no I/O on the source bytes), so the
        // bound is enforced without spending time or memory on the
        // actual payload.
        let conv = fs::metadata(src)
            .map_err(|e| crate::format::CodecError::Io(e.to_string()))
            .and_then(|m| {
                crate::format::check_input_size(m.len()).map_err(crate::format::CodecError::Io)?;
                codec.convert_one_with(src, &dst, &params)
            });
        let duration_ms = started.elapsed().as_millis() as u64;
        let (bytes_in, bytes_out) = match &conv {
            Ok(r) => (r.in_bytes, r.out_bytes),
            Err(_) => (0, 0),
        };
        // JSON mode emits one NDJSON record per candidate in
        // completion order (independent lines, no enclosing array).
        // Emitted from inside the parallel iterator so each
        // candidate's line lands as soon as it finishes; eprintln!
        // acquires the stderr lock per call so the JSON lines do
        // not interleave even though rayon runs the closure
        // concurrently.
        let ctx = ReportContext {
            duration_ms,
            ..ctx_template
        };
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
                                &ctx,
                                src,
                                &report,
                                &format!("cannot delete source: {e}"),
                            );
                        } else {
                            eprintln!("{BINARY_NAME}: cannot delete {}: {e}", src.display());
                        }
                        return;
                    }
                }
                count.fetch_add(1, Ordering::Relaxed);
                total_in.fetch_add(bytes_in, Ordering::Relaxed);
                total_out.fetch_add(bytes_out, Ordering::Relaxed);
                if cli.json {
                    emit_batch_record_success(&ctx, src, &report);
                }
            }
            Err(e) => {
                failed.fetch_add(1, Ordering::Relaxed);
                let kind = codec_error_kind(&e);
                let raw = codec_error_inner_message(&e);
                if cli.json {
                    emit_batch_record_failure(&ctx, src, bytes_in, kind, raw);
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
/// stdout. This helper establishes the read/write plumbing, the
/// per-file metadata line on stderr, and the exit-code contract.
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
        (Format::Jpg, Format::Jpg) | (Format::Png, Format::Png) | (Format::Webp, Format::Webp) => {
            let params = crate::params::Params::default();
            let ctx = ReportContext {
                cli,
                input_format,
                output_format,
                params: &params,
                duration_ms: 0,
            };
            emit_single_file_failure_report(
                &ctx,
                0,
                None,
                0,
                None,
                crate::report::ErrorKind::Io,
                "same input/output format is a no-op",
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
    // keep_source is silently ignored in single-file mode (no
    // source filesystem path to preserve).

    let started = Instant::now();
    let mut in_bytes_buf = Vec::new();
    // Bound stdin at `MAX_STDIN_BYTES + 1` so an oversized payload
    // is detected without reading it fully into memory. `take(MAX+1)`
    // stops the read once that many bytes are observed; the `+1`
    // lets us distinguish "exactly MAX" (allowed) from "at least
    // MAX+1" (rejected). The Vec may still grow up to MAX+1 bytes,
    // which is well within the binary's working set.
    let stdin_limit = crate::format::MAX_STDIN_BYTES.saturating_add(1);
    let read_result = std::io::stdin()
        .take(stdin_limit)
        .read_to_end(&mut in_bytes_buf);
    match read_result {
        Err(e) => {
            let ctx = ReportContext {
                cli,
                input_format,
                output_format,
                params: &params,
                duration_ms: started.elapsed().as_millis() as u64,
            };
            emit_single_file_failure_report(
                &ctx,
                0,
                None,
                0,
                None,
                crate::report::ErrorKind::Io,
                &format!("cannot read stdin: {e}"),
            );
            return ExitCode::from(1);
        }
        Ok(_) if in_bytes_buf.len() as u64 > crate::format::MAX_STDIN_BYTES => {
            let ctx = ReportContext {
                cli,
                input_format,
                output_format,
                params: &params,
                duration_ms: started.elapsed().as_millis() as u64,
            };
            emit_single_file_failure_report(
                &ctx,
                crate::format::MAX_STDIN_BYTES,
                None,
                0,
                None,
                crate::report::ErrorKind::Io,
                &format!(
                    "stdin input exceeds the per-file limit of {} bytes; see \
                     docs/contracts/codec-bounds.md § 4",
                    crate::format::MAX_STDIN_BYTES
                ),
            );
            return ExitCode::from(1);
        }
        Ok(_) => {}
    }

    let img = match codec.decode_bytes(&in_bytes_buf) {
        Ok(img) => img,
        Err(e) => {
            let raw = codec_error_inner_message(&e);
            let ctx = ReportContext {
                cli,
                input_format,
                output_format,
                params: &params,
                duration_ms: started.elapsed().as_millis() as u64,
            };
            emit_single_file_failure_report(
                &ctx,
                in_bytes_buf.len() as u64,
                None,
                0,
                None,
                crate::report::ErrorKind::Decode,
                raw,
            );
            return ExitCode::from(1);
        }
    };

    // Reject oversized decoded dimensions before any allocation
    // that depends on width * height. The codecs'
    // `checked_pixel_capacity` helper enforces the same bound at
    // the allocation site (see src/format.rs); this early check
    // produces a single, well-shaped report rather than a chain of
    // partial failures from inside the codec.
    if let Err(msg) = crate::format::check_dimensions(img.width(), img.height()) {
        let ctx = ReportContext {
            cli,
            input_format,
            output_format,
            params: &params,
            duration_ms: started.elapsed().as_millis() as u64,
        };
        emit_single_file_failure_report(
            &ctx,
            in_bytes_buf.len() as u64,
            Some((img.width(), img.height())),
            0,
            None,
            crate::report::ErrorKind::Decode,
            &msg,
        );
        return ExitCode::from(1);
    }

    let resized = crate::format::apply_resize(&img, params.resize);

    let encoded = match codec.encode_to_vec(&resized, params.quality) {
        Ok(b) => b,
        Err(e) => {
            let raw = codec_error_inner_message(&e);
            let ctx = ReportContext {
                cli,
                input_format,
                output_format,
                params: &params,
                duration_ms: started.elapsed().as_millis() as u64,
            };
            emit_single_file_failure_report(
                &ctx,
                in_bytes_buf.len() as u64,
                Some((img.width(), img.height())),
                0,
                None,
                crate::report::ErrorKind::Encode,
                raw,
            );
            return ExitCode::from(1);
        }
    };

    let stdout = std::io::stdout();
    let mut out = stdout.lock();
    if let Err(e) = out.write_all(&encoded) {
        let ctx = ReportContext {
            cli,
            input_format,
            output_format,
            params: &params,
            duration_ms: started.elapsed().as_millis() as u64,
        };
        emit_single_file_failure_report(
            &ctx,
            in_bytes_buf.len() as u64,
            Some((img.width(), img.height())),
            0,
            None,
            crate::report::ErrorKind::Io,
            &format!("cannot write stdout: {e}"),
        );
        return ExitCode::from(1);
    }
    if let Err(e) = out.flush() {
        let ctx = ReportContext {
            cli,
            input_format,
            output_format,
            params: &params,
            duration_ms: started.elapsed().as_millis() as u64,
        };
        emit_single_file_failure_report(
            &ctx,
            in_bytes_buf.len() as u64,
            Some((img.width(), img.height())),
            encoded.len() as u64,
            Some((resized.width(), resized.height())),
            crate::report::ErrorKind::Io,
            &format!("cannot flush stdout: {e}"),
        );
        return ExitCode::from(1);
    }

    let ctx = ReportContext {
        cli,
        input_format,
        output_format,
        params: &params,
        duration_ms: started.elapsed().as_millis() as u64,
    };
    emit_single_file_success_report(
        &ctx,
        in_bytes_buf.len() as u64,
        (img.width(), img.height()),
        encoded.len() as u64,
        (resized.width(), resized.height()),
    );
    ExitCode::from(0)
}

/// Emit the single-line metadata record on stderr in single-file
/// mode:
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
/// `--json` is set, emits a structured NDJSON record (one JSON
/// object per line). Otherwise falls back to the v0 key=value
/// shape emitted by `emit_single_file_metadata`.
///
/// Failure paths in single-file mode still use
/// `emit_single_file_metadata` directly; they are routed through
/// the JSON-capable dispatcher in `emit_single_file_failure_report`
/// below.
fn emit_single_file_success_report(
    ctx: &ReportContext,
    in_bytes: u64,
    input_dims: (u32, u32),
    out_bytes: u64,
    output_dims: (u32, u32),
) {
    if !ctx.cli.json {
        if ctx.cli.report_fd == 2 {
            emit_single_file_metadata("ok", in_bytes, out_bytes, ctx.duration_ms, None);
        } else {
            let line = format!(
                "status=ok in_bytes={in_bytes} out_bytes={out_bytes} duration_ms={}",
                ctx.duration_ms
            );
            emit_report_line(ctx.cli.report_fd, &line);
        }
        return;
    }
    use crate::report::{CodecMeta, ImageFormat, ImageInfo, Mode, Report, Status};
    let report = Report {
        mode: Mode::SingleFile,
        status: Status::Ok,
        input: Some(ImageInfo {
            format: ImageFormat::from(ctx.input_format),
            bytes: in_bytes,
            width: Some(input_dims.0),
            height: Some(input_dims.1),
        }),
        output: Some(ImageInfo {
            format: ImageFormat::from(ctx.output_format),
            bytes: out_bytes,
            width: Some(output_dims.0),
            height: Some(output_dims.1),
        }),
        codec: CodecMeta {
            quality: ctx.params.quality,
            resize_policy: resize_policy_to_string(ctx.params.resize),
        },
        host: host_meta(),
        duration_ms: ctx.duration_ms,
        error: None,
    };
    emit_report_line(ctx.cli.report_fd, &report.to_json());
}

/// Emit a failure-path single-file report on stderr. When `--json`
/// is set, emits a structured NDJSON record with `status: "err"`
/// and the documented `error` block. Otherwise falls back to the
/// v0 key=value shape with the kind prefix (`decode error:`,
/// `encode error:`, or `io error:`).
///
/// `input_dims` / `output_dims` are `None` when the corresponding
/// data is not known (decode failure → no output dims; encode
/// failure → output dims are zero; pre-decode failure → both
/// `None`). `in_bytes` / `out_bytes` of 0 produce `null` blocks
/// in the JSON record so output fields are present but zeroed
/// where not meaningful.
fn emit_single_file_failure_report(
    ctx: &ReportContext,
    in_bytes: u64,
    input_dims: Option<(u32, u32)>,
    out_bytes: u64,
    output_dims: Option<(u32, u32)>,
    error_kind: crate::report::ErrorKind,
    error_msg: &str,
) {
    if !ctx.cli.json {
        let kind_str = match error_kind {
            crate::report::ErrorKind::Decode => "decode error",
            crate::report::ErrorKind::Encode => "encode error",
            crate::report::ErrorKind::Io => "io error",
        };
        let full_msg = format!("{kind_str}: {error_msg}");
        if ctx.cli.report_fd == 2 {
            emit_single_file_metadata("err", in_bytes, out_bytes, ctx.duration_ms, Some(&full_msg));
        } else {
            let line = format!(
                "status=err in_bytes={in_bytes} out_bytes={out_bytes} \
                 duration_ms={} error={full_msg}",
                ctx.duration_ms
            );
            emit_report_line(ctx.cli.report_fd, &line);
        }
        return;
    }
    use crate::report::{CodecMeta, ImageFormat, ImageInfo, Mode, Report, ReportError, Status};
    let input = if in_bytes > 0 {
        Some(ImageInfo {
            format: ImageFormat::from(ctx.input_format),
            bytes: in_bytes,
            width: input_dims.map(|d| d.0),
            height: input_dims.map(|d| d.1),
        })
    } else {
        None
    };
    let output = if out_bytes > 0 {
        Some(ImageInfo {
            format: ImageFormat::from(ctx.output_format),
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
            quality: ctx.params.quality,
            resize_policy: resize_policy_to_string(ctx.params.resize),
        },
        host: host_meta(),
        duration_ms: ctx.duration_ms,
        error: Some(ReportError {
            kind: error_kind,
            message: error_msg.to_string(),
        }),
    };
    emit_report_line(ctx.cli.report_fd, &report.to_json());
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

/// Validate the `--report-fd` argument. Returns `Ok(())` if `fd`
/// is an acceptable report stream, `Err(msg)` otherwise. The
/// accepted set is:
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
        return Err(format!(
            "--report-fd must be a non-negative integer; got {fd}"
        ));
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
    let accmode = flags & libc::O_ACCMODE;
    if accmode != libc::O_WRONLY && accmode != libc::O_RDWR {
        return Err(format!("--report-fd {fd}: fd is not open for writing"));
    }
    Ok(())
}

/// Low-level writer abstraction used by `retry_write_all`. The
/// real implementation wraps `libc::write(2)`; tests inject a
/// `MockReportWriter` that simulates EINTR / EAGAIN / partial-write
/// behaviour without timing races.
///
/// SAFETY CONTRACT: the implementer MUST treat `buf` as
/// read-only memory that the caller continues to own. The
/// `libc::write(2)` wrapper in `LibcFdWriter` does not mutate
/// `buf`; the mock implementations in tests must not either.
trait ReportFdWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize>;
}

/// Real `libc::write(2)` wrapper. Holds the destination file
/// descriptor for the duration of the retry loop.
struct LibcFdWriter(i32);

impl ReportFdWriter for LibcFdWriter {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        // SAFETY: `self.0` is a file descriptor that was validated
        // for write access by `validate_report_fd`; `buf.as_ptr()`
        // points at `buf.len()` readable bytes for the duration of
        // the call (the borrow lives for the expression).
        // `libc::write(2)` does not mutate the buffer and the
        // kernel never retains the pointer past return, so the
        // temporary pointer cast is sound.
        let n = unsafe { libc::write(self.0, buf.as_ptr() as *const _, buf.len()) };
        if n < 0 {
            Err(std::io::Error::last_os_error())
        } else {
            Ok(n as usize)
        }
    }
}

/// Upper bound on EINTR retries. Linux historically returns EINTR
/// very rarely (only when the process is being shut down by a
/// signal), but POSIX permits any signal to interrupt a syscall.
/// 64 retries is well above the practical signal rate and bounds
/// the worst-case CPU cost (each retry is a single syscall).
const MAX_EINTR_RETRIES: u32 = 64;
/// Upper bound on EAGAIN / EWOULDBLOCK retries when the operator
/// has set `O_NONBLOCK` on the report fd. The backoff doubles each
/// attempt up to 1 ms, then caps; 16 attempts is enough to survive
/// a brief buffer-full window but caps the wait at ~16 ms.
const MAX_EAGAIN_RETRIES: u32 = 16;

/// Write `buf` to `writer` in full, retrying on partial writes,
/// EINTR (signal interruption), and EAGAIN / EWOULDBLOCK (the
/// fd was marked `O_NONBLOCK` and the kernel buffer was full).
/// On persistent EAGAIN the loop backs off with exponential delay
/// up to 1 ms per attempt. The retry counts are bounded by
/// `MAX_EINTR_RETRIES` and `MAX_EAGAIN_RETRIES` so the call cannot
/// spin indefinitely.
///
/// Returns `Ok(())` only when every byte of `buf` has been written.
/// Other `io::Error`s are returned to the caller without retry;
/// the report stream is best-effort and a write failure must not
/// crash the binary.
fn retry_write_all<W: ReportFdWriter>(writer: &mut W, buf: &[u8]) -> std::io::Result<()> {
    let mut eintr_retries: u32 = 0;
    let mut eagain_retries: u32 = 0;
    let mut written = 0;
    while written < buf.len() {
        match writer.write(&buf[written..]) {
            Ok(0) => {
                // Treat a 0-byte result the same as EAGAIN: the
                // kernel made no progress this round. Bounded by
                // `MAX_EAGAIN_RETRIES` so a permanently stuck fd
                // surfaces an error rather than spinning forever.
                eagain_retries += 1;
                if eagain_retries > MAX_EAGAIN_RETRIES {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::WriteZero,
                        "report fd write returned 0 repeatedly",
                    ));
                }
                let shift = eagain_retries.saturating_sub(1).min(5);
                let micros = 50u64.saturating_mul(1u64 << shift).min(1000);
                std::thread::sleep(std::time::Duration::from_micros(micros));
            }
            Ok(n) => {
                written += n;
                eintr_retries = 0;
                eagain_retries = 0;
            }
            Err(e)
                if e.raw_os_error() == Some(libc::EINTR)
                    || e.raw_os_error() == Some(libc::EAGAIN)
                    || e.raw_os_error() == Some(libc::EWOULDBLOCK) =>
            {
                let is_eintr = e.raw_os_error() == Some(libc::EINTR);
                if is_eintr {
                    eintr_retries += 1;
                    if eintr_retries > MAX_EINTR_RETRIES {
                        return Err(e);
                    }
                } else {
                    eagain_retries += 1;
                    if eagain_retries > MAX_EAGAIN_RETRIES {
                        return Err(e);
                    }
                    let shift = eagain_retries.saturating_sub(1).min(5);
                    let micros = 50u64.saturating_mul(1u64 << shift).min(1000);
                    std::thread::sleep(std::time::Duration::from_micros(micros));
                }
            }
            Err(e) => return Err(e),
        }
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
///
/// Writes to non-stderr fds go through `retry_write_all` (EINTR
/// retried, partial writes retried, bounded EAGAIN retries; no
/// unbounded spin).
fn emit_report_line(fd: i32, line: &str) {
    if fd == 2 {
        eprintln!("{}", line);
        return;
    }
    let _guard = report_fd_lock().lock().unwrap_or_else(|e| e.into_inner());
    let mut writer = LibcFdWriter(fd);
    let payload = line.as_bytes();
    if retry_write_all(&mut writer, payload).is_err() {
        return;
    }
    let newline: [u8; 1] = *b"\n";
    let _ = retry_write_all(&mut writer, &newline);
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
/// Called from inside the rayon parallel iterator; the line is
/// emitted as soon as the candidate finishes, in completion order
/// (no enclosing array).
fn emit_batch_record_success(
    ctx: &ReportContext,
    src: &Path,
    report: &crate::format::ConversionReport,
) {
    use crate::report::{CodecMeta, ImageFormat, ImageInfo, Mode, Report, Status};
    let r = Report {
        mode: Mode::Batch,
        status: Status::Ok,
        input: Some(ImageInfo {
            format: ImageFormat::from(ctx.input_format),
            bytes: report.in_bytes,
            width: Some(report.input_width),
            height: Some(report.input_height),
        }),
        output: Some(ImageInfo {
            format: ImageFormat::from(ctx.output_format),
            bytes: report.out_bytes,
            width: Some(report.output_width),
            height: Some(report.output_height),
        }),
        codec: CodecMeta {
            quality: ctx.params.quality,
            resize_policy: resize_policy_to_string(ctx.params.resize),
        },
        host: host_meta(),
        duration_ms: ctx.duration_ms,
        error: None,
    };
    // `src` is intentionally not embedded in the per-file record;
    // the caller's NDJSON line index identifies the file in
    // completion order.
    let _ = src;
    emit_report_line(ctx.cli.report_fd, &r.to_json());
}

/// Emit one NDJSON line for a batch-mode codec failure (decode /
/// encode / write). `in_bytes` is the source byte count when
/// known (0 when decode never ran); output dims are absent.
fn emit_batch_record_failure(
    ctx: &ReportContext,
    src: &Path,
    in_bytes: u64,
    error_kind: crate::report::ErrorKind,
    error_msg: &str,
) {
    use crate::report::{CodecMeta, ImageFormat, ImageInfo, Mode, Report, ReportError, Status};
    let input = if in_bytes > 0 {
        Some(ImageInfo {
            format: ImageFormat::from(ctx.input_format),
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
            quality: ctx.params.quality,
            resize_policy: resize_policy_to_string(ctx.params.resize),
        },
        host: host_meta(),
        duration_ms: ctx.duration_ms,
        error: Some(ReportError {
            kind: error_kind,
            message: error_msg.to_string(),
        }),
    };
    let _ = src;
    emit_report_line(ctx.cli.report_fd, &r.to_json());
}

/// Emit one NDJSON line for a post-conversion source-delete
/// failure (decode / encode / write succeeded but `fs::remove_file`
/// failed). The struct already carries input / output bytes and
/// dimensions because the codec returned Ok; the error.kind is
/// Io and the message names the delete step.
fn emit_batch_record_io_failure(
    ctx: &ReportContext,
    src: &Path,
    conv: &crate::format::ConversionReport,
    error_msg: &str,
) {
    use crate::report::{CodecMeta, ImageFormat, ImageInfo, Mode, Report, ReportError, Status};
    let r = Report {
        mode: Mode::Batch,
        status: Status::Err,
        input: Some(ImageInfo {
            format: ImageFormat::from(ctx.input_format),
            bytes: conv.in_bytes,
            width: Some(conv.input_width),
            height: Some(conv.input_height),
        }),
        output: Some(ImageInfo {
            format: ImageFormat::from(ctx.output_format),
            bytes: conv.out_bytes,
            width: Some(conv.output_width),
            height: Some(conv.output_height),
        }),
        codec: CodecMeta {
            quality: ctx.params.quality,
            resize_policy: resize_policy_to_string(ctx.params.resize),
        },
        host: host_meta(),
        duration_ms: ctx.duration_ms,
        error: Some(ReportError {
            kind: crate::report::ErrorKind::Io,
            message: error_msg.to_string(),
        }),
    };
    let _ = src;
    emit_report_line(ctx.cli.report_fd, &r.to_json());
}

fn print_usage() {
    // The canonical usage banner names `fast-image-converter`. The
    // legacy names appear only in the aliases list (so operators
    // discovering the binary via `convert-to-webp --help` or
    // `gallery-compress --help` still learn about their deprecation;
    // the aliases invoke the canonical binary after printing their
    // own deprecation hint).
    eprintln!(
        "Usage: fast-image-converter <dir> [--input-format <fmt>] [--output-format <fmt>]\n\
         \x20                                [--quality <1..100>] [--resize <policy>] [--keep-source]\n\
         \x20                                [--json] [--report-fd <N>]\n\
         \x20          fast-image-converter --single-file [--input-format <fmt>] [--output-format <fmt>]\n\
         \x20                                [--quality <1..100>] [--resize <policy>]\n\
         \x20                                [--json] [--report-fd <N>]\n\
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
         Compatibility aliases (deprecated; forward to this binary):\n\
         \x20 convert-to-webp         forwards to fast-image-converter; emits a one-line deprecation hint on stderr\n\
         \x20 gallery-compress        forwards to fast-image-converter; emits a one-line deprecation hint on stderr\n\
         \n\
         Examples:\n\
         \x20 fast-image-converter /tmp/my-images\n\
         \x20 fast-image-converter /tmp/my-images --input-format png --output-format webp\n\
         \x20 fast-image-converter /tmp/my-images --input-format webp --output-format png\n\
         \x20 fast-image-converter /tmp/my-images --input-format webp --output-format jpg\n\
         \x20 fast-image-converter /tmp/my-images --quality 75 --resize cap=1024 --keep-source\n\
         \x20 cat input.jpg | fast-image-converter --single-file --output-format webp > output.webp\n\
         \x20 cat input.jpg | fast-image-converter --single-file --output-format webp --json > out.webp 2> report.jsonl\n\
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
        // CLI parsing uses the canonical argv[0] name; alias
        // coverage lives in `tests/alias_forwarding.rs`.
        let cli = parse_cli(&v(&["fast-image-converter", "/tmp/x"])).unwrap();
        assert_eq!(
            cli.mode,
            Mode::Batch {
                dir: PathBuf::from("/tmp/x")
            }
        );
        assert_eq!(cli.input_format, None);
        assert_eq!(cli.output_format, None);
    }

    #[test]
    fn parses_explicit_flags() {
        let cli = parse_cli(&v(&[
            "fast-image-converter",
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
            "fast-image-converter",
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
            "fast-image-converter",
            "/tmp/x",
            "--output-format",
            "gif",
        ]))
        .unwrap_err();
        assert!(matches!(err, CliError::BadOutputFormat(s) if s == "gif"));
    }

    #[test]
    fn rejects_unknown_flag() {
        let err = parse_cli(&v(&["fast-image-converter", "--foo"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn rejects_help() {
        let err = parse_cli(&v(&["fast-image-converter", "--help"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn rejects_missing_positional() {
        let err = parse_cli(&v(&["fast-image-converter"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn rejects_extra_positional() {
        let err = parse_cli(&v(&["fast-image-converter", "/tmp/a", "/tmp/b"])).unwrap_err();
        assert_eq!(err, CliError::Usage);
    }

    #[test]
    fn parses_single_file_mode() {
        let cli = parse_cli(&v(&["fast-image-converter", "--single-file"])).unwrap();
        assert_eq!(cli.mode, Mode::SingleFile);
    }

    #[test]
    fn rejects_single_file_with_positional() {
        let err = parse_cli(&v(&["fast-image-converter", "--single-file", "/tmp/x"])).unwrap_err();
        assert_eq!(err, CliError::AmbiguousMode);
    }

    #[test]
    fn parse_cli_ignores_argv_zero_for_aliases() {
        // parse_cli must accept any argv[0] (the alias forwarders
        // spawn this binary with their own argv[0]). Behaviour must
        // be identical across the canonical name and both legacy
        // names.
        let canonical = parse_cli(&v(&["fast-image-converter", "/tmp/x"])).unwrap();
        let ctw = parse_cli(&v(&["convert-to-webp", "/tmp/x"])).unwrap();
        let gc = parse_cli(&v(&["gallery-compress", "/tmp/x"])).unwrap();
        assert_eq!(canonical.mode, ctw.mode);
        assert_eq!(canonical.mode, gc.mode);
        assert_eq!(canonical.input_format, ctw.input_format);
        assert_eq!(canonical.input_format, gc.input_format);
        assert_eq!(canonical.output_format, ctw.output_format);
        assert_eq!(canonical.output_format, gc.output_format);
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

// Tests exercise partial / interrupted / non-blocking write behaviour
// through the `ReportFdWriter` abstraction, without relying on
// kernel-level timing races. The mock is a deterministic state
// machine: each call to `write()` returns the next scripted outcome,
// then advances. EINTR and EAGAIN are explicit so the retry
// counts are observable.
#[cfg(test)]
mod report_fd_writer_tests {
    use super::*;
    use std::io::ErrorKind;
    use std::os::fd::IntoRawFd;

    /// Scripted mock that returns a pre-recorded list of
    /// `io::Result<usize>` outcomes. After the list is exhausted,
    /// subsequent calls panic so the test fails loudly rather than
    /// silently diverging from the script.
    struct ScriptedWriter {
        script: Vec<std::io::Result<usize>>,
        collected: Vec<u8>,
    }

    impl ScriptedWriter {
        #[allow(dead_code)]
        fn ok(n: usize) -> std::io::Result<usize> {
            Ok(n)
        }
        fn err_os(_kind: ErrorKind, errno: i32) -> std::io::Result<usize> {
            Err(std::io::Error::from_raw_os_error(errno))
        }
    }

    impl ReportFdWriter for ScriptedWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let outcome = self.script.remove(0);
            let n = outcome?;
            self.collected.extend_from_slice(&buf[..n]);
            Ok(n)
        }
    }

    fn collect(buf: &[u8]) -> Vec<u8> {
        buf.to_vec()
    }

    #[test]
    fn retry_write_all_writes_full_buffer_on_first_try() {
        let mut w = ScriptedWriter {
            script: vec![Ok(5), Ok(3)],
            collected: Vec::new(),
        };
        retry_write_all(&mut w, b"abcdefgh").unwrap();
        assert_eq!(w.collected, b"abcdefgh");
        // First call wrote 5 bytes; second call wrote the remaining 3.
        assert_eq!(w.script.len(), 0, "unexpected extra calls");
    }

    #[test]
    fn retry_write_all_retries_eintr_then_succeeds() {
        let payload = b"hello";
        // First call returns EINTR (no bytes written); second
        // call writes the full payload.
        let mut w = ScriptedWriter {
            script: vec![
                ScriptedWriter::err_os(ErrorKind::Interrupted, libc::EINTR),
                Ok(payload.len()),
            ],
            collected: Vec::new(),
        };
        retry_write_all(&mut w, payload).unwrap();
        assert_eq!(w.collected, payload);
    }

    #[test]
    fn retry_write_all_retries_eagain_then_succeeds() {
        let payload = b"world";
        let mut w = ScriptedWriter {
            script: vec![
                ScriptedWriter::err_os(ErrorKind::WouldBlock, libc::EAGAIN),
                ScriptedWriter::err_os(ErrorKind::WouldBlock, libc::EWOULDBLOCK),
                Ok(payload.len()),
            ],
            collected: Vec::new(),
        };
        retry_write_all(&mut w, payload).unwrap();
        assert_eq!(w.collected, payload);
    }

    #[test]
    fn retry_write_all_retries_partial_writes_until_full() {
        // 1-byte payload is split across three partial writes; the
        // third call writes the last byte. The retry loop must
        // accumulate the total without truncating the NDJSON record.
        let mut w = ScriptedWriter {
            script: vec![Ok(0), Ok(0), Ok(1)],
            collected: Vec::new(),
        };
        retry_write_all(&mut w, b"x").unwrap();
        assert_eq!(w.collected, b"x");
    }

    #[test]
    fn retry_write_all_bails_when_eagain_exceeds_budget() {
        // MAX_EAGAIN_RETRIES + 1 transient EAGAINs; the loop must
        // surface the final error and not spin indefinitely.
        let mut script = Vec::new();
        for _ in 0..=(MAX_EAGAIN_RETRIES) {
            script.push(ScriptedWriter::err_os(ErrorKind::WouldBlock, libc::EAGAIN));
        }
        let mut w = ScriptedWriter {
            script,
            collected: Vec::new(),
        };
        let result = retry_write_all(&mut w, b"never");
        assert!(result.is_err());
        assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::EAGAIN));
    }

    #[test]
    fn retry_write_all_surfaces_non_transient_errors_without_retry() {
        // EPIPE is not retryable; the function must surface it
        // immediately rather than retrying.
        let mut w = ScriptedWriter {
            script: vec![ScriptedWriter::err_os(ErrorKind::BrokenPipe, libc::EPIPE)],
            collected: Vec::new(),
        };
        let result = retry_write_all(&mut w, b"x");
        assert_eq!(result.unwrap_err().raw_os_error(), Some(libc::EPIPE));
    }

    #[test]
    fn retry_write_all_treats_repeated_write_zero_as_unrecoverable() {
        // `write(2)` returning 0 is rare in practice (a non-blocking
        // fd with an empty kernel buffer returns EAGAIN, not 0);
        // however a misbehaving fd could return 0 forever. After
        // `MAX_EAGAIN_RETRIES + 1` consecutive zeros the loop must
        // surface a `WriteZero` error rather than spin.
        let mut script = Vec::new();
        for _ in 0..=(MAX_EAGAIN_RETRIES) {
            script.push(Ok(0));
        }
        let mut w = ScriptedWriter {
            script,
            collected: Vec::new(),
        };
        let result = retry_write_all(&mut w, b"x");
        assert_eq!(result.unwrap_err().kind(), ErrorKind::WriteZero);
    }

    // The LibcFdWriter wrapper is exercised by the integration tests
    // that pipe NDJSON to a real fd via the --report-fd flag; the
    // unit tests above cover the retry semantics without any
    // dependency on the kernel scheduler.
    #[test]
    fn libc_fd_writer_smoke() {
        // Write to /dev/null via the wrapper; success means the
        // retry path returned Ok(()).
        let devnull = std::fs::OpenOptions::new()
            .write(true)
            .open("/dev/null")
            .expect("open /dev/null");
        let fd = devnull.into_raw_fd();
        let mut w = LibcFdWriter(fd);
        retry_write_all(&mut w, b"this is silently discarded").unwrap();
        let _ = unsafe { libc::close(fd) };
    }

    // Helper to silence unused-import warnings when this module's
    // `fn collect` helper is not used by every test.
    #[allow(dead_code)]
    fn unused() {
        let _ = collect(&[]);
    }
}

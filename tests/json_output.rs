//! Integration tests for the `--json` structured output mode (DE-005).
//!
//! Per `docs/contracts/report-shape.md` (and DE-005 § 2) the binary
//! emits one NDJSON record per converted file when `--json` is set:
//! one line total in `--single-file` mode, one line per candidate in
//! batch mode. The shape is hand-rolled (no `serde` runtime dep) and
//! versioned via `schema_version` (= 1).
//!
//! These tests exercise the binary as a black box: spawn it, capture
//! the report stream, parse with `serde_json` (dev-dependency;
//! release binary is unaffected), and assert every field documented
//! in the contract is present and correctly typed on the success and
//! failure paths.

use std::fs;
use std::io::Write;
use std::os::unix::io::{AsRawFd, FromRawFd};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value;

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_convert-to-webp")
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden_v0")
}

/// Spawn the binary with `args`, feed `stdin` to it, and return the
/// raw `(status, stdout, stderr)` triple. Both stdout and stderr
/// are captured in full.
fn spawn(args: &[&str], stdin: Option<&[u8]>) -> (std::process::ExitStatus, Vec<u8>, Vec<u8>) {
    let mut child = Command::new(binary())
        .args(args)
        .stdin(match stdin {
            Some(_) => Stdio::piped(),
            None => Stdio::null(),
        })
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn binary");
    if let Some(payload) = stdin {
        // Ignore BrokenPipe: when the binary rejects an argument
        // (exit 2 from CLI parsing or --report-fd validation), it
        // closes stdin before the test can finish writing the
        // input bytes. The close is the expected behaviour; the
        // BrokenPipe write is a test-harness artefact.
        let _ = child.stdin.as_mut().expect("stdin").write_all(payload);
    }
    let out = child.wait_with_output().expect("wait binary");
    (out.status, out.stdout, out.stderr)
}

/// Spawn the binary with `--report-fd <fd>`, redirecting the
/// chosen `fd` to a pipe whose read end is returned to the caller.
/// The pipe carries the JSON line(s) produced by the binary; other
/// fds keep their default destinations.
fn spawn_with_report_fd(
    args: &[&str],
    stdin: Option<&[u8]>,
    report_fd: i32,
) -> (std::process::ExitStatus, Vec<u8>, Vec<u8>, Option<String>) {
    let (read_fd, write_fd) = nix_pipe().expect("create pipe");
    let mut cmd = Command::new(binary());
    cmd.args(args);
    if stdin.is_some() {
        cmd.stdin(Stdio::piped());
    } else {
        cmd.stdin(Stdio::null());
    }
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
    // SAFETY: `OwnedFd::from_raw_fd` takes ownership; closing the
    // pipe write end in the parent is the caller's responsibility
    // (we dup `write_fd` into the child's fd table via pre_exec).
    let write_fd_owned = unsafe { std::os::unix::io::OwnedFd::from_raw_fd(write_fd) };
    unsafe {
        use std::os::unix::process::CommandExt;
        let dup_target = report_fd;
        let borrowed = write_fd_owned.as_raw_fd();
        cmd.pre_exec(move || {
            // dup2 the write-end into the report fd slot so the
            // child's writes hit the pipe; then close the original
            // write fd in the child so the read end sees EOF.
            if libc::dup2(borrowed, dup_target) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::close(borrowed) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().expect("spawn with report fd");
    drop(write_fd_owned);

    if let Some(payload) = stdin {
        child
            .stdin
            .as_mut()
            .expect("stdin")
            .write_all(payload)
            .expect("write stdin");
    }
    let out = child.wait_with_output().expect("wait binary");
    let report = read_pipe(read_fd);
    (out.status, out.stdout, out.stderr, report)
}

/// Create a POSIX pipe and return `(read_fd, write_fd)`. Both fds
/// are bare `i32` so the caller can install them via `dup2`.
fn nix_pipe() -> std::io::Result<(i32, i32)> {
    let mut fds = [0i32; 2];
    // SAFETY: libc::pipe writes two fd numbers into the array.
    let rc = unsafe { libc::pipe(fds.as_mut_ptr()) };
    if rc != 0 {
        return Err(std::io::Error::last_os_error());
    }
    Ok((fds[0], fds[1]))
}

/// Drain a pipe read-end to a `String` and close it. The pipe
/// reaches EOF once the writer (the child) exits.
fn read_pipe(fd: i32) -> Option<String> {
    use std::io::Read;
    let mut file = unsafe { std::fs::File::from_raw_fd(fd) };
    let mut buf = String::new();
    match file.read_to_string(&mut buf) {
        Ok(_) => Some(buf),
        Err(_) => None,
    }
}

/// Parse one NDJSON line. Returns `None` if the line is empty (so
/// callers can skip the v0 trailer `(processed N candidates, K
/// failed)` that lands on the report stream in batch mode).
fn parse_record(line: &str) -> Option<Value> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('(') {
        return None;
    }
    serde_json::from_str(trimmed).ok()
}

/// Assert the value of a top-level string field.
fn assert_string_field(v: &Value, key: &str, expected: &str) {
    let actual = v
        .get(key)
        .unwrap_or_else(|| panic!("missing field {key:?} in record: {v}"))
        .as_str()
        .unwrap_or_else(|| panic!("field {key:?} is not a string in: {v}"));
    assert_eq!(
        actual, expected,
        "field {key:?} mismatch (got {actual:?}, expected {expected:?})"
    );
}

/// Assert the value of a top-level integer field.
fn assert_int_field(v: &Value, key: &str, expected_min: i64) {
    let actual = v
        .get(key)
        .unwrap_or_else(|| panic!("missing field {key:?} in record: {v}"))
        .as_i64()
        .unwrap_or_else(|| panic!("field {key:?} is not an integer in: {v}"));
    assert!(
        actual >= expected_min,
        "field {key:?} below expected_min ({actual} < {expected_min}) in: {v}"
    );
}

fn fixture_jpeg() -> Vec<u8> {
    fs::read(fixtures().join("portrait_256x384.jpg")).expect("golden jpeg fixture")
}

// --- AC-1 / AC-2 / AC-3 / AC-6 / AC-9: single-file success path ---

#[test]
fn single_file_json_emits_one_parseable_line_on_stderr() {
    // AC-1: --json on --single-file emits exactly one JSON line on
    // the report stream (stderr by default).
    // AC-2: the line parses with any standard JSON parser.
    // AC-9: stdout still contains only the encoded bytes.
    let input = fixture_jpeg();
    let (status, stdout, stderr) = spawn(
        &["--single-file", "--output-format", "webp", "--json"],
        Some(&input),
    );
    assert!(status.success(), "exit {status:?}; stderr: {stderr:?}");

    // stderr is one JSON line + a trailing newline.
    let stderr_s = String::from_utf8_lossy(&stderr);
    let lines: Vec<&str> = stderr_s.lines().collect();
    assert_eq!(
        lines.len(),
        1,
        "stderr must be one JSON line; got: {lines:?}"
    );

    let v = parse_record(lines[0]).expect("stderr line parses as JSON");
    // AC-6: schema_version is 1.
    assert_eq!(
        v.get("schema_version").and_then(|x| x.as_u64()),
        Some(1),
        "schema_version must be 1: {v}"
    );
    // schema_version must be the first field in the raw JSON
    // string. (serde_json's `Value::Object` uses a sorted map
    // when parsed, so we check the raw string instead.)
    assert!(
        lines[0].trim_start().starts_with("{\"schema_version\":1,"),
        "schema_version must be the first field in the raw JSON; \
         got prefix: {:?}",
        &lines[0].trim_start()[..lines[0].trim_start().len().min(40)]
    );

    // stdout starts with the WebP RIFF header (encoded bytes).
    assert_eq!(&stdout[0..4], b"RIFF");
    assert_eq!(&stdout[8..12], b"WEBP");
}

#[test]
fn single_file_json_success_emits_all_documented_fields() {
    // AC-3: every documented field is present and correctly typed
    // on the success path; `error` is null.
    let input = fixture_jpeg();
    let (status, _stdout, stderr) = spawn(
        &["--single-file", "--output-format", "webp", "--json"],
        Some(&input),
    );
    assert!(status.success());
    let v =
        parse_record(&String::from_utf8_lossy(&stderr)).expect("one parseable JSON line on stderr");

    // Top-level scalars / strings.
    assert_string_field(&v, "mode", "single_file");
    assert_string_field(&v, "status", "ok");
    assert!(
        v.get("error").unwrap().is_null(),
        "error must be null on success"
    );

    // input.{format,bytes,width,height}.
    let input_obj = v.get("input").expect("input");
    assert_string_field(input_obj, "format", "jpeg");
    let in_bytes = input_obj.get("bytes").and_then(|x| x.as_u64()).unwrap();
    assert_eq!(in_bytes, input.len() as u64);
    assert_int_field(input_obj, "width", 1);
    assert_int_field(input_obj, "height", 1);

    // output.{format,bytes,width,height}.
    let output_obj = v.get("output").expect("output");
    assert_string_field(output_obj, "format", "webp");
    let out_bytes = output_obj.get("bytes").and_then(|x| x.as_u64()).unwrap();
    assert!(out_bytes > 0);
    assert_int_field(output_obj, "width", 1);
    assert_int_field(output_obj, "height", 1);

    // codec.{quality,resize_policy}.
    let codec = v.get("codec").expect("codec");
    assert!(codec.get("quality").and_then(|x| x.as_u64()).is_some());
    let policy = codec.get("resize_policy").and_then(|x| x.as_str()).unwrap();
    assert!(
        policy == "none" || policy.starts_with("cap=") || policy.starts_with("auto:"),
        "resize_policy is not a known shape: {policy}"
    );

    // host.{libwebp_version,build_commit_sha}.
    let host = v.get("host").expect("host");
    let libwebp_version = host
        .get("libwebp_version")
        .and_then(|x| x.as_str())
        .unwrap();
    assert!(
        !libwebp_version.is_empty(),
        "libwebp_version is the empty string: {host}"
    );
    // build_commit_sha is string-or-null; both are acceptable.
    let sha = host.get("build_commit_sha").unwrap();
    assert!(
        sha.is_string() || sha.is_null(),
        "build_commit_sha must be string or null: {host}"
    );

    // duration_ms is a non-negative integer.
    assert_int_field(&v, "duration_ms", 0);
}

// --- AC-4: single-file failure path ---

#[test]
fn single_file_json_decode_failure_emits_status_err_and_error_block() {
    // AC-4: on failure, status is "err" and error carries the
    // documented shape; output is null when not meaningful.
    let bad = vec![0u8; 1024]; // not a valid image
    let (status, stdout, stderr) = spawn(
        &["--single-file", "--output-format", "webp", "--json"],
        Some(&bad),
    );
    assert_eq!(status.code(), Some(1), "decode failure must exit 1");
    assert!(stdout.is_empty(), "stdout must be empty on decode failure");
    let v =
        parse_record(&String::from_utf8_lossy(&stderr)).expect("one parseable JSON line on stderr");

    assert_string_field(&v, "mode", "single_file");
    assert_string_field(&v, "status", "err");

    let err = v
        .get("error")
        .expect("error block")
        .as_object()
        .expect("error object");
    let kind = err.get("kind").and_then(|x| x.as_str()).unwrap();
    assert!(
        kind == "decode" || kind == "encode" || kind == "io",
        "error.kind is not in the documented enum: {kind}"
    );
    let msg = err.get("message").and_then(|x| x.as_str()).unwrap();
    assert!(!msg.is_empty(), "error.message is empty");

    // output is present but zeroed (null) where not meaningful.
    assert!(
        v.get("output").unwrap().is_null(),
        "output must be null on failure"
    );
}

// --- AC-5: batch mode ---

fn make_batch_run_dir(label: &str) -> PathBuf {
    let mut p = std::env::temp_dir();
    let pid = std::process::id();
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    p.push(format!("convert-to-webp-json-batch-{label}-{pid}-{nonce}"));
    fs::create_dir_all(&p).expect("create batch run dir");
    p
}

fn seed_with_jpegs(src: &Path, dst: &Path) {
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let from = entry.path();
        if !from.is_file() {
            continue;
        }
        if from.extension().and_then(|x| x.to_str()) == Some("jpg") {
            fs::copy(&from, dst.join(entry.file_name())).unwrap();
        }
    }
}

#[test]
fn batch_json_emits_one_line_per_candidate_with_status_ok() {
    // AC-5: --json on batch mode produces one JSON line per
    // candidate file, in completion order (independent NDJSON).
    let run = make_batch_run_dir("ok");
    seed_with_jpegs(&fixtures(), &run);
    let n_jpegs = fs::read_dir(&run).unwrap().filter_map(|e| e.ok()).count();
    assert!(n_jpegs >= 5, "test needs ≥5 jpeg candidates; got {n_jpegs}");

    let (status, _stdout, stderr) = {
        let child = Command::new(binary())
            .arg(&run)
            .arg("--json")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("spawn batch");
        let out = child.wait_with_output().expect("wait batch");
        (out.status, out.stdout, out.stderr)
    };
    assert!(status.success(), "batch must exit 0; stderr: {stderr:?}");
    let _ = fs::remove_dir_all(&run);

    // Split NDJSON: every JSON-parseable line is a candidate
    // record. The v0 trailer `(processed N candidates, K failed)`
    // is non-JSON and skipped via `parse_record`.
    let stderr_s = String::from_utf8_lossy(&stderr);
    let mut ok_count = 0usize;
    for line in stderr_s.lines() {
        if let Some(v) = parse_record(line) {
            assert_string_field(&v, "mode", "batch");
            assert_string_field(&v, "status", "ok");
            ok_count += 1;
        }
    }
    assert_eq!(
        ok_count, n_jpegs,
        "expected one JSON record per candidate; got {ok_count} for {n_jpegs} jpegs"
    );
}

#[test]
fn batch_json_without_input_dir_exits_nonzero_with_error_block() {
    // Failure-path coverage for batch mode: a missing directory
    // is not a JSON-record candidate (the binary bails before
    // walking candidates). The error appears on stderr outside
    // the NDJSON stream.
    let bogus = std::env::temp_dir().join(format!(
        "convert-to-webp-no-such-dir-{}",
        std::process::id()
    ));
    let (status, _stdout, stderr) = spawn(&[bogus.to_str().unwrap(), "--json"], None);
    assert_eq!(
        status.code(),
        Some(1),
        "missing dir must exit 1; got {status:?}"
    );
    let stderr_s = String::from_utf8_lossy(&stderr);
    assert!(
        stderr_s.contains("not a directory"),
        "stderr must explain the missing dir: {stderr_s}"
    );
    // No NDJSON records are emitted on a pre-walk failure.
    for line in stderr_s.lines() {
        if let Some(v) = parse_record(line) {
            // If the parser does manage to find something JSON-
            // shaped, make sure it is a coherent per-file record
            // (not a stray error string).
            assert!(v.get("mode").is_some(), "stray JSON on stderr: {v}");
        }
    }
}

// --- AC-8: v0 behaviour preserved when --json is absent ---

#[test]
fn single_file_without_json_emits_v0_key_value_line() {
    // AC-8: without --json, the v0 / DE-004 behaviour is preserved
    // (key=value on stderr in single-file mode).
    let input = fixture_jpeg();
    let (status, _stdout, stderr) =
        spawn(&["--single-file", "--output-format", "webp"], Some(&input));
    assert!(status.success());
    let line = String::from_utf8_lossy(&stderr)
        .lines()
        .next()
        .expect("one stderr line")
        .to_string();
    assert!(line.starts_with("status="), "v0 shape: {line}");
    assert!(line.contains(" in_bytes="), "v0 shape: {line}");
    assert!(line.contains(" out_bytes="), "v0 shape: {line}");
    assert!(line.contains(" duration_ms="), "v0 shape: {line}");
    assert!(
        !line.contains("error="),
        "success path omits error=: {line}"
    );
}

#[test]
fn batch_without_json_preserves_v0_no_per_file_metadata_on_stderr() {
    // AC-8 / v0: without --json, batch mode emits no per-file
    // metadata on stderr (the v0 trailer `(processed N candidates,
    // K failed)` is preserved; the candidate lines themselves
    // are absent).
    let run = make_batch_run_dir("no-json");
    seed_with_jpegs(&fixtures(), &run);
    let child = Command::new(binary())
        .arg(&run)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn batch");
    let out = child.wait_with_output().expect("wait batch");
    let _ = fs::remove_dir_all(&run);

    let stderr_s = String::from_utf8_lossy(&out.stderr);
    // No JSON lines on stderr.
    for line in stderr_s.lines() {
        assert!(
            parse_record(line).is_none(),
            "v0 batch must not emit NDJSON records; got: {line}"
        );
    }
}

// --- AC-7: --report-fd override + rejection set ---

#[test]
fn report_fd_three_streams_to_a_writable_pipe() {
    // AC-7: --report-fd <N> overrides the default stderr report
    // stream; here we install fd 3 in the child, redirect the
    // report stream there, and assert the JSON line lands on the
    // pipe (and only the pipe) when --report-fd 3 is supplied.
    let input = fixture_jpeg();
    let (status, stdout, stderr, report) = spawn_with_report_fd(
        &[
            "--single-file",
            "--output-format",
            "webp",
            "--json",
            "--report-fd",
            "3",
        ],
        Some(&input),
        3,
    );
    assert!(status.success(), "exit {status:?}; stderr: {stderr:?}");
    let report = report.expect("pipe read returned some data");
    let v = parse_record(report.trim()).expect("pipe carries one parseable JSON line");
    assert_string_field(&v, "status", "ok");

    // stderr is empty (report was diverted).
    assert!(
        stderr.is_empty(),
        "stderr must be empty when --report-fd 3 is set; got {stderr:?}"
    );
    // stdout still carries the encoded bytes.
    assert_eq!(&stdout[0..4], b"RIFF");
}

#[test]
fn report_fd_one_is_forbidden_with_usage() {
    // AC-7: N=1 is forbidden regardless of access mode; the binary
    // emits a usage message on stderr and exits 2.
    let (status, _stdout, stderr) = spawn(
        &[
            "--single-file",
            "--output-format",
            "webp",
            "--json",
            "--report-fd",
            "1",
        ],
        Some(&fixture_jpeg()),
    );
    assert_eq!(status.code(), Some(2), "N=1 must exit 2; got {status:?}");
    let s = String::from_utf8_lossy(&stderr);
    assert!(
        s.contains("--report-fd 1 is forbidden"),
        "stderr must explain the rejection: {s}"
    );
    assert!(
        s.contains("Usage"),
        "stderr must include the usage block: {s}"
    );
}

#[test]
fn report_fd_not_a_writable_fd_is_rejected_with_usage() {
    // AC-7: non-writable fds are rejected with usage + exit 2.
    // fd 0 (stdin) is closed / not writable in the test harness.
    let (status, _stdout, stderr) = spawn(
        &[
            "--single-file",
            "--output-format",
            "webp",
            "--json",
            "--report-fd",
            "0",
        ],
        Some(&fixture_jpeg()),
    );
    // The spawn harness closes stdin via Stdio::null(), so fd 0
    // is either closed (fcntl returns EBADF) or not writable.
    assert_eq!(
        status.code(),
        Some(2),
        "non-writable fd must exit 2; got {status:?}"
    );
    let s = String::from_utf8_lossy(&stderr);
    assert!(
        s.contains("--report-fd"),
        "stderr must explain the rejection: {s}"
    );
}

#[test]
fn report_fd_garbage_value_is_rejected_at_parse_time() {
    // AC-7: unparseable values rejected at parse time, usage +
    // exit 2.
    let (status, _stdout, stderr) = spawn(
        &[
            "--single-file",
            "--output-format",
            "webp",
            "--json",
            "--report-fd",
            "abc",
        ],
        Some(&fixture_jpeg()),
    );
    assert_eq!(status.code(), Some(2));
    let s = String::from_utf8_lossy(&stderr);
    assert!(
        s.contains("invalid --report-fd"),
        "stderr must explain the parse failure: {s}"
    );
}

#[test]
fn report_fd_two_default_is_stderr() {
    // Sanity check: --report-fd 2 (the default) is accepted and
    // routes to stderr, matching the no-override case.
    let input = fixture_jpeg();
    let (status, _stdout, stderr) = spawn(
        &[
            "--single-file",
            "--output-format",
            "webp",
            "--json",
            "--report-fd",
            "2",
        ],
        Some(&input),
    );
    assert!(status.success(), "{status:?}; stderr: {stderr:?}");
    let v = parse_record(
        String::from_utf8_lossy(&stderr)
            .lines()
            .next()
            .expect("one stderr line"),
    )
    .expect("parse JSON on stderr");
    assert_string_field(&v, "status", "ok");
}

// --- Schema-version bump guard (defensive) ---

#[test]
fn schema_version_constant_is_one_at_test_time() {
    // AC-6 / INV-RS-2: a `schema_version` bump is a coordinated
    // breaking change. This test guards the value at runtime via
    // the emitted JSON; if a future bump lands without
    // coordinating the Symfony consumer, the assertion below will
    // fail loudly. Update this assertion only when intentionally
    // bumping `report::SCHEMA_VERSION`.
    let input = fixture_jpeg();
    let (_status, _stdout, stderr) = spawn(
        &["--single-file", "--output-format", "webp", "--json"],
        Some(&input),
    );
    let v = parse_record(
        String::from_utf8_lossy(&stderr)
            .lines()
            .next()
            .expect("one stderr line"),
    )
    .expect("parses");
    assert_eq!(
        v.get("schema_version").and_then(|x| x.as_u64()),
        Some(1),
        "schema_version bump is a breaking change; coordinate with the consumer"
    );
}

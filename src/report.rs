//! Structured JSON output mode.
//!
//! The `--json` flag switches the per-file metadata line to an
//! NDJSON record (one JSON object per line). The shape is stable,
//! documented in `docs/contracts/report-shape.md`, and versioned via
//! `schema_version`.
//!
//! The encoder is hand-rolled (no `serde` dependency) per
//! `docs/architecture.md` § 6 External Dependencies: the release
//! binary is gated to keep its size small. The schema is small and
//! fixed, so a hand-rolled encoder is simpler and cheaper than
//! pulling in a code-generation dep.

use std::fmt::Write as _;

use crate::format::Format;

/// Schema version of the JSON report. Bumping this is a documented
/// breaking change per `docs/contracts/report-shape.md` and requires
/// a coordinated bump in the Symfony `BinaryConverter`.
pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    SingleFile,
    Batch,
}

impl Mode {
    fn as_str(&self) -> &'static str {
        match self {
            Mode::SingleFile => "single_file",
            Mode::Batch => "batch",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Status {
    Ok,
    Err,
}

impl Status {
    fn as_str(&self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Err => "err",
        }
    }
}

/// Error kind enum per `docs/contracts/codec-bounds.md` § 3
/// Outputs. Distinct from `format::CodecError` because the JSON
/// shape uses a smaller enum (three values, no `path` / `kind`
/// inner payload) and a snake_case string mapping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    Decode,
    Encode,
    Io,
}

impl ErrorKind {
    fn as_str(&self) -> &'static str {
        match self {
            ErrorKind::Decode => "decode",
            ErrorKind::Encode => "encode",
            ErrorKind::Io => "io",
        }
    }
}

/// Image format identifier used inside the JSON report. Distinct
/// from `format::Format` because the JSON shape uses `jpeg` for
/// the JPG family (matching Symfony's MIME-type expectations),
/// while the CLI flag accepts both `jpg` and `jpeg` interchangeably.
/// `Heic` is input-only per ADR-0004; the JSON value is `heic`
/// (matches the CLI token and the Apple file-extension convention).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageFormat {
    Jpeg,
    Png,
    Webp,
    Heic,
}

impl ImageFormat {
    fn as_str(&self) -> &'static str {
        match self {
            ImageFormat::Jpeg => "jpeg",
            ImageFormat::Png => "png",
            ImageFormat::Webp => "webp",
            ImageFormat::Heic => "heic",
        }
    }
}

impl From<Format> for ImageFormat {
    fn from(f: Format) -> Self {
        match f {
            Format::Jpg => ImageFormat::Jpeg,
            Format::Png => ImageFormat::Png,
            Format::Webp => ImageFormat::Webp,
            Format::Heic => ImageFormat::Heic,
        }
    }
}

/// Per-file image metadata. `width` / `height` are `Option` so the
/// failure paths can still produce a structurally valid report
/// without bogus dimensions (a decode failure has neither dimension
/// set; an encode failure has input dimensions but no output ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ImageInfo {
    pub format: ImageFormat,
    pub bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct CodecMeta {
    pub quality: u8,
    pub resize_policy: String,
}

/// Build-time host metadata. Both fields are `&'static str` so the
/// encoder doesn't need to allocate when emitting them.
#[derive(Debug, Clone, Copy)]
pub struct HostMeta {
    pub libwebp_version: &'static str,
    /// `None` on builds without git context (e.g. release tarballs
    /// that strip `.git/`). The field is always present in the JSON
    /// output, with value `null` in that case.
    pub build_commit_sha: Option<&'static str>,
}

#[derive(Debug, Clone)]
pub struct ReportError {
    pub kind: ErrorKind,
    pub message: String,
}

/// Per-file report. One NDJSON line per report in batch mode; one
/// line total in single-file mode. The `input`, `output`, and
/// `error` fields are `Option` so the failure paths can still
/// produce a structurally valid record.
#[derive(Debug, Clone)]
pub struct Report {
    pub mode: Mode,
    pub status: Status,
    pub input: Option<ImageInfo>,
    pub output: Option<ImageInfo>,
    pub codec: CodecMeta,
    pub host: HostMeta,
    pub duration_ms: u64,
    pub error: Option<ReportError>,
}

impl Report {
    /// Encode the report to a single-line JSON object. The result
    /// is `serde_json`-parseable but does not depend on serde; the
    /// encoder is hand-rolled. No trailing newline is appended;
    /// callers add the newline when emitting NDJSON.
    pub fn to_json(&self) -> String {
        let mut s = String::with_capacity(512);

        s.push('{');

        // schema_version (always first; breaking-change anchor)
        s.push_str("\"schema_version\":");
        s.push_str(&SCHEMA_VERSION.to_string());

        // mode
        s.push_str(",\"mode\":");
        write_json_string(&mut s, self.mode.as_str());

        // status
        s.push_str(",\"status\":");
        write_json_string(&mut s, self.status.as_str());

        // input
        s.push_str(",\"input\":");
        match &self.input {
            Some(i) => write_image_info(&mut s, i),
            None => s.push_str("null"),
        }

        // output
        s.push_str(",\"output\":");
        match &self.output {
            Some(o) => write_image_info(&mut s, o),
            None => s.push_str("null"),
        }

        // codec
        s.push_str(",\"codec\":{");
        s.push_str("\"quality\":");
        s.push_str(&self.codec.quality.to_string());
        s.push_str(",\"resize_policy\":");
        write_json_string(&mut s, &self.codec.resize_policy);
        s.push('}');

        // host
        s.push_str(",\"host\":{");
        s.push_str("\"libwebp_version\":");
        write_json_string(&mut s, self.host.libwebp_version);
        s.push_str(",\"build_commit_sha\":");
        match self.host.build_commit_sha {
            Some(sha) => write_json_string(&mut s, sha),
            None => s.push_str("null"),
        }
        s.push('}');

        // duration_ms
        s.push_str(",\"duration_ms\":");
        s.push_str(&self.duration_ms.to_string());

        // error
        s.push_str(",\"error\":");
        match &self.error {
            Some(e) => {
                s.push('{');
                s.push_str("\"kind\":");
                write_json_string(&mut s, e.kind.as_str());
                s.push_str(",\"message\":");
                write_json_string(&mut s, &e.message);
                s.push('}');
            }
            None => s.push_str("null"),
        }

        s.push('}');
        s
    }
}

fn write_image_info(out: &mut String, i: &ImageInfo) {
    out.push('{');
    out.push_str("\"format\":");
    write_json_string(out, i.format.as_str());
    out.push_str(",\"bytes\":");
    out.push_str(&i.bytes.to_string());
    out.push_str(",\"width\":");
    match i.width {
        Some(w) => out.push_str(&w.to_string()),
        None => out.push_str("null"),
    }
    out.push_str(",\"height\":");
    match i.height {
        Some(h) => out.push_str(&h.to_string()),
        None => out.push_str("null"),
    }
    out.push('}');
}

/// Append `s` to `out` as a JSON string literal with proper
/// escaping per RFC 8259 § 7. Control characters below 0x20 are
/// emitted as `\uXXXX`; the seven mandatory escapes are emitted
/// as their short forms.
fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\x08' => out.push_str("\\b"),
            '\x0c' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                // Unreachable for our inputs (we never embed
                // binary in the report), but kept defensively.
                write!(out, "\\u{:04x}", c as u32).expect("writing to String is infallible");
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host() -> HostMeta {
        HostMeta {
            libwebp_version: "1.6.0",
            build_commit_sha: Some("deadbeef"),
        }
    }

    fn codec() -> CodecMeta {
        CodecMeta {
            quality: 85,
            resize_policy: "auto:portrait=800,landscape=1000".to_string(),
        }
    }

    #[test]
    fn schema_version_is_first_field() {
        let r = Report {
            mode: Mode::SingleFile,
            status: Status::Ok,
            input: None,
            output: None,
            codec: codec(),
            host: host(),
            duration_ms: 0,
            error: None,
        };
        let s = r.to_json();
        assert!(
            s.starts_with("{\"schema_version\":1,"),
            "schema_version must be first; got: {s}"
        );
    }

    #[test]
    fn success_path_emits_all_documented_fields() {
        let r = Report {
            mode: Mode::SingleFile,
            status: Status::Ok,
            input: Some(ImageInfo {
                format: ImageFormat::Jpeg,
                bytes: 12345,
                width: Some(1920),
                height: Some(1080),
            }),
            output: Some(ImageInfo {
                format: ImageFormat::Webp,
                bytes: 6789,
                width: Some(1920),
                height: Some(1080),
            }),
            codec: codec(),
            host: host(),
            duration_ms: 42,
            error: None,
        };
        let s = r.to_json();
        // Each documented field is present and correctly typed.
        assert!(s.contains("\"mode\":\"single_file\""), "{s}");
        assert!(s.contains("\"status\":\"ok\""), "{s}");
        assert!(s.contains("\"format\":\"jpeg\""), "{s}");
        assert!(s.contains("\"format\":\"webp\""), "{s}");
        assert!(s.contains("\"bytes\":12345"), "{s}");
        assert!(s.contains("\"bytes\":6789"), "{s}");
        assert!(s.contains("\"width\":1920"), "{s}");
        assert!(s.contains("\"height\":1080"), "{s}");
        assert!(s.contains("\"quality\":85"), "{s}");
        assert!(
            s.contains("\"resize_policy\":\"auto:portrait=800,landscape=1000\""),
            "{s}"
        );
        assert!(s.contains("\"libwebp_version\":\"1.6.0\""), "{s}");
        assert!(s.contains("\"build_commit_sha\":\"deadbeef\""), "{s}");
        assert!(s.contains("\"duration_ms\":42"), "{s}");
        assert!(s.contains("\"error\":null"), "{s}");
    }

    #[test]
    fn failure_path_emits_error_block() {
        let r = Report {
            mode: Mode::SingleFile,
            status: Status::Err,
            input: Some(ImageInfo {
                format: ImageFormat::Jpeg,
                bytes: 1024,
                width: None,
                height: None,
            }),
            output: None,
            codec: codec(),
            host: host(),
            duration_ms: 7,
            error: Some(ReportError {
                kind: ErrorKind::Decode,
                message: "decode error: malformed jpeg".to_string(),
            }),
        };
        let s = r.to_json();
        assert!(s.contains("\"status\":\"err\""), "{s}");
        assert!(s.contains("\"kind\":\"decode\""), "{s}");
        assert!(
            s.contains("\"message\":\"decode error: malformed jpeg\""),
            "{s}"
        );
        assert!(s.contains("\"width\":null"), "{s}");
        assert!(s.contains("\"height\":null"), "{s}");
        assert!(s.contains("\"output\":null"), "{s}");
    }

    #[test]
    fn error_message_is_json_escaped() {
        let r = Report {
            mode: Mode::Batch,
            status: Status::Err,
            input: None,
            output: None,
            codec: codec(),
            host: host(),
            duration_ms: 0,
            error: Some(ReportError {
                kind: ErrorKind::Io,
                message: "bad\"quote and \\back and \nnewline".to_string(),
            }),
        };
        let s = r.to_json();
        // The embedded characters must be JSON-escaped; the raw
        // form would break a downstream parser.
        assert!(
            s.contains("\"message\":\"bad\\\"quote and \\\\back and \\nnewline\""),
            "{s}"
        );
        // No raw newline inside the string.
        assert!(!s.contains('\n'), "NDJSON record must be single-line: {s}");
    }

    #[test]
    fn batch_mode_string_is_distinct_from_single_file() {
        let r = Report {
            mode: Mode::Batch,
            status: Status::Ok,
            input: None,
            output: None,
            codec: codec(),
            host: host(),
            duration_ms: 0,
            error: None,
        };
        let s = r.to_json();
        assert!(s.contains("\"mode\":\"batch\""), "{s}");
    }

    #[test]
    fn image_format_mapping_from_format_enum_uses_jpeg_not_jpg() {
        // The JSON shape uses "jpeg" for the JPG family; the
        // mapping is the whole reason `ImageFormat` is distinct
        // from `format::Format`.
        assert_eq!(ImageFormat::from(Format::Jpg), ImageFormat::Jpeg);
        assert_eq!(ImageFormat::from(Format::Png), ImageFormat::Png);
        assert_eq!(ImageFormat::from(Format::Webp), ImageFormat::Webp);

        let r = Report {
            mode: Mode::Batch,
            status: Status::Ok,
            input: Some(ImageInfo {
                format: ImageFormat::from(Format::Jpg),
                bytes: 1,
                width: None,
                height: None,
            }),
            output: None,
            codec: codec(),
            host: host(),
            duration_ms: 0,
            error: None,
        };
        let s = r.to_json();
        assert!(s.contains("\"format\":\"jpeg\""), "{s}");
        assert!(!s.contains("\"format\":\"jpg\""), "{s}");
    }

    #[test]
    fn build_commit_sha_absent_emits_null() {
        let h = HostMeta {
            libwebp_version: "1.6.0",
            build_commit_sha: None,
        };
        let r = Report {
            mode: Mode::SingleFile,
            status: Status::Ok,
            input: None,
            output: None,
            codec: codec(),
            host: h,
            duration_ms: 0,
            error: None,
        };
        let s = r.to_json();
        assert!(
            s.contains("\"build_commit_sha\":null"),
            "missing sha must serialise as null; got: {s}"
        );
    }

    #[test]
    fn ndjson_record_has_no_trailing_newline() {
        let r = Report {
            mode: Mode::SingleFile,
            status: Status::Ok,
            input: None,
            output: None,
            codec: codec(),
            host: host(),
            duration_ms: 0,
            error: None,
        };
        let s = r.to_json();
        assert!(!s.ends_with('\n'), "the caller appends the newline");
    }

    #[test]
    fn schema_version_constant_is_one() {
        assert_eq!(SCHEMA_VERSION, 1);
    }
}

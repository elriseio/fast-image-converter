---
project_slug: convert-to-webp
doc_slug: contract_report_shape
doc_type: contract_doc
applicable_roles: [architect, developer, tester]
version: 1
source_artifacts:
  - src/report.rs (encoder + types)
  - src/main.rs::emit_single_file_success_report, emit_single_file_failure_report, emit_batch_record_success, emit_batch_record_failure, emit_batch_record_io_failure
  - src/main.rs::validate_report_fd, emit_report_line
  - build.rs (CONVERT_TO_WEBP_BUILD_COMMIT_SHA, CONVERT_TO_WEBP_LIBWEBP_VERSION env injection)
  - Issues/open/developer/DE-005_add_structured_json_output_mode.md
  - docs/contracts/codec-bounds.md (referenced for error.kind enum)
summary: "Wire contract for the --json structured-output mode (DE-005). NDJSON shape, schema_version anchor, field semantics, --report-fd stream override, host metadata source."
tags: [contract, json, ndjson, schema, report, de-005, breaking-change-anchor]
---

# Contract: `report-shape`

## 1. Direction

```
cli-frontend --(per-file Report struct)-->  report-encoder (src/report.rs)
cli-frontend --(--report-fd validation)--> POSIX fcntl(F_GETFL) on the fd
cli-frontend --(one NDJSON line per file)--> the report stream
build-time --(env vars via build.rs)--> host metadata fields
```

The contract runs in one direction: the binary emits one
NDJSON record per converted file (one line total in
`--single-file` mode; one line per candidate in batch mode).
The wire shape is hand-rolled (no `serde` dependency) per
`docs/architecture.md` § 6 External Dependencies: the release
binary is gated to keep its size small. The schema is small
and fixed, so a hand-rolled encoder is simpler and cheaper
than pulling in a code-generation dep.

## 2. Inputs

| Field | Type | Source |
|---|---|---|
| `--json` | boolean flag | CLI parser (`parse_cli`) |
| `--report-fd <N>` | `i32` | CLI parser (`parse_cli`); defaults to `2` (stderr) |
| Per-file conversion outcome | `Result<ConversionReport, CodecError>` | `format-codecs` via `Codec::convert_one_with` |
| Build-time host metadata | `(&'static str, Option<&'static str>)` | `build.rs` env injection |
| Operator-supplied fd (for `--report-fd`) | `i32` | CLI parser; validated with `libc::fcntl(F_GETFL)` |

## 3. Outputs

The shape (schema_version = 1):

```json
{
  "schema_version": 1,
  "mode": "single_file" | "batch",
  "status": "ok" | "err",
  "input": {
    "format": "jpeg" | "png" | "webp",
    "bytes": 12345,
    "width": 1920,
    "height": 1080
  } | null,
  "output": {
    "format": "webp" | "png" | "jpeg",
    "bytes": 6789,
    "width": 1920,
    "height": 1080
  } | null,
  "codec": {
    "quality": 85,
    "resize_policy": "auto:portrait=800,landscape=1000"
  },
  "host": {
    "libwebp_version": "1.6.0",
    "build_commit_sha": "<git rev-parse HEAD>" | null
  },
  "duration_ms": 42,
  "error": null | { "kind": "decode" | "encode" | "io", "message": "..." }
}
```

Per-field semantics:

| Field | Type | Semantics |
|---|---|---|
| `schema_version` | integer | always `1` (constant `report::SCHEMA_VERSION`); first field in the record; bumping is a documented breaking change |
| `mode` | string | `"single_file"` or `"batch"`; matches the CLI mode |
| `status` | string | `"ok"` on success, `"err"` on any failure path (decode, encode, io, post-conversion source-delete) |
| `input` | object or `null` | per-file input metadata; `null` when no input was consumed (pre-decode io failure); `width`/`height` are `null` when decode never produced a `DynamicImage` |
| `input.format` | string | image-format identifier on the input side; `"jpeg"` for the JPG family (matches Symfony's MIME-type expectations; the CLI flag accepts `jpg` and `jpeg` interchangeably) |
| `input.bytes` | integer | raw byte count of the input bytes |
| `input.width`, `input.height` | integer or `null` | pixel dimensions of the decoded image; `null` on decode failure |
| `output` | object or `null` | per-file output metadata; `null` when no output was produced (decode failure, encode failure, pre-encode io failure); `width`/`height` reflect post-resize dimensions (may differ from `input.*` when a resize policy was applied) |
| `output.format` | string | image-format identifier on the output side |
| `output.bytes` | integer | raw byte count of the encoded output |
| `codec.quality` | integer | the encoder's quality knob as supplied by the CLI (`1..100`) |
| `codec.resize_policy` | string | the resize policy in its CLI form (`"none"`, `"cap=<W>"`, or `"auto:portrait=<W>,landscape=<H>"`) |
| `host.libwebp_version` | string | `pkg-config --modversion libwebp`, baked at build time; matches the host `webp` crate's build dependency |
| `host.build_commit_sha` | string or `null` | `git rev-parse HEAD`, baked at build time; `null` on builds without git context (release tarballs, missing `.git/`) |
| `duration_ms` | integer | wall-clock duration of the per-file work in milliseconds |
| `error` | object or `null` | `null` on success; otherwise carries `kind` (one of `"decode"`, `"encode"`, `"io"`) and `message` (the codec-reported message) |

## 4. Invariants

- **INV-RS-1**: every NDJSON record is one line terminated by
  `\n`. Records are *independent* (no enclosing array). The
  shape matches RFC 8259; control characters below `0x20` in
  string values are emitted as `\uXXXX`, the seven mandatory
  escapes are emitted in short form.
- **INV-RS-2**: `schema_version` is the first field in every
  record; bumping `SCHEMA_VERSION` is a coordinated breaking
  change requiring an ADR and the Symfony `BinaryConverter`
  bump.
- **INV-RS-3**: in `--single-file` mode, the binary emits
  exactly one record on the report stream, regardless of
  success or failure.
- **INV-RS-4**: in batch mode, the binary emits one record per
  candidate, in completion order. Records are independent
  (NDJSON); consumers parse them line-by-line.
- **INV-RS-5**: stdout is not polluted. In `--single-file` mode
  stdout contains only the encoded bytes; in batch mode
  stdout contains only the byte summary (v0 behaviour
  preserved per `docs/components/cli-frontend.md` § 3).
- **INV-RS-6**: when `--json` is **not** set, the binary
  preserves the v0 / DE-004 behaviour: a single key=value
  line on stderr in single-file mode; no per-file metadata
  in batch mode (the v0 trailer `(processed N candidates,
  K failed)` still appears on stderr in batch mode — see
  § 6 Known Interactions).
- **INV-RS-7**: the report stream default is fd 2 (stderr).
  Override with `--report-fd <N>`. The accepted set is:
  - `N == 2` (stderr); accepted without further checks
    (writes may fail if stderr was closed; the binary stays
    consistent).
  - `N` is a positive integer where `fcntl(N, F_GETFL)`
    returns an access mode of `O_WRONLY` or `O_RDWR`.
    Read-only fds are rejected.
  - `N == 0` (stdin) is accepted only if it is open for
    writing.
  - `N == 1` (stdout) is **forbidden** regardless of access
    mode: in single-file mode stdout carries the encoded
    bytes and the report stream would collide with the
    payload.
  - Any other value is rejected with a usage message on
    stderr and exit code `2`.

## 5. Enforcement

- **INV-RS-1..INV-RS-5**: enforced by `report::tests::*` (unit
  tests in `src/report.rs`) and by `tests/json_output.rs`
  (integration tests that spawn the binary and parse the
  NDJSON stream).
- **INV-RS-6**: enforced by `tests/single_file.rs`
  (`single_file_metadata_line_shape` for the v0 single-file
  shape) and by `tests/golden_v0.rs` (the default pipeline
  must continue to byte-match the golden under
  `--resize auto:portrait=800,landscape=1000`).
- **INV-RS-7**: enforced by `main::validate_report_fd` (via
  `libc::fcntl`); the corresponding behaviour tests live in
  `tests/json_output.rs::report_fd_*`.

## 6. Known Interactions

- The v0 stderr trailer `(processed N candidates, K failed)`
  (see `src/main.rs:401` and `docs/components/cli-frontend.md`
  § 3) appears on the report stream in batch mode regardless
  of `--json`. Consumers that pipe the report stream to a JSON
  parser must filter for parseable lines (e.g.
  `grep -v '^(' /tmp/batch.jsonl | jq -c .`). The trailer is
  preserved as v0 behaviour; suppressing or relocating it
  requires a separate change.
- The `gallery-compress` binary is a thin forwarder to
  `convert-to-webp` (see `src/bin/gallery-compress.rs`); the
  JSON contract is owned by the canonical binary and is not
  re-emitted by the forwarder.

## 7. Schema Versioning

- `schema_version = 1` is the current version.
- Adding an optional field with a default-safe interpretation
  is **non-breaking** and does **not** bump `schema_version`.
- Removing a field, changing the type of a field, renaming a
  field, or changing the enum value set (`mode`, `status`,
  `input.format`, `output.format`, `error.kind`) is a
  **breaking change** that requires:
  1. Bumping `SCHEMA_VERSION` in `src/report.rs`.
  2. An ADR under `docs/adr/` documenting the change.
  3. A coordinated bump in the Symfony `BinaryConverter`
     consumer.
- `schema_version` is **not** a feature flag: a single value
  is active at any time. Consumers that need to support
  multiple versions MUST branch on the field value.

## 8. Open Questions (architect hand-off to developer)

- **Q1**: should the v0 stderr trailer
  `(processed N candidates, K failed)` be suppressed (or
  relocated to stdout) when `--json` is set? Current
  behaviour: trailer is preserved on stderr in both modes.
  A consumer that does `jq -c . /tmp/batch.jsonl` will
  fail to parse the trailer line; filter on parseable
  lines is the documented workaround.
- **Q2**: should the trailer be replaced by a structured
  summary record at the end of the batch? Pro: consistent
  NDJSON shape; Con: changes the wire format and forces a
  `schema_version` bump if the summary is in the same
  shape, or a separate field if it is appended outside the
  per-file records.

## 9. Future Work

- **F-1**: `--report-fd 1` could be relaxed for batch mode
  (where stdout carries the byte summary rather than the
  payload); gated on a follow-up ADR.
- **F-2**: a streaming-progress variant that emits one
  record per chunk for very large images; gated on operator
  request.
- **F-3**: a binary wire format (Protobuf, msgpack, Cap'n
  Proto) for non-JSON consumers; gated on the appearance
  of such a consumer.

## 10. Cross-References

- `docs/components/cli-frontend.md` § 2 Inputs (CLI flag
  surface) and § 3 Outputs (channel contract).
- `docs/contracts/codec-bounds.md` § 3 Outputs (the codec
  result types that feed the per-file Report struct).
- `docs/architecture.md` § 6 External Dependencies
  (hand-rolled JSON encoder rationale).
- `Issues/open/developer/DE-005_add_structured_json_output_mode.md`
  — the originating task; this contract documents the
  schema_version = 1 shape from § 2 of that issue.
- `Issues/open/developer/DE-006_add_server_side_skeleton.md`
  — the Symfony `BinaryConverter` consumer of this shape.

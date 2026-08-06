---
project_slug: fast-image-converter
doc_slug: component_cli_frontend
doc_type: component_doc
applicable_roles: [architect, developer, tester]
version: 1
source_artifacts:
  - src/main.rs:17-99
  - README.md § Usage
summary: "CLI front-end component: argv + env parsing, usage printer, exit-code mapper."
tags: [component, cli, argv, exit-codes]
---

# Component: `cli-frontend`

## 1. Purpose

Parse the operator's command-line invocation (argv + environment
variables), print usage on bad invocation, and delegate to
`converter-core` for actual work. Map the work's outcome to the
documented exit codes.

## 2. Inputs

| Source | Field | Type | Notes |
|---|---|---|---|
| `argv` | positional arg #1 | `&str` | either a directory path containing `/` or a bare year segment appended to `GALLERY_BASE` |
| env | `GALLERY_BASE` | `&str` | optional; default falls back to v0 hard-coded `DEFAULT_GALLERY_BASE` (tech debt, see `RUNBOOK.md` § AD-001) |
| env | (planned) `CONVERT_TO_WEBP_*` | various | gated on Wave 2+ |

Wave 1 additions (per ADR-0001):

| Source | Field | Type | Notes |
|---|---|---|---|
| `argv` | `--input-format <fmt>` | `&str` | optional; `jpg` \| `png` \| `webp`; default = inferred from extension |
| `argv` | `--output-format <fmt>` | `&str` | optional; `jpg` \| `png` \| `webp`; default = `webp` |
| `argv` | `--quality <n>` | `u8` | optional; planned Wave 2 (not Wave 1) |

Wave 4 additions (per ADR-0001):

| Source | Field | Type | Notes |
|---|---|---|---|
| `argv` | `--resize <policy>` | `&str` | optional; `none` \| `cap=<W>` \| `auto:portrait=<W>,landscape=<H>`; default = `auto:portrait=800,landscape=1000` |
| `argv` | `--keep-source` | bool flag | optional; default `false`; v0 baseline removes the source after a successful conversion |

Wave 4 (DE-004) additions:

| Source | Field | Type | Notes |
|---|---|---|---|
| `argv` | `--single-file`, `-1` | bool flag | optional; default `false`; switches the binary to single-file stdin/stdout mode |

Wave 5 (DE-005) additions:

| Source | Field | Type | Notes |
|---|---|---|---|
| `argv` | `--json` | bool flag | optional; default `false`; switches the per-file metadata line to a structured NDJSON record (schema_version = 1; see `docs/contracts/report-shape.md`) |
| `argv` | `--report-fd <N>` | `i32` | optional; default `2` (stderr); overrides the report stream; `N == 1` is forbidden (would collide with the encoded bytes in single-file mode); non-writable fds rejected with usage + exit `2` |

## 3. Outputs

| Channel | Content |
|---|---|
| stdout | batch mode: single-line summary `fast-image-converter: <N> files in <DIR>: <IN_BYTES> -> <OUT_BYTES>` (preserved verbatim from v0); single-file mode: the encoded image bytes (raw; no header / framing) |
| stderr | per-file error lines `<file>: <error>` (preserved verbatim from v0) + the v0 stderr trailer `(processed N candidates, K failed)` (preserved in batch mode); single-file mode metadata line in v0/DE-004 shape; with `--json`, the per-file NDJSON record on the configured report stream (default fd 2) per `docs/contracts/report-shape.md` |
| exit code | `0` \| `1` \| `2` (see § 4) |

## 4. Invariants

- **INV-CLI-1**: exit code `2` is reserved for wrong invocation
  (arg count != 1 in v0; arg parse failure in Wave 1+).
- **INV-CLI-2**: exit code `1` is reserved for runtime failure
  (bad directory, decode/encode failure, ≥ 1 file failed).
- **INV-CLI-3**: exit code `0` is success (including the
  no-candidates case).
- **INV-CLI-4**: the stdout summary line is exactly one line, no
  trailing newline, grep-friendly.
- **INV-CLI-5**: `--help` (or invocation with no args) prints
  usage to stderr (matching v0 `print_usage` behaviour) and exits
  with code `2`.

## 5. Failure Modes

| Mode | Trigger | Behaviour |
|---|---|---|
| Missing arg | `argv.len() < 2` | print usage to stderr; exit `2` |
| Too many args | `argv.len() > 2` (Wave 1 adds flags; for Wave 1 the count is `≥ 2`) | print usage to stderr; exit `2` |
| Unknown flag | `--foo` not in the registered flag set | print usage to stderr naming the unknown flag; exit `2` |
| Bad `--input-format` / `--output-format` value | `<fmt>` not in `{jpg, png, webp}` (Wave 1) | print usage to stderr; exit `2` |
| Bad directory | arg resolves to a non-existent / non-directory path | print `not a directory: <path>` to stderr; exit `1` |
| Unreadable directory | permission denied | print `cannot read <path>: <errno>` to stderr; exit `1` |
| ≥ 1 file failed | per-file decode / encode failure surfaced by `converter-core` | exit `1`; per-file errors already on stderr from `converter-core` |

## 6. Related Contracts

- `contracts/codec-bounds.md` — codec ↔ converter-core contract;
  CLI does not directly touch codecs.
- `architecture.md` § 7 Quality Attributes (binary size, exit codes).

## 7. Future Work

- Wave 1: add `--input-format` / `--output-format` flag parsing.
- Wave 2: add `--quality <n>`.
- Wave 3: add `--resize <policy>`.
- Wave 4: add `--dry-run`, `--keep-source`, `--jobs <N>`.
- Wave 4 (DE-004): add `--single-file` mode (single-file stdin/stdout pipeline).
- Wave 5 (DE-005): add `--json` and `--report-fd` flags; the NDJSON shape is governed by `contracts/report-shape.md`.

# convert-to-webp

A single Rust binary that batch-converts a directory of images between
`jpg`, `png`, and `webp`, and that also runs as a single-file
stdin/stdout pipeline for server-side callers. The default pipeline
(no flags) walks a directory of `.jpg` files, applies the v0
orientation-based resize policy, encodes each image as `.webp` via
the host `libwebp`, and removes the source on success.

For an in-depth operator / consumer contract (full JSON schema,
exit-code rules, latency budgets, failure-mode mapping), see
[`docs/integration-contract.md`](docs/integration-contract.md).
For incident handling and operational triage, see
[`docs/RUNBOOK.md`](docs/RUNBOOK.md).

## Executive Summary

- **What it is**: a self-contained CLI for batch and single-file image
  conversion between `jpg`, `png`, and `webp`.
- **Why it exists**: the v0 `gallery-compress` pipeline (JPG → WebP
  with the per-orientation resize policy) was repackaged as a generic
  converter without re-spawning a separate tool per format.
- **State today** (v0.2.0): production baseline landed. Multi-format
  scope (`jpg`, `png`, `webp`), `keep-source`, `quality` and
  `resize` flags, `--single-file` mode, `--json` / `--report-fd`
  structured output, and `GALLERY_BASE` env-only contract all
  implemented. The repo is in stabilization; see
  [`docs/architecture/STATUS.md`](docs/architecture/STATUS.md) § 5
  for the gating P0 queue.
- **Audience**: operators running bulk conversion jobs in shell,
  CI pipelines, or server-side wrappers; contributors extending the
  codec surface or fixing defects.

## Supported Conversions

Six pipelines are exposed through the
`--input-format` / `--output-format` flag pair. All three format
identifiers (`jpg`, `jpeg`, `png`, `webp`) are accepted
case-insensitively; `jpg` and `jpeg` are aliases for the same codec.

| Pipeline | Decoder | Encoder | Quality honoured |
|---|---|---|---|
| `jpg` → `webp` (default) | `image` (`jpeg` feature) | `webp` crate (lossy VP8) | yes |
| `png` → `webp` | `image` (`png` feature) | `webp` crate | yes |
| `webp` → `png` | `image` (`webp` feature) | `image` (`png` feature, lossless) | no (lossless) |
| `webp` → `jpg` | `image` (`webp` feature) | `image` (`jpeg` feature) | yes |
| `jpg` → `png` | `image` (`jpeg` feature) | `image` (`png` feature, lossless) | no (lossless) |
| `png` → `jpg` | `image` (`png` feature) | `image` (`jpeg` feature) | yes |

The `image` crate's enabled features are `jpeg`, `png`, and `webp`
(see [`Cargo.toml`](Cargo.toml)); the `webp` crate is the dedicated
encoder-only binding to host `libwebp` (see
[`docs/components/format-codecs.md`](docs/components/format-codecs.md)
for the codec-layer specification).

## Live Demo

A hosted demo of the converter is available at
**<https://converter.elrise.io>**. Use it to try the default
`jpg → webp` pipeline, the WebP decoder (`webp → png` / `webp → jpg`),
and the resize policies without building the binary locally. The
demo runs the same `convert-to-webp` CLI behind a web UI and
shares the documented JSON report shape when `--json` is enabled.

## Quick Start

### Prerequisites

- **Rust toolchain** (1.75 or newer; tested with 1.97).
- **`libwebp` development files** plus `pkg-config` and a C
  toolchain. The build links against host `libwebp`.
  - Debian / Ubuntu: `sudo apt install libwebp-dev build-essential pkg-config`
  - Arch: `sudo pacman -S libwebp base-devel pkgconf`
  - Homebrew (macOS): `brew install webp pkg-config`

Verify the host library is visible to `pkg-config`:

```bash
pkg-config --modversion libwebp   # expected: 1.6.0 or newer
```

### Build

```bash
cargo build --release
```

The canonical binary is `target/release/convert-to-webp`. A
backward-compatible forwarder named `target/release/gallery-compress`
is also produced (see [Binary Rename](#binary-rename)).

### First Successful Invocation

Convert a directory of `.jpg` images in place (source files removed
on success, WebP output alongside them):

```bash
./target/release/convert-to-webp /path/to/your/images
```

Run a single conversion through the stdin/stdout pipeline (encoded
bytes on stdout, one-line report on stderr):

```bash
cat input.jpg | ./target/release/convert-to-webp --single-file --output-format webp > output.webp
```

Run the same conversion with structured NDJSON output:

```bash
cat input.jpg | ./target/release/convert-to-webp --single-file --output-format webp --json \
    > output.webp 2> report.jsonl
```

## CLI Surface

`--help` is the authoritative flag reference; the table below is a
curated operator-facing view.

```bash
./target/release/convert-to-webp --help
```

| Flag | Type | Default | Meaning |
|---|---|---|---|
| `<dir>` | positional | (required in batch mode) | directory containing the input images |
| `--input-format <fmt>` | enum | `jpg` | `jpg`, `jpeg`, `png`, or `webp` (case-insensitive; `jpg` and `jpeg` are aliases) |
| `--output-format <fmt>` | enum | `webp` | `jpg`, `jpeg`, `png`, or `webp` (case-insensitive; `jpg` and `jpeg` are aliases) |
| `--quality <1..100>` | integer | `85` | encoder quality; honoured by WebP and JPEG outputs, ignored by PNG (lossless) |
| `--resize <policy>` | enum | `auto:portrait=800,landscape=1000` | one of `none`, `cap=<W>`, or `auto:portrait=<W>,landscape=<H>` |
| `--keep-source` | flag | `false` | leave the source file in place after a successful conversion; **batch mode only**, silently ignored in `--single-file` |
| `--single-file`, `-1` | flag | `false` | read one image from stdin, write the encoded image bytes to stdout |
| `--json` | flag | `false` | emit per-file metadata as a structured NDJSON record (`schema_version: 1`) instead of the v0 key=value line |
| `--report-fd <N>` | integer fd | `2` (stderr) | override the report stream; `N=1` is forbidden (would collide with encoded bytes in `--single-file`); non-writable fds rejected with exit `2` |
| `-h`, `--help` | flag | — | print usage to stderr and exit `2` |

### Two Modes

| Mode | Trigger | Input | Output |
|---|---|---|---|
| Batch | positional `<dir>` argument | a directory of candidate files | encoded files written into the input directory; v0 baseline removes the source after a successful conversion |
| Single-file | `--single-file` flag | raw image bytes on stdin | raw encoded bytes on stdout; per-file metadata line on stderr (or NDJSON record with `--json`) |

Mixing the two (`--single-file` + a positional dir) is a usage
error and exits `2`. The full per-mode channel contract, including
stdout/stderr split and exit-code mapping for external consumers,
is documented in
[`docs/integration-contract.md`](docs/integration-contract.md).

### Safety and Data-Loss Behaviour

The source file is removed **only after** a successful encode and
write. On any failure path (bad directory, decode failure, encode
failure, post-encode source-delete failure), the source is left
untouched. The detailed codec ↔ converter-core contract — what is
preserved on `Err`, the meaning of `INV-CB-3` (source removal) and
`INV-CB-4` (source untouched on `Err`), and the `CodecError`
variant semantics — lives in
[`docs/contracts/codec-bounds.md`](docs/contracts/codec-bounds.md).

In single-file mode there is no source filesystem path to remove;
`--keep-source` is silently ignored in that mode because stdin has
no associated filesystem metadata.

### Environment Variables

| Variable | Default | Meaning |
|---|---|---|
| `GALLERY_BASE` | _unset (no built-in default)_ | When set, used as the parent directory for a bare positional argument (e.g. a year like `2025`). The binary does not fall back to any host-specific path; pass an absolute path or set this explicitly. See [Bare Argument Resolution](#bare-argument-resolution). |

### Exit Codes

| Code | Meaning |
|---|---|
| `0` | success (including the no-candidates case in batch mode) |
| `1` | runtime failure (bad directory, decode / encode failure, ≥ 1 file failed) |
| `2` | wrong invocation (arg count, unknown flag, bad enum value, same input/output format, `--single-file` + positional, `--report-fd 1`, non-writable `--report-fd`) |

The exit-code contract is fixed at three codes for the lifetime of
major version `1`; introducing a fourth is a breaking change
(captured in [`docs/integration-contract.md`](docs/integration-contract.md) § 3
and [`docs/components/cli-frontend.md`](docs/components/cli-frontend.md) § 4).

### Bare Argument Resolution

A positional argument that contains a `/` is treated as an absolute
or relative path and used verbatim (v0 contract preserved). A bare
argument without a `/` (e.g. `2025`) is joined to `GALLERY_BASE`;
if `GALLERY_BASE` is unset, the binary prints a usage message and
exits `2`. This contract was fixed by DE-006 (no host-specific
fallback path remains in the source tree).

## Structured JSON Output

The `--json` flag switches the per-file metadata line from the v0
key=value shape to a versioned NDJSON record (one JSON object per
line; `schema_version: 1`). The flag is orthogonal to the mode:
both batch and single-file support `--json`. The full schema —
field semantics, error taxonomy, `--report-fd` validation rules,
schema-versioning policy — lives in
[`docs/contracts/report-shape.md`](docs/contracts/report-shape.md).
A worked example on the success path:

```json
{
  "schema_version": 1,
  "mode": "single_file",
  "status": "ok",
  "input":  { "format": "jpeg", "bytes": 43058, "width": 256, "height": 384 },
  "output": { "format": "webp", "bytes": 43694, "width": 256, "height": 384 },
  "codec":  { "quality": 85, "resize_policy": "auto:portrait=800,landscape=1000" },
  "host":   { "libwebp_version": "1.6.0", "build_commit_sha": null },
  "duration_ms": 17,
  "error":  null
}
```

`host.build_commit_sha` is **`null` on builds without git context**
(release tarballs, missing `.git/` directory). When git is
available, it carries `git rev-parse HEAD` baked at compile time via
[`build.rs`](build.rs). The recorded host
`libwebp_version` is `pkg-config --modversion libwebp`, also baked
at build time.

In batch mode the v0 trailer `(processed N candidates, K failed)`
still lands on the same report stream (stderr by default). Filter
parseable lines with `grep '^{' /tmp/batch.jsonl | jq -c .` if your
consumer cannot tolerate the trailer line.

## Binary Rename

The canonical binary is `convert-to-webp` (matching the project
slug). The legacy v0 name `gallery-compress` is retained as a thin
forwarder ([`src/bin/gallery-compress.rs`](src/bin/gallery-compress.rs))
that prints a deprecation hint on stderr and forwards argv to
`convert-to-webp`. The forwarder preserves the inner binary's exit
code. The legacy name is kept for at least the next major version.

## Performance

The v0 baseline observed an ~8.5× wall-time speed-up over a
bash + ImageMagick pipeline on a 50-file / 3 MB mixed-orientation
JPG batch on a 12-core host with `libwebp 1.6.0` (1.18 s vs 10.09 s).
The win comes from eliminating ~100 process spawns (`identify` +
`magick` per image) and parallelising across cores with `rayon`.
Output bytes match within 0.1 % of the `libwebp` reference encoder
(golden-batch regression test, see [Testing](#testing)).

> **Reproducibility note (environment-dependent)**. The numbers
> above are host-specific. Re-measure on your host with:
>
> ```bash
> time ./target/release/convert-to-webp /path/to/your/jpg-batch
> ```
>
> The 0.1 % fidelity tolerance is enforced against
> `tests/fixtures/golden_v0/expected/` recorded on a host with
> `libwebp 1.6.0`; if your host's `libwebp` differs, re-record the
> golden (see [Re-recording the Golden Batch](#re-recording-the-golden-batch)).

The release binary size is ~2.4 MiB (stripped) on the reference
host. **Reproducibility note (environment-dependent)**: rebuild
locally with `cargo build --release` and inspect
`ls -lh target/release/convert-to-webp`. Size varies with the
target triple, the `image` crate's decoder table, and the host
linker.

## Limitations

- **Input formats**: `jpg`, `png`, `webp` only. `gif`, `bmp`,
  `tiff`, and `avif` are gated on follow-up waves; see
  [`docs/ROADMAP.md`](docs/ROADMAP.md) § Wave 2.
- **Animated GIF / APNG**: out of scope; only the first frame is
  processed.
- **ICC profiles / colour-management transforms**: passed through
  `image` crate defaults; no explicit handling.
- **Library API**: the binary is the only public surface. An
  in-process library API is gated on a follow-up ADR; see
  [`docs/integration-contract.md`](docs/integration-contract.md) § 10.
- **Network / telemetry**: the binary makes no network calls and
  emits no telemetry.
- **Cross-`libwebp`-version fidelity**: the 0.1 % tolerance is
  enforced against `libwebp 1.6.0` (the recorded golden host
  version). Drift on a different host `libwebp` is documented but
  not auto-corrected; re-record the golden if your host `libwebp`
  changes (see [Re-recording the Golden Batch](#re-recording-the-golden-batch)).
- **Single-host**: no distributed batch mode; the v0 baseline is
  single-host, multi-core via `rayon`.

## Troubleshooting

For triage decision trees and known incidents, see
[`docs/RUNBOOK.md`](docs/RUNBOOK.md). The most common build-time and
runtime failures, in summary:

| Symptom | Likely cause | First-action fix |
|---|---|---|
| `error: failed to run custom build command for webp` | host missing `libwebp-dev` | install via system package manager (`apt install libwebp-dev`, `pacman -S libwebp`, `brew install webp`) |
| `not a directory: <path>` (exit 1) | wrong arg, or `GALLERY_BASE` not set for a bare argument | pass an absolute path or set `GALLERY_BASE` |
| `cannot read <path>: <errno>` (exit 1) | permission denied on the input directory | `chmod` / `chown` |
| per-file `<file>: <decode error>` (exit 1) | corrupt JPEG / non-JPEG with `.jpg` extension | fix the source file; failed files are counted and surfaced via stderr |
| exit `2` with `invalid --report-fd` | `--report-fd 1` (forbidden) or non-writable fd | use `0` / `2` / an open writable fd, or omit the flag |

## Development

### Repository Layout

```
Cargo.toml                   # workspace manifest (image, webp, rayon, libc, serde_json[dev])
Cargo.lock                   # pinned dependency graph
build.rs                     # bakes CONVERT_TO_WEBP_BUILD_COMMIT_SHA + libwebp_version
src/
  main.rs                    # argv parser, usage printer, mode dispatcher, batch + single-file loops
  format.rs                  # Codec trait, ResizePolicy, per-codec impls, Format enum (jpg/jpeg/png/webp)
  params.rs                  # Params { quality, resize } — defaults to v0 baseline
  report.rs                  # Report struct, hand-rolled NDJSON encoder (schema_version 1)
  bin/gallery-compress.rs    # legacy forwarder to convert-to-webp
tests/
  golden_v0.rs               # golden-batch regression (10 fixtures; libwebp 1.6.0 golden)
  single_file.rs             # --single-file mode integration (success, failure, byte-equivalence)
  json_output.rs             # --json / --report-fd integration (round-trip, parse, fd validation)
  fixtures/golden_v0/        # 10 JPG fixtures + 10 WebP golden outputs (re-recordable)
docs/
  architecture.md            # C4-style architecture overview
  architecture/STATUS.md     # architect meta-document (goals, trade-offs, gating P0 queue)
  ROADMAP.md                 # active wave + planned waves
  RUNBOOK.md                 # operator runbook (triage, build failures, incidents)
  adr/                       # decision log
  components/                # per-component contracts (cli-frontend, converter-core, format-codecs)
  contracts/                 # cross-component contracts (codec-bounds, report-shape)
  integration-contract.md    # outward-facing consumer contract (server-side callers)
```

### Build Variants

```bash
cargo build                  # debug build (slower, larger binary)
cargo build --release        # release build (~2.4 MiB stripped on the reference host)
cargo test                   # full suite: unit tests + 3 integration suites
cargo test --release         # release-mode test run (matches CI)
cargo test --test golden_v0  # golden-batch only (regression ground truth)
cargo test --test single_file
cargo test --test json_output
```

## Testing

The repository has a unit-test module inside `src/main.rs` (CLI
parsing invariants) and three integration suites that spawn the
binary and assert on its behaviour as a black box.

| Suite | File | What it asserts |
|---|---|---|
| `golden_v0` | `tests/golden_v0.rs` | The default pipeline (no flags) produces byte-equivalent output to `tests/fixtures/golden_v0/expected/*.webp` within 0.1 % tolerance (`BYTE_TOLERANCE`), per ADR-0002 |
| `single_file` | `tests/single_file.rs` | `--single-file` mode reads stdin, encodes, writes stdout, emits the v0 key=value metadata on stderr, and round-trips byte-identically with the batch mode reference |
| `json_output` | `tests/json_output.rs` | `--json` mode emits one parseable NDJSON record per file with the documented `schema_version: 1` shape; `--report-fd` validation accepts `2`/writable fds and rejects `1`/non-writable fds |

Run the full suite:

```bash
cargo test --release
```

Run a focused suite:

```bash
cargo test --release --test golden_v0
cargo test --release --test single_file
cargo test --release --test json_output
```

### Re-recording the Golden Batch

If a host `libwebp` ABI change pushes the byte-equivalence drift
above 0.1 % (`BYTE_TOLERANCE` in `tests/golden_v0.rs`), the golden
must be re-recorded:

```bash
tmp=$(mktemp -d)
cp tests/fixtures/golden_v0/*.jpg "$tmp/"
./target/release/convert-to-webp "$tmp"
cp "$tmp"/*.webp tests/fixtures/golden_v0/expected/
rm -rf "$tmp"
```

Then update `GOLDEN_LIBWEBP_VERSION` in `tests/golden_v0.rs` to
your host's `pkg-config --modversion libwebp`, run
`cargo test --test golden_v0`, and commit both the golden files
and the version bump in one commit. The full procedure lives at
[`tests/fixtures/golden_v0/expected/README.md`](tests/fixtures/golden_v0/expected/README.md).

## Release and Versioning

- **Version anchor**: `Cargo.toml` `version` field (currently
  `0.2.0`). The CLI flag surface is additive-only within a major
  version; removing a flag is a breaking change.
- **`schema_version`**: bumped in
  [`src/report.rs`](src/report.rs) (`SCHEMA_VERSION`). Bumping the
  JSON schema is a coordinated breaking change requiring an ADR
  under [`docs/adr/`](docs/adr/) and a coordinated bump in any
  external consumer (e.g. the Symfony `BinaryConverter`). See
  [`docs/contracts/report-shape.md`](docs/contracts/report-shape.md) § 7.
- **Exit-code contract**: frozen at three codes (`0` / `1` / `2`)
  for the lifetime of major version `1`.
- **Release tag**: `git tag v0.2.0` after `cargo build --release`
  passes and the full suite is green. SHA-pinned builds carry the
  recorded `host.build_commit_sha` in every JSON record.

## License

This project is licensed under the **MIT License** (declared in
[`Cargo.toml`](Cargo.toml) `license` field). A copy of the license
text is conventionally distributed alongside the source tree as
`LICENSE`; if it is missing in your checkout, add it from the
canonical MIT template.

## Documentation Map

| Document | Audience | Purpose |
|---|---|---|
| `README.md` (this file) | operator, contributor | entry point: prereqs, build, quick start, CLI surface, limitations, troubleshooting, testing, release |
| [`docs/integration-contract.md`](docs/integration-contract.md) | external consumer (server-side wrapper, CI) | outward-facing contract: stdin/stdout, JSON shape, exit codes, latency budget, failure-mode mapping |
| [`docs/architecture.md`](docs/architecture.md) | architect, contributor | C4-style architecture overview (context, container, components, contracts) |
| [`docs/architecture/STATUS.md`](docs/architecture/STATUS.md) | architect, maintainer | goals, quality properties, captured trade-offs, architect cycles, gating P0 queue |
| [`docs/ROADMAP.md`](docs/ROADMAP.md) | architect, contributor | active wave + planned waves + cross-cutting track anchors |
| [`docs/RUNBOOK.md`](docs/RUNBOOK.md) | operator (on call) | first-30-minutes triage, build-time failures, runtime incidents, resolved defects |
| [`docs/components/cli-frontend.md`](docs/components/cli-frontend.md) | developer | per-component contract for argv / env / usage / exit-code mapping |
| [`docs/components/converter-core.md`](docs/components/converter-core.md) | developer | per-component contract for directory walk + parallel dispatcher + per-file orchestrator |
| [`docs/components/format-codecs.md`](docs/components/format-codecs.md) | developer | per-component contract for the codec layer (jpg / png / webp) |
| [`docs/contracts/codec-bounds.md`](docs/contracts/codec-bounds.md) | developer, tester | codec ↔ converter-core contract: `CodecError` variants, byte-count invariants, source-removal rules |
| [`docs/contracts/report-shape.md`](docs/contracts/report-shape.md) | developer, tester, external consumer | NDJSON wire shape for `--json` mode; `schema_version`, `--report-fd`, error taxonomy |
| [`docs/adr/0001-multi-format-cli-scope.md`](docs/adr/0001-multi-format-cli-scope.md) | contributor | scope decision: extend `gallery-compress` to a generic multi-format converter |
| [`docs/adr/0002-preserve-jpg-to-webp-baseline.md`](docs/adr/0002-preserve-jpg-to-webp-baseline.md) | contributor | backward-compat decision: keep the v0 default pipeline byte-equivalent |

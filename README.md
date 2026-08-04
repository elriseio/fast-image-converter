# convert-to-webp

Minimal standalone project for the `convert-to-webp` workflow.

Single Rust binary that batch-converts a directory of images
between formats. The default pipeline (no flags) walks a directory
of `.jpg` files and converts each to `.webp` using the system
`libwebp` encoder. Per-orientation size policy matches the
bash+ImageMagick original:

| orientation | rule | quality |
|---|---|---|
| portrait  (h ≥ w) | resize to `800x>` (max width 800, height proportional) | 85 |
| landscape (w > h) | resize to `1000x>` (max width 1000, height proportional) | 85 |
| square    (h == w) | treated as portrait (`800x>`) | 85 |

The source file is removed after a successful conversion. The
script prints a one-line summary of converted bytes.

## Why Rust (vs bash + ImageMagick)

On a 50-file batch of mixed-orientation JPGs (3 MB total) the speedup
on this host (12 cores, libwebp 1.6.0) is:

| engine                  | wall time | user time | sys time |
|---|---|---|---|
| bash + ImageMagick 7.1  | 10.09 s   | 8.60 s    | 2.75 s   |
| Rust + rayon + libwebp  |  1.18 s   | 7.20 s    | 0.77 s   |

~8.5x faster wall time. The CPU work itself is similar; the win comes
from eliminating ~100 process spawns (`identify` + `magick` per image)
and parallelising across cores with `rayon`. Output bytes match within
0.1% (same `libwebp` encoder under the hood).

## Requirements

- Rust toolchain (1.75+; tested with 1.97)
- `libwebp` development files (e.g. `libwebp-dev` on Debian/Ubuntu,
  `libwebp` on Arch, `webp` on Homebrew). At build time `pkg-config`
  must find `libwebp`.
- Standard build toolchain (`cc`, `pkg-config`).

## Build

```bash
cargo build --release
```

Resulting binary: `target/release/convert-to-webp` (~1.6 MB stripped).
The legacy `gallery-compress` name is retained as a backward-
compatible alias; see "Binary rename" below.

## Usage

```bash
# default pipeline (jpg -> webp, no flags)
./target/release/convert-to-webp /tmp/my-images

# by year, against the default GALLERY_BASE
./target/release/convert-to-webp 2025

# explicit format pair
./target/release/convert-to-webp /tmp/my-images --input-format png --output-format webp
./target/release/convert-to-webp /tmp/my-images --input-format webp --output-format png
./target/release/convert-to-webp /tmp/my-images --input-format webp --output-format jpg

# tune quality / resize / keep-source
./target/release/convert-to-webp /tmp/my-images --quality 75 --resize cap=1024
./target/release/convert-to-webp /tmp/my-images --resize auto:portrait=800,landscape=1000
./target/release/convert-to-webp /tmp/my-images --keep-source

# single-file stdin/stdout mode (DE-004)
cat /tmp/my-images/01.jpg | \
  ./target/release/convert-to-webp --single-file --output-format webp \
  > /tmp/01.webp
cat /tmp/01.webp | \
  ./target/release/convert-to-webp --single-file --output-format png \
  > /tmp/01.png

# structured JSON output (DE-005): single-file mode
cat /tmp/my-images/01.jpg | \
  ./target/release/convert-to-webp --single-file --output-format webp --json \
  > /tmp/01.webp 2> /tmp/report.jsonl

# structured JSON output (DE-005): batch mode
./target/release/convert-to-webp --json /tmp/my-images 2> /tmp/batch.jsonl

# help
./target/release/convert-to-webp --help
```

## Flags

| Flag | Value | Default | Meaning |
|---|---|---|---|
| `--input-format <fmt>` | `jpg`, `png`, `webp` (case-insensitive) | `jpg` | input format (files with this extension are processed) |
| `--output-format <fmt>` | `jpg`, `png`, `webp` (case-insensitive) | `webp` | output format |
| `--quality <n>` | integer in `1..100` | `85` | encode quality (honoured by WebP and JPEG outputs; PNG output is lossless and ignores it) |
| `--resize <policy>` | `none` \| `cap=<W>` \| `auto:portrait=<W>,landscape=<H>` | `auto:portrait=800,landscape=1000` | resize policy applied before encoding |
| `--keep-source` | boolean flag | `false` | leave the source file in place after a successful conversion (v0 baseline removes it; **batch mode only**, silently ignored in `--single-file`) |
| `--single-file`, `-1` | boolean flag | `false` | read one image from stdin, encode it, write the encoded bytes to stdout. The encoded bytes are the only thing on stdout; a single-line key=value record on stderr carries the per-file metadata |
| `--json` | boolean flag | `false` | emit the per-file metadata as a structured NDJSON record (schema_version 1) instead of the v0 key=value line. See "Structured JSON output (DE-005)" below |
| `--report-fd <N>` | integer fd | `2` (stderr) | override the report stream. `N=1` is forbidden (would collide with the encoded bytes in single-file mode); non-writable fds rejected with usage + exit 2 |
| `-h`, `--help` | — | — | print usage to stderr and exit 2 |

### Single-file mode (DE-004)

The `--single-file` flag switches the binary from batch (directory)
mode to single-file mode:

- **Input**: image bytes on stdin (raw bytes; no header / framing).
- **Output**: encoded image bytes on stdout (raw bytes; no header /
  framing).
- **Per-file metadata**: a single line on stderr in the
  documented key=value shape:

  ```
  status=<ok|err> in_bytes=<N> out_bytes=<N> duration_ms=<N> error=<message>
  ```

  `error=` is omitted on success and present (with the codec-
  reported message) on failure. The shape is intentionally simple
  in v0 and is superseded by `--json` (DE-005) for callers that
  need structured fields.
- **`--keep-source`**: silently ignored (no source filesystem path
  to preserve; stdin has no metadata to remove).
- **Exit-code contract**: preserved (`0` / `1` / `2`). `2` for
  wrong invocation (e.g. `--single-file` + a positional argument,
  or same input/output format); `1` for runtime failure; `0` for
  success.

The single-file mode is the integration surface for server-side
callers (Symfony, etc.) that hold the image as an in-memory
buffer rather than a filesystem path. The output bytes are
byte-identical to the same conversion via the directory mode (the
golden-batch regression test under `tests/single_file.rs` enforces
this).

The default pair `--input-format jpg --output-format webp` is the v0
`gallery-compress` pipeline preserved bit-for-bit (see ADR-0002).

### Structured JSON output (DE-005)

The `--json` flag switches the per-file metadata line from the v0
key=value shape to a structured NDJSON record (one JSON object per
line). The shape is stable, documented in
`docs/contracts/report-shape.md`, and versioned via the
`schema_version` field (= 1; bumping is a coordinated breaking
change).

The flag is orthogonal to the directory / single-file mode: both
modes can use `--json`.

#### Schema (v1)

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
  },
  "output": {
    "format": "webp" | "png" | "jpeg",
    "bytes": 6789,
    "width": 1920,
    "height": 1080
  },
  "codec": {
    "quality": 85,
    "resize_policy": "auto:portrait=800,landscape=1000"
  },
  "host": {
    "libwebp_version": "1.6.0",
    "build_commit_sha": "<git rev-parse HEAD>"
  },
  "duration_ms": 42,
  "error": null | { "kind": "decode" | "encode" | "io", "message": "..." }
}
```

`input` / `output` are `null` when no bytes were consumed or
produced (pre-decode io failure, decode failure); `width` /
`height` are `null` when the decode did not produce a
`DynamicImage`. `host.build_commit_sha` is `null` on builds
without git context (release tarballs, missing `.git/`).

#### Examples

Single-file mode — success path:

```bash
cat tests/fixtures/golden_v0/portrait_256x384.jpg | \
  ./target/release/convert-to-webp --single-file --json \
  > /tmp/out.webp 2> /tmp/report.jsonl
# /tmp/report.jsonl contains:
{"schema_version":1,"mode":"single_file","status":"ok","input":{"format":"jpeg","bytes":43058,"width":256,"height":384},"output":{"format":"webp","bytes":43694,"width":256,"height":384},"codec":{"quality":85,"resize_policy":"auto:portrait=800,landscape=1000"},"host":{"libwebp_version":"1.6.0","build_commit_sha":"<sha>"},"duration_ms":12,"error":null}
```

Single-file mode — failure path (non-image bytes on stdin):

```bash
head -c 1024 /dev/urandom | \
  ./target/release/convert-to-webp --single-file --json \
  > /dev/null 2> /tmp/fail.jsonl
# /tmp/fail.jsonl contains:
{"schema_version":1,"mode":"single_file","status":"err","input":{"format":"jpeg","bytes":1024,"width":null,"height":null},"output":null,"codec":{"quality":85,"resize_policy":"auto:portrait=800,landscape=1000"},"host":{"libwebp_version":"1.6.0","build_commit_sha":"<sha>"},"duration_ms":0,"error":{"kind":"decode","message":"The image format could not be determined"}}
```

Batch mode — one JSON line per candidate (NDJSON; the lines
are independent, no enclosing array):

```bash
./target/release/convert-to-webp --json /tmp/my-images 2> /tmp/batch.jsonl
wc -l /tmp/batch.jsonl                            # one line per candidate
jq -c '.status' /tmp/batch.jsonl | sort | uniq -c
# Note: the v0 trailer '(processed N candidates, K failed)' lands
# on the same report stream; filter parseable lines with
# `grep '^{' /tmp/batch.jsonl | jq -c .` if your consumer cannot
# tolerate the trailer line.
```

#### `--report-fd <N>` override

The report stream defaults to fd 2 (stderr). Override with
`--report-fd <N>`:

- `N == 2` (stderr): accepted without further checks; this is
  the default.
- `N == 0` (stdin): accepted only if it is open for writing
  (rare; validated via `fcntl(F_GETFL)`).
- `N == 1` (stdout): **forbidden** regardless of access mode —
  in single-file mode stdout carries the encoded bytes and the
  report stream would collide with the payload. The binary
  emits a usage message and exits 2.
- Any other value: the access mode is queried via
  `fcntl(F_GETFL)`; read-only fds are rejected with usage +
  exit 2.

```bash
# Pipe the JSON report to a separate file (no stderr capture).
./target/release/convert-to-webp --single-file --json --report-fd 3 \
  3> /tmp/report.jsonl > /tmp/out.webp < input.jpg
```

#### Compatibility

- Without `--json`, the v0 / DE-004 behaviour is preserved:
  single-file mode emits a single key=value line on stderr;
  batch mode emits no per-file metadata on stderr.
- Stdout is not polluted: in single-file mode, stdout contains
  only the encoded bytes.
- Exit-code contract is preserved: `0` / `1` / `2` only.

## Environment

| Var | Default | Meaning |
|---|---|---|
| `GALLERY_BASE` | `<vfatina-home>/public/images/gallery` | Base directory used when the argument is a bare year (e.g. `2025`). Override for other projects. |

## Exit codes

| code | meaning |
|---|---|
| `0` | success (or no candidates found) |
| `1` | runtime error (bad directory, decode/encode failure, ≥1 file failed) |
| `2` | wrong CLI invocation (arg count, unknown flag, bad `--input-format` / `--output-format`, same input/output format) |

## Binary rename

The canonical binary is `convert-to-webp` (matching the project slug).
The legacy v0 name `gallery-compress` is retained as a thin
forwarder that prints a one-time deprecation hint on stderr and
forwards the arguments to the new binary. The deprecation hint
mentions the new name; the legacy name is kept for the next major
version at minimum.

## Layout

```
convert-to-webp/
├── Cargo.toml
├── Cargo.lock
├── README.md
├── .gitignore
├── build.rs                         # DE-005 commit 6: bakes build_commit_sha + libwebp_version
├── docs/
│   ├── architecture.md
│   ├── architecture/STATUS.md
│   ├── ROADMAP.md
│   ├── RUNBOOK.md
│   ├── adr/
│   ├── components/
│   └── contracts/
├── src/
│   ├── main.rs                      # CLI + report emit + report-fd validation
│   ├── params.rs                    # Params + parse_resize
│   ├── format.rs                    # Codec trait + Jpeg/Png/Webp codecs
│   ├── report.rs                    # DE-005 commit 1: Report struct + JSON encoder
│   └── bin/
│       └── gallery-compress.rs      # legacy forwarder
└── tests/
    ├── golden_v0.rs                 # DE-002: byte-equivalence regression
    ├── single_file.rs               # DE-004: single-file mode integration
    ├── json_output.rs               # DE-005: --json / --report-fd integration
    └── fixtures/golden_v0/
```

## See also

- `docs/architecture.md` — system architecture overview.
- `docs/architecture/STATUS.md` — architect status meta-document.
- `docs/ROADMAP.md` — active wave + planned waves.
- `docs/RUNBOOK.md` — operator runbook.
- `docs/adr/0001-multi-format-cli-scope.md` — scope decision.
- `docs/adr/0002-preserve-jpg-to-webp-baseline.md` — backward-compat
  baseline decision.
- `docs/components/format-codecs.md` — codec layer specification.
- `docs/components/converter-core.md` — orchestrator specification.
- `docs/components/cli-frontend.md` — CLI layer specification.
- `docs/contracts/codec-bounds.md` — codec ↔ converter-core contract.
- `docs/contracts/report-shape.md` — `--json` NDJSON wire contract
  (DE-005; schema_version 1).

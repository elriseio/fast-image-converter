---
project_slug: fast-image-converter
doc_slug: integration_contract
doc_type: integration_contract
applicable_roles: [architect, developer, external_consumer]
version: 1
summary: "Outward-facing integration contract for the fast-image-converter CLI. Documents the single-file stdin/stdout mode, the structured JSON report, the exit-code contract, and the per-flag surface that any external consumer (shell script, CI pipeline, server-side wrapper) can rely on. This contract is the library-side complement to per-consumer integration projects (which live outside this repository)."
source_artifacts:
  - docs/components/cli-frontend.md
  - docs/components/converter-core.md
  - docs/components/format-codecs.md
  - docs/contracts/codec-bounds.md
  - Issues/open/developer/DE-004_add_single_file_stdin_stdout_mode.md
  - Issues/open/developer/DE-005_add_structured_json_output_mode.md
tags: [integration, contract, external-consumer, stdin, stdout, json]
---

# Integration Contract (outward-facing)

> **Audience**: external consumers of the `fast-image-converter` binary —
> shell scripts, CI pipelines, server-side wrappers, build tools,
> containerised batch jobs. Not intended for in-process (library)
> callers; for that, a separate ADR is needed to expose a Rust
> library API.
>
> **Scope of this document**: the binary's public invocation
> surface (argv, env, stdio, exit codes, structured output). Per-
> consumer integration projects (PHP+Symfony, Node.js, Go, etc.)
> are out of scope for this repository; they belong to their own
> project trees.

## 1. Two Modes

The binary exposes two mutually-exclusive invocation modes:

| Mode | Trigger | Input | Output |
|---|---|---|---|
| `batch` | positional `<dir>` argument | a directory of candidate files | the encoded files written into the input directory (v0 behaviour: source removed) + a summary line on stdout |
| `single_file` | `--single-file` flag | raw image bytes on stdin | raw encoded bytes on stdout + a metadata line on stderr |

The mode is determined at argv parse time; mixing the two
invocations (e.g. `--single-file` + positional dir) is a usage
error (exit `2`).

## 2. Flag Surface

| Flag | Type | Applies to | Default |
|---|---|---|---|
| `--input-format <fmt>` | enum (`jpg` \| `png` \| `webp`, Wave 1) | both modes | inferred from extension (batch) or content sniffing (single-file) |
| `--output-format <fmt>` | enum (`jpg` \| `png` \| `webp`, Wave 1) | both modes | `webp` |
| `--quality <1..100>` | integer | both modes | `85` |
| `--resize <policy>` | enum (`none` \| `cap=<W>` \| `auto:portrait=<W>,landscape=<H>` \| `fit=<mode> long-edge=<N>`) | both modes | `auto:portrait=800,landscape=1000` (v0 baseline) |
| `--keep-source` | boolean flag | batch mode only | `false` (source removed on success) |
| `--single-file` | boolean flag | switches mode | `false` (batch) |
| `--json` | boolean flag | both modes | `false` (legacy text metadata) |
| `--report-fd <N>` | integer fd | both modes | `2` (stderr) |

The `--keep-source` flag is silently ignored in single-file mode
because stdin has no associated filesystem metadata to remove.

### 2.1. `--resize` 3-arg form

`fit=<mode> long-edge=<N>` is a 3-arg shape that supports the
elrise.io page-side advanced panel's three fit semantics
(the elrise.io side is DE-031; the
Go backend already wires `X-Resize-Mode` / `X-Resize-Max-Long-Edge`
and constructs the subprocess invocation as
`--resize fit=<mode> long-edge=<N>`).

| `mode` | Output dimensions | Resize operation |
|---|---|---|
| `contain` | longest side = `N`, other side proportional | `image::DynamicImage::resize` (aspect-preserving, no crop, no pad) |
| `cover`   | exactly `N × N` (square), centre-cropped | `image::DynamicImage::resize_to_fill` (aspect-preserving scale + centre crop) |
| `stretch` | exactly `N × N` (square, ignoring aspect) | `image::DynamicImage::resize_exact` (distorts to target) |

`<N>` must satisfy `1 ≤ N ≤ 20000` (mirrors the Go backend's
`parseResize` validation; the upper bound keeps the result well
below `MAX_DIMENSION` so the resize path can never overflow
`width * height`).

The 3-arg shape is only consumed when the first token after
`--resize` starts with `fit=`. The legacy 1-arg shapes
(`none`, `cap=<W>`, `auto:portrait=<W>,landscape=<H>`) keep their
original semantics; the parser branches on
the prefix and consumes either 1 or 2 additional positional
tokens depending on the first token.

The JSON `codec.resize_policy` field carries the round-trippable
form: `fit=cover long-edge=512`,
`fit=contain long-edge=1024`, etc.

## 3. Exit-Code Contract

| Code | Meaning | When |
|---|---|---|
| `0` | success (including the no-candidates case in batch mode) | every file succeeded |
| `1` | runtime failure | bad directory, decode / encode failure, ≥ 1 file failed |
| `2` | wrong invocation | arg count / unknown flag / bad enum value / mode-arg ambiguity |

The contract is **fixed at three codes**. New error classes in
future waves must be expressible as one of these three; introducing
a fourth code is a breaking change.

## 4. Stdout / Stderr Contract

### 4.1 Batch mode

- **stdout**: a single summary line
  `<BINARY>: <N> files in <DIR>: <IN_BYTES> -> <OUT_BYTES>`
  followed by a stderr trailer
  `(processed <N> candidates, <K> failed)`. Per-file errors are
  reported individually on stderr as `<file>: <error>`.
- **stderr**: per-file error lines + the trailer + (if `--json`)
  one NDJSON line per candidate.

### 4.2 Single-file mode (the integration-mode default)

- **stdout**: raw encoded bytes only. **Nothing else.** No log
  lines, no summary, no warning. Consumers MUST be able to
  redirect stdout straight to a file or socket.
- **stderr**: a single metadata line. Two shapes:

  **Without `--json`** (legacy key=value; v0 of the metadata
  shape):

  ```
  status=<ok|err> in_bytes=<N> out_bytes=<N> duration_ms=<N> error=<message>
  ```

  `error=` is omitted on success and present (with the codec-
  reported message) on failure.

  **With `--json`** (structured, versioned; supersedes the legacy
  shape):

  ```json
  {
    "schema_version": 1,
    "mode": "single_file" | "batch",
    "status": "ok" | "err",
    "input":  { "format": "...", "bytes": N, "width": W, "height": H },
    "output": { "format": "...", "bytes": N, "width": W, "height": H },
    "codec":  { "quality": N, "resize_policy": "..." },
    "host":   { "libwebp_version": "...", "build_commit_sha": "..." },
    "duration_ms": N,
    "error":  null | { "kind": "decode|encode|io", "message": "..." }
  }
  ```

  Consumers MUST ignore unknown fields and MUST NOT require
  `schema_version` to remain at `1` indefinitely — they SHOULD
  branch on the field and degrade gracefully on bump.

- **`--report-fd <N>`**: redirects the report stream (stderr by
  default) to fd `<N>`. `N=1` is rejected because it would collide
  with the encoded bytes in single-file mode. Fds other than `1`,
  `2`, or a writable integer fd are rejected as usage errors
  (exit `2`).

## 5. Latency Budget

The single-file mode is targeted at per-request invocation by a
server-side wrapper. Latency budget (per `architecture.md` § 7):

| Input size | Target wall-time |
|---|---|
| 1 MiB JPG | < 50 ms |
| 5 MiB JPG | < 200 ms |
| 20 MiB JPG | < 1 s |

These are best-effort targets on a 12-core host with hot caches.
Consumers MUST set a process timeout ≥ 30 s and SHOULD set one
proportional to the input size (≈ 50 ms / MiB + 50 ms base).

## 6. Output-Fidelity Contract

The WebP output bytes are within **0.1 %** of the `libwebp`
reference encoder for the same quality parameter, on the same
host `libwebp` version. This is enforced by the golden-batch
regression test (`tests/golden_v0.rs`) per ADR-0002.

Cross-`libwebp`-version drift is documented but not enforced.
Consumers SHOULD record the `host.libwebp_version` field from the
JSON report and pin it in their deployment manifest.

## 7. Process Spawn Cost

Each invocation of the binary has a fixed overhead of ≈ 10–50 ms
(process spawn + crate initialisation) before any encoding work
begins. Consumers SHOULD batch multiple inputs inside one
invocation when latency is critical; the single-file mode is for
the opposite case (one input, predictable per-request cost).

## 8. Failure-Mode Mapping (consumer side)

| Binary exit | Consumer action |
|---|---|
| `0` | treat as success; consume stdout (single-file) or directory (batch) |
| `1` | surface as a structured error using the `error` block from the JSON report (or the `error=` key=value in legacy mode); do **not** echo stderr to the user |
| `2` | surface as a programming error (misconfiguration); full stderr is useful for the developer and SHOULD be logged at WARN level |

Stderr MUST NOT be included in any user-facing response body. It
is for log emission only.

## 9. Versioning

- The CLI flag surface is **additive-only** within a major
  version. Removing a flag is a breaking change.
- The JSON `schema_version` is bumped on any **backwards-
  incompatible** change to field names, types, or semantics.
  Adding optional fields is a minor bump.
- The exit-code contract is **frozen at three codes** for the
  lifetime of major version `1`. A fourth code is a breaking
  change requiring a major version bump and a migration guide.

## 10. Out-of-Scope (explicit non-goals for the contract)

- In-process library API (Rust crate consumer). Gated on a
  follow-up ADR.
- HTTP / daemon mode. The contract assumes per-invocation
  process spawn.
- Authentication / authorisation. The binary is unauthenticated;
  consumers are responsible for any access control in front of
  it.
- Telemetry / metrics emission from the binary itself.
- Distributed batch execution across hosts.
- Streamed progress events during a long encode.

## 11. References

- `docs/components/cli-frontend.md` — per-component contract.
- `docs/components/converter-core.md` — orchestration contract.
- `docs/components/format-codecs.md` — codec surface.
- `docs/contracts/codec-bounds.md` — codec ↔ converter-core
  contract.
- `docs/contracts/report-shape.md` — `--json` wire shape.
- `docs/ROADMAP.md` — wave plan for the flag surface and
  `--json` shape.
- `docs/adr/0002-preserve-jpg-to-webp-baseline.md` — fidelity
  contract.
- `docs/RUNBOOK.md` — incident handling.
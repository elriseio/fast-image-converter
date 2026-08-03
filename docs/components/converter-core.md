---
project_slug: convert-to-webp
doc_slug: component_converter_core
doc_type: component_doc
applicable_roles: [architect, developer, tester]
version: 1
source_artifacts:
  - src/main.rs:39-99, 101-136
  - README.md "Why Rust" benchmark
summary: "Converter-core component: directory walk, parallel dispatch, per-file orchestrator, summary printer."
tags: [component, orchestration, rayon, parallel, dispatcher]
---

# Component: `converter-core`

## 1. Purpose

Take a target directory and a codec selection (from
`format-codecs`), walk the directory for candidates matching the
codec's accepted input extensions, dispatch per-file conversion
in parallel via `rayon`, aggregate the result, and emit the
stdout summary line.

## 2. Inputs

- From `cli-frontend`: resolved directory path + codec selection.
- From `format-codecs`: the chosen `Codec` instance.

## 3. Outputs

- For each candidate file: a `(src_bytes, dst_bytes, error?)`
  triple.
- Aggregate: total candidate count, success count, failure count,
  total source bytes, total output bytes.
- Stdout summary line (emitted by `cli-frontend` after
  `converter-core` returns).
- Per-file error lines on stderr (emitted as errors are reported
  by `rayon::par_iter`).

## 4. Invariants

- **INV-CC-1**: the set of candidate files is the set of regular
  files in the resolved directory whose extension (case-insensitive)
  matches the codec's accepted input extensions.
- **INV-CC-2**: the directory walk is non-recursive in v0 (matches
  v0 `read_dir` semantics). Recursion is gated on Wave 2+ (`-r`
  flag, off by default).
- **INV-CC-3**: per-file work is dispatched via
  `rayon::par_iter().map(...)` — order of completion is
  non-deterministic; the summary aggregate is order-insensitive.
- **INV-CC-4**: per-file errors are reported individually on
  stderr; the aggregate failure count drives the exit code (via
  `cli-frontend`).
- **INV-CC-5**: the success path of a single file (decode →
  resize → encode → write → remove source) is atomic per file. A
  panic in one worker does not affect other workers (rayon's
  default panic handling; failures are caught and reported).

## 5. Failure Modes

| Mode | Trigger | Behaviour |
|---|---|---|
| Empty directory | 0 candidates | print `no .jpg files in <dir>` to stderr (v0); exit `0` |
| Per-file decode failure | codec returns `Err(Decode)` | print `<file>: <error>` to stderr; count as failure; continue with other files |
| Per-file encode failure | codec returns `Err(Encode)` | same as decode failure |
| Per-file I/O failure | source not readable / dst not writable | same as decode failure |
| Output write success, source removal failure | `fs::remove_file` fails | print `<file>: <error>` to stderr; count as failure (the converted `.webp` is left on disk; operator can clean up) |
| rayon thread panic | uncaught panic inside the codec | v0 behaviour: panics propagate; Wave 1+ planned behaviour: catch, count as failure, continue |

## 6. Performance Budget

| Metric | Target | Notes |
|---|---|---|
| Wall-time on 50 mixed-orientation JPGs (3 MB total) | < 2 s on 12 cores | v0 measured: 1.18 s |
| Throughput on large batches (≥ 100 files) | linear in file count, plateau at `nproc × per-file-cost` | bounded by `rayon` thread pool default (nproc) |
| Memory peak per worker | < 256 MiB (8K RGBA decoded) | see `format-codecs.md` § Memory budget |
| Summary overhead | < 50 ms | negligible vs file work |

## 7. Concurrency Contract

- `rayon::ThreadPool` default (one global pool sized to `nproc`).
- No shared mutable state across workers except via the
  `Vec<Result<...>>` collector (rayon `collect()`).
- No locks taken on the source tree during the parallel phase
  (the source tree is read-only; the output tree is written
  per-file).

## 8. Future Work

- Wave 1: plumb codec selection (single codec per invocation in
  Wave 1).
- Wave 2: add `--jobs <N>` flag.
- Wave 4: add `--dry-run` (no writes, just report).

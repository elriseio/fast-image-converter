---
project_slug: fast-image-converter
doc_slug: architect_status
doc_type: architecture_meta
applicable_roles: [architect, developer, fixer, code_researcher]
version: 1
source_artifacts:
  - docs/architecture.md
  - docs/ROADMAP.md
  - docs/RUNBOOK.md
  - src/main.rs (v0 baseline reference)
summary: "Architect meta-document for fast-image-converter. Captures goals, key properties, captured trade-offs, architect cycles, last-updated."
tags: [meta, architect, status, fast-image-converter]
---

# Architect Status (fast-image-converter)

> **Last Updated**: 2026-08-04 — accepted ADR-0003 to rename the product
> and canonical CLI to `fast-image-converter`; runtime, release, and
> documentation rename tasks are queued.

## 1. Goals

### 1.1 Business Goals

- **G1**: Provide a single binary that handles bulk image-format
  conversion for operator-driven asset pipelines (web galleries,
  photo archives, marketing-asset batches).
- **G2**: Preserve the v0 baseline behaviour (JPG → WebP with the
  fixed portrait/landscape resize policy and `quality=85`) as the
  default — any existing operator script keeps working unchanged.
- **G3**: Extend the binary to a generic converter supporting
  configurable input / output formats without re-spawning a separate
  tool per format.

### 1.2 Key Quality Properties

- **QP1 — Wall-time**: < 2 s on a 50-file / 3 MB JPG batch on a
  12-core host (current v0 baseline: 1.18 s).
- **QP2 — Output fidelity**: WebP output bytes within 0.1 % of the
  `libwebp` reference encoder for the same quality parameter.
- **QP3 — Predictability**: identical input + identical flags
  produce byte-identical output across runs (no timestamps, no
  non-deterministic ordering).
- **QP4 — Operator ergonomics**: zero-config invocation for the
  default pipeline; flags are surfaced only when the operator asks
  for them.
- **QP5 — Audit**: per-file failures are reported on stderr with
  path + error; the summary line on stdout is single-line and
  grep-friendly.

## 2. Key Properties (Project Size / Phase)

| Property | Value |
|---|---|
| Service count | 1 (single Rust binary) |
| Source-of-truth files | `Cargo.toml`, `src/main.rs` |
| Lines of code (v0) | ~164 (one file) |
| Contributors (intended) | 1 (operator, primary) + architect + developer on demand |
| External services | 0 |
| External dependencies | 3 crates (`image`, `webp`, `rayon`) + host `libwebp` |
| Phase | **Exploration** — scope expansion in progress |
| Risk class | Low (no network, no credentials, no user data) |

## 3. Captured Trade-offs

| Trade-off | Resolution | Reference |
|---|---|---|
| WebP-only output vs multi-format output | Multi-format, with WebP as default | `adr/0001-multi-format-cli-scope.md` |
| Single-file CLI vs library + CLI | CLI only (library API out of scope) | `architecture.md` § 1 Purpose |
| ImageMagick pipeline vs native libwebp via `image`/`webp` crates | Native (8.5× wall-time win observed in v0 baseline) | `README.md` "Why Rust" |
| Per-orientation resize policy vs always-uniform policy | Keep v0 per-orientation as the default; allow override | `adr/0002-preserve-jpg-to-webp-baseline.md` |
| Product identity: `fast-image-converter` vs `fast-image-converter` | Adopt `fast-image-converter` as canonical product and CLI name; retain `fast-image-converter` and `gallery-compress` as compatibility aliases for at least one major version | `adr/0003-fast-image-converter-product-name.md` |
| Hard-coded `DEFAULT_GALLERY_BASE` vs env-only vs CLI-only | Env-only with explicit "must-be-set" error for bare args. Resolved by `DE-006`; no host-specific default remains. | `RUNBOOK.md` § RD-001 |

## 4. Architect Cycles

| Cadence | Activity | Output |
|---|---|---|
| On every issue closure | Sync `docs/architecture.md` + `architecture/STATUS.md` with the actual system state | doc-update record |
| On every ADR | Author the ADR, link from `architecture.md` § 8 | new file under `docs/adr/` |
| On every wave close | Update `ROADMAP.md` with closed-wave entry, move planned waves to active | one-line row update |
| On every incident | Append to `RUNBOOK.md` § Active Defect or § Resolved Defect | one-line row update |
| Quarterly | Review `STATUS.md` § 3 Captured Trade-offs; retire stale entries | one-line edit |

## 5. Stack

- **Language**: Rust (edition 2021), single binary.
- **Build**: `cargo build --release`; `Cargo.lock` pinned.
- **Native deps**: `libwebp` via `pkg-config` + `cc`.
- **Parallelism**: `rayon` data-parallel scheduler (intra-process).
- **Output formatting**: `image::DynamicImage` → format-specific
  encoder (baseline) → `webp::Encoder` for the default WebP path.

## 6. Terminology

- **`pipeline`**: an ordered set of `(input_format, output_format,
  resize_policy, quality)` parameters. The v0 baseline is the
  `jpg-to-webp` pipeline.
- **`candidate`**: a file in the input directory whose extension
  matches the pipeline's accepted input extensions (case-insensitive).
- **`converter-core`**: the orchestration component that walks the
  directory, dispatches per-file jobs to the codec layer, and
  aggregates the result.
- **`codec`**: a single input-format decoder + output-format encoder
  pair with its resize policy and quality parameters.
- **`policy`**: the resize rule applied to a decoded image before
  encoding (currently: per-orientation max-width cap).

## 7. Project-Specific Notes

- The v0 binary is named `gallery-compress`. Under ADR-0001 the
  canonical binary name will move to `fast-image-converter` (matching
  the project slug); the v0 name is kept as a backward-compatible
  alias until the next major version (see AR-001 § 6 Migration).
- The `DEFAULT_GALLERY_BASE` constant in `src/main.rs` was removed
  by `DE-006` (commit `f9f940e`). A bare positional argument
  (e.g. a year) now requires `GALLERY_BASE` to be set; the binary
  no longer carries a host-specific default. Captured as
  `RUNBOOK.md` § RD-001 (Resolved Defect).
- The product rename to `fast-image-converter` is accepted by ADR-0003.
  Runtime and release work must land before the documentation sweep;
  `fast-image-converter` and `gallery-compress` remain compatibility aliases.
- The operational project slug remains `fast-image-converter` until a separate
  identity migration is explicitly approved and executed.

## 8. Source Refs

- `docs/architecture.md` — C4-style architecture overview.
- `docs/ROADMAP.md` — active wave + planned waves.
- `docs/RUNBOOK.md` — operator runbook.
- `docs/adr/0001-multi-format-cli-scope.md` — scope decision.
- `docs/adr/0002-preserve-jpg-to-webp-baseline.md` —
  backward-compat baseline decision.
- `docs/components/README.md` — component registry.
- `docs/contracts/README.md` — contract registry.
- `Issues/open/architect/AR-001_initiate_multi_format_cli.md` —
  initiating proposal.
- `README.md` — operator-facing overview.
- `src/main.rs` — v0 reference implementation (read-only).

---
project_slug: convert-to-webp
doc_slug: architect_status
doc_type: architecture_meta
applicable_roles: [architect, developer, fixer, code_researcher]
version: 2
source_artifacts:
  - docs/architecture.md
  - docs/ROADMAP.md
  - docs/RUNBOOK.md
  - src/main.rs (v0 baseline reference)
  - docs/adr/0004-add-heic-input-support.md (HEIC input-only decision)
  - Issues/open/architect/AR-003_add_heic_input_support.md (driver proposal)
  - Issues/open/developer/DE-040_add_heic_input_codec.md (implementation task)
summary: "Architect meta-document for convert-to-webp. Captures goals, key properties, captured trade-offs, architect cycles, last-updated. v2 adds HEIC input-only trade-off row (ADR-0004)."
tags: [meta, architect, status, convert-to-webp]
---

# Architect Status (convert-to-webp)

> **Last Updated**: 2026-08-08 — drafted ADR-0004 (HEIC input-only) and
> AR-003 / DE-040 (driver proposal + implementation task); HEIC delivered
> ahead of the broader AR-017 format-coverage wave so the operator can
> validate the `libheif`-based build-time approach before committing to
> GIF / BMP / AVIF / JXL. Parked ADR-0005 / AR-004 (PDF input-only via
> `pdfium-render` static bundling) — activation gates on HEIC shipping,
> explicit operator authorisation, and the broader Wave 2 slot.
> Expanded Wave 2 into four sub-waves (2.3 tiny formats, 2.4 animated,
> 2.5 multi-page, 2.6 special) per ADR-0006 / AR-005 — sequenced
> smallest-first so each delivery's review cost stays bounded; the eleven
> remaining input formats (`bmp`, `pnm`, `tga`, `hdr`, `ff`, `qoi`,
> `gif`, `apng`, `tiff`, `ico`, `avif`, `exr`) land across these four
> sub-waves. Runtime, release, and documentation rename tasks (ADR-0003)
> remain queued.

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
| Product identity: `fast-image-converter` vs `fast-image-converter` | Adopt `fast-image-converter` as canonical product and CLI name; retain `gallery-compress` as a compatibility alias; the `convert-to-webp` v0 alias was removed per DE-045 | `adr/0003-fast-image-converter-product-name.md` |
| Hard-coded `DEFAULT_GALLERY_BASE` vs env-only vs CLI-only | Env-only with explicit "must-be-set" error for bare args. Resolved by `DE-006`; no host-specific default remains. | `RUNBOOK.md` § RD-001 |
| HEIC input-only vs full HEIC encode/decode parity | Input-only: HEIC decoded via `image` crate `heif` feature (statically links `libheif` + `libde265` + `dav1d` via `libheif-sys`); `--output-format heic` exits 2 with usage. Rationale: every operator use case targets the existing WebP / PNG / JPEG output set; no demand for HEIC output has been expressed; the `image` crate does not expose HEIF encoding as of 0.25. Captured as ADR-0004 F-2 future work if demand emerges. | `adr/0004-add-heic-input-support.md` |
| PDF input delivery (parked, not active) | PDF deferred until HEIC (DE-040) ships + operator explicitly authorises. Library `pdfium-render` (BSD-3-Clause, permissive-only) replaces the `libheif-sys` static-link pattern with the crate's default `static` PDFium bundling (no system-level `libpdfium-dev` required at build time). Combined binary delta ~9-14 MiB vs ~2.4 MiB today after HEIC + PDF. | `adr/0005-add-pdf-input-support-parked.md` |
| Wave 2 expansion (planned, not active) | Remaining format coverage (`bmp`, `pnm`, `tga`, `hdr`, `ff`, `qoi`, `gif`, `apng`, `tiff`, `ico`, `avif`, `exr` — 11 formats) split into 4 sub-waves (2.3 tiny, 2.4 animated, 2.5 multi-page, 2.6 special) sequenced smallest-first. All input-only; AVIF encoder remains out of scope. Wave 2.5 gates on Wave 2.2 PDF activation (the `MultiPageConversionReport` Codec trait variant is shared). Combined binary delta across all four sub-waves: ~+0.5-3.5 MiB (most from AVIF + multi-page format decoders); no new system-level C libraries required. | `adr/0006-expand-format-coverage-wave-2-3-to-2-6.md` |

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
- **Native deps**: `libwebp` via `pkg-config` + `cc` (WebP
  encode); `libheif` via `pkg-config` + `cc` (HEIC decode,
  added in DE-040 / ADR-0004; statically links `libde265` for
  HEVC + `dav1d` for AV1 via `libheif-sys`).
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
  `gallery-compress` remains the only compatibility alias; the
  `convert-to-webp` v0 alias was removed entirely per DE-045.
- The operational project slug remains `convert-to-webp` until a separate
  identity migration is explicitly approved and executed.

## 8. Source Refs

- `docs/architecture.md` — C4-style architecture overview.
- `docs/ROADMAP.md` — active wave + planned waves.
- `docs/RUNBOOK.md` — operator runbook.
- `docs/adr/0001-multi-format-cli-scope.md` — scope decision.
- `docs/adr/0002-preserve-jpg-to-webp-baseline.md` —
  backward-compat baseline decision.
- `docs/adr/0003-fast-image-converter-product-name.md` —
  product rename decision.
- `docs/adr/0004-add-heic-input-support.md` — HEIC input-only
  decision (DE-040 driver).
- `docs/adr/0005-add-pdf-input-support-parked.md` — PDF
  input parked decision (Wave 2.2).
- `docs/adr/0006-expand-format-coverage-wave-2-3-to-2-6.md` —
  Wave 2 expansion decision (sub-waves 2.3 / 2.4 / 2.5 / 2.6).
- `docs/components/README.md` — component registry.
- `docs/contracts/README.md` — contract registry.
- `Issues/open/architect/AR-003_add_heic_input_support.md` —
  HEIC input proposal (Wave 2.1 driver).
- `Issues/open/architect/AR-004_park_pdf_input_support.md` —
  PDF input parking record (Wave 2.2 driver).
- `Issues/open/architect/AR-005_expand_format_coverage_wave_2.md` —
  Wave 2 expansion proposal (sub-waves 2.3-2.6 driver).
- `Issues/open/developer/DE-040_add_heic_input_codec.md` —
  HEIC input implementation task.
- `Issues/open/architect/AR-001_initiate_multi_format_cli.md` —
  initiating proposal (Wave 1).
- `README.md` — operator-facing overview.
- `src/main.rs` — v0 reference implementation (read-only).

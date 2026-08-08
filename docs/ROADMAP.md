# Roadmap

> **Status**: Draft. See `architecture/STATUS.md` for the meta-document.
> Waves are sequenced smallest-first; each wave is independently
> committable per `TASK_PLANNING_GUIDE.md`.

## Active Wave

### Wave 1 — Multi-format CLI scope expansion

**Goal**: extend the v0 `gallery-compress` binary to a generic
configurable input/output image-format converter while preserving
the v0 default behaviour.

**Driver issue**: `Issues/open/architect/AR-001_initiate_multi_format_cli.md`
(proposal) + the downstream developer task to be created in
`Issues/open/developer/`.

**Acceptance criteria** (top-level, will be re-decomposed into
per-wave tasks):

1. The binary accepts `--input-format <fmt>` and
   `--output-format <fmt>` CLI flags with `<fmt> ∈ {jpg, png, webp}`
   in Wave 1; additional formats (gif, bmp, tiff, avif) gated on
   follow-up waves.
2. The default pipeline (no flags) keeps the v0 behaviour exactly:
   JPG → WebP, portrait 800 px, landscape 1000 px, quality 85.
3. The exit-code contract is preserved (0 / 1 / 2).
4. The summary line on stdout remains a single grep-friendly line.
5. The per-file error line on stderr remains a single
   `<path>: <error>` line.

**Sub-tasks (planned decomposition for developer)**:

| Sub-task | Owner | Status |
|---|---|---|
| 1.1 Introduce `format` module + `Codec` trait | developer | queued |
| 1.2 Wire `--input-format` / `--output-format` CLI flags | developer | queued |
| 1.3 Plumb per-format quality / resize policy | developer | queued |
| 1.4 Update README + add `--help` examples | developer (with lamport) | queued |
| 1.5 Smoke test on a 3-format sample | developer + tester | queued |

**Cross-cutting tracks touched**:

- `cli_ergonomics` — `--help` text + flag surface.
- `output_fidelity` — encoder-config audit per format.
- `regression_risk` — v0 default pipeline must be byte-equivalent.

**Out of Wave 1 scope**: GIF animation, ICC profiles, AVIF encoder,
distributed batch mode, library API.

## Rename Wave — fast-image-converter

The next breaking product-identity wave adopts `fast-image-converter` as
canonical name. Runtime and release changes are sequenced before the
documentation sweep so examples cannot get ahead of shipped artifacts.

| Task | Owner | Priority | Dependency | Status |
|---|---|---:|---|---|
| AR-009 Rename runtime and Cargo identity | developer | P0 | ADR-0003 | queued |
| AR-010 Rename release and deployment surfaces | developer | P0 | AR-009 | queued |
| AR-011 Rename product documentation | lamport | P0 | AR-009, AR-010 | queued |

Compatibility aliases `fast-image-converter` and `gallery-compress` remain
supported for at least one major version. The operational project slug
is intentionally unchanged and requires a separate identity migration.

## Planned Waves

### Wave 2 — Format expansion

- Add `bmp`, `tiff`, animated `gif`, `apng` to the input set.
- Add `avif` to the output set (depends on host `libaom` /
  `libavif` availability; gated on operator environment check).
- Per-format quality presets surfaced via `--quality <n>` flag.

### Wave 2.1 — HEIC input (active; DE-040 / ADR-0004)

**Goal**: add HEIC (HEIF container) as an input format so the
converter accepts the dominant still-image format on Apple
devices (iOS 11..16 HEVC; iOS 17+ AV1).

**Driver issue**:
`Issues/open/architect/AR-003_add_heic_input_support.md`
(proposal) + `Issues/open/developer/DE-040_add_heic_input_codec.md`
(implementation task).

**Sub-tasks** (decomposed for developer):

| Sub-task | Owner | Status |
|---|---|---|
| 2.1.1 Enable `heif` feature on the `image` crate | developer | queued |
| 2.1.2 Implement `HeicToWebp` / `HeicToPng` / `HeicToJpeg` codecs | developer | queued |
| 2.1.3 Wire `--input-format heic` into the CLI parser; reject `--output-format heic` | developer | queued |
| 2.1.4 Extend `report::ImageFormat` with `Heic` variant (JSON `"heic"`) | developer | queued |
| 2.1.5 HEIC fixtures + `tests/heic.rs` integration suite | developer | queued |
| 2.1.6 Update README, integration-contract, RUNBOOK, format-codecs § 6.4 | developer (with lamport for operator-facing copy) | queued |
| 2.1.7 `make ci` green; binary-size delta recorded in PR | developer + tester | queued |

**Cross-cutting tracks touched**:

- `output_fidelity` — HEIC decoder fidelity (HEVC + AV1 dual-plugin).
- `regression_risk` — v0 default pipeline (jpg→webp) remains
  byte-equivalent; HEIC is additive.
- `build_health` — new build-time system dep `libheif-dev`;
  CI workflow update; ~1.5-2.5 MiB binary-size delta; ~60-90 s
  clean-build duration delta.

**Out of Wave 2.1 scope**: HEIC output (encoder); HEIC
multi-image containers (Live Photos, depth variants); HEIC
depth-metadata preservation; opt-in Cargo feature (`heic-input`
default off). Captured as ADR-0004 F-1 + F-2 future-work items.

**Why HEIC is delivered ahead of GIF / BMP / AVIF / JXL**: the
`libheif`-based build-time approach (R1..R3 in ADR-0004) is the
first time this project statically links a third-party C library
beyond `libwebp`. Delivering HEIC first lets the operator
validate the build-time + binary-size + license model before
committing to the broader format-coverage wave.

### Wave 2.2 — PDF input (parked; AR-004 / ADR-0005)

**Goal** (when activated): add PDF (Portable Document Format,
ISO 32000) as an input format so the converter accepts the
dominant document container for scanned-page archives, reports,
and ebooks. Renders every page of the PDF to the chosen output
format.

**Status**: **parked** (2026-08-08). Architectural analysis
captured in `docs/adr/0005-add-pdf-input-support-parked.md`
and the parking record `Issues/open/architect/AR-004_park_pdf_input_support.md`.
No implementation work scheduled in the current wave plan.

**Activation criteria** (all must hold):

1. The HEIC (DE-040 / ADR-0004) wave has shipped and the
   `libheif` build-time approach has been validated
   end-to-end on the operator's production deployment.
2. The operator has explicitly authorised PDF activation in
   chat ("add PDF input" or equivalent), not just a soft ask.
3. The broader Wave 2 (GIF / BMP / AVIF / JXL — AR-017
   placeholder) has not yet started, OR has completed
   without scope for PDF.

**Captured decisions** (parked; full detail in ADR-0005):

- **Library**: `pdfium-render` (PDFium / BSD-3-Clause).
  Static bundling via the crate's default `static` feature;
  no system-level `libpdfium-dev` required at build time.
  Alternatives (poppler GPL-2, mupdf AGPL-3) rejected on
  license grounds.
- **Page semantics**: all pages by default; `--first-page`
  opt-in flag; `--pages <spec>` (comma-separated list +
  ranges) optional; 999-page hard cap per PDF.
- **DPI**: `--pdf-dpi <N>` flag with default `150`, range
  `[72, 600]`.
- **Output naming**: `<input_stem>-<NNN>.<ext>` where `NNN`
  is the zero-padded page number (3 digits ≤ 99; 4 digits
  ≤ 999; 5 digits above).
- **JSON shape**: one NDJSON record per page (not one per
  PDF). `input.path` is the PDF, `output.path` is the
  per-page image. `input.format = "pdf"`. `schema_version`
  does not bump (additive shape).
- **Single-file mode**: `--single-file --input-format pdf`
  outputs a zip archive on stdout (one image per page).
  New `zip` crate dependency added at activation.
- **Source removal**: source PDF removed only after all
  pages encode and write successfully (mirrors v0
  all-or-nothing semantics).
- **Scope cap**: PDF input-only; PDF output (image → PDF)
  out of scope.

**Risks (when activated)**: R1 binary-size delta ~5-10 MiB
(combined with HEIC: ~9-14 MiB total); R2 PDFium first-build
download (~30-40 MiB); R3 multi-page JSON stream length;
R4 DPI-driven memory at 300 DPI (~35 MiB per A4 page);
R5 encrypted PDFs out of scope v1; R6 vector graphics
flattened to bitmaps.

**Why parked (not active)**:

- **Scope management**: HEIC delivery exercises the
  build-time + binary-size + license model for a third-
  party C dependency first; PDF adds another 5-10 MiB on
  top.
- **Validation opportunity**: lessons from HEIC's
  `libheif-sys` static link inform PDFium bundling at
  activation.
- **Soft ask**: 2026-08-08 chat "может добавить" is a soft
  ask, not an authorisation to schedule. Activation
  requires an explicit operator "now".

**Cross-cutting tracks touched (when activated)**:
`output_fidelity`, `regression_risk`, `build_health` (PDFium
static bundling).

### Wave 3 — Resize policy generalisation

- Replace the v0 per-orientation hard policy with a `--resize`
  flag accepting `<W>x<H>` (cap), `<W>x` (max-width), or `none`.
- Keep the v0 policy as `--resize=auto:portrait=800,landscape=1000`
  default.

### Wave 4 — Operator UX

- `--dry-run` mode that prints the candidate list + planned
  pipeline without writing.
- `--keep-source` mode that preserves the input file (v0 baseline
  removes it after a successful conversion).
- `--jobs <N>` flag for capping the rayon thread pool.

## Recently Closed

_This section is intentionally empty on first publish. The architect
will append here as waves close._

## Cross-Cutting Track Anchors

| Track | Anchor doc | Cadence |
|---|---|---|
| `cli_ergonomics` | `components/cli-frontend.md` | per-flag-add |
| `output_fidelity` | `contracts/codec-bounds.md` | per-codec-add |
| `regression_risk` | `RUNBOOK.md` § Regression incidents | per-release |
| `build_health` | `RUNBOOK.md` § Build-time failures | per-host-update |
| `host_path_leak` | `RUNBOOK.md` § Tech-debt hot list | per-cleanup |

## Source Refs

- `architecture.md` — architecture overview.
- `architecture/STATUS.md` — meta-document + captured trade-offs.
- `adr/0001-multi-format-cli-scope.md` — scope decision.
- `adr/0002-preserve-jpg-to-webp-baseline.md` — backward-compat decision.
- `adr/0003-fast-image-converter-product-name.md` — product rename decision.
- `adr/0004-add-heic-input-support.md` — HEIC input-only decision.
- `adr/0005-add-pdf-input-support-parked.md` — PDF input parked decision (Wave 2.2 parking).
- `Issues/open/architect/AR-003_add_heic_input_support.md` — HEIC input proposal (Wave 2.1 driver).
- `Issues/open/developer/DE-040_add_heic_input_codec.md` — HEIC input implementation task (Wave 2.1).
- `Issues/open/architect/AR-004_park_pdf_input_support.md` — PDF input parking record (Wave 2.2 driver).
- `Issues/open/architect/AR-001_initiate_multi_format_cli.md` — driver proposal (Wave 1).

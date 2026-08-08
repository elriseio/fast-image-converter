---
project_slug: convert-to-webp
doc_slug: adr_0006_expand_format_coverage_wave_2_3_to_2_6
doc_type: adr
applicable_roles: [architect, developer]
version: 1
date: 2026-08-08
status: proposed
supersedes: docs/ROADMAP.md § "Wave 2 — Format expansion" (the original Wave 2 entry enumerated `bmp`, `tiff`, animated `gif`, `apng`, `avif`; this ADR expands the wave into four sequenced sub-waves covering 11 formats)
summary: "Expand Wave 2 (format expansion) into four sub-waves — 2.3 tiny formats (bmp/pnm/tga/hdr/ff/qoi), 2.4 animated (gif/apng), 2.5 multi-page (tiff/ico; gates on PDF activation), 2.6 special (avif/exr). All input-only; AVIF encoder remains out of scope. Sequenced smallest-first so each sub-wave's review cost stays bounded per TASK_PLANNING_GUIDE.md."
source_artifacts:
  - docs/ROADMAP.md § Wave 2.3 / 2.4 / 2.5 / 2.6 (sub-wave structure)
  - Issues/open/architect/AR-005_expand_format_coverage_wave_2.md (driver proposal)
  - docs/adr/0004-add-heic-input-support.md (HEIC precedent — input-only, single-feature enable pattern)
  - docs/adr/0005-add-pdf-input-support-parked.md (PDF precedent — multi-page Codec trait variant)
  - src/format.rs (Codec trait extension point; new Format variants + codecs)
  - src/main.rs (CLI parser; new `--input-format <fmt>` enum values)
  - src/report.rs (ImageFormat enum; new JSON `input.format` values)
  - Cargo.toml (image crate features; new feature flags)
tags: [adr, format-expansion, wave-2.3, wave-2.4, wave-2.5, wave-2.6, image-crate-features, codec-trait, multi-page, avif, exr, animated]
---

# ADR-0006 — Expand Wave 2 format coverage into sub-waves 2.3..2.6

## Status

**Proposed** (2026-08-08). Driver proposal:
`Issues/open/architect/AR-005_expand_format_coverage_wave_2.md`.

## Date

2026-08-08

## Authors

- Architect (system) — proposal authoring
- Operator (pending approval)
- Developer (pending execution per sub-wave activation)

## Context

Operator instruction 2026-08-08 chat: «`avif`, `bmp`, `exr`,
`ff`, `gif`, `hdr`, `ico`, `jpeg`, `png`, `pnm`, `qoi`, `tga`,
`tiff`, `webp`. будем добавлять оставшиеся форматы?».

The `image` crate 0.25 (the only image-format abstraction in
`Cargo.toml`) exposes decoders for the eleven formats not yet
supported via Cargo feature flags:

| Format | `image` feature | Decoder | Encoder | Special |
|---|---|---|---|---|
| `bmp` | `bmp` | yes | yes | trivial |
| `pnm` (PPM / PGM / PBM) | `pnm` | yes | yes | trivial |
| `tga` | `tga` | yes | yes | trivial |
| `hdr` (Radiance) | `hdr` | yes | yes | trivial |
| `ff` (Farbfeld) | `farbfeld` | yes | yes | trivial |
| `qoi` | `qoi` | yes | yes | trivial |
| `gif` | `gif` | yes | no | animated (first-frame only per v0) |
| `apng` | reuses `png` | yes (existing PNG) | yes (existing PNG) | APNG is PNG with `acTL` chunk; folded into existing `Format::Png` |
| `tiff` | `tiff` | yes | no | multi-page |
| `ico` | `ico` | yes | no | multi-image (Windows icon variants) |
| `avif` | `avif` (uses `avif-decoder` pure-Rust crate) | yes | no (no AVIF encoder exposed as of `image` 0.25) | special |
| `exr` | `exr` | yes | no (no EXR writer exposed as of `image` 0.25) | special; HDR float16 / float32 |

The v0 baseline (Wave 1 / DE-001) supports `jpg` / `png` /
`webp` plus all six cross-combinations (`jpg ↔ png ↔ webp`).
Wave 2.1 (DE-040 / ADR-0004) adds HEIC. Wave 2.2 (parked /
ADR-0005) adds PDF. This ADR adds the remaining **eleven**
formats.

The original Wave 2 (planned) entry in `docs/ROADMAP.md`
enumerated only `bmp`, `tiff`, animated `gif`, `apng`, `avif`
— five of the eleven. This ADR expands that single line
into four sequenced sub-waves covering all eleven formats
with explicit implementation profile, activation gates,
and binary-size / scope-cap notes.

## Decision

We will expand Wave 2 into four sub-waves, sequenced
smallest-first per `TASK_PLANNING_GUIDE.md`:

### Wave 2.3 — Tiny formats (single-feature enable)

**Formats**: `bmp`, `pnm`, `tga`, `hdr`, `ff`, `qoi`.

**Implementation profile**: each format is a single Cargo
feature flag on the `image` crate. Six new `Format` enum
variants; eighteen new codecs (`<fmt>ToWebp` /
`<fmt>ToPng` / `<fmt>ToJpeg` per format); six new
`ImageFormat` variants; six new CLI flag values. One
single-PR developer task. Expected binary-size delta:
+0.5-1.5 MiB total across all six features.

### Wave 2.4 — Animated formats (first-frame-only)

**Formats**: `gif`, `apng`.

**Implementation profile**: `gif` is a new `Format` variant
+ 3 codecs. APNG is folded into the existing `Format::Png`
(the `image` crate's PNG decoder handles APNG; animated
frames are discarded per the v0 baseline behaviour
documented in `docs/components/format-codecs.md` § 9).
One single-PR developer task. Expected binary-size delta:
+0.1-0.3 MiB for the `gif` feature.

### Wave 2.5 — Multi-page / multi-image formats

**Formats**: `tiff`, `ico` (PDF is parked separately in
Wave 2.2).

**Implementation profile**: reuses the
`MultiPageConversionReport` Codec trait variant that
Wave 2.2 (PDF activation) introduces. TIFF pages and ICO
variants at different sizes each emit one NDJSON record.
Source-removal is all-or-nothing (mirrors the PDF contract
from ADR-0005 § Decision § 9). One single-PR developer task.

**Activation gate**: **Wave 2.2 PDF activation** must land
first; otherwise this sub-wave's developer task includes
the multi-page Codec trait extension as part of its scope
(which duplicates the PDF work and creates two parallel
implementations).

### Wave 2.6 — Special formats

**Formats**: `avif`, `exr`.

**Implementation profile**: `avif` is a new `Format` variant
+ 3 codecs. The `image` crate's `avif` feature uses the
pure-Rust `avif-decoder` crate (MIT/Apache-2.0) by default
— no system `libavif` / `libaom` required at build time.
This is documented as F-1 future work for performance-
critical deployments that prefer the system `libavif` path.
`exr` is a new `Format` variant + 3 codecs; EXR is high-
dynamic-range and the decoded `DynamicImage` is clamped to
8-bit on encode (documented as a known limitation).
One single-PR developer task. Expected binary-size delta:
+0.5-1.5 MiB.

**AVIF encoder is out of scope**. The `image` crate does not
expose AVIF encoding as of 0.25, and adding a separate
`avif-encoder` crate adds binary size for a feature the
operator has not explicitly requested. If operator demand
emerges, a separate ADR authorises the encoder.

**Cross-sub-wave scope cap (all four sub-waves)**: every
new format is **input-only**. `--output-format <fmt>` for
each new format is rejected with exit 2 + usage. This
mirrors the HEIC (ADR-0004 § Decision § 6) and PDF
(ADR-0005 § Decision § 11) precedents.

**Cross-sub-wave JSON shape**: every new format adds one
new `ImageFormat` variant; the JSON value is the lowercase
format name (`"bmp"`, `"pnm"`, `"tga"`, `"hdr"`, `"ff"`,
`"qoi"`, `"gif"`, `"tiff"`, `"ico"`, `"avif"`, `"exr"`).
`schema_version` does **not** bump (additive field,
backwards-compatible interpretation per
`docs/contracts/report-shape.md` § 7 INV-RS-2).
Multi-page formats (Wave 2.5) emit one NDJSON record per
page or per ICO variant; this is the same shape that Wave
2.2 (PDF) introduces.

**Build-time system dependencies**: none added. All
eleven format decoders are reached via `image` crate
feature flags; the `image` crate's `avif` feature uses
the pure-Rust `avif-decoder` crate, so no system `libavif`
/ `libaom` is required. This is a meaningful contrast to
Wave 2.1 (HEIC adds `libheif-dev`) and Wave 2.2 (PDF
statically bundles PDFium via `pdfium-render`'s default
`static` feature but downloads ~30-40 MiB on first build).

## Alternatives Considered

### A1 — Single mega-wave covering all eleven formats

One developer task; one PR; 30+ commits; ~1000+ LOC;
3+ new dependencies; unreviewable review burden.

**Rejected**: violates `TASK_PLANNING_GUIDE.md` ("при
планировании задач предпочитать разбиение на множество
мелких задач с bounded scope вместо одной большой"). The
eleven formats have heterogeneous implementation profiles
(tiny / animated / multi-page / special); splitting lets
each delivery's review cost stay in the 8-15 commit
envelope that matches the existing `DE-001` / `DE-039` /
`DE-040` precedent.

### A2 — One PR per format

Eleven developer tasks; eleven PRs; each ~1-2 commits;
~50-100 LOC per format.

**Rejected**: multiplies the review burden without
architectural benefit for the "tiny" formats (bmp, pnm,
tga, hdr, ff, qoi share the identical implementation
profile — single Cargo feature + per-format glue; one PR
is the natural shape). For the "special" formats (avif,
exr) one PR each is also the natural shape because each
needs its own codec implementation pattern. The four
sub-waves capture the natural grouping.

### A3 — Defer all eleven formats until Wave 2.2 PDF lands

Parking all eleven formats alongside PDF (AR-004) until
PDF activation.

**Rejected**: the Wave 2.3 / 2.4 / 2.6 sub-waves do not
require PDF activation; they only need HEIC (Wave 2.1) to
ship. Parking the entire Wave 2 until PDF lands is
excessively conservative; the operator can activate
Wave 2.3 (smallest, lowest risk) immediately after HEIC.

### A4 — Add output encoders for the new formats

Add AVIF / TIFF / HDR / QOI / FF encoders in addition
to decoders.

**Rejected**: the operator's stated use case is "convert
PDF (and other input formats) to the existing WebP / PNG
/ JPEG output set"; no operator demand for new output
formats has been expressed. Adding encoders doubles the
scope and the binary-size delta; deferred to a future ADR
if demand emerges.

### A5 — Replace existing Wave 2 entry entirely

Delete the original Wave 2 line (`bmp`, `tiff`, `gif`,
`apng`, `avif`) and replace with the four sub-waves.

**Rejected**: the original Wave 2 entry has historical
value (operator's initial scoping in 2026-08-03 ROADMAP);
the four sub-waves supersede it explicitly per the
`supersedes` front-matter field. Future ROADMAP edits
will collapse the original Wave 2 entry into a single
forwarding line that says "see Wave 2.3 / 2.4 / 2.5 /
2.6 below".

## Consequences

### Positive

- The CLI accepts eleven additional input formats — the
  remaining standard raster image formats plus AVIF
  (next-gen still image) and EXR (high-dynamic-range).
  Re-uses the existing Codec trait + `Format` enum
  extension point; no architectural restructuring.
- Four sub-waves each fit the existing DE-001 / DE-039 /
  DE-040 precedent for "add formats" tasks (single PR,
  8-15 commits, AC-NN-A1..A6 closure evidence).
- Wave 2.3 lands first (lowest risk, smallest scope,
  validates the "many formats per PR" pattern); subsequent
  sub-waves layer on.
- Wave 2.5 reuses the multi-page Codec trait variant from
  Wave 2.2 PDF activation — no parallel implementation.
- All eleven formats are reached via `image` crate
  features; no new system-level C libraries required
  (vs HEIC's `libheif-dev` and PDF's PDFium static
  bundling).

### Negative / Risks

- **R1 (binary-size)**: combined delta across all four
  sub-waves is ~+0.5-3.5 MiB (most from AVIF + multi-page
  format decoders). Combined with HEIC (+1.5-2.5 MiB)
  and PDF (~5-10 MiB), the post-HEIC + post-PDF + post-Wave-2
  release binary size is ~11-19 MiB (vs ~2.4 MiB today).
  Still well under any operator-acceptable envelope;
  documented in `docs/RUNBOOK.md` § 8.2 at each
  sub-wave activation.
- **R2 (CI build time)**: each new `image` feature
  extends the `image` crate's compile-graph. The
  delta is bounded (each feature adds one decoder
  table, ~5-15 s extra compile time); documented at
  each sub-wave activation.
- **R3 (multi-page JSON shape)**: Wave 2.5 (tiff, ico)
  emits one NDJSON record per TIFF page or per ICO
  variant. A 100-page TIFF emits 100 NDJSON records;
  same caveat as the PDF activation (R3 in ADR-0005).
- **R4 (EXR 8-bit clamp)**: EXR is high-dynamic-range
  (float16 / float32); the Wave 2.6 implementation
  clamps to 8-bit on encode. This is a known fidelity
  loss; documented in `RUNBOOK.md` § 3.6 (added on
  Wave 2.6 activation). Operators requiring HDR
  fidelity need a different tool.
- **R5 (animated multi-frame discarded)**: per the v0
  baseline, animated GIF and APNG decoders return the
  primary frame only. Wave 2.4 inherits this behaviour;
  operators requiring animated output need a different
  tool. Documented in `format-codecs.md` § 6.6 (added
  on Wave 2.4 activation).

### License notes (per sub-wave)

All eleven format decoders are reached via `image`
crate feature flags; the `image` crate is MIT or
Apache-2.0. AVIF uses the pure-Rust `avif-decoder` crate
(MIT/Apache-2.0) by default. No copyleft contamination.
Combined license footprint matches the v0 baseline
(permissive-only).

## Follow-up

- `docs/ROADMAP.md` § Wave 2.3 / 2.4 / 2.5 / 2.6 — full
  acceptance criteria per sub-wave; activation gates
  documented.
- `docs/architecture/STATUS.md` § 3 Captured Trade-offs —
  Wave 2 expansion trade-off row added (combined binary-
  size delta + scope cap + activation sequence).
- `docs/architecture.md` § 6 External Dependencies —
  rows for the planned image-crate features.
- `docs/components/format-codecs.md` — § 6.5 (Wave 2.3
  tiny formats), § 6.6 (Wave 2.4 animated), § 6.7
  (Wave 2.5 multi-page), § 6.8 (Wave 2.6 special).
- `Issues/open/architect/AR-005_expand_format_coverage_wave_2.md`
  — driver proposal (sub-wave activation sequencing).
- Per-sub-wave developer tasks (`DE-NNN_*`) created on
  activation; each task receives the next free `DE-`
  numeric core after `DE-040`.

## Cross-References

- `docs/adr/0001-multi-format-cli-scope.md` — multi-format
  CLI scope (extends the converter beyond JPG/PNG/WebP;
  Wave 2 covers the remaining eleven formats).
- `docs/adr/0002-preserve-jpg-to-webp-baseline.md` —
  backward-compat baseline (Wave 2 must NOT alter the
  default pipeline).
- `docs/adr/0004-add-heic-input-support.md` — HEIC input
  precedent; the build-time pattern that Wave 2.3
  inherits (single Cargo feature + per-format glue).
- `docs/adr/0005-add-pdf-input-support-parked.md` — PDF
  parked decision; the `MultiPageConversionReport`
  Codec trait variant that Wave 2.5 reuses.
- `Issues/open/architect/AR-003_add_heic_input_support.md`
  — HEIC active proposal (Wave 2.1).
- `Issues/open/developer/DE-040_add_heic_input_codec.md`
  — HEIC active implementation task (Wave 2.1).
- `Issues/open/architect/AR-004_park_pdf_input_support.md`
  — PDF parking record (Wave 2.2; gates Wave 2.5).
- `Issues/open/architect/AR-005_expand_format_coverage_wave_2.md`
  — driver proposal (this ADR's origin).
- `docs/components/format-codecs.md` § 9 — original
  "Animated GIF / APNG: out of scope; only the first
  frame is processed" baseline (Wave 2.4 inherits).
- `docs/contracts/report-shape.md` § 7 — JSON shape
  versioning; additive fields across all four sub-waves.
- `docs/TASK_PLANNING_GUIDE.md` (in `docs/agent_context/`)
  — heuristic for splitting tasks; this ADR applies
  heuristic #1 (touches more than 3 components →
  split into sub-waves).
- `docs/RUNBOOK.md` — binary-size + clean-build duration
  delta tracked at each sub-wave activation.
- shared memory::`ar-017-format-coverage-wave-activated`
  — broader format-coverage wave context; Wave 2.3-2.6
  supersede the AR-017 placeholder's `DE-022..DE-025`
  numbering scheme with a sequenced activation model.

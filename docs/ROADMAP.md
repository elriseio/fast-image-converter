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

### Wave 2 — Format expansion (overview)

Wave 2 covers the remaining input formats beyond the v0
baseline (jpg/png/webp), HEIC (Wave 2.1), and PDF
(Wave 2.2, parked). Sub-waves group formats by their
implementation profile:

- **Wave 2.3 — Tiny formats** (single-feature enable): `bmp`,
  `pnm`, `tga`, `hdr`, `ff`, `qoi`. All six require a single
  Cargo feature flag on the `image` crate; per-format glue
  is mechanical.
- **Wave 2.4 — Animated formats** (first-frame-only): `gif`,
  `apng`. Animated multi-frame files are decoded to the
  primary frame per the v0 baseline behaviour
  (per `docs/components/format-codecs.md` § 9 "Animated GIF /
  APNG: out of scope; only the first frame is processed").
- **Wave 2.5 — Multi-page / multi-image formats**:
  `tiff`, `ico` (PDF is parked separately in Wave 2.2).
  These require the `MultiPageConversionReport` Codec trait
  variant that Wave 2.2 (PDF activation) introduces; this
  sub-wave gates on Wave 2.2.
- **Wave 2.6 — Special formats**: `avif`, `exr`. AVIF
  encoder requires `libaom` or the `avif-encoder` crate;
  EXR is a high-dynamic-range format with a specialised
  decoder. Both are the most complex additions and land
  last.

**Scope cap across Wave 2.3..2.6**: input-only (mirrors the
HEIC and PDF precedent). `--output-format <fmt>` for each
new format is rejected with exit 2 + usage. The
`image` crate feature flags expose decoders for all
eleven formats; the encoder side is the existing
WebP / PNG / JPEG (or MozJPEG for JPEG output) plumbing
re-used unchanged. The single exception is AVIF encoder,
which is out of scope for Wave 2.6 — only AVIF decoder
lands; AVIF encoder stays parked until operator demand.

**Per-format quality presets surfaced via `--quality <n>`
flag**: carried over from the original Wave 2 plan; the
quality knob is already implemented in `src/params.rs` for
WebP and JPEG output. The new formats inherit the
existing quality plumbing without per-format additions.

**Activation order**: 2.3 → 2.4 → 2.5 → 2.6 (after
Wave 2.1 HEIC ships; after Wave 2.2 PDF activation
unblocks 2.5; after each sub-wave the operator validates
the build-time / binary-size / CI delta before
activating the next). Each sub-wave is a single
developer task (`DE-NNN`) with the next free numeric core
in the `DE-` namespace after `DE-040`.

**Cross-cutting tracks touched**:
`output_fidelity`, `regression_risk`, `build_health`
(image crate features pull in new decoders; no new
system-level C libraries required for 2.3 / 2.4 /
2.5; 2.6 may add `libaom` for AVIF encoder).

**Out of Wave 2 scope**: AVIF output (encoder);
animated multi-frame preservation for GIF / APNG;
multi-image preservation for ICO; EXR writer;
per-format quality presets beyond the existing
`--quality <n>` plumbing.

**Why sub-waves instead of one big wave**: each sub-wave
is independently committable per the
`TASK_PLANNING_GUIDE.md` (one commit per concrete child
issue; one DoD per sub-wave; bounded review cost). A
single 11-format PR would be 30+ commits, 1000+ LOC,
3+ new dependencies, and an unreviewable review burden.
Splitting keeps each sub-wave at the 8-15 commit
envelope that matches the existing `DE-001` / `DE-039` /
`DE-040` precedent.

### Wave 2.1 — HEIC input (active; DE-040 / ADR-0004)

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

### Wave 2.3 — Tiny formats (planned; `bmp`, `pnm`, `tga`, `hdr`, `ff`, `qoi`)

**Goal**: enable six single-feature raster formats via the
`image` crate's existing feature flags. Each format lands
a Codec trio (`<fmt>ToWebp`, `<fmt>ToPng`, `<fmt>ToJpeg`)
following the HEIC / Wave 1 precedent.

**Driver issue** (when activated):
`Issues/open/architect/AR-006_extend_format_coverage_wave_2_3.md`
(proposal) + `Issues/open/developer/DE-NNN_add_tiny_format_codecs.md`
(implementation task).

**Acceptance criteria** (top-level, re-decomposed per
sub-task in the developer task):

1. `Cargo.toml` `image` crate features list extends from
   `["jpeg", "png", "webp", "heif"]` (post-DE-040) to
   `["jpeg", "png", "webp", "heif", "bmp", "pnm", "tga",
   "hdr", "farbfeld", "qoi"]`. `Cargo.lock` regenerated.
2. `Format` enum extends with `Bmp` / `Pnm` / `Tga` / `Hdr`
   / `Ff` / `Qoi` variants. `Format::parse` accepts the
   six format names (case-insensitive). `Format::Display`
   renders the canonical spelling.
3. Six Codec structs (`BmpToWebp`, `BmpToPng`, `BmpToJpeg`,
   ... etc., 18 codecs total) implement the `Codec` trait
   with `accepted_extensions = &["<ext>"]` and decode via
   `image::ImageReader::open(...).with_guessed_format()?.decode()?`.
   Encode side re-uses the existing WebP / PNG / JPEG
   encoder plumbing unchanged.
4. `CodecImpl` enum extended with 18 variants. The
   compile-time exhaustiveness check
   (`cargo clippy --all-targets --all-features -- -D warnings`)
   enforces no missing match arm.
5. `report::ImageFormat` extended with 6 variants;
   `From<Format> for ImageFormat` updated.
6. CLI parser `--input-format <fmt>` accepts the six new
   formats; `--output-format <fmt>` rejected with exit 2
   + usage (input-only scope, mirrors HEIC / PDF).
7. `tests/tiny_formats.rs` integration suite covers each
   format: round-trip to WebP / PNG / JPEG; `--single-file`
   mode round-trip; bad-bytes failure; `--output-format
   <fmt>` rejection; `--input-format` alias acceptance
   (where applicable, e.g. `ppm` / `pgm` / `pbm` aliases
   for PNM).
8. Fixtures under `tests/fixtures/tiny_formats/` for each
   format (small, public-domain or generated; total
   < 5 MiB across all six).
9. README.md, `docs/integration-contract.md`,
   `docs/components/format-codecs.md` § 6.5, `RUNBOOK.md`
   updated per the existing DE-001 / DE-039 / DE-040
   pattern.
10. `make ci` green; binary-size delta documented
    (expected: +0.5-1.5 MiB total across all six features,
    most of which is `qoi` / `pnm` / `tga` / `hdr` table
    expansion in the `image` crate).

**Activation gate**: HEIC (DE-040 / ADR-0004) shipped +
operator authorisation.

**Why six in one PR**: these six formats share the
identical implementation profile (single Cargo feature
+ 18 codecs + 6 Format variants + 6 ImageFormat variants
+ 6 CLI flags). Splitting into six separate PRs would
multiply the review burden without architectural benefit.
The single-PR shape matches the existing `DE-001` /
`DE-039` precedent for "add many formats" tasks.

**Cross-cutting tracks touched**: `output_fidelity`,
`regression_risk` (v0 default pipeline unchanged), `cli_ergonomics`
(new `--input-format` enum values).

**Out of Wave 2.3 scope**: PNM output (encoder); HDR writer;
Farbfeld writer; QOI writer beyond what the `image` crate
exposes (all four are available, but operator-facing
acceptance is decoder-only per the Wave 2 input-only
scope cap).

### Wave 2.4 — Animated formats (planned; `gif`, `apng`)

**Goal**: enable `gif` and `apng` input via the `image`
crate's `gif` feature. Both formats carry multiple frames;
per the v0 baseline (`docs/components/format-codecs.md`
§ 9), only the **first frame** is decoded.

**Driver issue** (when activated):
`Issues/open/architect/AR-007_add_animated_format_codecs.md`
+ `Issues/open/developer/DE-NNN_add_animated_format_codecs.md`.

**Acceptance criteria** (top-level):

1. `Cargo.toml` `image` crate features list extends with
   `gif`. APNG requires no additional feature beyond the
   existing `png` (APNG is PNG with an `acTL` chunk; the
   `image` crate's PNG decoder discards animation frames
   by default — this is the v0 behaviour).
2. `Format` enum extends with `Gif` (case-insensitive
   parse). `apng` is not a separate `Format` variant — PNG
   with an `acTL` chunk is still parsed as `Format::Png`
   (the existing pipeline); only `Gif` is new.
3. `GifToWebp` / `GifToPng` / `GifToJpeg` codecs implement
   the `Codec` trait with `accepted_extensions = &["gif"]`.
   Decode uses `image::ImageReader::open(...).with_guessed_format()?.decode()?`
   which returns the primary frame only.
4. `CodecImpl` extended with 3 variants.
5. `report::ImageFormat` extended with `Gif`.
6. CLI parser `--input-format gif` accepted;
   `--output-format gif` rejected with exit 2 + usage.
7. `tests/animated_formats.rs` integration suite:
   animated GIF with 5+ frames → primary frame converted
   to WebP / PNG / JPEG (output bytes match the primary
   frame of the input); APNG with `acTL` chunk → primary
   frame converted.
8. Fixtures: one animated GIF (public-domain, 5+ frames)
   + one APNG (public-domain or generated).
9. README.md + RUNBOOK + `format-codecs.md` § 6.6
   updated; the v0 "first-frame only" policy is documented
   as the Wave 2.4 behaviour.
10. `make ci` green; binary-size delta documented
    (expected: +0.1-0.3 MiB for the `gif` feature; APNG
    reuses existing PNG plumbing).

**Activation gate**: Wave 2.3 shipped + operator
authorisation.

**Why APNG is folded into PNG**: APNG is PNG with an
animation chunk; the existing `Format::Png` pipeline
already accepts APNG files via `image`'s PNG decoder.
The behaviour is "primary frame only" by default. Splitting
APNG into a separate format would require an `apng`-aware
PNG decoder; not justified for v1.

**Cross-cutting tracks touched**: `output_fidelity`
(animated → static conversion; first-frame fidelity is
identical to v0 GIF / APNG handling).

**Out of Wave 2.4 scope**: animated multi-frame
preservation (writing animated GIF or APNG output);
`--all-frames` opt-in flag; per-frame latency budget.

### Wave 2.5 — Multi-page / multi-image formats (planned; `tiff`, `ico`)

**Goal**: enable `tiff` and `ico` input. Both formats
support multiple images per file (TIFF pages, ICO icon
variants at different sizes). The Codec trait gains the
`MultiPageConversionReport` wrapper variant that
Wave 2.2 (PDF activation) introduces.

**Driver issue** (when activated):
`Issues/open/architect/AR-008_add_multipage_format_codecs.md`
+ `Issues/open/developer/DE-NNN_add_multipage_format_codecs.md`.

**Acceptance criteria** (top-level):

1. `Cargo.toml` `image` crate features list extends with
   `tiff` and `ico`. (The `ico` feature is decoder-only in
   the `image` crate; this matches the Wave 2 input-only
   scope cap.)
2. `Format` enum extends with `Tiff` and `Ico`. `Ico` is
   input-only (no encoder in `image`); `Tiff` is input-only
   (no TIFF writer needed for the Wave 2 scope cap).
3. `MultiPageConversionReport` (introduced by Wave 2.2 /
   ADR-0005) is reused for both formats. Each TIFF page
   or ICO variant emits one NDJSON record. Per-page
   source-removal: the source file is removed only after
   **all** pages decode + encode + write successfully
   (mirrors the PDF contract from ADR-0005 § Decision § 9).
4. `TiffToWebp` / `TiffToPng` / `TiffToJpeg` codecs
   implement the multi-page Codec extension. `IcoToWebp` /
   `IcoToPng` / `IcoToJpeg` codecs implement the
   multi-image Codec extension (ICO variants at different
   sizes).
5. `CodecImpl` extended with 6 variants.
6. `report::ImageFormat` extended with `Tiff` and `Ico`.
7. CLI parser `--input-format tiff` / `--input-format ico`
   accepted; `--output-format tiff` / `--output-format ico`
   rejected with exit 2 + usage.
8. `tests/multipage_formats.rs` integration suite:
   - TIFF with 3+ pages → 3+ NDJSON records, one per page.
   - ICO with 3+ sizes → 3+ NDJSON records, one per size.
   - `--output-format tiff` rejected with exit 2.
9. Fixtures: one multi-page TIFF (3+ pages) + one multi-
   size ICO.
10. README.md + RUNBOOK + `format-codecs.md` § 6.7 +
    `integration-contract.md` (NDJSON multi-page semantics
    documented; same shape as the PDF activation will
    introduce).
11. `make ci` green; binary-size delta documented.

**Activation gate**: **Wave 2.2 PDF activation** (the
`MultiPageConversionReport` Codec trait variant is
introduced by PDF; this wave reuses that variant) +
operator authorisation.

**Cross-cutting tracks touched**: `output_fidelity`,
`regression_risk` (multi-page JSON shape; documented at
PDF activation), `cli_ergonomics` (new `--input-format`
enum values).

**Out of Wave 2.5 scope**: TIFF writer; ICO writer;
animated multi-page preservation; `--first-page` flag
(shared with PDF activation); per-page `--pdf-dpi`
analog (`--tiff-resolution` / `--ico-resolution` for
non-1:1 sizing).

### Wave 2.6 — Special formats (planned; `avif`, `exr`)

**Goal**: enable `avif` (AV1 Image File Format) and `exr`
(OpenEXR) input. Both require special handling beyond
the standard `image` crate feature-flag pattern.

**Driver issue** (when activated):
`Issues/open/architect/AR-009_add_special_format_codecs.md`
+ `Issues/open/developer/DE-NNN_add_special_format_codecs.md`.

**Acceptance criteria** (top-level):

1. `Cargo.toml` `image` crate features list extends with
   `avif` and `exr`.
2. `Format` enum extends with `Avif` and `Exr`. Both are
   input-only (matching the Wave 2 scope cap). AVIF
   encoder is **out of scope** — the `image` crate does
   not expose AVIF encoding as of 0.25, and a separate
   `avif-encoder` crate adds binary size for a feature
   the operator has not explicitly requested.
3. `AvifToWebp` / `AvifToPng` / `AvifToJpeg` codecs
   implement the `Codec` trait. The `image` crate's `avif`
   feature uses `avif-decoder` (pure Rust, MIT/Apache-2.0)
   by default — no system `libavif` / `libaom` required
   at build time. (The host-system alternative exists if
   performance demands it; documented as F-1 future work.)
4. `ExrToWebp` / `ExrToPng` / `ExrToJpeg` codecs
   implement the `Codec` trait. EXR is high-dynamic-range;
   the decoded `DynamicImage` is clamped to 8-bit on
   encode (documented as a known limitation in
   `RUNBOOK.md` § 3.6).
5. `CodecImpl` extended with 6 variants.
6. `report::ImageFormat` extended with `Avif` and `Exr`.
7. CLI parser `--input-format avif` / `--input-format exr`
   accepted; `--output-format avif` / `--output-format exr`
   rejected with exit 2 + usage.
8. `tests/special_formats.rs` integration suite:
   - AVIF (AV1-encoded) → WebP / PNG / JPEG round-trip.
   - EXR (HDR float16 / float32) → WebP / PNG / JPEG
     round-trip (output is 8-bit; fidelity loss is
     documented).
   - `--output-format avif` / `--output-format exr`
     rejected with exit 2.
9. Fixtures: one AVIF (small, public-domain or generated)
   + one EXR (small, low-resolution for fixture
   practicality).
10. README.md + RUNBOOK + `format-codecs.md` § 6.8 +
    `integration-contract.md` updated.
11. `make ci` green; binary-size delta documented
    (expected: +0.5-1.5 MiB for AVIF + EXR features).

**Activation gate**: Wave 2.3 + Wave 2.4 + Wave 2.5 shipped
+ operator authorisation + AVIF encoder demand signal
(if operator requests AVIF output, this wave extends to
cover encoder; otherwise stays decoder-only).

**Cross-cutting tracks touched**: `output_fidelity`
(EXR → 8-bit clamp; AVIF decoder fidelity), `build_health`
(`avif-decoder` pure-Rust path; no new system deps).

**Out of Wave 2.6 scope**: AVIF output (encoder); EXR
writer; HDR-fidelity preservation beyond 8-bit clamp;
per-format metadata preservation (EXR channels, AVIF
alpha); F-1 system `libavif` path (documented as future
work for performance-critical AVIF deployments).

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

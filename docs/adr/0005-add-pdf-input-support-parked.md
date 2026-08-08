---
project_slug: convert-to-webp
doc_slug: adr_0005_add_pdf_input_support_parked
doc_type: adr
applicable_roles: [architect, developer]
version: 1
date: 2026-08-08
status: parked
supersedes: null
summary: "Parked: PDF (input-only) is the next format-coverage target after HEIC, but the implementation is deferred until the HEIC (DE-040) wave ships. Captures the architectural analysis so it is not lost. Library: pdfium-render (BSD-3-Clause). Multi-page semantics: render all pages by default with --first-page opt-in. Output naming: input-NNN.ext (zero-padded page index). Scope cap: input-only; --output-format pdf rejected."
source_artifacts:
  - Issues/open/architect/AR-004_park_pdf_input_support.md (parking record)
  - Issues/open/architect/AR-003_add_heic_input_support.md (HEIC precedent; deliver before PDF)
  - Issues/open/developer/DE-040_add_heic_input_codec.md (HEIC implementation; gate on this for PDF activation)
  - docs/adr/0004-add-heic-input-support.md (HEIC; demonstrates the libheif-style build-time approach that PDF will inherit)
  - src/format.rs (Codec trait extension point; will need a multi-page variant for PDF)
  - docs/architecture.md § 6 External Dependencies (PDFium row to be added on activation)
tags: [adr, pdf, pdfium, parked, future-work, format-expansion, wave-2.2, multi-page, input-only, libpdfium]
---

# ADR-0005 — Add PDF input support (parked / future work)

## Status

**Parked** (2026-08-08). Architectural analysis captured; implementation deferred until:

1. The HEIC (DE-040 / ADR-0004) wave ships and the operator
   validates the `libheif`-based build-time approach
   end-to-end on production.
2. The operator expresses renewed demand for PDF input
   (operator chat 2026-08-08 captured the soft ask but did
   not authorise immediate implementation).

Driver parking record:
`Issues/open/architect/AR-004_park_pdf_input_support.md`.

## Date

2026-08-08

## Authors

- Architect (system) — analysis + parking
- Operator (deferred; awaiting activation)

## Context

Operator instruction 2026-08-08 chat: «может добавить еще и
конвертацию pdf в картинку? например кидаешь pdf а тебе
страницы в выбранном формате?».

PDF (Portable Document Format, ISO 32000) is the dominant
document container for scanned pages, reports, ebooks, and
archival material. PDF is **fundamentally different** from the
existing raster image formats (jpg/png/webp/heic) in three
respects:

- **Multi-page**: a single PDF has N pages. The current
  batch-mode contract is "one candidate file in → one or more
  encoded files out per candidate". For PDF, N is variable and
  unbounded; the binary must enforce a page-count cap.
- **Content type**: PDF carries vector graphics, embedded fonts,
  embedded raster images, text, and forms. Rasterisation
  flattens everything to a bitmap per page (the only viable
  approach without re-implementing a layout engine).
- **Container semantics**: PDF is a document format, not an image
  format. The `image` crate does not support PDF as of 0.25;
  this ADR adds a new third-party renderer (PDFium via
  `pdfium-render`).

Three operator use cases motivate PDF input support:

- **U1**: scanned-page archive ingestion — operator has a
  directory of `.pdf` files (one per scanned document, each
  with 1-20 pages) and wants them flattened to per-page
  WebP / JPEG / PNG images for the existing WebP compression
  pipeline.
- **U2**: report extraction — operator receives a PDF report
  from a third party and needs the pages as individual
  images for downstream OCR or visual review.
- **U3**: server-side webhook — same as DE-040's HEIC U3 case,
  but for PDF uploads from a mobile / web client.

## Decision

This ADR is **parked**. The architectural analysis below is
captured so the next activation cycle does not have to
re-derive it, but no implementation work is scheduled in the
current wave plan. The activation criteria are in
§ Activation Criteria below.

When activated, the implementation will commit to:

1. **Renderer**: `pdfium-render` Rust crate binding to
   **PDFium** (Google's PDF engine from Chromium,
   BSD-3-Clause). `pdfium-render` bundles PDFium statically
   via its `static` feature (default in 0.8+) so no
   system-level `libpdfium-dev` is required at build time —
   unlike HEIC's `libheif-dev`. Documented in
   `docs/RUNBOOK.md` § 2.5 (added on activation).

2. **Format enum**: `Format::Pdf` (input only, mirrors the
   `Format::Heic` precedent from ADR-0004). `--output-format
   pdf` is rejected with exit 2 + usage.

3. **Codec family**: `PdfToWebp` / `PdfToPng` / `PdfToJpeg`.
   Each codec wraps a per-page iteration that rasterises one
   page into a `DynamicImage` and re-uses the existing output
   encoder plumbing unchanged.

4. **Page semantics**: **all pages by default**. `--first-page`
   opt-in flag renders only the first page (thumbnail use
   case). `--pages <spec>` optional flag accepts a
   comma-separated list and ranges (e.g. `--pages 1,3,5-7`).

5. **DPI**: `--pdf-dpi <N>` flag with default `150`, range
   `[72, 600]`. 150 DPI is the operator-confirmed default —
   matches the typical web-display density for scanned pages
   and keeps the decoded buffer well under `MAX_DIMENSION`
   (= 16384 px).

6. **Output naming**: `<input_stem>-<NNN>.<ext>` where `NNN`
   is the zero-padded page number. Padding scales: 3 digits
   for ≤ 99 pages (`001`..`099`); 4 digits for ≤ 999 pages
   (`0001`..`0999`); 5 digits above.

7. **JSON shape (multi-page awareness)**: NDJSON emits **one
   record per page**, not one per PDF. `input.path` is the
   PDF, `output.path` is the per-page image. `input.format =
   "pdf"`; `output.format` is the existing `"webp" | "png" |
   "jpeg"`. `schema_version` does **not** bump (additive
   field; consumers parse records independently per
   `docs/contracts/report-shape.md` § 4 INV-RS-4).

8. **Page count cap**: hard limit of 999 pages per PDF
   (matches the zero-padding upper bound). PDFs with > 999
   pages are rejected with `CodecError::Decode` and a
   "page count exceeds limit" message. The cap is
   configurable at the constant site; 999 is the v1
   baseline.

9. **Batch mode source-removal**: the source PDF is removed
   only after **all** pages encode and write successfully. A
   per-page failure leaves the source intact (mirrors the v0
   "all-or-nothing" semantics for single-page candidates).
   Per-page partial outputs are removed by the codec; the
   source stays on disk for operator inspection.

10. **Single-file mode**: `--single-file --input-format pdf`
    is supported but the semantics differ from raster
    formats. The output is a **zip archive** containing one
    image per page, named `<input_stem>-<NNN>.<ext>`. The
    JSON metadata record on stderr includes a new field
    `pages: <N>`. The zip is produced by the `zip` crate
    (added as a dependency on activation).

11. **Scope cap**: PDF support is **input-only**, mirroring
    the HEIC precedent (ADR-0004 § Decision § 6). Image → PDF
    encoder path is out of scope; if operator demand emerges
    later, a separate ADR will authorise it.

## Alternatives Considered (library choice)

### A1 — `pdfium-render` (PDFium / BSD-3-Clause) **[chosen]**

Pure-bindings to Google PDFium (Chromium's PDF engine).
Permissive license (BSD-3-Clause) — same tier as the current
`dav1d` (BSD-2-Clause). Stable Rust crate (0.8.x). Larger
binary delta (~5-10 MiB) because PDFium ships its own
FreeType, libjpeg, libpng, etc. — same libraries the `image`
crate already pulls in, so the duplication is acceptable.
`pdfium-render` bundles PDFium statically by default; no
system-level `libpdfium-dev` required at build time.

### A2 — `poppler-rs` (poppler-cpp / GPL-2)

Rust bindings to poppler via poppler-cpp + cairo + fontconfig.
GPL-2.0 with some components LGPL — copyleft flow-through on
the statically-linked binary. Smaller delta (~1-2 MiB) but
requires the operator to accept GPL contamination, which
contradicts the project's permissive-only dependency
policy. Host system needs `libpoppler-glib-dev` + `libcairo2-dev`
+ `libfontconfig1-dev` (new system dependencies on top of
the existing `libwebp-dev` + `libheif-dev`).

### A3 — `mupdf` (AGPL-3 / commercial)

Bindings to Artifex MuPDF. AGPL-3.0 license — copyleft
contamination UNLESS the operator purchases a commercial
license from Artifex. Best raw rendering quality and speed;
small delta (~1 MiB). Rejected for license reasons: AGPL
flow-through on a static binary is unacceptable without a
paid agreement, and adding a paid dependency is out of
scope for a single-operator CLI.

### A4 — `lopdf` + custom rasterizer (pure Rust)

`lopdf` parses PDF structure but does NOT rasterise. A
custom rasterizer is thousands of LOC and months of work —
not viable for the v1 timeline. Listed for completeness;
rejected on engineering-effort grounds.

## Parking Rationale

Three reasons the implementation is deferred rather than
activated immediately:

1. **Scope-management**: the project just shipped / is
   shipping HEIC (DE-040, ADR-0004), which adds a new
   build-time system dependency (`libheif-dev`) and
   introduces the operator to the static-link-FFI-C-library
   pattern. Adding PDF on top of HEIC — with another
   5-10 MiB binary delta, another static-link-FFI
   dependency (`pdfium-render` + bundled PDFium), and a
   new multi-page JSON shape — risks overwhelming the
   operator's review capacity in a single window.

2. **Validation opportunity**: the HEIC delivery exercises
   the entire build → test → deploy → monitor cycle for a
   third-party C dependency. The PDF activation will
   re-use that operational pattern, and the lessons from
   HEIC's `libheif-sys` static link inform the PDFium
   bundling configuration. Parking PDF until HEIC ships
   reduces the operator's concurrent-waves cognitive load.

3. **Operator demand signal**: the 2026-08-08 chat asked
   "может добавить" ("maybe add") — a soft ask, not an
   authorisation to schedule implementation. Parking
   captures the architectural intent without committing
   the team. When the operator says "now", the activation
   sequence is well-defined (see § Activation Criteria
   below).

## Activation Criteria

The parked ADR becomes a proposed (operator-pending) ADR
when **all** of the following hold:

- The HEIC (DE-040 / ADR-0004) wave has shipped and the
  `libheif` build-time approach has been validated
  end-to-end on the operator's production deployment.
- The operator has explicitly authorised PDF activation in
  chat ("add PDF input" or equivalent), not just a soft ask.
- The broader Wave 2 (GIF / BMP / AVIF / JXL — AR-017
  placeholder) has not yet started, OR has completed
  without scope for PDF. PDF activation slots between
  Wave 2.1 (HEIC) and Wave 2 (broader format expansion)
  — see `docs/ROADMAP.md` § Wave 2.2 (parked).

When activated, the parking record `AR-004` moves from
`Issues/open/architect/` to `Issues/done/architect/`, the
file's `## Metadata.routing` flips from `architect` to
`developer`, and the file is `git mv`-ed to
`Issues/open/developer/<new-id>_*.md` per
`docs/agent_context/AGENT_ISSUE_ROUTING_AND_LOCATION.md`
Rule 4 + Rule 5. The activated developer task receives the
next free `DE-<NNN>` numeric core in the `DE-` namespace.

## Consequences (when activated)

### Positive

- The CLI accepts PDF input — the dominant document
  container for scanned-page archives, reports, and
  ebooks. Operators gain a single-binary, self-contained
  PDF → image pipeline without manual `pdftoppm` /
  ImageMagick pre-conversion.
- Per-page JSON records preserve the v0 NDJSON invariant
  (`docs/contracts/report-shape.md` § 4 INV-RS-4): one
  record per converted file. PDF consumers process N
  records per PDF in completion order.
- `pdfium-render` static bundling (PDFium BSD-3) keeps
  the project's permissive-only dependency policy
  intact (no AGPL / GPL flow-through).
- `--first-page` opt-in flag serves the thumbnail use
  case without re-implementing the v0 "render all" path.

### Negative / Risks

- **R1 (binary-size)**: +5-10 MiB release binary delta
  (PDFium static link). Combined with the +1.5-2.5 MiB
  HEIC delta, the post-HEIC + post-PDF release binary
  size is ~9-14 MiB (vs ~2.4 MiB today). The binary is
  still well under the 50 MiB operator-acceptable
  envelope; documented in `docs/RUNBOOK.md` § 8.2.
- **R2 (PDFium first-build download)**: `pdfium-render`
  bundles a pre-built PDFium binary at first build
  (~30-40 MiB download). Reproducible builds require
  pinning the PDFium version (the crate's
  `Cargo.lock` + the bundled-binary integrity check
  handle this). CI must allow network egress during
  the first build.
- **R3 (multi-page JSON stream length)**: a 100-page
  PDF emits 100 NDJSON records. Consumers parsing
  line-by-line are unaffected; consumers buffering the
  whole stream before parsing need to size their buffer
  accordingly. Documented in
  `docs/integration-contract.md` § 4 (added on
  activation).
- **R4 (DPI-driven memory)**: at 300 DPI, an A4 page is
  ~2480×3508 px ≈ 35 MiB RGBA. The existing
  `MAX_DIMENSION` (16384 px) bound protects against
  pathological input, but the default `--pdf-dpi 150`
  keeps the typical case at ~9 MiB per page. Page count
  cap of 999 limits the cumulative working set on a
  per-file basis; the rayon-driven batch path
  multiplies this by the per-host parallelism.
- **R5 (encrypted PDFs)**: out of scope for v1. Password-
  protected PDFs return `CodecError::Decode` with the
  libpdfium error message; the operator must decrypt
  upstream. Captured in `docs/RUNBOOK.md` § 3.5 (added on
  activation).
- **R6 (vector graphics fidelity)**: rasterisation
  flattens vector graphics to bitmaps. The fidelity is
  comparable to Chromium's PDF rendering at the chosen
  DPI; this is the operator-acceptable behaviour for
  "convert PDF to images" workflows. Operators requiring
  vector preservation need a different tool (e.g.
  `inkscape --export-type=svg`); out of scope.

### License notes (when activated)

- `pdfium-render` (Rust binding): Apache-2.0 OR MIT
  (permissive).
- PDFium (bundled): BSD-3-Clause (permissive, from
  Chromium).
- `zip` (new dep for single-file mode output): MIT or
  Apache-2.0 (permissive; exact license per chosen
  version).

The combined license footprint is acceptable for the
operator's distribution model (single binary,
operator-deployed); no copyleft contamination.

## Follow-up (when activated)

- `Issues/open/architect/AR-004_park_pdf_input_support.md`
  moves to `Issues/done/architect/`; the activated
  developer task file is created under
  `Issues/open/developer/DE-NNN_add_pdf_input_codec.md`
  with the next free `DE-` numeric core.
- `docs/components/format-codecs.md` § 6.5 — new per-
  codec spec for the PDF codec family.
- `docs/architecture.md` § 6 — External Dependencies row
  for `pdfium-render` / PDFium.
- `docs/architecture/STATUS.md` § 3 Captured Trade-offs —
  new row for the PDF build-time + binary-size trade-off.
- `docs/architecture/STATUS.md` § 5 Stack — PDFium row.
- `docs/ROADMAP.md` § Wave 2.2 — un-park, link to
  `DE-NNN` + AR-004.
- `docs/RUNBOOK.md` § 2.5 — operator note for
  `pdfium-render` static-link build (no system dep
  needed).
- `docs/integration-contract.md` — flag surface table
  (`--input-format pdf`, `--first-page`, `--pdf-dpi`,
  `--pages`) + JSON shape multi-page semantics.
- `docs/RUNBOOK.md` § 3.5 — Known limitations (encrypted
  PDFs).

## Cross-References

- `docs/adr/0001-multi-format-cli-scope.md` —
  multi-format CLI scope (extends the converter beyond
  JPG/PNG/WebP; PDF is the next document-format target).
- `docs/adr/0002-preserve-jpg-to-webp-baseline.md` —
  backward-compat baseline (PDF must NOT alter the
  default pipeline).
- `docs/adr/0004-add-heic-input-support.md` — HEIC
  precedent; the build-time pattern validated by HEIC
  is reused for PDF (with `pdfium-render`'s static
  bundling replacing `libheif-sys`).
- `Issues/open/architect/AR-003_add_heic_input_support.md`
  — HEIC proposal (currently active).
- `Issues/open/developer/DE-040_add_heic_input_codec.md`
  — HEIC implementation task (currently queued);
  PDF activation gates on this delivery.
- `Issues/open/architect/AR-004_park_pdf_input_support.md`
  — parking record for this ADR.
- `docs/components/format-codecs.md` — Codec trait +
  per-codec notes (extension site for the PDF codec
  family, added on activation as § 6.5).
- `docs/contracts/codec-bounds.md` — INV-CB-1..INV-CB-8
  apply with multi-page extensions (added on
  activation).
- `docs/contracts/report-shape.md` § 7 — JSON shape
  versioning; the new per-page record shape is
  additive.
- `docs/ROADMAP.md` § Wave 2.2 — parked wave slot.
- `docs/RUNBOOK.md` § 2.4 — operator note for HEIC's
  `libheif-dev` (the precedent pattern; PDF replaces this
  with `pdfium-render` static bundling).
- shared memory::`ar-017-format-coverage-wave-activated`
  — broader format-coverage wave context; PDF is
  distinct from the GIF / BMP / AVIF / JXL that
  AR-017 reserved.

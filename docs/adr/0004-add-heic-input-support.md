---
project_slug: convert-to-webp
doc_slug: adr_0004_add_heic_input_support
doc_type: adr
applicable_roles: [architect, developer]
version: 1
date: 2026-08-08
status: proposed
supersedes: null
summary: "Add HEIC (HEIF container) as a supported input format for the convert-to-webp CLI. Routes through the `image` crate's `heif` feature (which statically links libheif + libde265 + dav1d via libheif-sys). HEIC remains input-only; the converter does not emit HEIC output."
source_artifacts:
  - Issues/open/architect/AR-003_add_heic_input_support.md (driver proposal)
  - Issues/open/developer/DE-040_add_heic_input_codec.md (implementation task)
  - src/format.rs (Codec trait, Format enum, CodecImpl dispatch — existing extension point)
  - src/main.rs (CLI parser + format dispatch — extension site)
  - src/report.rs (ImageFormat enum — JSON shape extension site)
  - Cargo.toml (image crate features — dependency surface)
  - docs/architecture.md § 6 (External Dependencies — addition site)
  - docs/components/format-codecs.md (per-codec spec — extension site)
  - docs/ROADMAP.md Wave 2 (planned format expansion)
  - shared memory::ar-017-format-coverage-wave-activated (wave context — DE-025 was reserved for HEIC under AR-017; this ADR supersedes that placeholder by allocating DE-040 for an isolated HEIC-only task)
tags: [adr, heic, heif, format-expansion, libheif, libde265, dav1d, image-crate, codec, input-only]
---

# ADR-0004 — Add HEIC input support

## Status

**Proposed** (2026-08-08). Awaiting operator approval. Driver:
`Issues/open/architect/AR-003_add_heic_input_support.md`.
Implementation task: `Issues/open/developer/DE-040_add_heic_input_codec.md`.

## Date

2026-08-08

## Authors

- Architect (system) — proposal authoring
- Operator (pending approval)
- Developer (pending execution per DE-040)

## Context

The `convert-to-webp` CLI is being extended from a JPG/PNG/WebP
converter to a broader multi-format converter (ADR-0001, ADR-0002).
Operator has expressed intent (2026-08-08 chat directive): «Нужно
научить конвертер работать с HEIC форматом на вход».

HEIC (High Efficiency Image Container) is the default still-image
format on Apple iOS / iPadOS / macOS since iOS 11 (2017). It is the
container; the inner codec is HEVC (H.265) for files produced by
iOS 11..16, and AV1 for files produced by iOS 17+ (and by recent
Android / camera vendors). The container specification is
ISO/IEC 23008-12 (HEIF).

Three emerging use cases motivate HEIC input support:

- **U1**: an operator has a directory of iPhone photos in HEIC and
  wants to feed them through the existing WebP compression pipeline
  without manual `sips` / ImageMagick pre-conversion.
- **U2**: a photo-archive ingestion job lands mixed JPG + HEIC files
  in one directory; the converter must accept both transparently.
- **U3**: a server-side webhook receives HEIC uploads from a mobile
  client; the convert-to-webp subprocess needs to decode them in
  `--single-file` mode.

The `image` crate (version 0.25, currently the only image-format
abstraction in `Cargo.toml`) exposes HEIF decoding through the
optional `heif` feature. The `heif` feature pulls in `libheif-sys`,
which statically links `libheif` together with its default codec
plugins (`libde265` for HEVC, `dav1d` for AV1). The `image` crate
does not currently expose HEIF *encoding* through any feature flag
(no public HEIF encoder surface as of `image` 0.25).

## Decision

We will add HEIC as an **input-only** format to the converter, with
the following technical commitments:

1. **Decoder crate**: enable the `heif` feature on the existing
   `image` crate dependency (`Cargo.toml`). This brings in
   `libheif-sys` transitively. `libheif-sys` builds `libheif` from
   source together with `libde265` (HEVC decoder) and `dav1d` (AV1
   decoder) — both plugins are required to cover the iOS 11..16
   (HEVC) and iOS 17+ (AV1) file populations.

2. **Build-time system dependency**: `libheif` 1.21+ development
   headers are required at build time when the `heif` feature is
   enabled. The floor is set by `libheif-sys` 5.x (pulled in
   transitively via `libheif-rs` 2.7 + `image::heif`): its
   `system_deps` table declares `v1_17..v1_23` floors at the
   matching versions, and the `latest` feature (default in
   `Cargo.lock`) activates `v1_23`, so anything below 1.21 fails
   the binding build with `requires libheif >= 1.21` before
   touching the rest of the build graph. On Debian / Ubuntu when
   apt ships a recent enough `libheif-dev`:
   `sudo apt install libheif-dev libde265-dev libdav1d-dev`. On
   Debian / Ubuntu when apt only has 1.17.x / 1.18.x (the common
   case on Ubuntu 22.04 / 24.04 LTS and Debian 12 — the failure
   surface that motivated DE-044): install only the codec
   dependencies and rebuild libheif from source via the repo
   helper, `sudo scripts/install_libheif.sh --yes`. On
   Arch: `sudo pacman -S libheif` (rolling distros ship 1.21+).
   On macOS via Homebrew: `brew install libheif`. The system
   `pkg-config` must find `libheif`; the existing `build.rs` for
   `image` / `webp` already establishes the `pkg-config` toolchain.

3. **Codec integration**: implement three new codecs in `src/format.rs`
   — `HeicToWebp`, `HeicToPng`, `HeicToJpeg` — following the
   existing Codec trait (`accepted_extensions`,
   `output_extension`, `decode`, `encode_to_vec`). Wire them into
   the `Format` enum (`Format::Heic`) and the `CodecImpl` dispatch
   in `src/main.rs:350-364`. The default pipeline remains
   `Format::Jpg → Format::Webp` (ADR-0002 byte-equivalence invariant
   is preserved).

4. **JSON shape**: extend `report::ImageFormat` with a `Heic` variant
   (`src/report.rs:79-103`). The JSON value is `"heic"` (matches the
   CLI token; not `"heif"` — the file extension is `.heic` for the
   Apple convention). The `schema_version` does **not** bump
   (additive field, backwards-compatible interpretation per
   `docs/contracts/report-shape.md` § 7 INV-RS-2).

5. **CLI parser**: `--input-format heic` (and `heif` as an alias
   for input, matching the container/container-name convention) is
   accepted; `--output-format heic` is rejected with usage + exit 2
   (input-only scope, mirrors how the binary does not currently
   emit AVIF or TIFF either).

6. **Scope cap**: HEIC support is **input-only**. We will not add a
   HEIC encoder path. The justification is operational: the
   target output formats (WebP, JPEG, PNG) cover every downstream
   consumer the operator has expressed, and adding a HEIC encoder
   would require either the `image` crate's HEIF encoder (not
   currently exposed as a feature in 0.25) or a separate
   `libheif` encoder binding — a 2-3x binary-size delta with no
   operator demand for HEIC output.

7. **Test fixtures**: HEIC fixtures are committed to
   `tests/fixtures/heic/` in their canonical iOS-emitted form. At
   minimum: 1× photographic portrait (HEVC, ~3 MP), 1× photographic
   landscape (HEVC, ~12 MP), 1× AV1-encoded iOS-17+ sample
   (down-sampled to < 1 MiB to keep the repo lean). The expected
   WebP outputs are recorded under `tests/fixtures/heic/expected/`
   for the deterministic golden test.

## Alternatives Considered

### A1 — Shell out to `heif-convert` (libheif CLI)

Spawn the `heif-convert` CLI as a subprocess for every HEIC input.
**Rejected**: doubles the per-file process-spawn cost (the v0
baseline showed an 8.5× win over ImageMagick by avoiding process
spawns; reverting to subprocess-per-file for HEIC would erase that
win on the HEIC subset). Adds a deploy-time binary dependency on
`heif-convert`.

### A2 — Use `libheif-rs` (safe Rust wrapper around libheif)

Direct FFI through `libheif-rs` instead of the `image` crate's
`heif` feature. **Rejected**: duplicates the existing decoder
abstraction (`image::ImageReader`); creates two parallel code paths
(HEIC through `libheif-rs`, JPG/PNG/WebP through `image`). The
`image` crate integration is the architecturally consistent choice.

### A3 — Build HEIC decoder statically only (no HEVC plugin)

Build `libheif` with only the `dav1d` (AV1) plugin and skip
`libde265` (HEVC). **Rejected**: drops iOS 11..16 file compatibility.
Roughly half of the HEIC files in the wild are HEVC-encoded; we
cannot ship with that gap.

### A4 — Build HEIC decoder statically only (no AV1 plugin)

Build `libheif` with only the `libde265` (HEVC) plugin and skip
`dav1d`. **Rejected**: drops iOS 17+ file compatibility. Newer
operators (2024+) may have only AV1-encoded HEIC files.

### A5 — Make HEIC an opt-in build feature (default off)

Add a Cargo feature flag `heic-input` that defaults to off; only
builds with `--features heic-input` link `libheif`. **Rejected**:
contradicts the v0 architectural principle that the binary is
self-contained (one binary covers all input formats). Operators
running a mixed-format batch would have to remember to enable the
feature. Captured as a future-work item (F-1) if binary-size
pressure becomes a real concern.

### A6 — Use the `heif` crate directly (Rust wrapper around libheif 2.x)

The `heif` crate (distinct from `libheif-rs`) provides a more
direct binding. **Rejected**: as A2, this duplicates the
`image::ImageReader` abstraction. The `image` crate already
maintains the format-detection + decode-error-mapping plumbing.

### A7 — Add HEIC encoder (output-side) at the same time

Symmetric surface: input HEIC, output HEIC. **Rejected**: no
operator demand for HEIC output (the target outputs WebP/JPEG/PNG
already cover all stated use cases); adds 2-3x to the encoder
plumbing without justification. Scoped as a separate future ADR if
demand emerges.

## Consequences

### Positive

- The CLI accepts the dominant still-image format on Apple
  devices, removing a manual pre-conversion step from operator
  pipelines.
- Re-uses the existing Codec trait + `Format` enum extension
  point; no architectural restructuring.
- HEVC + AV1 dual-plugin coverage ensures both iOS 11..16 and
  iOS 17+ files decode end-to-end.
- The `image` crate's `heif` feature keeps the dependency surface
  aligned with the existing decoder pattern (one feature per
  input format).

### Negative / Risks

- **R1 (build-time complexity)**: `libheif-sys` pulls in
  `libde265` + `dav1d`. The combined static-link adds roughly
  1.5-2.5 MiB to the release binary (vs ~2.4 MiB today). Mitigated
  by documenting the delta in the developer PR description.
- **R2 (HEVC patent exposure)**: `libde265` decodes HEVC. The
  patent landscape around HEVC is jurisdiction-dependent; in many
  jurisdictions, decoding HEVC for personal / non-commercial use
  is unencumbered, while redistributing a HEVC encoder is
  encumbered. The decoder-only use here is the safer side, but
  operators in jurisdictions with broader HEVC patent claims
  (notably the US, until the patent pool expires) should evaluate
  the risk. Captured in `docs/RUNBOOK.md` § Build-time failures
  (operator-facing note); the binary itself remains free of
  redistributing a HEVC encoder.
- **R3 (CI build time)**: building `libheif` + `libde265` +
  `dav1d` from source adds ~60-90 s to a clean CI build. Mitigated
  by caching (`sccache`) and by documenting the expected CI
  duration delta.
- **R4 (single-image HEIC in `--single-file` mode)**: the
  existing `decode_bytes` path uses `image::load_from_memory` which
  honors the `heif` feature transparently. Verified during
  development; no separate code path needed.
- **R5 (HEIC files with depth maps / multiple images)**: HEIF
  supports multi-image containers (Apple's "Live Photos",
  depth-of-field variants). The `image` crate's HEIF decoder
  extracts the primary image only, which matches the v0 baseline
  behaviour for animated GIF / APNG (first frame only). Captured
  in `docs/RUNBOOK.md` § Known limitations.

### Patent / license notes

- `libheif`: LGPL-2.1+ (or GPL-2.0+ at the user's option). Static
  linking under LGPL-2.1 requires the operator to retain the
  ability to relink the binary against a modified `libheif`;
  because the binary is statically linked, this is documented in
  `docs/RUNBOOK.md` (operator note: source-available relink
  procedure).
- `libde265`: GPL-2.0+ with a linking exception that allows
  linking from non-GPL applications when `libde265` is used as
  a `libheif` plugin. The exception text is reproduced in
  `docs/RUNBOOK.md` for operator visibility.
- `dav1d`: BSD-2-Clause (permissive, no redistribution constraint).
- `libheif-sys` (Rust binding): MIT.

The combined license footprint is acceptable for the operator's
distribution model (single binary, operator-deployed); no
copyleft contamination of the application code.

## Follow-up

- `Issues/open/architect/AR-003_add_heic_input_support.md` —
  driver proposal.
- `Issues/open/developer/DE-040_add_heic_input_codec.md` —
  implementation task with acceptance criteria + commit sequence.
- `Issues/open/developer/DE-044_upgrade_libheif_build_requirement.md`
  — supply-side fix that ensures the libheif >= 1.21 floor is
  satisfied on distros whose apt only ships 1.17.x / 1.18.x.
  Provides `scripts/install_libheif.sh` and the CI workflow
  wiring.
- `docs/components/format-codecs.md` § 6.4 — new per-codec spec.
- `docs/architecture.md` § 6 — External Dependencies row for
  `libheif`.
- `docs/architecture/STATUS.md` § 3 Captured Trade-offs — new
  row for "HEIC input-only" trade-off.
- `docs/ROADMAP.md` — Wave 2 update for HEIC delivery.
- `docs/RUNBOOK.md` § Build-time failures — operator note for
  `libheif-dev` install + license / patent notes.
- `Issues/open/developer/DE-044_upgrade_libheif_build_requirement.md`
  — supply-side fix that ensures the libheif >= 1.21 floor is
  satisfied on distros whose apt only ships 1.17.x / 1.18.x.
  Provides `scripts/install_libheif.sh` and the CI workflow
  wiring.

## F-1 — Future work (out of scope)

If the release binary size budget becomes a real constraint:

- Add a Cargo feature `heic-input` (default off) that gates the
  `image` crate's `heif` feature. Build with `--features
  heic-input` to enable HEIC input; builds without the feature
  skip the `libheif` / `libde265` / `dav1d` static-link step
  (~1.5-2.5 MiB binary delta recovered).
- The default behaviour (HEIC enabled, self-contained binary) is
  preserved until the operator explicitly requests the feature
  flag.

## F-2 — Future work (out of scope)

If operator demand emerges for HEIC output (e.g. archival
ingestion that needs to preserve HEIC-encoded metadata):

- Add a separate ADR (`adr/0005-add-heic-output.md` or similar)
  authorising a HEIC encoder path. Likely implementation:
  direct `libheif` encoder FFI (the `image` crate does not
  expose HEIF encoding as of 0.25). Documented here so the
  future-ADR author has a clear precedent for the
  alternatives-considered set.

## Cross-references

- ADR-0001 — Multi-format CLI scope (extends the converter beyond
  JPG/PNG/WebP; HEIC is one of the listed Wave 2 targets).
- ADR-0002 — Preserve v0 JPG → WebP baseline (HEIC must NOT alter
  the default pipeline).
- `docs/components/format-codecs.md` — Codec trait + per-codec
  notes (extension site for the new codec).
- `docs/contracts/codec-bounds.md` — INV-CB-1..INV-CB-8 apply
  unchanged to the new codec.
- `docs/contracts/report-shape.md` § 7 — JSON shape versioning;
  the new `heic` value for `input.format` is additive (no
  `schema_version` bump).
- `docs/RUNBOOK.md` § Build-time failures — operator note for
  `libheif-dev` + license / patent notes.
- `Issues/open/architect/AR-003_add_heic_input_support.md` —
  driver proposal.
- `Issues/open/developer/DE-040_add_heic_input_codec.md` —
  implementation task.
- shared memory::`ar-017-format-coverage-wave-activated` — wave
  context. AR-017 reserved `DE-025` for HEIC input under the
  format-coverage wave; this ADR supersedes that reservation by
  allocating `DE-040` for an isolated HEIC-only task (HEIC is
  delivered ahead of GIF / BMP / AVIF / JXL so the operator can
  validate the build-time + dependency approach before the
  broader wave).

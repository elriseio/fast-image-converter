---
project_slug: convert-to-webp
doc_slug: component_format_codecs
doc_type: component_doc
applicable_roles: [architect, developer]
version: 2
source_artifacts:
  - src/main.rs:11-15, 101-136
  - src/format.rs (Codec trait, Format enum, CodecImpl dispatch; extension point for new formats)
  - Cargo.toml (image crate features; heif extension site per ADR-0004)
  - docs/adr/0004-add-heic-input-support.md (HEIC input-only decision)
  - Issues/open/architect/AR-003_add_heic_input_support.md (driver proposal)
  - Issues/open/developer/DE-040_add_heic_input_codec.md (implementation task)
summary: "Format codecs component: per-format decode/encode/resize policy. Wave 1 surfaces the Codec trait with jpg/png/webp implementations; Wave 2 adds HEIC input via libheif (ADR-0004, DE-040)."
tags: [component, codecs, jpeg, png, webp, heic, image-crate, libheif]
---

# Component: `format-codecs`

## 1. Purpose

Encode and decode images per the chosen pipeline. Each
implementation owns its input-extension set, decode function,
resize policy, quality parameter, and encode function. The
v0 baseline has one implementation: `jpeg → webp (portrait/landscape,
quality=85)`. Wave 1 introduces a `Codec` trait and at least three
implementations (jpg, png, webp) so the CLI can dispatch
generically.

## 2. Inputs

- A source `&Path` (a regular file).
- The codec's per-format parameters (quality, resize policy).

## 3. Outputs

- A `(in_bytes, out_bytes)` pair on success.
- An `Err(String)` on failure (decode / encode / I/O).

## 4. Invariants

- **INV-CODEC-1**: a codec MUST NOT mutate its inputs (the source
  file is read-only during decode).
- **INV-CODEC-2**: a codec MUST report the byte counts it observed
  (in: `fs::metadata(src).len()`, out: encoded buffer length).
- **INV-CODEC-3**: the encode path MUST produce deterministic
  output for deterministic inputs (same file, same flags →
  byte-identical output). Enforced for WebP via `webp::Encoder`
  with fixed quality; PNG / JPEG determinism is library-dependent
  and is documented per-impl.
- **INV-CODEC-4**: a codec MUST NOT depend on `converter-core`
  state; it is a pure function `(path, params) -> Result`.
- **INV-CODEC-5**: the v0 `gallery-compress` codec (jpg → webp,
  per-orientation resize, quality 85) is preserved bit-for-bit
  for the no-flags path; see `adr/0002-preserve-jpg-to-webp-baseline.md`.

## 5. Wave 1 Trait Sketch

```rust
trait Codec {
    fn accepted_extensions(&self) -> &'static [&'static str];
    fn quality(&self) -> u8;
    fn resize_policy(&self) -> ResizePolicy;

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError>;
    fn encode(&self, img: &image::DynamicImage, dst: &Path)
        -> Result<u64, CodecError>;
}

enum ResizePolicy {
    None,
    MaxWidth(u32),
    PortraitLandscape { portrait: u32, landscape: u32 },
}
```

## 6. Per-Codec Notes

### 6.1 `jpeg → webp` (v0 baseline)

- Decode: `image::ImageReader::open(...).with_guessed_format()?.decode()?`
- Resize: `PortraitLandscape { portrait: 800, landscape: 1000 }`
  with `FilterType::Lanczos3`.
- Encode: `webp::Encoder::from_rgb(...).encode(85.0)`.
- Quality scale: `f32` in `[0.0, 100.0]` (v0 uses `85.0`).
- Memory peak: dominated by the decoded `DynamicImage` (~4 bytes
  per pixel × width × height). A 4096×4096 RGBA image is ~64 MiB.

### 6.2 `png → webp` (Wave 1)

- Decode: same as `jpeg → webp` (the `image` crate auto-detects
  PNG via `with_guessed_format`).
- Resize: same default `PortraitLandscape` policy; overridable.
- Encode: same as `jpeg → webp`.
- Caveat: PNG can carry an alpha channel. `to_rgb8()` in v0
  drops the alpha. Wave 1 must preserve alpha via `to_rgba8()`
  when present, and the WebP encoder MUST be configured for
  lossy-with-alpha encoding. This is a behavioural change vs v0
  for PNG sources and is captured in ADR-0001 § Decision § 2.

### 6.3 `webp → png` / `webp → jpg` (Wave 1)

- Decode: `image::ImageReader` auto-detects WebP. The
  `Cargo.toml` `image` features list (`["jpeg", "png", "webp"]`)
  already enables the WebP decoder feature; the dedicated `webp`
  crate is encoder-only and is not used on the decode path.
- Resize: same `PortraitLandscape` policy.
- Encode: `image::codecs::png::PngEncoder` /
  `image::codecs::jpeg::JpegEncoder`. PNG is lossless; JPEG uses
  the same `quality` value mapped to the JPEG `[1, 100]` scale.

### 6.4 `heic → webp` / `heic → png` / `heic → jpg` (Wave 2, DE-040)

- Scope: **input-only**. The `--output-format heic` invocation
  exits 2 with usage. HEIC is not a supported output format. The
  rationale is operational: every operator use case (Apple device
  photo ingestion, mixed-batch ingestion, server-side webhook
  upload) targets the existing WebP / PNG / JPEG output set; no
  demand for HEIC output has been expressed. Captured as
  ADR-0004 § Decision § 6; future ADR if demand emerges.
- Accepted extensions: `["heic"]`. The `.heif` extension is
  rejected at the file-extension match to keep the operator's
  mental model consistent with Apple convention (Apple devices
  emit `.heic`, not `.heif`; the `.heif` extension is reserved
  for raw HEIF container use). The CLI flag `--input-format heif`
  is accepted as an alias for `--input-format heic`.
- Decode: `libheif_rs::HeifContext::read_from_reader(Box::new(StreamReader::new(Cursor::new(bytes), total)))`
  — the `libheif-rs` safe wrapper around `libheif-sys` (which
  links the system `libheif` C library together with the
  `libde265` HEVC and `dav1d` AV1 decoder plugins). The
  `image` crate's `heif` feature flag does **not** exist in
  the 0.25 line (or 0.24), so HEIC decoding routes through
  `libheif-rs` directly rather than via the `image::ImageReader`
  content-sniffing path. Both inner codecs are required to cover
  the iOS 11..16 (HEVC) and iOS 17+ (AV1) file populations. The
  decode honours the HEIF container's `irot` / `imir`
  geometric transformations via `LibHeif::decode`'s automatic
  transform pass; alpha is preserved when the source carries an
  alpha auxiliary image. The decoded interleaved plane is
  copied into a fresh `image::RgbImage` or `image::RgbaImage`
  (depending on `has_alpha_channel`) and wrapped in a
  `DynamicImage`. See `decode_heic_bytes` in `src/format.rs` for
  the implementation.
- Resize: same `PortraitLandscape` policy as the existing codecs
  (`auto:portrait=800,landscape=1000` by default; overridable via
  `--resize`).
- Encode: re-uses the existing output encoders unchanged — WebP
  through `webp::Encoder::from_rgb` (or `from_rgba` for sources
  with alpha), PNG through `image::codecs::png::PngEncoder`,
  JPEG through `encode_jpeg_mozjpeg` (MozJPEG). HEIC is on the
  input side only; the encode side is the existing plumbing.
- Alpha: HEIC supports alpha (the HEIF container carries alpha
  in a separate auxiliary image; `libheif` recombines them on
  decode). The existing `to_rgb8` / `to_rgba8` plumbing in the
  encoder path handles the alpha correctly per output format
  (WebP and PNG preserve alpha; JPEG drops it via `to_rgb8`).
- Build-time dependency: `libheif` 1.14+ development headers
  must be present at compile time when the `heif` feature is
  enabled. On Debian / Ubuntu: `sudo apt install libheif-dev
  libde265-dev dav1d-dev`. On Arch: `sudo pacman -S libheif`.
  On macOS via Homebrew: `brew install libheif`. The
  `pkg-config` toolchain must find `libheif`; the existing
  build.rs for `image` / `webp` already establishes that
  toolchain. Documented in `docs/RUNBOOK.md` § 2.4 (operator
  note).
- Multi-image containers: HEIF supports multi-image files
  (Apple's "Live Photos", depth-of-field variants). The `image`
  crate's HEIF decoder extracts the primary image only, which
  matches the v0 baseline behaviour for animated GIF / APNG
  (first frame only). Captured in `docs/RUNBOOK.md` § 3.4
  (Known limitations).
- License / patent: `libheif` (LGPL-2.1+, static-link relink
  procedure documented in RUNBOOK); `libde265` (GPL-2.0+ with
  linking exception when used as a `libheif` plugin); `dav1d`
  (BSD-2-Clause, permissive). HEVC decoder-only use does not
  encumber HEVC patent claims in the operator's distribution
  model; the operator should evaluate the patent landscape in
  their jurisdiction. Documented in `docs/RUNBOOK.md` § 2.4.

## 7. Memory Budget

| Source size | Decoded buffer (RGBA) | Notes |
|---|---|---|
| 1024×1024 | ~4 MiB | comfortable on any host |
| 4096×4096 | ~64 MiB | fits the Wave 1 budget |
| 8192×8192 | ~256 MiB | at the Wave 1 budget ceiling; may OOM on small hosts |

Future Wave 2+: tiled decode / streaming encode for very large
images. Out of Wave 1 scope.

## 8. Failure Modes

| Mode | Trigger | Behaviour |
|---|---|---|
| Decode failure | corrupt / non-image / unsupported format | `Err(CodecError::Decode)`; surfaced to `converter-core` |
| Resize failure | (not expected; `image::imageops` does not return Result) | n/a |
| Encode failure | encoder OOM / unsupported pixel layout | `Err(CodecError::Encode)` |
| Output write failure | disk full / permission denied | `Err(CodecError::Io)` |
| Source delete failure (v0 only) | `fs::remove_file` fails | `Err(CodecError::Io)`; converted artefact remains on disk |

## 9. Future Work

- Wave 2 (post-DE-040): add `gif`, `bmp`, `tiff`, `avif` per
  ROADMAP § Wave 2. HEIC is delivered ahead of these (DE-040)
  so the operator can validate the `libheif`-based build-time
  approach (R1..R3 in ADR-0004) before committing to the
  broader wave.
- Wave 2: alpha-channel-aware WebP encoding (currently lossy
  without alpha via `to_rgb8`).
- Wave 3: streaming encode for very large images.
- ADR-0004 F-1: if binary-size pressure becomes a real
  constraint, add a Cargo feature `heic-input` (default off)
  that gates the `image` crate's `heif` feature; builds without
  the feature skip the `libheif` / `libde265` / `dav1d`
  static-link step (~1.5-2.5 MiB binary delta recovered).
- ADR-0004 F-2: if operator demand emerges for HEIC output,
  add a separate ADR authorising a HEIC encoder path; the
  `image` crate does not expose HEIF encoding as of 0.25,
  so this requires direct `libheif` encoder FFI.

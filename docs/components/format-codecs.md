---
project_slug: fast-image-converter
doc_slug: component_format_codecs
doc_type: component_doc
applicable_roles: [architect, developer]
version: 1
source_artifacts:
  - src/main.rs:11-15, 101-136
  - Cargo.toml (dependency declaration)
summary: "Format codecs component: per-format decode/encode/resize policy. Wave 1 surfaces the Codec trait with jpg/png/webp implementations."
tags: [component, codecs, jpeg, png, webp, image-crate]
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

- Wave 2: add `gif`, `bmp`, `tiff`, `avif`.
- Wave 2: alpha-channel-aware WebP encoding (currently lossy
  without alpha via `to_rgb8`).
- Wave 3: streaming encode for very large images.

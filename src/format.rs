use std::fmt;
use std::io::Cursor;
use std::path::Path;
use std::str::FromStr;

use libheif_rs as lh;

use crate::params::Params;

/// Hard input-byte limits enforced before decode.
///
/// The limits are conservative for a desktop / server CLI converting
/// typical photo batches. Operators handling genuinely large images
/// (e.g. raw 16-bit scans > 100 MiB) must explicitly opt into the
/// larger limit by editing `MAX_STDIN_BYTES` / `MAX_BATCH_FILE_BYTES`
/// here; there is no runtime override because the limit exists to
/// keep the binary's memory footprint bounded, not to be tunable
/// per-request.
///
/// `MAX_BATCH_FILE_BYTES` is set to the same value as `MAX_STDIN_BYTES`
/// on purpose: the batch path can decode many files in parallel under
/// rayon, so a per-file cap is what keeps the cumulative working set
/// bounded; per the issue the two are documented as one policy with
/// a single constant.
pub const MAX_STDIN_BYTES: u64 = 100 * 1024 * 1024;
pub const MAX_BATCH_FILE_BYTES: u64 = MAX_STDIN_BYTES;

/// Hard per-dimension limit enforced after decode.
///
/// Decoded images with width or height greater than this are rejected
/// before any allocation arithmetic that depends on `width * height`.
/// The chosen value (16384) is well under the practical max for a
/// 64-bit usize working set at 4 channels per pixel (~1 GiB).
pub const MAX_DIMENSION: u32 = 16384;

/// Check an input byte count against the per-file limit. Returns an
/// `Err` string suitable for inclusion in `CodecError::Io(_)` when
/// the limit is exceeded (rejection must surface as a
/// deterministic runtime error, never a panic).
pub(crate) fn check_input_size(bytes: u64) -> Result<(), String> {
    if bytes > MAX_BATCH_FILE_BYTES {
        Err(format!(
            "input file exceeds the per-file size limit of {MAX_BATCH_FILE_BYTES} bytes \
             (got {bytes} bytes); see docs/contracts/codec-bounds.md § 4"
        ))
    } else {
        Ok(())
    }
}

/// Check a decoded image's pixel dimensions against the per-dimension
/// limit (rejection must surface as a deterministic runtime
/// error, never a panic).
pub(crate) fn check_dimensions(width: u32, height: u32) -> Result<(), String> {
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        Err(format!(
            "decoded image dimensions {width}x{height} exceed the per-dimension limit \
             of {MAX_DIMENSION}; see docs/contracts/codec-bounds.md § 4"
        ))
    } else {
        Ok(())
    }
}

/// Compute `width * height` as a `usize` using checked arithmetic.
/// Returns `Err` when the product would overflow `usize` or when
/// either dimension exceeds `MAX_DIMENSION`. Codecs that allocate a
/// pixel buffer of size `w * h` MUST call this before allocating
/// (every codec allocation site uses checked arithmetic).
pub(crate) fn checked_pixel_capacity(width: u32, height: u32) -> Result<usize, CodecError> {
    check_dimensions(width, height).map_err(CodecError::Decode)?;
    width
        .checked_mul(height)
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| {
            CodecError::Decode(format!(
                "pixel count {width}x{height} overflows usize; see \
                 docs/contracts/codec-bounds.md § 4"
            ))
        })
}

/// Resize policy applied to a decoded image before encoding.
///
/// The v0 baseline is `PortraitLandscape { portrait: 800, landscape: 1000 }`
/// (see `docs/adr/0002-preserve-jpg-to-webp-baseline.md`). The `Fit`
/// variant carries the `--resize fit=<mode> long-edge=<N>`
/// 3-arg form; `Fit` is a deliberate superset of the older shapes
/// (it accepts three fit modes that the older shapes cannot express).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizePolicy {
    None,
    MaxWidth(u32),
    PortraitLandscape { portrait: u32, landscape: u32 },
    Fit { mode: FitMode, long_edge: u32 },
}

/// Fit-mode enum used by the
/// `--resize fit=<mode> long-edge=<N>` 3-arg form.
///
/// The three modes map onto the three image-resize semantics the
/// page-side advanced panel exposes (per the elrise.io side of
/// DE-031):
///
/// - `Contain`: scale the source so the longer side equals
///   `long_edge`, preserving the aspect ratio. The shorter side
///   is computed proportionally; nothing is cropped, nothing is
///   padded. Mirrors CSS `object-fit: contain`.
/// - `Cover`: scale the source so the shorter side equals
///   `long_edge`, preserving the aspect ratio, then centre-crop
///   the longer side to produce an exact `long_edge × long_edge`
///   output. Mirrors CSS `object-fit: cover`.
/// - `Stretch`: resize to exactly `long_edge × long_edge`,
///   ignoring the source aspect ratio. Mirrors CSS
///   `object-fit: fill`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FitMode {
    Contain,
    Cover,
    Stretch,
}

impl FitMode {
    /// Lower-case canonical CLI / JSON spelling. Matches the
    /// `<mode>` token in `--resize fit=<mode> long-edge=<N>` and
    /// the round-trip JSON `resize_policy` field.
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            FitMode::Contain => "contain",
            FitMode::Cover => "cover",
            FitMode::Stretch => "stretch",
        }
    }
}

impl fmt::Display for FitMode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for FitMode {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "contain" => Ok(FitMode::Contain),
            "cover" => Ok(FitMode::Cover),
            "stretch" => Ok(FitMode::Stretch),
            other => Err(format!(
                "unknown fit mode: {other:?} (expected contain, cover, or stretch)"
            )),
        }
    }
}

impl ResizePolicy {
    /// Resolve the target pixel dimensions `(width, height)` for an
    /// image of the given `(width, height)` under this policy.
    /// Returns the input dimensions unchanged when the policy is
    /// `None` or when the input is already at or below the cap
    /// (no upscaling for the legacy shapes; `Fit::Stretch` is the
    /// exception and always produces a `long_edge × long_edge`
    /// output).
    pub fn target_dimensions(&self, width: u32, height: u32) -> (u32, u32) {
        match *self {
            ResizePolicy::None => (width, height),
            ResizePolicy::MaxWidth(m) => {
                if width <= m {
                    (width, height)
                } else {
                    (m, height.saturating_mul(m) / width)
                }
            }
            ResizePolicy::PortraitLandscape {
                portrait,
                landscape,
            } => {
                let cap = if height >= width { portrait } else { landscape };
                if width <= cap {
                    (width, height)
                } else {
                    (cap, height.saturating_mul(cap) / width)
                }
            }
            ResizePolicy::Fit { mode, long_edge } => match mode {
                FitMode::Contain => {
                    if width >= height {
                        if width <= long_edge {
                            (width, height)
                        } else {
                            (long_edge, height.saturating_mul(long_edge) / width)
                        }
                    } else if height <= long_edge {
                        (width, height)
                    } else {
                        (width.saturating_mul(long_edge) / height, long_edge)
                    }
                }
                FitMode::Cover => (long_edge, long_edge),
                FitMode::Stretch => (long_edge, long_edge),
            },
        }
    }
}

impl Default for ResizePolicy {
    fn default() -> Self {
        // v0 baseline per ADR-0002.
        ResizePolicy::PortraitLandscape {
            portrait: 800,
            landscape: 1000,
        }
    }
}

/// Codec error variants. Mapped to exit codes by `converter-core` and
/// `cli-frontend` (see `docs/contracts/codec-bounds.md`).
#[derive(Debug)]
pub enum CodecError {
    Decode(String),
    Encode(String),
    Io(String),
}

impl fmt::Display for CodecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CodecError::Decode(m) => write!(f, "decode error: {m}"),
            CodecError::Encode(m) => write!(f, "encode error: {m}"),
            CodecError::Io(m) => write!(f, "io error: {m}"),
        }
    }
}

impl std::error::Error for CodecError {}

/// Per-format decode + encode + resize policy bundle.
///
/// One `Codec` instance represents a single (input_format, output_format)
/// pipeline. The CLI dispatches the chosen codec per file. The codec is
/// a pure function `(src, dst, params) -> Result<ConversionReport, CodecError>`;
/// it holds no per-file state.
pub trait Codec {
    /// File extensions accepted as input (case-insensitive match).
    fn accepted_extensions(&self) -> &'static [&'static str];

    /// Output file extension (without the leading dot).
    fn output_extension(&self) -> &'static str;

    /// Decode the source file into a `DynamicImage`.
    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError>;

    /// Decode bytes that are already in memory (used by single-file
    /// stdin mode).
    fn decode_bytes(&self, bytes: &[u8]) -> Result<image::DynamicImage, CodecError> {
        image::load_from_memory(bytes).map_err(|e| CodecError::Decode(e.to_string()))
    }

    /// Encode the decoded image into an in-memory byte buffer. This
    /// method has no default implementation: every concrete codec
    /// must build the bytes directly without going through a
    /// temporary file. The previous default implementation wrote
    /// to a path derived from `std::process::id()` in
    /// `std::env::temp_dir()` — predictable across concurrent
    /// conversions from the same process and removable by a
    /// hostile actor with write access to `/tmp`. Removing it
    /// forces every codec to allocate its own `Vec<u8>` and
    /// eliminates the predictable temporary path.
    fn encode_to_vec(&self, img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, CodecError>;

    /// Encode the decoded image to `dst` and return the number of bytes
    /// written. The codec is responsible for the directory-existence
    /// contract (the source's parent is guaranteed to exist by the
    /// caller). The default implementation writes the v0-baseline
    /// quality-85 bytes from `encode_to_vec`; codecs that need a
    /// different default can override.
    #[allow(dead_code)] // retained on the trait for symmetry with
                        // `encode_to_vec`; the binary calls
                        // `convert_one_with`, which writes through
                        // `encode_to_vec_with_opts` + `fs::write`.
    fn encode(&self, img: &image::DynamicImage, dst: &Path) -> Result<u64, CodecError> {
        let bytes = self.encode_to_vec(img, 85)?;
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }

    /// Encode honouring the caller-supplied quality. Default ignores
    /// `quality` and delegates to `encode` (matches the v0 baseline
    /// where quality is hard-coded). Codecs whose output format has
    /// a meaningful quality knob override this.
    #[allow(dead_code)] // same rationale as `encode`: retained for
                        // trait symmetry; the binary goes through
                        // `convert_one_with` / `convert_bytes_with`.
    fn encode_with_quality(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
        _quality: u8,
    ) -> Result<u64, CodecError> {
        self.encode(img, dst)
    }

    /// Encode honouring both the caller-supplied quality and the
    /// caller-supplied MozJPEG options (subsampling, trellis AC,
    /// cosine-aligned 4:2:0 sub-mode). Default delegates to
    /// `encode_to_vec` (which honours only quality), so non-JPEG
    /// codecs and v0-baseline callers behave unchanged. The JPEG
    /// output codecs (WebpToJpeg, PngToJpeg) override this method
    /// to route through MozJPEG.
    fn encode_to_vec_with_opts(
        &self,
        img: &image::DynamicImage,
        quality: u8,
        _jpeg: &crate::params::JpegOptions,
    ) -> Result<Vec<u8>, CodecError> {
        self.encode_to_vec(img, quality)
    }

    /// Convert one file end-to-end with the caller-supplied params
    /// (resize + quality + MozJPEG options). The codec MUST NOT
    /// remove the source file — that is the caller's responsibility
    /// (see INV-CB-3 in `docs/contracts/codec-bounds.md`).
    fn convert_one_with(
        &self,
        src: &Path,
        dst: &Path,
        params: &Params,
    ) -> Result<ConversionReport, CodecError> {
        let img = self.decode(src)?;
        let resized = apply_resize(&img, params.resize);
        let bytes = self.encode_to_vec_with_opts(&resized, params.quality, &params.jpeg)?;
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(ConversionReport {
            in_bytes: std::fs::metadata(src)
                .map_err(|e| CodecError::Io(e.to_string()))?
                .len(),
            out_bytes: bytes.len() as u64,
            input_width: img.width(),
            input_height: img.height(),
            output_width: resized.width(),
            output_height: resized.height(),
        })
    }

    /// Decode + encode in-memory; used by single-file stdin mode
    /// where the bytes are already in a `Vec<u8>`.
    #[allow(dead_code)] // the binary's single-file mode goes through
                        // `run_single_file` directly, not through this
                        // default impl; retained on the trait for
                        // callers that want the in-memory shortcut.
    fn convert_bytes_with(
        &self,
        bytes: &[u8],
        params: &Params,
    ) -> Result<ConversionReport, CodecError> {
        let img = self.decode_bytes(bytes)?;
        let resized = apply_resize(&img, params.resize);
        let out_bytes = self.encode_to_vec_with_opts(&resized, params.quality, &params.jpeg)?;
        Ok(ConversionReport {
            in_bytes: bytes.len() as u64,
            out_bytes: out_bytes.len() as u64,
            input_width: img.width(),
            input_height: img.height(),
            output_width: resized.width(),
            output_height: resized.height(),
        })
    }

    /// Convert one file with `Params::default()` (the v0 baseline).
    /// Retained for the round-trip tests under `format::tests`.
    #[allow(dead_code)] // used only by `format::tests`; the binary calls convert_one_with
    fn convert_one(&self, src: &Path, dst: &Path) -> Result<ConversionReport, CodecError> {
        self.convert_one_with(src, dst, &Params::default())
    }
}

/// Conversion result: source byte count, destination byte count,
/// and the pre- / post-resize pixel dimensions. The dimensions
/// are populated on success (the `convert_one_with` default impl
/// populates them from the decoded and resized images). Callers
/// that need them in JSON output should consume the full struct;
/// see `docs/contracts/report-shape.md` for the wire shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionReport {
    pub in_bytes: u64,
    pub out_bytes: u64,
    pub input_width: u32,
    pub input_height: u32,
    pub output_width: u32,
    pub output_height: u32,
}

/// Encode an RGB8 image to JPEG bytes via the MozJPEG library.
/// The libjpeg fallback used by the `image::codecs::jpeg`
/// encoder is removed; every JPEG output produced by the
/// `WebpToJpeg` and `PngToJpeg` codecs goes through this helper so
/// the elrise.io side's MozJPEG fine-tune flags
/// (`--optimize-cru`, `--trellis-ac`) take effect end-to-end.
///
/// Subsampling is honoured from `opts.subsampling`; trellis AC
/// quantisation is enabled when `opts.trellis_ac` is `Some(n)`
/// with `n > 0` (the `mozjpeg` 0.10 wrapper exposes
/// `set_use_scans_in_trellis` as a binary toggle — strength is not
/// directly tunable from the safe Rust API). The cosine-aligned
/// 4:2:0 sub-mode (`opts.cru`) is currently a no-op pass-through:
/// MozJPEG's `4:2:0` subsampling already matches `4:2:0-cosited`
/// (the libjpeg-compatible default), and the non-cosited variant
/// is not exposed in the 0.10 wrapper. Tracking the elrise.io
/// side's `--optimize-cru` keys end-to-end still works because
/// the page-side capability discovery keys the flag off the
/// encoder's effective subsampling, not the literal CLI token
/// (see AR-017 / AR-018 on the elrise.io side).
fn encode_jpeg_mozjpeg(
    img: &image::DynamicImage,
    quality: u8,
    opts: &crate::params::JpegOptions,
) -> Result<Vec<u8>, CodecError> {
    use mozjpeg::ColorSpace;
    use std::io::Cursor;

    let rgb = img.to_rgb8();
    let width = rgb.width() as usize;
    let height = rgb.height() as usize;
    let raw = rgb.into_raw();
    let stride = width * 3;

    let mut compress = mozjpeg::Compress::new(ColorSpace::JCS_RGB);
    compress.set_size(width, height);
    compress.set_quality(quality as f32);
    let (chroma_h, chroma_v) = match opts.subsampling {
        crate::params::MozjpegSubsampling::None => (1u8, 1u8),
        crate::params::MozjpegSubsampling::Half => (2u8, 1u8),
        crate::params::MozjpegSubsampling::Quarter => (2u8, 2u8),
    };
    compress.set_chroma_sampling_pixel_sizes((chroma_h, chroma_v), (chroma_h, chroma_v));
    compress.set_optimize_coding(true);
    if let Some(trellis) = opts.trellis_ac {
        if trellis > 0 {
            compress.set_use_scans_in_trellis(true);
        }
    }

    let mut started = compress
        .start_compress(Cursor::new(Vec::new()))
        .map_err(|e| CodecError::Encode(e.to_string()))?;
    for y in 0..height {
        let row = &raw[y * stride..(y + 1) * stride];
        started
            .write_scanlines(row)
            .map_err(|e| CodecError::Encode(e.to_string()))?;
    }
    let writer = started
        .finish()
        .map_err(|e| CodecError::Encode(e.to_string()))?;
    Ok(writer.into_inner())
}

/// Apply the resize policy to a decoded image. Returns the original
/// image unchanged when the target dimensions equal the current
/// dimensions. The legacy shapes (None, MaxWidth, PortraitLandscape)
/// keep their original "resize-down only" semantics: when the input
/// is already at or below the cap, no work is done. `Fit` modes
/// always produce a non-trivial output (Cover crops to
/// `long_edge × long_edge`; Stretch always resizes to that square;
/// Contain only resizes when the source is larger than `long_edge`).
pub(crate) fn apply_resize(img: &image::DynamicImage, policy: ResizePolicy) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    let (target_w, target_h) = policy.target_dimensions(w, h);
    if target_w == w && target_h == h {
        return img.clone();
    }
    match policy {
        ResizePolicy::Fit { mode, .. } => match mode {
            FitMode::Contain => {
                img.resize(target_w, target_h, image::imageops::FilterType::Lanczos3)
            }
            FitMode::Cover => {
                img.resize_to_fill(target_w, target_h, image::imageops::FilterType::Lanczos3)
            }
            FitMode::Stretch => {
                img.resize_exact(target_w, target_h, image::imageops::FilterType::Lanczos3)
            }
        },
        _ => img.resize(target_w, target_h, image::imageops::FilterType::Lanczos3),
    }
}

/// Decode HEIC / HEIF container bytes into a `DynamicImage`.
///
/// Routes through `libheif-rs` (which links the system `libheif`
/// C library together with the `libde265` HEVC and `dav1d` AV1
/// decoder plugins per ADR-0004 § Decision § 1). The decode
/// honours the HEIF container's `irot`/`imir` transformations
/// (rotation + mirroring) via `LibHeif::decode`'s automatic
/// geometric-transform pass; alpha is preserved when the source
/// has an alpha channel.
///
/// Returns:
/// - `image::RgbImage` (RGB8) for sources without alpha;
/// - `image::RgbaImage` (RGBA8) for sources with alpha.
///
/// Errors:
/// - truncated or corrupt HEIF bytes -> `CodecError::Decode`;
/// - missing or unavailable HEVC/AV1 plugin on the host -> surfaced
///   verbatim from `libheif` via `CodecError::Decode`.
fn decode_heic_bytes(bytes: &[u8]) -> Result<image::DynamicImage, CodecError> {
    let total = bytes.len() as u64;
    let cursor = Cursor::new(bytes);
    let stream_reader = lh::StreamReader::new(cursor, total);
    let context = lh::HeifContext::read_from_reader(Box::new(stream_reader))
        .map_err(|e| CodecError::Decode(format!("heif context: {e}")))?;
    let image_handle = context
        .primary_image_handle()
        .map_err(|e| CodecError::Decode(format!("heif primary image handle: {e}")))?;
    let has_alpha = image_handle.has_alpha_channel();
    let color_space = if has_alpha {
        lh::ColorSpace::Rgb(lh::RgbChroma::Rgba)
    } else {
        lh::ColorSpace::Rgb(lh::RgbChroma::Rgb)
    };
    let lib_heif = lh::LibHeif::new();
    let img = lib_heif
        .decode(&image_handle, color_space, None)
        .map_err(|e| CodecError::Decode(format!("heif decode: {e}")))?;
    let planes = img.planes();
    let plane = planes.interleaved.ok_or_else(|| {
        CodecError::Decode("heif image is not interleaved (planar HEIF not supported)".to_string())
    })?;
    let width = plane.width;
    let height = plane.height;
    let stride = plane.stride;
    let bytes_per_pixel = plane.storage_bits_per_pixel / 8;
    let bytes_per_pixel = bytes_per_pixel as usize;
    if bytes_per_pixel != 3 && bytes_per_pixel != 4 {
        return Err(CodecError::Decode(format!(
            "heif decode produced unexpected channel depth ({bytes_per_pixel} bytes/pixel; \
             expected 3 or 4)"
        )));
    }
    let row_size = width as usize * bytes_per_pixel;
    let mut out = vec![0u8; row_size * height as usize];
    for y in 0..height as usize {
        let src_row = &plane.data[y * stride..y * stride + row_size];
        let dst_row = &mut out[y * row_size..(y + 1) * row_size];
        dst_row.copy_from_slice(src_row);
    }
    if has_alpha {
        Ok(image::DynamicImage::ImageRgba8(
            image::RgbaImage::from_raw(width, height, out)
                .ok_or_else(|| CodecError::Decode("heif rgba buffer shape mismatch".to_string()))?,
        ))
    } else {
        Ok(image::DynamicImage::ImageRgb8(
            image::RgbImage::from_raw(width, height, out)
                .ok_or_else(|| CodecError::Decode("heif rgb buffer shape mismatch".to_string()))?,
        ))
    }
}

/// v0 baseline codec: JPEG input -> WebP output with the per-orientation
/// resize policy and quality 85.
///
/// This codec preserves the v0 `gallery-compress` pipeline bit-for-bit
/// for the no-flags invocation path (ADR-0002). The order of operations
/// (decode -> resize -> to_rgb8 -> webp encode) is preserved exactly
/// to keep output bytes deterministic.
#[derive(Debug, Clone, Copy)]
pub struct JpegToWebp;

impl Codec for JpegToWebp {
    fn accepted_extensions(&self) -> &'static [&'static str] {
        &["jpg", "jpeg"]
    }

    fn output_extension(&self) -> &'static str {
        "webp"
    }

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError> {
        image::ImageReader::open(src)
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .with_guessed_format()
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .decode()
            .map_err(|e| CodecError::Decode(e.to_string()))
    }

    fn encode(&self, img: &image::DynamicImage, dst: &Path) -> Result<u64, CodecError> {
        let rgb = img.to_rgb8();
        let encoder = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height());
        let memory = encoder.encode(WEBP_QUALITY);
        let bytes: Vec<u8> = memory.as_ref().to_vec();
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }

    fn encode_with_quality(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
        quality: u8,
    ) -> Result<u64, CodecError> {
        let bytes = self.encode_to_vec(img, quality)?;
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }

    fn encode_to_vec(&self, img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, CodecError> {
        let rgb = img.to_rgb8();
        let encoder = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height());
        let memory = encoder.encode(quality as f32);
        Ok(memory.as_ref().to_vec())
    }
}

/// v0 baseline WebP quality. Matches the QUALITY constant in v0 src/main.rs.
pub const WEBP_QUALITY: f32 = 85.0;

/// PNG input -> WebP output with the per-orientation resize policy and
/// lossy-with-alpha encoding (`quality=85`).
///
/// `to_rgb8` would silently drop the alpha channel for PNG sources;
/// this codec uses `to_rgba8` and the `webp::Encoder::from_rgba`
/// entry point so the alpha survives the round-trip. This is the
/// deliberate behavioural change documented in ADR-0001 § Decision § 2.
#[derive(Debug, Clone, Copy)]
pub struct PngToWebp;

impl Codec for PngToWebp {
    fn accepted_extensions(&self) -> &'static [&'static str] {
        &["png"]
    }

    fn output_extension(&self) -> &'static str {
        "webp"
    }

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError> {
        image::ImageReader::open(src)
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .with_guessed_format()
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .decode()
            .map_err(|e| CodecError::Decode(e.to_string()))
    }

    fn encode(&self, img: &image::DynamicImage, dst: &Path) -> Result<u64, CodecError> {
        let rgba = img.to_rgba8();
        let encoder = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
        let memory = encoder.encode(WEBP_QUALITY);
        let bytes: Vec<u8> = memory.as_ref().to_vec();
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }

    fn encode_with_quality(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
        quality: u8,
    ) -> Result<u64, CodecError> {
        let bytes = self.encode_to_vec(img, quality)?;
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }

    fn encode_to_vec(&self, img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, CodecError> {
        let rgba = img.to_rgba8();
        let encoder = webp::Encoder::from_rgba(rgba.as_raw(), rgba.width(), rgba.height());
        let memory = encoder.encode(quality as f32);
        Ok(memory.as_ref().to_vec())
    }
}

/// WebP input -> PNG output (lossless).
#[derive(Debug, Clone, Copy)]
pub struct WebpToPng;

impl Codec for WebpToPng {
    fn accepted_extensions(&self) -> &'static [&'static str] {
        &["webp"]
    }

    fn output_extension(&self) -> &'static str {
        "png"
    }

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError> {
        image::ImageReader::open(src)
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .with_guessed_format()
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .decode()
            .map_err(|e| CodecError::Decode(e.to_string()))
    }

    fn encode_to_vec(
        &self,
        img: &image::DynamicImage,
        _quality: u8,
    ) -> Result<Vec<u8>, CodecError> {
        let rgba = img.to_rgba8();
        let mut buf = Vec::with_capacity(checked_pixel_capacity(rgba.width(), rgba.height())?);
        {
            let mut writer = std::io::Cursor::new(&mut buf);
            let encoder = image::codecs::png::PngEncoder::new(&mut writer);
            use image::ImageEncoder;
            encoder
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| CodecError::Encode(e.to_string()))?;
        }
        Ok(buf)
    }

    fn encode(&self, img: &image::DynamicImage, dst: &Path) -> Result<u64, CodecError> {
        let bytes = self.encode_to_vec(img, 85)?;
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }
}

/// WebP input -> JPEG output (lossy, quality 85).
///
/// The encoder is routed through the `mozjpeg` crate (MozJPEG) via
/// `mozjpeg-sys` static linkage. The libjpeg fallback used by
/// `image::codecs::jpeg` is removed; every JPEG output produced by
/// this codec goes through MozJPEG so the elrise.io side's
/// `--optimize-cru` / `--trellis-ac` flags are honoured end-to-end.
#[derive(Debug, Clone, Copy)]
pub struct WebpToJpeg;

impl Codec for WebpToJpeg {
    fn accepted_extensions(&self) -> &'static [&'static str] {
        &["webp"]
    }

    fn output_extension(&self) -> &'static str {
        "jpg"
    }

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError> {
        image::ImageReader::open(src)
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .with_guessed_format()
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .decode()
            .map_err(|e| CodecError::Decode(e.to_string()))
    }

    fn encode_to_vec(&self, img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, CodecError> {
        encode_jpeg_mozjpeg(img, quality, &crate::params::JpegOptions::default())
    }

    fn encode_to_vec_with_opts(
        &self,
        img: &image::DynamicImage,
        quality: u8,
        jpeg: &crate::params::JpegOptions,
    ) -> Result<Vec<u8>, CodecError> {
        encode_jpeg_mozjpeg(img, quality, jpeg)
    }
}

/// Image format identifier used by the CLI parser (`--input-format`,
/// `--output-format`). Maps case-insensitively to accepted extensions.
///
/// `Heic` is **input-only** per ADR-0004: the CLI rejects
/// `--output-format heic` with usage + exit 2 (the `image` crate
/// does not currently expose a HEIF encoder feature flag in 0.25).
/// The container name `heif` is accepted as an alias on the
/// `--input-format` side for operator convenience; the file
/// extension accepted by the codec (`accepted_extensions`) is
/// `.heic` to match Apple convention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpg,
    Png,
    Webp,
    Heic,
}

impl Format {
    /// Parse a CLI string. Returns `None` for unknown values.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Format::Jpg),
            "png" => Some(Format::Png),
            "webp" => Some(Format::Webp),
            "heic" | "heif" => Some(Format::Heic),
            _ => None,
        }
    }
}

impl std::fmt::Display for Format {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Format::Jpg => "jpg",
            Format::Png => "png",
            Format::Webp => "webp",
            Format::Heic => "heic",
        };
        f.write_str(s)
    }
}

/// Concrete codec instance. The enum wraps the unit-sized codec
/// structs so the CLI can dispatch on `(input_format, output_format)`
/// without leaning on `Box<dyn Codec>` (which is not `Sync` and
/// therefore not `rayon`-friendly).
#[derive(Debug, Clone, Copy)]
pub enum CodecImpl {
    JpegToWebp(JpegToWebp),
    PngToWebp(PngToWebp),
    WebpToPng(WebpToPng),
    WebpToJpeg(WebpToJpeg),
    JpegToPng(JpegToPng),
    PngToJpeg(PngToJpeg),
    HeicToWebp(HeicToWebp),
    HeicToPng(HeicToPng),
    HeicToJpeg(HeicToJpeg),
}

impl CodecImpl {
    pub fn accepted_extensions(&self) -> &'static [&'static str] {
        match self {
            CodecImpl::JpegToWebp(c) => c.accepted_extensions(),
            CodecImpl::PngToWebp(c) => c.accepted_extensions(),
            CodecImpl::WebpToPng(c) => c.accepted_extensions(),
            CodecImpl::WebpToJpeg(c) => c.accepted_extensions(),
            CodecImpl::JpegToPng(c) => c.accepted_extensions(),
            CodecImpl::PngToJpeg(c) => c.accepted_extensions(),
            CodecImpl::HeicToWebp(c) => c.accepted_extensions(),
            CodecImpl::HeicToPng(c) => c.accepted_extensions(),
            CodecImpl::HeicToJpeg(c) => c.accepted_extensions(),
        }
    }

    pub fn output_extension(&self) -> &'static str {
        match self {
            CodecImpl::JpegToWebp(c) => c.output_extension(),
            CodecImpl::PngToWebp(c) => c.output_extension(),
            CodecImpl::WebpToPng(c) => c.output_extension(),
            CodecImpl::WebpToJpeg(c) => c.output_extension(),
            CodecImpl::JpegToPng(c) => c.output_extension(),
            CodecImpl::PngToJpeg(c) => c.output_extension(),
            CodecImpl::HeicToWebp(c) => c.output_extension(),
            CodecImpl::HeicToPng(c) => c.output_extension(),
            CodecImpl::HeicToJpeg(c) => c.output_extension(),
        }
    }

    pub fn convert_one_with(
        &self,
        src: &Path,
        dst: &Path,
        params: &Params,
    ) -> Result<ConversionReport, CodecError> {
        match self {
            CodecImpl::JpegToWebp(c) => c.convert_one_with(src, dst, params),
            CodecImpl::PngToWebp(c) => c.convert_one_with(src, dst, params),
            CodecImpl::WebpToPng(c) => c.convert_one_with(src, dst, params),
            CodecImpl::WebpToJpeg(c) => c.convert_one_with(src, dst, params),
            CodecImpl::JpegToPng(c) => c.convert_one_with(src, dst, params),
            CodecImpl::PngToJpeg(c) => c.convert_one_with(src, dst, params),
            CodecImpl::HeicToWebp(c) => c.convert_one_with(src, dst, params),
            CodecImpl::HeicToPng(c) => c.convert_one_with(src, dst, params),
            CodecImpl::HeicToJpeg(c) => c.convert_one_with(src, dst, params),
        }
    }

    pub fn decode_bytes(&self, bytes: &[u8]) -> Result<image::DynamicImage, CodecError> {
        match self {
            CodecImpl::JpegToWebp(c) => c.decode_bytes(bytes),
            CodecImpl::PngToWebp(c) => c.decode_bytes(bytes),
            CodecImpl::WebpToPng(c) => c.decode_bytes(bytes),
            CodecImpl::WebpToJpeg(c) => c.decode_bytes(bytes),
            CodecImpl::JpegToPng(c) => c.decode_bytes(bytes),
            CodecImpl::PngToJpeg(c) => c.decode_bytes(bytes),
            CodecImpl::HeicToWebp(c) => c.decode_bytes(bytes),
            CodecImpl::HeicToPng(c) => c.decode_bytes(bytes),
            CodecImpl::HeicToJpeg(c) => c.decode_bytes(bytes),
        }
    }

    pub fn encode_to_vec(
        &self,
        img: &image::DynamicImage,
        quality: u8,
    ) -> Result<Vec<u8>, CodecError> {
        match self {
            CodecImpl::JpegToWebp(c) => c.encode_to_vec(img, quality),
            CodecImpl::PngToWebp(c) => c.encode_to_vec(img, quality),
            CodecImpl::WebpToPng(c) => c.encode_to_vec(img, quality),
            CodecImpl::WebpToJpeg(c) => c.encode_to_vec(img, quality),
            CodecImpl::JpegToPng(c) => c.encode_to_vec(img, quality),
            CodecImpl::PngToJpeg(c) => c.encode_to_vec(img, quality),
            CodecImpl::HeicToWebp(c) => c.encode_to_vec(img, quality),
            CodecImpl::HeicToPng(c) => c.encode_to_vec(img, quality),
            CodecImpl::HeicToJpeg(c) => c.encode_to_vec(img, quality),
        }
    }
}

/// JPEG input -> PNG output (lossless).
#[derive(Debug, Clone, Copy)]
pub struct JpegToPng;

impl Codec for JpegToPng {
    fn accepted_extensions(&self) -> &'static [&'static str] {
        &["jpg", "jpeg"]
    }

    fn output_extension(&self) -> &'static str {
        "png"
    }

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError> {
        image::ImageReader::open(src)
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .with_guessed_format()
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .decode()
            .map_err(|e| CodecError::Decode(e.to_string()))
    }

    fn encode_to_vec(
        &self,
        img: &image::DynamicImage,
        _quality: u8,
    ) -> Result<Vec<u8>, CodecError> {
        let rgba = img.to_rgba8();
        let mut buf = Vec::with_capacity(checked_pixel_capacity(rgba.width(), rgba.height())?);
        {
            let mut writer = std::io::Cursor::new(&mut buf);
            let encoder = image::codecs::png::PngEncoder::new(&mut writer);
            use image::ImageEncoder;
            encoder
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| CodecError::Encode(e.to_string()))?;
        }
        Ok(buf)
    }

    fn encode(&self, img: &image::DynamicImage, dst: &Path) -> Result<u64, CodecError> {
        let bytes = self.encode_to_vec(img, 85)?;
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }
}

/// PNG input -> JPEG output (lossy, quality 85).
///
/// The encoder is routed through the `mozjpeg` crate (MozJPEG) via
/// `mozjpeg-sys` static linkage. The libjpeg fallback used by
/// `image::codecs::jpeg` is removed; every JPEG output produced by
/// this codec goes through MozJPEG so the elrise.io side's
/// `--optimize-cru` / `--trellis-ac` flags are honoured end-to-end.
#[derive(Debug, Clone, Copy)]
pub struct PngToJpeg;

impl Codec for PngToJpeg {
    fn accepted_extensions(&self) -> &'static [&'static str] {
        &["png"]
    }

    fn output_extension(&self) -> &'static str {
        "jpg"
    }

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError> {
        image::ImageReader::open(src)
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .with_guessed_format()
            .map_err(|e| CodecError::Decode(e.to_string()))?
            .decode()
            .map_err(|e| CodecError::Decode(e.to_string()))
    }

    fn encode_to_vec(&self, img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, CodecError> {
        encode_jpeg_mozjpeg(img, quality, &crate::params::JpegOptions::default())
    }

    fn encode_to_vec_with_opts(
        &self,
        img: &image::DynamicImage,
        quality: u8,
        jpeg: &crate::params::JpegOptions,
    ) -> Result<Vec<u8>, CodecError> {
        encode_jpeg_mozjpeg(img, quality, jpeg)
    }
}

/// HEIC (HEIF container) input -> WebP output.
///
/// The HEIC path goes through the `image` crate's `heif` feature,
/// which statically links `libheif` + `libde265` (HEVC) + `dav1d`
/// (AV1) via `libheif-sys`. The decode re-uses the existing
/// `image::ImageReader::open(...).with_guessed_format()?.decode()?`
/// plumbing (the `heif` feature is selected automatically when
/// `with_guessed_format` recognises the HEIF ftyp box). The encode
/// side re-uses the WebP encoder shared with `JpegToWebp` and
/// `PngToWebp`. HEIC is **input-only** per ADR-0004.
#[derive(Debug, Clone, Copy)]
pub struct HeicToWebp;

impl Codec for HeicToWebp {
    fn accepted_extensions(&self) -> &'static [&'static str] {
        &["heic"]
    }

    fn output_extension(&self) -> &'static str {
        "webp"
    }

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError> {
        let bytes = std::fs::read(src).map_err(|e| CodecError::Io(e.to_string()))?;
        decode_heic_bytes(&bytes)
    }

    fn decode_bytes(&self, bytes: &[u8]) -> Result<image::DynamicImage, CodecError> {
        decode_heic_bytes(bytes)
    }

    fn encode_to_vec(&self, img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, CodecError> {
        let rgb = img.to_rgb8();
        let encoder = webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height());
        let memory = encoder.encode(quality as f32);
        Ok(memory.as_ref().to_vec())
    }

    fn encode(&self, img: &image::DynamicImage, dst: &Path) -> Result<u64, CodecError> {
        let bytes = self.encode_to_vec(img, WEBP_QUALITY as u8)?;
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }

    fn encode_with_quality(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
        quality: u8,
    ) -> Result<u64, CodecError> {
        let bytes = self.encode_to_vec(img, quality)?;
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }
}

/// HEIC (HEIF container) input -> PNG output (lossless).
///
/// Decode path is identical to `HeicToWebp` (routes through
/// `libheif-rs` via the shared `decode_heic_bytes` helper; the
/// source's alpha channel is preserved when present). Encode path
/// mirrors `WebpToPng` / `JpegToPng`: `image::codecs::png::PngEncoder`
/// with RGBA8. HEIC supports alpha; the existing `to_rgba8`
/// plumbing preserves it end-to-end.
#[derive(Debug, Clone, Copy)]
pub struct HeicToPng;

impl Codec for HeicToPng {
    fn accepted_extensions(&self) -> &'static [&'static str] {
        &["heic"]
    }

    fn output_extension(&self) -> &'static str {
        "png"
    }

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError> {
        let bytes = std::fs::read(src).map_err(|e| CodecError::Io(e.to_string()))?;
        decode_heic_bytes(&bytes)
    }

    fn decode_bytes(&self, bytes: &[u8]) -> Result<image::DynamicImage, CodecError> {
        decode_heic_bytes(bytes)
    }

    fn encode_to_vec(
        &self,
        img: &image::DynamicImage,
        _quality: u8,
    ) -> Result<Vec<u8>, CodecError> {
        let rgba = img.to_rgba8();
        let mut buf = Vec::with_capacity(checked_pixel_capacity(rgba.width(), rgba.height())?);
        {
            let mut writer = std::io::Cursor::new(&mut buf);
            let encoder = image::codecs::png::PngEncoder::new(&mut writer);
            use image::ImageEncoder;
            encoder
                .write_image(
                    rgba.as_raw(),
                    rgba.width(),
                    rgba.height(),
                    image::ExtendedColorType::Rgba8,
                )
                .map_err(|e| CodecError::Encode(e.to_string()))?;
        }
        Ok(buf)
    }

    fn encode(&self, img: &image::DynamicImage, dst: &Path) -> Result<u64, CodecError> {
        let bytes = self.encode_to_vec(img, 85)?;
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
    }
}

/// HEIC (HEIF container) input -> JPEG output via MozJPEG.
///
/// Decode path goes through `libheif-rs` via the shared
/// `decode_heic_bytes` helper (system libheif + libde265 + dav1d);
/// encode path goes through `mozjpeg` to keep the elrise.io
/// side's `--optimize-cru` / `--trellis-ac` flags honoured
/// end-to-end, matching the `WebpToJpeg` / `PngToJpeg` codecs.
/// HEIC is **input-only** per ADR-0004; the encode side here
/// always emits JPEG.
#[derive(Debug, Clone, Copy)]
pub struct HeicToJpeg;

impl Codec for HeicToJpeg {
    fn accepted_extensions(&self) -> &'static [&'static str] {
        &["heic"]
    }

    fn output_extension(&self) -> &'static str {
        "jpg"
    }

    fn decode(&self, src: &Path) -> Result<image::DynamicImage, CodecError> {
        let bytes = std::fs::read(src).map_err(|e| CodecError::Io(e.to_string()))?;
        decode_heic_bytes(&bytes)
    }

    fn decode_bytes(&self, bytes: &[u8]) -> Result<image::DynamicImage, CodecError> {
        decode_heic_bytes(bytes)
    }

    fn encode_to_vec(&self, img: &image::DynamicImage, quality: u8) -> Result<Vec<u8>, CodecError> {
        encode_jpeg_mozjpeg(img, quality, &crate::params::JpegOptions::default())
    }

    fn encode_to_vec_with_opts(
        &self,
        img: &image::DynamicImage,
        quality: u8,
        jpeg: &crate::params::JpegOptions,
    ) -> Result<Vec<u8>, CodecError> {
        encode_jpeg_mozjpeg(img, quality, jpeg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    static SEQ: AtomicU64 = AtomicU64::new(0);

    fn workspace_tmp() -> PathBuf {
        let mut p = std::env::temp_dir();
        let seq = SEQ.fetch_add(1, Ordering::SeqCst);
        p.push(format!(
            "fast-image-converter-test-{}-{}",
            std::process::id(),
            seq
        ));
        std::fs::create_dir_all(&p).expect("tmp dir");
        p
    }

    #[test]
    fn jpeg_to_webp_round_trip_matches_v0_quality() {
        let tmp = workspace_tmp();
        let src = tmp.join("input.jpg");
        let dst = tmp.join("output.webp");
        let img = image::RgbImage::from_fn(320, 240, |x, y| {
            image::Rgb([(x * 3) as u8, (y * 5) as u8, ((x + y) * 7) as u8])
        });
        img.save(&src).unwrap();
        let report = JpegToWebp.convert_one(&src, &dst).unwrap();
        assert!(report.out_bytes > 0);
        assert!(report.in_bytes > 0);
        assert!(dst.exists());
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn png_to_webp_preserves_alpha_dimensions() {
        let tmp = workspace_tmp();
        let src = tmp.join("input.png");
        let dst = tmp.join("output.webp");
        let img = image::RgbaImage::from_fn(320, 240, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, ((x + y) * 7) as u8, 200])
        });
        img.save(&src).unwrap();
        let report = PngToWebp.convert_one(&src, &dst).unwrap();
        assert!(report.out_bytes > 0);
        assert!(dst.exists());
        // WebP with alpha starts with the RIFF/WEBP magic header; the
        // first 12 bytes round-trip the encoder entry point (the alpha
        // bit is encoded in the VP8X chunk, beyond this header).
        let head = std::fs::read(&dst).unwrap();
        assert_eq!(&head[0..4], b"RIFF");
        assert_eq!(&head[8..12], b"WEBP");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn webp_to_png_round_trip_is_lossless() {
        let tmp = workspace_tmp();
        let src = tmp.join("input.webp");
        let dst = tmp.join("output.png");
        let img = image::RgbImage::from_fn(320, 240, |x, y| {
            image::Rgb([(x * 3) as u8, (y * 5) as u8, ((x + y) * 7) as u8])
        });
        let rgba = image::DynamicImage::ImageRgb8(img.clone()).to_rgba8();
        let enc = webp::Encoder::from_rgba(rgba.as_raw(), 320, 240);
        let mem = enc.encode(WEBP_QUALITY);
        std::fs::write(&src, mem.as_ref()).unwrap();
        let report = WebpToPng.convert_one(&src, &dst).unwrap();
        assert!(report.out_bytes > 0);
        assert!(dst.exists());
        let head = std::fs::read(&dst).unwrap();
        assert_eq!(
            &head[0..8],
            &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]
        );
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn webp_to_jpeg_writes_jpeg_magic() {
        let tmp = workspace_tmp();
        let src = tmp.join("input.webp");
        let dst = tmp.join("output.jpg");
        let rgba = image::RgbaImage::from_fn(320, 240, |x, y| {
            image::Rgba([(x * 3) as u8, (y * 5) as u8, ((x + y) * 7) as u8, 255])
        });
        let enc = webp::Encoder::from_rgba(rgba.as_raw(), 320, 240);
        let mem = enc.encode(WEBP_QUALITY);
        std::fs::write(&src, mem.as_ref()).unwrap();
        let report = WebpToJpeg.convert_one(&src, &dst).unwrap();
        assert!(report.out_bytes > 0);
        let head = std::fs::read(&dst).unwrap();
        // JPEG SOI marker: FF D8 FF
        assert_eq!(head[0], 0xFF);
        assert_eq!(head[1], 0xD8);
        assert_eq!(head[2], 0xFF);
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn resize_policy_target_dimensions_is_correct() {
        // v0 baseline: portrait uses portrait cap (h >= w), landscape
        // uses landscape cap (w > h). The refactor unifies width and
        // height resolution into one call; the legacy shapes keep
        // their original "resize-down only" semantics: when the input
        // is already at or below the cap, the target dimensions equal
        // the input dimensions exactly.
        let p = ResizePolicy::PortraitLandscape {
            portrait: 800,
            landscape: 1000,
        };
        assert_eq!(p.target_dimensions(640, 960), (640, 960)); // already smaller than portrait
        assert_eq!(p.target_dimensions(960, 640), (960, 640)); // already smaller than landscape
        assert_eq!(p.target_dimensions(800, 800), (800, 800)); // square (h >= w), at cap
        assert_eq!(p.target_dimensions(500, 800), (500, 800)); // already smaller
        assert_eq!(p.target_dimensions(1200, 800), (1000, 666)); // landscape, scaled down
        assert_eq!(p.target_dimensions(800, 1200), (800, 1200)); // portrait, at cap

        let m = ResizePolicy::MaxWidth(1000);
        assert_eq!(m.target_dimensions(800, 600), (800, 600)); // already smaller
        assert_eq!(m.target_dimensions(1200, 800), (1000, 666)); // scaled down
        assert_eq!(m.target_dimensions(1000, 800), (1000, 800)); // exactly at cap

        let n = ResizePolicy::None;
        assert_eq!(n.target_dimensions(1920, 1080), (1920, 1080)); // always unchanged
    }

    #[test]
    fn fit_contain_target_dimensions_respects_orientation() {
        let p = ResizePolicy::Fit {
            mode: FitMode::Contain,
            long_edge: 512,
        };
        // Landscape source (w > h): the long edge IS the width; width
        // is clamped to `long_edge`, height is proportional.
        assert_eq!(p.target_dimensions(1200, 800), (512, 341));
        // Portrait source (h > w): the long edge IS the height;
        // height is clamped to `long_edge`, width is proportional.
        assert_eq!(p.target_dimensions(800, 1200), (341, 512));
        // Square source: both dimensions equal `long_edge`.
        assert_eq!(p.target_dimensions(1024, 1024), (512, 512));
        // Source already at or below `long_edge`: no resize.
        assert_eq!(p.target_dimensions(400, 300), (400, 300));
        assert_eq!(p.target_dimensions(512, 512), (512, 512));
    }

    #[test]
    fn fit_cover_target_dimensions_is_always_square() {
        // Cover always produces `long_edge × long_edge` regardless of
        // input orientation; the image is scaled and centre-cropped
        // in `apply_resize`.
        let p = ResizePolicy::Fit {
            mode: FitMode::Cover,
            long_edge: 512,
        };
        assert_eq!(p.target_dimensions(1200, 800), (512, 512));
        assert_eq!(p.target_dimensions(800, 1200), (512, 512));
        assert_eq!(p.target_dimensions(400, 300), (512, 512));
        // Square source equal to long_edge: still 512×512 (Cover
        // is a no-op in dimensions but the operation is well-defined).
        assert_eq!(p.target_dimensions(512, 512), (512, 512));
    }

    #[test]
    fn fit_stretch_target_dimensions_is_always_square() {
        // Stretch always resizes to `long_edge × long_edge` even when
        // the source is smaller than the cap (Stretch is the only
        // Fit mode that upscales).
        let p = ResizePolicy::Fit {
            mode: FitMode::Stretch,
            long_edge: 512,
        };
        assert_eq!(p.target_dimensions(1200, 800), (512, 512));
        assert_eq!(p.target_dimensions(800, 1200), (512, 512));
        assert_eq!(p.target_dimensions(400, 300), (512, 512));
        assert_eq!(p.target_dimensions(512, 512), (512, 512));
    }

    // Exact-boundary success and one-byte-over failure for the
    // per-file byte limit. Boundary cases are exercised at unit
    // level (the helper is the source of truth) and at integration
    // level (the binary bounds stdin via `take(MAX+1)` and batch
    // mode via `fs::metadata().len()`).
    #[test]
    fn check_input_size_accepts_exact_boundary() {
        assert!(check_input_size(MAX_BATCH_FILE_BYTES).is_ok());
    }

    #[test]
    fn check_input_size_rejects_one_byte_over() {
        let err = check_input_size(MAX_BATCH_FILE_BYTES + 1).unwrap_err();
        assert!(
            err.contains("exceeds the per-file size limit"),
            "unexpected message: {err}"
        );
    }

    // Exact-boundary success and one-pixel-over failure for the
    // per-dimension limit.
    #[test]
    fn check_dimensions_accept_exact_boundary() {
        assert!(check_dimensions(MAX_DIMENSION, MAX_DIMENSION).is_ok());
    }

    #[test]
    fn check_dimensions_rejects_one_pixel_over_width() {
        let err = check_dimensions(MAX_DIMENSION + 1, MAX_DIMENSION).unwrap_err();
        assert!(err.contains("exceed the per-dimension limit"), "{err}");
    }

    #[test]
    fn check_dimensions_rejects_one_pixel_over_height() {
        let err = check_dimensions(MAX_DIMENSION, MAX_DIMENSION + 1).unwrap_err();
        assert!(err.contains("exceed the per-dimension limit"), "{err}");
    }

    // checked_pixel_capacity must surface dimension overflow
    // before allocation. The `width * height` product for
    // `u32::MAX * u32::MAX` does not overflow `usize` on a 64-bit
    // host (the product sits just under `usize::MAX`), but the
    // dimension check still rejects it via `check_dimensions`.
    #[test]
    fn checked_pixel_capacity_rejects_oversized_dimensions() {
        let err = checked_pixel_capacity(MAX_DIMENSION + 1, 1).unwrap_err();
        assert!(matches!(err, CodecError::Decode(_)), "{err:?}");
    }

    #[test]
    fn checked_pixel_capacity_accepts_normal_dimensions() {
        let cap = checked_pixel_capacity(1920, 1080).unwrap();
        assert_eq!(cap, 1920 * 1080);
    }

    // DE-040 (HEIC input codec). The structural-only invariants
    // below exercise the parser alias and the codec's accepted /
    // output extensions without requiring an actual HEIC decode
    // (the runtime decode path is blocked on a replacement decoder
    // crate — see the DE-040 issue body for the dependency-status
    // note). The image::ImageReader content-sniffing path is the
    // reason these tests stay green even when the heif feature is
    // absent: `Format::parse` and the Codec trait methods are pure
    // data structure accessors that do not touch the decoder.
    #[test]
    fn format_parse_accepts_heic_and_heif_alias() {
        assert_eq!(Format::parse("heic"), Some(Format::Heic));
        assert_eq!(Format::parse("heif"), Some(Format::Heic));
        assert_eq!(Format::parse("HEIC"), Some(Format::Heic));
        assert_eq!(Format::parse("HEIF"), Some(Format::Heic));
        assert_eq!(Format::parse("Heic"), Some(Format::Heic));
        // Display round-trip: the canonical token is `heic` (Apple
        // convention), not `heif` (the alias is parse-only).
        assert_eq!(Format::Heic.to_string(), "heic");
    }

    #[test]
    fn format_parse_rejects_unknown_format_unchanged() {
        // Adding the Heic variant must not affect the existing
        // parse-rejection surface; this guards against accidental
        // catch-all additions to the `match` in `Format::parse`.
        assert_eq!(Format::parse("gif"), None);
        assert_eq!(Format::parse("avif"), None);
        assert_eq!(Format::parse("tiff"), None);
        assert_eq!(Format::parse(""), None);
        assert_eq!(Format::parse("heifx"), None);
    }

    #[test]
    fn heic_codec_structural_extensions() {
        // The HEIC codecs accept only `.heic` (Apple convention).
        // The `heif` alias is parse-only — the codec's
        // `accepted_extensions` is the file-extension filter that
        // drives the batch-mode candidate filter in `src/main.rs`,
        // so it stays `.heic` even though the parser accepts both.
        assert_eq!(HeicToWebp.accepted_extensions(), &["heic"]);
        assert_eq!(HeicToPng.accepted_extensions(), &["heic"]);
        assert_eq!(HeicToJpeg.accepted_extensions(), &["heic"]);
        assert_eq!(HeicToWebp.output_extension(), "webp");
        assert_eq!(HeicToPng.output_extension(), "png");
        assert_eq!(HeicToJpeg.output_extension(), "jpg");
    }

    #[test]
    fn codec_impl_heic_variants_expose_structural_extensions() {
        // The CodecImpl dispatch must surface the HEIC codecs'
        // structural bits (accepted_extensions, output_extension)
        // identically to the inner structs. The compile-time
        // exhaustiveness check catches missing arms; this test
        // double-checks the values are propagated at runtime.
        assert_eq!(
            CodecImpl::HeicToWebp(HeicToWebp).accepted_extensions(),
            &["heic"]
        );
        assert_eq!(
            CodecImpl::HeicToPng(HeicToPng).accepted_extensions(),
            &["heic"]
        );
        assert_eq!(
            CodecImpl::HeicToJpeg(HeicToJpeg).accepted_extensions(),
            &["heic"]
        );
        assert_eq!(CodecImpl::HeicToWebp(HeicToWebp).output_extension(), "webp");
        assert_eq!(CodecImpl::HeicToPng(HeicToPng).output_extension(), "png");
        assert_eq!(CodecImpl::HeicToJpeg(HeicToJpeg).output_extension(), "jpg");
    }
}

use std::fmt;
use std::path::Path;

use crate::params::Params;

/// Resize policy applied to a decoded image before encoding.
///
/// The v0 baseline is `PortraitLandscape { portrait: 800, landscape: 1000 }`
/// (see `docs/adr/0002-preserve-jpg-to-webp-baseline.md`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResizePolicy {
    None,
    MaxWidth(u32),
    PortraitLandscape { portrait: u32, landscape: u32 },
}

impl ResizePolicy {
    /// Resolve the target width for an image of the given dimensions.
    /// `None` policy always returns `width` unchanged.
    pub fn target_width(&self, width: u32, height: u32) -> u32 {
        match *self {
            ResizePolicy::None => width,
            ResizePolicy::MaxWidth(m) => width.min(m),
            ResizePolicy::PortraitLandscape { portrait, landscape } => {
                if height >= width {
                    portrait
                } else {
                    landscape
                }
            }
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

    /// Encode the decoded image to `dst` and return the number of bytes
    /// written. The codec is responsible for the directory-existence
    /// contract (the source's parent is guaranteed to exist by the
    /// caller).
    fn encode(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
    ) -> Result<u64, CodecError>;

    /// Encode honouring the caller-supplied quality. Default ignores
    /// `quality` and delegates to `encode` (matches the v0 baseline
    /// where quality is hard-coded). Codecs whose output format has
    /// a meaningful quality knob override this.
    fn encode_with_quality(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
        _quality: u8,
    ) -> Result<u64, CodecError> {
        self.encode(img, dst)
    }

    /// Convert one file end-to-end with the caller-supplied params
    /// (resize + quality). The codec MUST NOT remove the source
    /// file — that is the caller's responsibility (see INV-CB-3 in
    /// `docs/contracts/codec-bounds.md`).
    fn convert_one_with(
        &self,
        src: &Path,
        dst: &Path,
        params: &Params,
    ) -> Result<ConversionReport, CodecError> {
        let img = self.decode(src)?;
        let resized = apply_resize(&img, params.resize);
        let out_bytes = self.encode_with_quality(&resized, dst, params.quality)?;
        Ok(ConversionReport {
            in_bytes: std::fs::metadata(src)
                .map_err(|e| CodecError::Io(e.to_string()))?
                .len(),
            out_bytes,
        })
    }

    /// Convert one file with `Params::default()` (the v0 baseline).
    /// Retained for the round-trip tests under `format::tests`.
    #[allow(dead_code)] // used only by `format::tests`; the binary calls convert_one_with
    fn convert_one(
        &self,
        src: &Path,
        dst: &Path,
    ) -> Result<ConversionReport, CodecError> {
        self.convert_one_with(src, dst, &Params::default())
    }
}

/// Conversion result: source byte count + destination byte count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ConversionReport {
    pub in_bytes: u64,
    pub out_bytes: u64,
}

/// Apply the resize policy to a decoded image. Returns the original
/// image unchanged when the target width equals the current width.
fn apply_resize(
    img: &image::DynamicImage,
    policy: ResizePolicy,
) -> image::DynamicImage {
    let (w, h) = (img.width(), img.height());
    let target_w = policy.target_width(w, h);
    if target_w >= w {
        return img.clone();
    }
    img.resize(target_w, u32::MAX, image::imageops::FilterType::Lanczos3)
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

    fn encode(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
    ) -> Result<u64, CodecError> {
        let rgb = img.to_rgb8();
        let encoder =
            webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height());
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
        let rgb = img.to_rgb8();
        let encoder =
            webp::Encoder::from_rgb(rgb.as_raw(), rgb.width(), rgb.height());
        let memory = encoder.encode(quality as f32);
        let bytes: Vec<u8> = memory.as_ref().to_vec();
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
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

    fn encode(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
    ) -> Result<u64, CodecError> {
        let rgba = img.to_rgba8();
        let encoder = webp::Encoder::from_rgba(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
        );
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
        let rgba = img.to_rgba8();
        let encoder = webp::Encoder::from_rgba(
            rgba.as_raw(),
            rgba.width(),
            rgba.height(),
        );
        let memory = encoder.encode(quality as f32);
        let bytes: Vec<u8> = memory.as_ref().to_vec();
        std::fs::write(dst, &bytes).map_err(|e| CodecError::Io(e.to_string()))?;
        Ok(bytes.len() as u64)
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

    fn encode(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
    ) -> Result<u64, CodecError> {
        let rgba = img.to_rgba8();
        let file = std::fs::File::create(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?;
        let mut writer = std::io::BufWriter::new(file);
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
        let out_bytes = std::fs::metadata(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?
            .len();
        Ok(out_bytes)
    }
}

/// WebP input -> JPEG output (lossy, quality 85).
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

    fn encode(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
    ) -> Result<u64, CodecError> {
        let rgb = img.to_rgb8();
        let file = std::fs::File::create(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?;
        let mut writer = std::io::BufWriter::new(file);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut writer,
            WEBP_QUALITY as u8,
        );
        use image::ImageEncoder;
        encoder
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| CodecError::Encode(e.to_string()))?;
        let out_bytes = std::fs::metadata(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?
            .len();
        Ok(out_bytes)
    }

    fn encode_with_quality(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
        quality: u8,
    ) -> Result<u64, CodecError> {
        let rgb = img.to_rgb8();
        let file = std::fs::File::create(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?;
        let mut writer = std::io::BufWriter::new(file);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut writer,
            quality,
        );
        use image::ImageEncoder;
        encoder
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| CodecError::Encode(e.to_string()))?;
        let out_bytes = std::fs::metadata(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?
            .len();
        Ok(out_bytes)
    }
}

/// Image format identifier used by the CLI parser (`--input-format`,
/// `--output-format`). Maps case-insensitively to accepted extensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Jpg,
    Png,
    Webp,
}

impl Format {
    /// Parse a CLI string. Returns `None` for unknown values.
    pub fn parse(s: &str) -> Option<Self> {
        match s.to_ascii_lowercase().as_str() {
            "jpg" | "jpeg" => Some(Format::Jpg),
            "png" => Some(Format::Png),
            "webp" => Some(Format::Webp),
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
        };
        f.write_str(s)
    }
}

/// Concrete codec instance. The enum wraps the six unit-sized codec
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

    fn encode(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
    ) -> Result<u64, CodecError> {
        let rgba = img.to_rgba8();
        let file = std::fs::File::create(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?;
        let mut writer = std::io::BufWriter::new(file);
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
        let out_bytes = std::fs::metadata(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?
            .len();
        Ok(out_bytes)
    }
}

/// PNG input -> JPEG output (lossy, quality 85).
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

    fn encode(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
    ) -> Result<u64, CodecError> {
        let rgb = img.to_rgb8();
        let file = std::fs::File::create(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?;
        let mut writer = std::io::BufWriter::new(file);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut writer,
            WEBP_QUALITY as u8,
        );
        use image::ImageEncoder;
        encoder
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| CodecError::Encode(e.to_string()))?;
        let out_bytes = std::fs::metadata(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?
            .len();
        Ok(out_bytes)
    }

    fn encode_with_quality(
        &self,
        img: &image::DynamicImage,
        dst: &Path,
        quality: u8,
    ) -> Result<u64, CodecError> {
        let rgb = img.to_rgb8();
        let file = std::fs::File::create(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?;
        let mut writer = std::io::BufWriter::new(file);
        let encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(
            &mut writer,
            quality,
        );
        use image::ImageEncoder;
        encoder
            .write_image(
                rgb.as_raw(),
                rgb.width(),
                rgb.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| CodecError::Encode(e.to_string()))?;
        let out_bytes = std::fs::metadata(dst)
            .map_err(|e| CodecError::Io(e.to_string()))?
            .len();
        Ok(out_bytes)
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
            "convert-to-webp-test-{}-{}",
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
        assert_eq!(&head[0..8], &[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);
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
    fn resize_policy_target_width_is_correct() {
        // v0 baseline: portrait >= landscape use portrait, landscape uses landscape.
        let p = ResizePolicy::PortraitLandscape {
            portrait: 800,
            landscape: 1000,
        };
        assert_eq!(p.target_width(640, 960), 800);   // portrait
        assert_eq!(p.target_width(960, 640), 1000);  // landscape
        assert_eq!(p.target_width(800, 800), 800);   // square (h >= w)
        assert_eq!(p.target_width(500, 800), 800);   // already smaller -> clamp
        assert_eq!(p.target_width(1200, 800), 1000); // landscape
        assert_eq!(p.target_width(800, 1200), 800);  // portrait
    }
}

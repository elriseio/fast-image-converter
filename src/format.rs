use std::fmt;
use std::path::Path;

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
/// a pure function `(src, dst) -> Result<ConversionReport, CodecError>`;
/// it holds no per-file state.
pub trait Codec {
    /// File extensions accepted as input (case-insensitive match).
    fn accepted_extensions(&self) -> &'static [&'static str];

    /// Output file extension (without the leading dot).
    fn output_extension(&self) -> &'static str;

    /// Resize policy applied before encoding.
    fn resize_policy(&self) -> ResizePolicy;

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

    /// Convert one file end-to-end: decode, resize, encode, write.
    /// The codec MUST NOT remove the source file — that is the
    /// caller's responsibility (see INV-CB-3 in
    /// `docs/contracts/codec-bounds.md`).
    fn convert_one(
        &self,
        src: &Path,
        dst: &Path,
    ) -> Result<ConversionReport, CodecError> {
        let img = self.decode(src)?;
        let resized = apply_resize(&img, self.resize_policy());
        let out_bytes = self.encode(&resized, dst)?;
        Ok(ConversionReport {
            in_bytes: std::fs::metadata(src)
                .map_err(|e| CodecError::Io(e.to_string()))?
                .len(),
            out_bytes,
        })
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

use std::fmt;
use std::str::FromStr;

use crate::format::{FitMode, ResizePolicy};

const DEFAULT_QUALITY: u8 = 85;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Params {
    pub quality: u8,
    pub resize: ResizePolicy,
    pub keep_source: bool,
    pub jpeg: JpegOptions,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            quality: DEFAULT_QUALITY,
            resize: ResizePolicy::default(),
            keep_source: false,
            jpeg: JpegOptions::default(),
        }
    }
}

/// MozJPEG-specific options for the JPEG encoder path (WebpToJpeg,
/// PngToJpeg). The JPEG encoder was migrated from libjpeg (via the
/// `image` crate) to MozJPEG so the elrise.io side can expose
/// MozJPEG fine-tune flags without silent no-op headers.
///
/// The default profile matches the v0 baseline (quarter-subsampled
/// 4:2:0, Huffman-optimised). Each flag is opt-in: an absent flag
/// falls back to the MozJPEG default, which is also libjpeg-compatible
/// for the documented flag set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JpegOptions {
    /// Chroma subsampling mode. Maps to `mozjpeg`'s `Subsampling`
    /// enum (None = 4:4:4, Half = 4:2:2, Quarter = 4:2:0). The v0
    /// default is `Quarter` (matches libjpeg's 4:2:0 default and
    /// preserves byte-stability for callers that do not override).
    pub subsampling: MozjpegSubsampling,
    /// MozJPEG's cosine-aligned / non-aligned 4:2:0 sub-mode. Only
    /// meaningful when `subsampling == Quarter`. Cosited sub-samples
    /// chroma at half-pixel boundaries; non-cosited at integer
    /// boundaries. `None` keeps MozJPEG's default.
    pub cru: Option<MozjpegCru>,
    /// Trellis AC quantisation strength, 0..=50. `0` disables the
    /// pass; values above 0 trade encode time for additional
    /// compression at the chosen quality. `None` keeps MozJPEG's
    /// default (no trellis).
    pub trellis_ac: Option<u8>,
}

impl Default for JpegOptions {
    fn default() -> Self {
        JpegOptions {
            subsampling: MozjpegSubsampling::Quarter,
            cru: None,
            trellis_ac: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MozjpegSubsampling {
    /// 4:4:4 — no chroma subsampling.
    None,
    /// 4:2:2 — half-rate chroma horizontal.
    Half,
    /// 4:2:0 — quarter-rate chroma (the v0 default; libjpeg-compatible).
    Quarter,
}

impl MozjpegSubsampling {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            MozjpegSubsampling::None => "4:4:4",
            MozjpegSubsampling::Half => "4:2:2",
            MozjpegSubsampling::Quarter => "4:2:0",
        }
    }
}

impl fmt::Display for MozjpegSubsampling {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MozjpegSubsampling {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "4:4:4" => Ok(MozjpegSubsampling::None),
            "4:2:2" => Ok(MozjpegSubsampling::Half),
            "4:2:0" => Ok(MozjpegSubsampling::Quarter),
            other => Err(format!(
                "unknown subsampling: {other:?} (expected 4:4:4, 4:2:2, or 4:2:0)"
            )),
        }
    }
}

/// MozJPEG cosine-aligned 4:2:0 sub-mode. Mirrors the elrise.io
/// side's `--optimize-cru` contract (see DE-018 / AR-018 on the
/// elrise.io side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MozjpegCru {
    /// 4:4:4 — same as MozjpegSubsampling::None.
    None,
    /// 4:2:2 — half-rate chroma horizontal.
    Half,
    /// 4:2:0-cosited — quarter-rate, cosited at half-pixel boundaries.
    QuarterCosited,
    /// 4:2:0-non-cosited — quarter-rate, cosited at integer boundaries.
    QuarterNonCosited,
}

impl MozjpegCru {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            MozjpegCru::None => "4:4:4",
            MozjpegCru::Half => "4:2:2",
            MozjpegCru::QuarterCosited => "4:2:0-cosited",
            MozjpegCru::QuarterNonCosited => "4:2:0-non-cosited",
        }
    }
}

impl fmt::Display for MozjpegCru {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for MozjpegCru {
    type Err = String;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "4:4:4" => Ok(MozjpegCru::None),
            "4:2:2" => Ok(MozjpegCru::Half),
            "4:2:0-cosited" => Ok(MozjpegCru::QuarterCosited),
            "4:2:0-non-cosited" => Ok(MozjpegCru::QuarterNonCosited),
            other => Err(format!(
                "unknown optimize-cru: {other:?} \
                 (expected 4:4:4, 4:2:2, 4:2:0-cosited, or 4:2:0-non-cosited)"
            )),
        }
    }
}

/// Parse the CLI value of `--resize`. Accepted shapes:
///
/// - `none`
/// - `cap=<W>` (uniform max-width cap)
/// - `auto:portrait=<W>,landscape=<H>` (per-orientation caps;
///
/// the v0 baseline is `auto:portrait=800,landscape=1000`).
pub fn parse_resize(s: &str) -> Result<ResizePolicy, String> {
    if s == "none" {
        return Ok(ResizePolicy::None);
    }
    if let Some(rest) = s.strip_prefix("cap=") {
        let w: u32 = rest
            .parse()
            .map_err(|_| format!("invalid cap width: {rest:?}"))?;
        if w == 0 {
            return Err("cap width must be > 0".to_string());
        }
        return Ok(ResizePolicy::MaxWidth(w));
    }
    if let Some(rest) = s.strip_prefix("auto:") {
        let mut portrait: Option<u32> = None;
        let mut landscape: Option<u32> = None;
        for kv in rest.split(',') {
            let (k, v) = kv
                .split_once('=')
                .ok_or_else(|| format!("expected key=value in {kv:?}"))?;
            let n: u32 = v.parse().map_err(|_| format!("invalid {k} width: {v:?}"))?;
            if n == 0 {
                return Err(format!("{k} width must be > 0"));
            }
            match k {
                "portrait" => portrait = Some(n),
                "landscape" => landscape = Some(n),
                other => return Err(format!("unknown auto key: {other}")),
            }
        }
        return Ok(ResizePolicy::PortraitLandscape {
            portrait: portrait.ok_or_else(|| "auto: missing portrait=<W>".to_string())?,
            landscape: landscape.ok_or_else(|| "auto: missing landscape=<H>".to_string())?,
        });
    }
    Err(format!("got {s:?}"))
}

/// Parse the second and third args of the
/// `--resize fit=<mode> long-edge=<N>` 3-arg form.
///
/// `mode` is the raw token after `fit=` is stripped by the CLI parser
/// (e.g. `"contain"`, `"cover"`, `"stretch"`); `long_edge_arg` is the
/// verbatim CLI token (e.g. `"long-edge=1024"`) so the `long-edge=`
/// prefix is validated explicitly. Accepted modes: `contain`,
/// `cover`, `stretch`. `<N>` must be in `1..=20000` (mirrors the
/// Go backend's `parseResize` validation; the upper bound keeps
/// the result well below `MAX_DIMENSION` so the resize path can
/// never overflow `width * height`).
pub fn parse_resize_fit(mode: &str, long_edge_arg: &str) -> Result<ResizePolicy, String> {
    let long_edge: u32 = long_edge_arg
        .strip_prefix("long-edge=")
        .ok_or_else(|| format!("expected long-edge=<N>, got {long_edge_arg:?}"))?
        .parse()
        .map_err(|_| format!("invalid long-edge: {long_edge_arg:?}"))?;
    if !(1..=20000).contains(&long_edge) {
        return Err(format!("long-edge out of range 1..=20000: {long_edge}"));
    }
    let mode = FitMode::from_str(mode)?;
    Ok(ResizePolicy::Fit { mode, long_edge })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_resize_fit_accepts_contain() {
        let p = parse_resize_fit("contain", "long-edge=1024").unwrap();
        assert_eq!(
            p,
            ResizePolicy::Fit {
                mode: FitMode::Contain,
                long_edge: 1024
            }
        );
    }

    #[test]
    fn parse_resize_fit_accepts_cover() {
        let p = parse_resize_fit("cover", "long-edge=512").unwrap();
        assert_eq!(
            p,
            ResizePolicy::Fit {
                mode: FitMode::Cover,
                long_edge: 512
            }
        );
    }

    #[test]
    fn parse_resize_fit_accepts_stretch() {
        let p = parse_resize_fit("stretch", "long-edge=256").unwrap();
        assert_eq!(
            p,
            ResizePolicy::Fit {
                mode: FitMode::Stretch,
                long_edge: 256
            }
        );
    }

    #[test]
    fn parse_resize_fit_accepts_long_edge_one() {
        // The lower bound (`1`) must be inclusive: a 1-pixel long
        // edge is degenerate but valid syntax; the resize path
        // surfaces the underflow downstream if a caller really
        // asks for it. The parser does not second-guess.
        let p = parse_resize_fit("contain", "long-edge=1").unwrap();
        assert_eq!(
            p,
            ResizePolicy::Fit {
                mode: FitMode::Contain,
                long_edge: 1
            }
        );
    }

    #[test]
    fn parse_resize_fit_accepts_long_edge_upper_bound() {
        // The upper bound (`20000`) must be inclusive. The Go
        // backend's `parseResize` accepts `20000`; staying
        // symmetric avoids round-trip drift across the boundary.
        let p = parse_resize_fit("cover", "long-edge=20000").unwrap();
        assert_eq!(
            p,
            ResizePolicy::Fit {
                mode: FitMode::Cover,
                long_edge: 20000
            }
        );
    }

    #[test]
    fn parse_resize_fit_rejects_unknown_mode() {
        let err = parse_resize_fit("bogus", "long-edge=100").unwrap_err();
        assert!(
            err.contains("unknown fit mode") && err.contains("bogus"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_resize_fit_rejects_missing_long_edge_prefix() {
        let err = parse_resize_fit("contain", "512").unwrap_err();
        assert!(
            err.contains("expected long-edge=") && err.contains("512"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_resize_fit_rejects_zero_long_edge() {
        let err = parse_resize_fit("contain", "long-edge=0").unwrap_err();
        assert!(
            err.contains("out of range 1..=20000") && err.contains("0"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_resize_fit_rejects_oversized_long_edge() {
        let err = parse_resize_fit("cover", "long-edge=20001").unwrap_err();
        assert!(
            err.contains("out of range 1..=20000") && err.contains("20001"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn parse_resize_fit_rejects_non_numeric_long_edge() {
        let err = parse_resize_fit("contain", "long-edge=abc").unwrap_err();
        assert!(err.contains("invalid long-edge"), "unexpected error: {err}");
    }
}

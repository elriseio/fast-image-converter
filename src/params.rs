use crate::format::ResizePolicy;

const DEFAULT_QUALITY: u8 = 85;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Params {
    pub quality: u8,
    pub resize: ResizePolicy,
    pub keep_source: bool,
}

impl Default for Params {
    fn default() -> Self {
        Params {
            quality: DEFAULT_QUALITY,
            resize: ResizePolicy::default(),
            keep_source: false,
        }
    }
}

/// Parse the CLI value of `--resize`. Accepted shapes:
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

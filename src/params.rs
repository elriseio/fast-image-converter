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
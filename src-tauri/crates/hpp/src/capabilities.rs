use crate::{PluginError, Result};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Codec {
    H264,
    Hevc,
    Av1,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MediaFormat {
    Hls,
    Progressive,
}

/// Supplied by the actual playback frontend, never inferred from the host OS.
/// Headless hosts start with empty capabilities until their adapter registers them.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaCapabilities {
    pub codecs: Vec<Codec>,
    pub formats: Vec<MediaFormat>,
}

impl MediaCapabilities {
    pub(crate) fn validate(&self) -> Result<()> {
        if self.codecs.len() > 3
            || self.formats.len() > 2
            || self
                .codecs
                .iter()
                .enumerate()
                .any(|(i, v)| self.codecs[..i].contains(v))
            || self
                .formats
                .iter()
                .enumerate()
                .any(|(i, v)| self.formats[..i].contains(v))
            || (self.formats.is_empty() && !self.codecs.is_empty())
        {
            return Err(PluginError::invalid("播放能力列表无效"));
        }
        Ok(())
    }
}

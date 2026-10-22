//! Metadata attached to every audio recording Zikos stores.

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AudioFormat {
    /// Lossless, the default for musical takes.
    Flac,
    /// Uncompressed capture; the on-device recording format before FLAC
    /// transcoding.
    Wav,
    /// Lossy, for spoken voice notes.
    Opus,
}

impl AudioFormat {
    pub fn is_lossless(self) -> bool {
        matches!(self, AudioFormat::Flac | AudioFormat::Wav)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RecordingMeta {
    /// Human title shown in the tree; also the default file name.
    pub title: String,
    pub format: AudioFormat,
    /// Unix timestamp (seconds) of the moment recording started.
    pub started_at: u64,
    pub duration_secs: f64,
    pub sample_rate_hz: u32,
    pub channels: u16,
    /// Optional musical context, set by the musician.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bpm: Option<u16>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

#[derive(Debug, Error, PartialEq)]
pub enum MetaError {
    #[error("title must be 1..=255 chars")]
    InvalidTitle,
    #[error("duration must be finite and >= 0, got {0}")]
    InvalidDuration(f64),
    #[error("sample rate must be in 8_000..=192_000 Hz, got {0}")]
    InvalidSampleRate(u32),
    #[error("channels must be in 1..=32, got {0}")]
    InvalidChannels(u16),
}

impl RecordingMeta {
    pub fn validate(&self) -> Result<(), MetaError> {
        if self.title.is_empty() || self.title.chars().count() > 255 {
            return Err(MetaError::InvalidTitle);
        }
        if !self.duration_secs.is_finite() || self.duration_secs < 0.0 {
            return Err(MetaError::InvalidDuration(self.duration_secs));
        }
        if !(8_000..=192_000).contains(&self.sample_rate_hz) {
            return Err(MetaError::InvalidSampleRate(self.sample_rate_hz));
        }
        if !(1..=32).contains(&self.channels) {
            return Err(MetaError::InvalidChannels(self.channels));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> RecordingMeta {
        RecordingMeta {
            title: "take-20261022-1015".into(),
            format: AudioFormat::Flac,
            started_at: 1_776_000_000,
            duration_secs: 94.5,
            sample_rate_hz: 48_000,
            channels: 2,
            bpm: Some(120),
            comment: None,
        }
    }

    #[test]
    fn valid_meta_passes_and_roundtrips() {
        let meta = sample();
        assert_eq!(meta.validate(), Ok(()));
        let json = serde_json::to_string(&meta).unwrap();
        assert_eq!(serde_json::from_str::<RecordingMeta>(&json).unwrap(), meta);
    }

    #[test]
    fn invalid_meta_is_rejected() {
        let mut meta = sample();
        meta.title = String::new();
        assert_eq!(meta.validate(), Err(MetaError::InvalidTitle));

        let mut meta = sample();
        meta.duration_secs = f64::NAN;
        assert!(
            matches!(meta.validate(), Err(MetaError::InvalidDuration(d)) if d.is_nan()),
            "NaN duration must be rejected as InvalidDuration(NaN)"
        );

        let mut meta = sample();
        meta.sample_rate_hz = 1_000;
        assert_eq!(meta.validate(), Err(MetaError::InvalidSampleRate(1_000)));

        let mut meta = sample();
        meta.channels = 0;
        assert_eq!(meta.validate(), Err(MetaError::InvalidChannels(0)));
    }

    #[test]
    fn lossless_flag_matches_format() {
        assert!(AudioFormat::Flac.is_lossless());
        assert!(AudioFormat::Wav.is_lossless());
        assert!(!AudioFormat::Opus.is_lossless());
    }
}

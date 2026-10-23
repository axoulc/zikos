//! Zikos audio engine: the dictaphone capture pipeline.
//!
//! Capture streams through a [`SampleSource`] (deterministic fixtures in
//! tests, [`CpalSource`] from the system input on desktop) into a
//! crash-safe staged WAV ([`RecorderSession`]), then finalizes to
//! lossless FLAC with `RecordingMeta` and `take-YYYYMMDD-HHMM` naming.
//! Sessions killed mid-recording are detected and recovered on next
//! start ([`recoverable_sessions`]). This module also keeps the
//! in-memory WAV encoder/inspector and recorder configuration policy.

use std::io::Cursor;
use thiserror::Error;
use zikos_core::AudioFormat;

mod cpal_source;
mod flac;
mod naming;
mod recovery;
mod session;
mod source;
mod wav_stream;

pub use cpal_source::{CpalSource, negotiate};
pub use naming::default_title;
pub use recovery::{recover, recoverable_sessions};
pub use session::{
    FinalizedTake, META_JSON, RecorderSession, SESSION_JSON, SessionManifest, SessionState,
    StagedSession, TAKE_FLAC, TAKE_WAV,
};
pub use source::{SampleSource, SineSource, SourceFormat};
pub use wav_stream::{StreamingWavWriter, repair_wav};

/// Everything that can go wrong in the audio pipeline.
#[derive(Debug, Error)]
pub enum AudioError {
    /// A WAV could not be encoded (bad chunk shape, 4 GiB ceiling).
    #[error("wav encoding failed: {0}")]
    Encode(String),
    /// A WAV or session manifest could not be parsed.
    #[error("wav parsing failed: {0}")]
    Parse(String),
    /// A recorder configuration or input negotiation was invalid.
    #[error("invalid recorder config: {0}")]
    InvalidConfig(String),
    /// The session is not in a state that allows this operation (e.g.
    /// pausing an already-paused capture).
    #[error("invalid session state: {0}")]
    InvalidState(String),
    /// The requested output format is not implemented.
    #[error("unsupported format: {0}")]
    Unsupported(String),
    /// Filesystem I/O failed.
    #[error("audio i/o failed: {0}")]
    Io(#[from] std::io::Error),
    /// hound could not read a WAV.
    #[error("wav reader failed: {0}")]
    Hound(#[from] hound::Error),
    /// Generated recording metadata failed validation.
    #[error("invalid recording metadata: {0}")]
    Meta(String),
    /// The audio device or stream failed (cpal).
    #[error("audio device error: {0}")]
    Device(#[from] cpal::Error),
    /// Persisting/loading JSON state failed.
    #[error("serialization failed: {0}")]
    Serde(#[from] serde_json::Error),
}

/// Policy for a recording session. Defaults target musical takes: 48 kHz
/// stereo, lossless.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecorderConfig {
    /// Container the take is finalized to.
    pub format: AudioFormat,
    /// Requested capture rate in Hz; the source's actual rate wins.
    pub sample_rate_hz: u32,
    /// Requested channel count; the source's actual count wins.
    pub channels: u16,
}

impl Default for RecorderConfig {
    fn default() -> Self {
        Self {
            format: AudioFormat::Flac,
            sample_rate_hz: 48_000,
            channels: 2,
        }
    }
}

impl RecorderConfig {
    /// Reject out-of-range rates (8–192 kHz) and channel counts (1–32).
    pub fn validate(&self) -> Result<(), AudioError> {
        if !(8_000..=192_000).contains(&self.sample_rate_hz) {
            return Err(AudioError::InvalidConfig(format!(
                "sample rate {} out of 8_000..=192_000",
                self.sample_rate_hz
            )));
        }
        if !(1..=32).contains(&self.channels) {
            return Err(AudioError::InvalidConfig(format!(
                "channels {} out of 1..=32",
                self.channels
            )));
        }
        Ok(())
    }
}

/// Parsed facts about a WAV buffer.
#[derive(Debug, Clone, PartialEq)]
pub struct WavInfo {
    /// Sample rate in Hz.
    pub sample_rate_hz: u32,
    /// Number of interleaved channels.
    pub channels: u16,
    /// Bits per sample (Zikos captures at 16).
    pub bits_per_sample: u16,
    /// Frames (samples per channel), matching the WAV fact/data chunk.
    pub frames: u32,
    /// Frames divided by sample rate.
    pub duration_secs: f64,
}

/// Encode interleaved 16-bit PCM into a complete WAV buffer (canonical
/// 44-byte RIFF header + data chunk, little-endian). Parsing is done with
/// hound, so tests round-trip through an independent implementation.
pub fn pcm_to_wav(
    samples: &[i16],
    sample_rate_hz: u32,
    channels: u16,
) -> Result<Vec<u8>, AudioError> {
    let data_len = u32::try_from(samples.len().saturating_mul(2))
        .map_err(|_| AudioError::Encode("WAV data exceeds 4 GiB".into()))?;
    if data_len as u64 + 36 > u32::MAX as u64 {
        return Err(AudioError::Encode("WAV total size exceeds 4 GiB".into()));
    }
    let block_align = channels.saturating_mul(2);
    let byte_rate = sample_rate_hz.saturating_mul(block_align as u32);

    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVE");
    out.extend_from_slice(b"fmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&sample_rate_hz.to_le_bytes());
    out.extend_from_slice(&byte_rate.to_le_bytes());
    out.extend_from_slice(&block_align.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    Ok(out)
}

/// Parse a WAV buffer back into facts.
pub fn wav_info(bytes: &[u8]) -> Result<WavInfo, AudioError> {
    let reader =
        hound::WavReader::new(Cursor::new(bytes)).map_err(|e| AudioError::Parse(e.to_string()))?;
    let spec = reader.spec();
    let frames = reader.duration();
    let duration_secs = if spec.sample_rate == 0 {
        0.0
    } else {
        frames as f64 / spec.sample_rate as f64
    };
    Ok(WavInfo {
        sample_rate_hz: spec.sample_rate,
        channels: spec.channels,
        bits_per_sample: spec.bits_per_sample,
        frames,
        duration_secs,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One second of a 440 Hz sine at the given rate, mono, as i16.
    fn sine(rate: u32) -> Vec<i16> {
        (0..rate)
            .map(|i| {
                let t = i as f64 / rate as f64;
                (t * 440.0 * std::f64::consts::TAU).sin() * i16::MAX as f64
            })
            .map(|v| v as i16)
            .collect()
    }

    #[test]
    fn wav_roundtrip_mono() {
        let pcm = sine(8_000);
        let wav = pcm_to_wav(&pcm, 8_000, 1).unwrap();
        let info = wav_info(&wav).unwrap();
        assert_eq!(info.sample_rate_hz, 8_000);
        assert_eq!(info.channels, 1);
        assert_eq!(info.bits_per_sample, 16);
        assert_eq!(info.frames, 8_000);
        assert!((info.duration_secs - 1.0).abs() < 1e-6);
    }

    #[test]
    fn wav_roundtrip_stereo_interleaved() {
        // Two frames of stereo: L,R,L,R.
        let pcm = [100i16, -100, 200, -200];
        let wav = pcm_to_wav(&pcm, 48_000, 2).unwrap();
        let info = wav_info(&wav).unwrap();
        assert_eq!(info.channels, 2);
        assert_eq!(info.frames, 2); // 4 interleaved samples = 2 frames
        assert!((info.duration_secs - (2.0 / 48_000.0)).abs() < 1e-9);
    }

    #[test]
    fn wav_header_is_canonical_44_bytes() {
        let wav = pcm_to_wav(&[0i16; 4], 48_000, 2).unwrap();
        assert_eq!(wav.len(), 44 + 8);
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
        assert_eq!(&wav[12..16], b"fmt ");
        assert_eq!(&wav[36..40], b"data");
        let riff_size = u32::from_le_bytes(wav[4..8].try_into().unwrap());
        let data_size = u32::from_le_bytes(wav[40..44].try_into().unwrap());
        assert_eq!(riff_size, 36 + 8);
        assert_eq!(data_size, 8);
    }

    #[test]
    fn garbage_is_rejected() {
        assert!(wav_info(b"not a wav at all").is_err());
    }

    #[test]
    fn default_config_targets_musical_takes() {
        let cfg = RecorderConfig::default();
        assert_eq!(cfg.format, AudioFormat::Flac);
        assert_eq!(cfg.sample_rate_hz, 48_000);
        assert_eq!(cfg.channels, 2);
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn invalid_configs_are_rejected() {
        assert!(
            RecorderConfig {
                sample_rate_hz: 1_000,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            RecorderConfig {
                channels: 0,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
    }
}

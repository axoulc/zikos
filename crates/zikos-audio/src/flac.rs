//! Lossless WAV → FLAC transcoding via pure-Rust `flacenc` (design D1).
//!
//! Streams the staged WAV through a hound-backed [`flacenc::source::Source`]
//! so long takes never sit fully in memory.

use crate::AudioError;
use flacenc::component::BitRepr;
use flacenc::error::{SourceError, SourceErrorReason, Verify};
use flacenc::source::{Fill, Source as FlacencSource};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::rc::Rc;

/// hound-based flacenc source: pulls interleaved i16 samples from a WAV
/// file and exposes them as i32 frames. `into_samples` consumes the
/// reader, so the iterator is fully owned — no borrowing acrobatics.
struct WavFileSource {
    samples: hound::WavIntoSamples<BufReader<File>, i16>,
    sample_rate: u32,
    channels: u16,
    total_frames: Option<u64>,
}

impl WavFileSource {
    fn open(path: &Path) -> Result<Self, AudioError> {
        let reader = hound::WavReader::open(path)?;
        let spec = reader.spec();
        if spec.sample_format != hound::SampleFormat::Int || spec.bits_per_sample != 16 {
            return Err(AudioError::Encode(
                "staged WAV is not 16-bit integer PCM".into(),
            ));
        }
        let total_frames = u64::from(reader.duration());
        let samples = reader.into_samples::<i16>();
        Ok(Self {
            samples,
            sample_rate: spec.sample_rate,
            channels: spec.channels,
            total_frames: Some(total_frames),
        })
    }
}

impl FlacencSource for WavFileSource {
    fn channels(&self) -> usize {
        usize::from(self.channels)
    }

    fn bits_per_sample(&self) -> usize {
        16
    }

    fn sample_rate(&self) -> usize {
        self.sample_rate as usize
    }

    fn read_samples<F: Fill>(
        &mut self,
        block_size: usize,
        dest: &mut F,
    ) -> Result<usize, SourceError> {
        let take = block_size * usize::from(self.channels);
        let mut buf: Vec<i32> = Vec::with_capacity(take);
        for s in self.samples.by_ref().take(take) {
            let s = s.map_err(|e| {
                SourceError::by_reason(SourceErrorReason::IO(Some(Rc::new(std::io::Error::other(
                    e.to_string(),
                )))))
            })?;
            buf.push(i32::from(s));
        }
        if buf.is_empty() {
            return Ok(0);
        }
        dest.fill_interleaved(&buf)?;
        Ok(buf.len() / usize::from(self.channels))
    }

    fn len_hint(&self) -> Option<usize> {
        self.total_frames.map(|f| f as usize)
    }
}

/// Transcode a 16-bit PCM WAV to FLAC, streaming. The FLAC file is
/// written only after the full stream encodes successfully.
pub(crate) fn transcode_wav_to_flac(wav_path: &Path, flac_path: &Path) -> Result<(), AudioError> {
    let config = flacenc::config::Encoder::default()
        .into_verified()
        .map_err(|_| AudioError::Encode("invalid flacenc default config".into()))?;
    let source = WavFileSource::open(wav_path)?;
    let stream = flacenc::encode_with_fixed_block_size(&config, source, config.block_size)
        .map_err(|e| AudioError::Encode(format!("flac encode failed: {e}")))?;
    let mut sink = flacenc::bitsink::ByteSink::new();
    stream
        .write(&mut sink)
        .map_err(|e| AudioError::Encode(format!("flac serialization failed: {e}")))?;
    std::fs::write(flac_path, sink.as_slice())?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sine_samples(rate: u32, channels: u16, frames: u64) -> Vec<i16> {
        (0..frames * channels as u64)
            .map(|i| {
                let f = i / channels as u64;
                let t = f as f64 / rate as f64;
                ((t * 440.0 * std::f64::consts::TAU).sin() * i16::MAX as f64) as i16
            })
            .collect()
    }

    #[test]
    fn transcode_is_lossless() {
        let samples = sine_samples(48_000, 2, 48_000); // 1 s stereo
        let dir = tempfile::tempdir().unwrap();
        let wav = dir.path().join("take.wav");
        let flac = dir.path().join("take.flac");
        std::fs::write(&wav, crate::pcm_to_wav(&samples, 48_000, 2).unwrap()).unwrap();

        transcode_wav_to_flac(&wav, &flac).unwrap();

        let mut reader = claxon::FlacReader::open(&flac).unwrap();
        let info = reader.streaminfo();
        assert_eq!(info.sample_rate, 48_000);
        assert_eq!(info.channels, 2);
        assert_eq!(info.bits_per_sample, 16);
        assert_eq!(info.samples, Some(48_000));
        // Decoded PCM is bit-identical to the captured samples.
        let decoded: Result<Vec<i32>, _> = reader.samples().collect();
        let decoded = decoded.unwrap();
        assert_eq!(
            decoded,
            samples.iter().map(|&s| i32::from(s)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn transcode_rejects_missing_wav() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("nope.wav");
        assert!(transcode_wav_to_flac(&missing, &dir.path().join("x.flac")).is_err());
    }
}

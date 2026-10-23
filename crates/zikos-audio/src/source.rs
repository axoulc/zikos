//! Sample sources feeding the recorder pipeline.
//!
//! The recorder consumes interleaved 16-bit PCM through the
//! [`SampleSource`] trait so the whole pipeline can be driven by the
//! deterministic [`SineSource`] fixture in tests — no microphone needed.

/// The format a source actually delivers. This is what the recorder must
/// honor downstream (WAV header, metadata) — never the requested config.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceFormat {
    /// Sample rate in Hz.
    pub sample_rate_hz: u32,
    /// Number of interleaved channels.
    pub channels: u16,
}

/// A pull-based producer of interleaved 16-bit PCM chunks.
///
/// `next_chunk` returns `None` once the source is exhausted (finite
/// fixture) or stopped (live adapter). Chunks are interleaved frames;
/// their length is always a multiple of the channel count.
pub trait SampleSource {
    /// The format this source actually delivers.
    fn format(&self) -> SourceFormat;

    /// Pull the next chunk of interleaved samples, or `None` when done.
    fn next_chunk(&mut self) -> Option<Vec<i16>>;

    /// Signal a live source to end: buffered chunks still drain, then
    /// `next_chunk` returns `None`. Finite sources need no override.
    fn stop(&mut self) {}
}

/// Deterministic sine fixture: `freq_hz` tone, odd channels phase-inverted
/// so channel order is observable. Drives the whole pipeline in tests.
#[derive(Debug, Clone)]
pub struct SineSource {
    sample_rate_hz: u32,
    channels: u16,
    freq_hz: f64,
    remaining_frames: u64,
    emitted_frames: u64,
}

/// Fixture chunk granularity: 50 ms of frames at 48 kHz.
const CHUNK_FRAMES: u64 = 2_400;

impl SineSource {
    /// 440 Hz sine of `total_frames` frames at the given format.
    pub fn new(sample_rate_hz: u32, channels: u16, total_frames: u64) -> Self {
        Self {
            sample_rate_hz,
            channels,
            freq_hz: 440.0,
            remaining_frames: total_frames,
            emitted_frames: 0,
        }
    }
}

impl SampleSource for SineSource {
    fn format(&self) -> SourceFormat {
        SourceFormat {
            sample_rate_hz: self.sample_rate_hz,
            channels: self.channels,
        }
    }

    fn next_chunk(&mut self) -> Option<Vec<i16>> {
        if self.remaining_frames == 0 {
            return None;
        }
        let frames = self.remaining_frames.min(CHUNK_FRAMES);
        let channels = u64::from(self.channels);
        let start = self.emitted_frames;
        let rate = self.sample_rate_hz;
        let freq = self.freq_hz;
        let chunk: Vec<i16> = (0..frames * channels)
            .map(|i| {
                let frame = start + i / channels;
                let channel = (i % channels) as u16;
                let t = frame as f64 / rate as f64;
                let s = ((t * freq * std::f64::consts::TAU).sin() * i16::MAX as f64) as i16;
                if channel % 2 == 0 { s } else { -s }
            })
            .collect();
        self.emitted_frames += frames;
        self.remaining_frames -= frames;
        Some(chunk)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Independent reference: full sine as one flat interleaved buffer.
    fn reference_sine(rate: u32, channels: u16, frames: u64) -> Vec<i16> {
        let mut out = Vec::with_capacity((frames * channels as u64) as usize);
        for f in 0..frames {
            let t = f as f64 / rate as f64;
            let l = ((t * 440.0 * std::f64::consts::TAU).sin() * i16::MAX as f64) as i16;
            for c in 0..channels {
                out.push(if c % 2 == 0 { l } else { -l });
            }
        }
        out
    }

    #[test]
    fn sine_source_matches_reference() {
        let mut src = SineSource::new(48_000, 2, 24_000);
        let mut collected = Vec::new();
        while let Some(chunk) = src.next_chunk() {
            collected.extend_from_slice(&chunk);
        }
        assert_eq!(collected, reference_sine(48_000, 2, 24_000));
    }

    #[test]
    fn source_reports_actual_format() {
        // The source is the authority on format, whatever the recorder
        // requested: a 44.1 kHz mono source reports exactly that.
        let mut src = SineSource::new(44_100, 1, 500);
        assert_eq!(
            src.format(),
            SourceFormat {
                sample_rate_hz: 44_100,
                channels: 1
            }
        );
        // Chunks stay non-empty until the source is exhausted, and
        // exhaustion is terminal.
        let mut chunks = 0;
        while let Some(chunk) = src.next_chunk() {
            assert!(!chunk.is_empty());
            chunks += 1;
        }
        assert!(chunks > 0);
        assert!(src.next_chunk().is_none());
    }

    #[test]
    fn chunks_are_frame_aligned() {
        let mut src = SineSource::new(48_000, 2, 5_000);
        while let Some(chunk) = src.next_chunk() {
            assert_eq!(chunk.len() % 2, 0, "interleaved stereo chunk");
        }
    }
}

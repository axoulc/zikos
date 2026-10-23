//! Streaming, crash-safe WAV staging (design D3).
//!
//! Captures land on disk as a canonical 44-byte-header WAV whose RIFF and
//! data sizes are patched only on a clean close. The on-disk invariant is
//! that the frame count always equals `(file_len − 44) / block_align`,
//! valid even when the process died before patching — [`repair_wav`]
//! recomputes the sizes from that formula and drops a trailing partial
//! frame.

use crate::AudioError;
use std::fs::{File, OpenOptions};
use std::io::{Seek, SeekFrom, Write};
use std::path::Path;

const HEADER_LEN: u64 = 44;

/// Incremental WAV writer over an open file.
#[derive(Debug)]
pub struct StreamingWavWriter {
    file: File,
    sample_rate_hz: u32,
    channels: u16,
    frames_written: u64,
}

impl StreamingWavWriter {
    /// Create (or truncate) the WAV at `path` with a zero-sized canonical
    /// header. Samples appended afterwards stream to disk.
    pub fn create(path: &Path, sample_rate_hz: u32, channels: u16) -> Result<Self, AudioError> {
        let mut file = File::create(path)?;
        let block_align = channels.saturating_mul(2);
        let byte_rate = sample_rate_hz.saturating_mul(block_align as u32);
        let mut header = Vec::with_capacity(HEADER_LEN as usize);
        header.extend_from_slice(b"RIFF");
        header.extend_from_slice(&0u32.to_le_bytes()); // patched on close/repair
        header.extend_from_slice(b"WAVE");
        header.extend_from_slice(b"fmt ");
        header.extend_from_slice(&16u32.to_le_bytes());
        header.extend_from_slice(&1u16.to_le_bytes()); // PCM
        header.extend_from_slice(&channels.to_le_bytes());
        header.extend_from_slice(&sample_rate_hz.to_le_bytes());
        header.extend_from_slice(&byte_rate.to_le_bytes());
        header.extend_from_slice(&block_align.to_le_bytes());
        header.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        header.extend_from_slice(b"data");
        header.extend_from_slice(&0u32.to_le_bytes()); // patched on close/repair
        file.write_all(&header)?;
        file.flush()?;
        Ok(Self {
            file,
            sample_rate_hz,
            channels,
            frames_written: 0,
        })
    }

    /// Append interleaved samples. `samples.len()` must be a multiple of
    /// the channel count; partial chunks are rejected, not split.
    pub fn append(&mut self, samples: &[i16]) -> Result<(), AudioError> {
        let channels = usize::from(self.channels);
        if samples.len() % channels != 0 {
            return Err(AudioError::Encode(format!(
                "chunk of {} samples is not frame-aligned for {} channels",
                samples.len(),
                channels
            )));
        }
        let mut buf = Vec::with_capacity(samples.len() * 2);
        for s in samples {
            buf.extend_from_slice(&s.to_le_bytes());
        }
        self.file.write_all(&buf)?;
        self.frames_written += (samples.len() / channels) as u64;
        Ok(())
    }

    /// Flush buffered data so it is visible to other processes (already
    /// written bytes survive a process crash in the OS page cache).
    pub fn flush(&mut self) -> std::io::Result<()> {
        self.file.flush()
    }

    /// Frames written so far.
    pub fn frames_written(&self) -> u64 {
        self.frames_written
    }

    /// Close cleanly, patching RIFF/data sizes to the true frame count.
    pub fn close(mut self) -> Result<(), AudioError> {
        self.flush()?;
        patch_sizes(&mut self.file, self.channels, self.frames_written)?;
        self.file.sync_data()?;
        Ok(())
    }

    /// The format this writer was opened with.
    pub fn format(&self) -> crate::SourceFormat {
        crate::SourceFormat {
            sample_rate_hz: self.sample_rate_hz,
            channels: self.channels,
        }
    }
}

/// Patch the RIFF and data sizes for `frames`. Exposed to the recovery
/// path via [`repair_wav`].
fn patch_sizes(file: &mut File, channels: u16, frames: u64) -> Result<(), AudioError> {
    let data_len = frames
        .checked_mul(u64::from(channels) * 2)
        .filter(|len| *len <= u32::MAX as u64)
        .ok_or_else(|| AudioError::Encode("WAV data exceeds 4 GiB".into()))?;
    file.seek(SeekFrom::Start(4))?;
    file.write_all(&(36 + data_len as u32).to_le_bytes())?;
    file.seek(SeekFrom::Start(40))?;
    file.write_all(&(data_len as u32).to_le_bytes())?;
    Ok(())
}

/// Repair a staged WAV left unpatched by a crash: recompute the frame
/// count from the bytes actually on disk, drop a trailing partial frame,
/// and patch the header. Returns the recovered frame count.
pub fn repair_wav(path: &Path) -> Result<u64, AudioError> {
    let mut file = OpenOptions::new().read(true).write(true).open(path)?;
    let mut header = [0u8; HEADER_LEN as usize];
    std::io::Read::read_exact(&mut file, &mut header)?;
    if &header[0..4] != b"RIFF" || &header[8..12] != b"WAVE" || &header[36..40] != b"data" {
        return Err(AudioError::Parse("not a staged Zikos WAV".into()));
    }
    let channels = u16::from_le_bytes(header[22..24].try_into().expect("2-byte slice"));
    if channels == 0 {
        return Err(AudioError::Parse(
            "staged WAV declares zero channels".into(),
        ));
    }
    let len = file.metadata()?.len();
    let block_align = u64::from(channels) * 2;
    let data_bytes = len - HEADER_LEN;
    let frames = data_bytes / block_align;
    let partial = data_bytes % block_align;
    if partial > 0 {
        file.set_len(HEADER_LEN + frames * block_align)?;
    }
    patch_sizes(&mut file, channels, frames)?;
    file.sync_data()?;
    Ok(frames)
}

#[cfg(test)]
mod tests {
    use super::super::{StreamingWavWriter, pcm_to_wav, repair_wav, wav_info};
    use std::io::Write;

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
    fn streamed_header_is_canonical_44_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("take.wav");
        let _w = StreamingWavWriter::create(&path, 48_000, 2).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44);
        assert_eq!(&bytes[0..4], b"RIFF");
        assert_eq!(&bytes[8..12], b"WAVE");
        assert_eq!(&bytes[12..16], b"fmt ");
        assert_eq!(&bytes[36..40], b"data");
        // Sizes are zeroed until close/repair patches them.
        assert_eq!(u32::from_le_bytes(bytes[4..8].try_into().unwrap()), 0);
        assert_eq!(u32::from_le_bytes(bytes[40..44].try_into().unwrap()), 0);
    }

    #[test]
    fn append_grows_file_by_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("take.wav");
        let mut w = StreamingWavWriter::create(&path, 48_000, 2).unwrap();
        let chunk = [100i16, -100, 200, -200]; // 2 stereo frames
        w.append(&chunk).unwrap();
        w.flush().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(bytes.len(), 44 + 8);
        assert_eq!(&bytes[44..46], &100i16.to_le_bytes());
        assert_eq!(&bytes[46..48], &(-100i16).to_le_bytes());
        assert_eq!(w.frames_written(), 2);
    }

    #[test]
    fn streamed_wav_roundtrips_through_hound() {
        let samples = sine_samples(48_000, 2, 48_000); // 1 s stereo
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("take.wav");
        let mut w = StreamingWavWriter::create(&path, 48_000, 2).unwrap();
        for chunk in samples.chunks(2_400 * 2) {
            w.append(chunk).unwrap();
        }
        w.close().unwrap();

        let bytes = std::fs::read(&path).unwrap();
        let info = wav_info(&bytes).unwrap();
        assert_eq!(info.sample_rate_hz, 48_000);
        assert_eq!(info.channels, 2);
        assert_eq!(info.bits_per_sample, 16);
        assert_eq!(info.frames, 48_000);
        assert!((info.duration_secs - 1.0).abs() < 1e-6);
        // Byte-identical to the in-memory encoder for the same samples.
        assert_eq!(bytes, pcm_to_wav(&samples, 48_000, 2).unwrap());
    }

    #[test]
    fn unpatched_file_is_repairable() {
        // Simulated crash: header sizes still zeroed, data fully on disk.
        let samples = sine_samples(48_000, 2, 24_000); // 0.5 s stereo
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("take.wav");
        let mut w = StreamingWavWriter::create(&path, 48_000, 2).unwrap();
        for chunk in samples.chunks(2_400 * 2) {
            w.append(chunk).unwrap();
        }
        w.flush().unwrap();
        drop(w); // no close(): sizes stay zeroed

        let frames = repair_wav(&path).unwrap();
        assert_eq!(frames, 24_000);
        let bytes = std::fs::read(&path).unwrap();
        let info = wav_info(&bytes).unwrap();
        assert_eq!(info.frames, 24_000);
        assert!((info.duration_secs - 0.5).abs() < 1e-6);
    }

    #[test]
    fn repair_truncates_partial_frame_and_rejects_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("take.wav");
        let mut w = StreamingWavWriter::create(&path, 44_100, 1).unwrap();
        w.append(&[1i16, 2, 3]).unwrap(); // 3 whole mono frames
        w.flush().unwrap();
        drop(w);
        // Crash mid-sample-write: one stray byte makes a partial frame.
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        f.write_all(&[0xAB]).unwrap();
        drop(f);

        let frames = repair_wav(&path).unwrap();
        assert_eq!(frames, 3, "partial frame dropped");
        assert_eq!(wav_info(&std::fs::read(&path).unwrap()).unwrap().frames, 3);

        let garbage = dir.path().join("garbage.wav");
        std::fs::write(&garbage, b"not a wav").unwrap();
        assert!(repair_wav(&garbage).is_err());
        let short = dir.path().join("short.wav");
        std::fs::write(&short, b"RIFF").unwrap();
        assert!(repair_wav(&short).is_err());
    }
}

//! Desktop capture adapter over cpal (design D7).
//!
//! [`negotiate`] is a pure function picking the nearest supported format;
//! [`CpalSource`] opens the default input device and bridges its callback
//! into the pull-based [`SampleSource`] world via a channel. The device
//! path itself is deliberately untested (no microphone in tests, per
//! AGENTS.md); the bridge is a separate [`ChannelSource`] covered by
//! tests.

use crate::{AudioError, SampleSource, SourceFormat};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender};
use std::time::Duration;

/// Pick the entry of `supported` closest to `requested`: minimal sample
/// rate distance first, then minimal channel distance. Empty support is
/// an error.
pub fn negotiate(
    requested: SourceFormat,
    supported: &[SourceFormat],
) -> Result<SourceFormat, AudioError> {
    supported
        .iter()
        .min_by_key(|f| {
            let rate_dist = f.sample_rate_hz.abs_diff(requested.sample_rate_hz);
            let ch_dist = f.channels.abs_diff(requested.channels);
            (rate_dist, ch_dist)
        })
        .copied()
        .ok_or_else(|| AudioError::InvalidConfig("no supported input formats".into()))
}

/// Pull-based source fed by a push channel; the bridge half of
/// [`CpalSource`], testable without a device. After [`SampleSource::stop`],
/// buffered chunks still drain, then the source ends.
pub(crate) struct ChannelSource {
    format: SourceFormat,
    rx: Receiver<Vec<i16>>,
    stopped: Arc<AtomicBool>,
}

impl ChannelSource {
    pub(crate) fn new(format: SourceFormat) -> (Sender<Vec<i16>>, Self) {
        let (tx, rx) = std::sync::mpsc::channel();
        let source = Self {
            format,
            rx,
            stopped: Arc::new(AtomicBool::new(false)),
        };
        (tx, source)
    }

    pub(crate) fn stop_flag(&self) -> Arc<AtomicBool> {
        Arc::clone(&self.stopped)
    }
}

impl SampleSource for ChannelSource {
    fn format(&self) -> SourceFormat {
        self.format
    }

    fn next_chunk(&mut self) -> Option<Vec<i16>> {
        loop {
            if self.stopped.load(Ordering::Relaxed) {
                return self.rx.try_recv().ok();
            }
            match self.rx.recv_timeout(Duration::from_millis(50)) {
                Ok(chunk) => return Some(chunk),
                Err(RecvTimeoutError::Timeout) => continue,
                Err(RecvTimeoutError::Disconnected) => return None,
            }
        }
    }

    fn stop(&mut self) {
        self.stopped.store(true, Ordering::Relaxed);
    }
}

/// Desktop capture source: default input device via cpal, 16-bit PCM,
/// falling back to the nearest supported format (see [`negotiate`]).
///
/// The stream runs on cpal's audio thread and only pushes chunks into a
/// channel — no allocation-heavy work or I/O on the callback beyond the
/// copy. Dropping the [`CpalSource`] stops capture.
pub struct CpalSource {
    format: SourceFormat,
    inner: ChannelSource,
    _stream: cpal::Stream,
}

impl CpalSource {
    /// Open the default input device targeting `requested`. The actual
    /// delivered format is reported by [`SampleSource::format`] and must
    /// be what downstream outputs record.
    pub fn open(requested: SourceFormat) -> Result<Self, AudioError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or_else(|| AudioError::InvalidConfig("no default input device".into()))?;

        // Candidate formats: one per I16 config range, with the requested
        // rate clamped into the range's min/max (the clamped rate is then
        // legal for that range, so the range itself is not needed).
        let mut candidates: Vec<SourceFormat> = Vec::new();
        for range in device.supported_input_configs()? {
            if range.sample_format() != cpal::SampleFormat::I16 {
                continue;
            }
            let rate = requested
                .sample_rate_hz
                .clamp(range.min_sample_rate(), range.max_sample_rate());
            candidates.push(SourceFormat {
                sample_rate_hz: rate,
                channels: range.channels(),
            });
        }
        let chosen = negotiate(requested, &candidates)?;

        let config = cpal::StreamConfig {
            channels: chosen.channels,
            sample_rate: chosen.sample_rate_hz,
            buffer_size: cpal::BufferSize::Default,
        };
        let (tx, inner) = ChannelSource::new(chosen);
        let err_flag = inner.stop_flag();
        let stream = device.build_input_stream::<i16, _, _>(
            config,
            move |data, _| {
                if !err_flag.load(Ordering::Relaxed) {
                    // Channel send failure means the consumer is gone;
                    // dropping the sample chunk is correct then.
                    let _ = tx.send(data.to_vec());
                }
            },
            |err| eprintln!("zikos: input stream error: {err}"),
            None,
        )?;
        stream.play()?;
        Ok(Self {
            format: chosen,
            inner,
            _stream: stream,
        })
    }
}

impl SampleSource for CpalSource {
    fn format(&self) -> SourceFormat {
        self.format
    }

    fn next_chunk(&mut self) -> Option<Vec<i16>> {
        self.inner.next_chunk()
    }

    fn stop(&mut self) {
        self.inner.stop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{SampleSource, SourceFormat};

    fn fmt(rate: u32, ch: u16) -> SourceFormat {
        SourceFormat {
            sample_rate_hz: rate,
            channels: ch,
        }
    }

    #[test]
    fn negotiation_falls_back_to_nearest() {
        let requested = fmt(48_000, 2);
        // Rate distance is primary: 48 kHz mono beats 44.1 kHz mono.
        let supported = vec![fmt(44_100, 1), fmt(48_000, 1)];
        assert_eq!(negotiate(requested, &supported).unwrap(), fmt(48_000, 1));
        // Only option wins even when both dims differ.
        let supported = vec![fmt(44_100, 2)];
        assert_eq!(negotiate(requested, &supported).unwrap(), fmt(44_100, 2));
        // Exact match preferred.
        let supported = vec![fmt(44_100, 2), fmt(48_000, 2)];
        assert_eq!(negotiate(requested, &supported).unwrap(), fmt(48_000, 2));
        // Channel distance breaks rate ties.
        let supported = vec![fmt(48_000, 1), fmt(48_000, 6)];
        assert_eq!(negotiate(requested, &supported).unwrap(), fmt(48_000, 1));
    }

    #[test]
    fn negotiation_rejects_empty_support() {
        assert!(negotiate(fmt(48_000, 2), &[]).is_err());
    }

    #[test]
    fn channel_bridge_yields_source_chunks() {
        let format = fmt(48_000, 2);
        let (tx, mut src) = ChannelSource::new(format);
        assert_eq!(src.format(), format);

        tx.send(vec![1, -1, 2, -2]).unwrap();
        tx.send(vec![3, -3]).unwrap();
        assert_eq!(src.next_chunk(), Some(vec![1, -1, 2, -2]));
        assert_eq!(src.next_chunk(), Some(vec![3, -3]));

        // After stop, buffered chunks still drain, then the source ends.
        tx.send(vec![4, -4]).unwrap();
        src.stop();
        assert_eq!(src.next_chunk(), Some(vec![4, -4]));
        assert_eq!(src.next_chunk(), None);
        assert_eq!(src.next_chunk(), None);
    }
}

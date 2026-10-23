//! Recorder sessions: capture → staged WAV, with a crash-recoverable
//! per-session directory (design D4).
//!
//! Layout: `<sessions_root>/<uuid>/take.wav` (streaming staging) +
//! `session.json` (atomically written manifest holding the actual format,
//! `started_at`, and state).

use crate::{AudioError, RecorderConfig, SampleSource, StreamingWavWriter};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Streaming staging file inside a session directory.
pub const TAKE_WAV: &str = "take.wav";
/// Atomically written manifest inside a session directory.
pub const SESSION_JSON: &str = "session.json";
/// Finalized FLAC inside a session directory.
pub const TAKE_FLAC: &str = "take.flac";
/// Finalized take metadata inside a session directory.
pub const META_JSON: &str = "meta.json";

/// Lifecycle state of a session, persisted in [`SESSION_JSON`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum SessionState {
    /// Recording; `take.wav` may be unpatched on disk.
    InProgress,
    /// Cleanly stopped; WAV is valid and awaits finalization.
    Staged,
    /// Finalized to FLAC + metadata.
    Finalized,
}

/// Durable facts about a recording session; doubles as the crash-recovery
/// marker (`state == InProgress` marks a session worth recovering).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SessionManifest {
    /// What the recorder asked for (kept for diagnostics).
    pub requested_sample_rate_hz: u32,
    /// What the recorder asked for (kept for diagnostics).
    pub requested_channels: u16,
    /// What the source actually delivered; all outputs report this.
    pub sample_rate_hz: u32,
    /// What the source actually delivered; all outputs report this.
    pub channels: u16,
    /// Unix timestamp (seconds) of the moment recording started.
    pub started_at: u64,
    /// Lifecycle marker; `InProgress` marks a session worth recovering.
    pub state: SessionState,
}

impl SessionManifest {
    /// Parse the manifest from a session directory.
    pub fn load(dir: &Path) -> Result<Self, AudioError> {
        let bytes = fs::read(dir.join(SESSION_JSON))?;
        serde_json::from_slice(&bytes)
            .map_err(|e| AudioError::Parse(format!("invalid {SESSION_JSON}: {e}")))
    }

    /// Persist the manifest atomically (temp file + rename).
    pub fn save(&self, dir: &Path) -> Result<(), AudioError> {
        let tmp = dir.join(format!("{SESSION_JSON}.tmp"));
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(&tmp, dir.join(SESSION_JSON))?;
        Ok(())
    }
}

/// A live recording session: owns the source and the streaming WAV.
pub struct RecorderSession {
    dir: PathBuf,
    manifest: SessionManifest,
    writer: StreamingWavWriter,
    source: Box<dyn SampleSource>,
    paused: bool,
}

impl std::fmt::Debug for RecorderSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RecorderSession")
            .field("dir", &self.dir)
            .field("manifest", &self.manifest)
            .finish_non_exhaustive()
    }
}

impl RecorderSession {
    /// Start recording into a fresh session directory under
    /// `sessions_root`. `started_at` (unix seconds) is persisted
    /// immediately so naming and metadata survive crashes.
    pub fn start(
        config: &RecorderConfig,
        source: Box<dyn SampleSource>,
        sessions_root: &Path,
        started_at: u64,
    ) -> Result<Self, AudioError> {
        config.validate()?;
        let actual = source.format();
        let dir = sessions_root.join(uuid::Uuid::new_v4().simple().to_string());
        fs::create_dir_all(&dir)?;
        let writer = StreamingWavWriter::create(
            &dir.join(TAKE_WAV),
            actual.sample_rate_hz,
            actual.channels,
        )?;
        let manifest = SessionManifest {
            requested_sample_rate_hz: config.sample_rate_hz,
            requested_channels: config.channels,
            sample_rate_hz: actual.sample_rate_hz,
            channels: actual.channels,
            started_at,
            state: SessionState::InProgress,
        };
        manifest.save(&dir)?;
        Ok(Self {
            dir,
            manifest,
            writer,
            source,
            paused: false,
        })
    }

    /// Pause capture: while paused, [`pull`](Self::pull) drains the source
    /// but appends nothing, so paused spans are excluded from the take's
    /// audio and duration. Pausing an already-paused session is an error.
    pub fn pause(&mut self) -> Result<(), AudioError> {
        if self.paused {
            return Err(AudioError::InvalidState("already paused".into()));
        }
        self.paused = true;
        Ok(())
    }

    /// Resume a paused session; the same take keeps filling. Resuming a
    /// session that is not paused is an error.
    pub fn resume(&mut self) -> Result<(), AudioError> {
        if !self.paused {
            return Err(AudioError::InvalidState("not paused".into()));
        }
        self.paused = false;
        Ok(())
    }

    /// Whether the session is currently paused.
    pub fn is_paused(&self) -> bool {
        self.paused
    }

    /// Drain everything the source currently offers into the staged WAV
    /// (flushed to disk). Returns the number of frames appended. While
    /// paused, chunks are drained and discarded — nothing is written.
    /// The session stays `in-progress`.
    pub fn pull(&mut self) -> Result<u64, AudioError> {
        let channels = usize::from(self.manifest.channels);
        let mut frames = 0u64;
        while let Some(chunk) = self.source.next_chunk() {
            if !self.paused {
                frames += (chunk.len() / channels) as u64;
                self.writer.append(&chunk)?;
            }
        }
        self.writer.flush()?;
        Ok(frames)
    }

    /// Stop recording: signal the source to end, drain whatever it still
    /// holds, patch the WAV header, mark the session `staged`.
    pub fn stop(mut self) -> Result<StagedSession, AudioError> {
        self.source.stop();
        self.pull()?;
        self.writer.close()?;
        self.manifest.state = SessionState::Staged;
        self.manifest.save(&self.dir)?;
        Ok(StagedSession {
            dir: self.dir,
            manifest: self.manifest,
        })
    }

    /// The session directory on disk.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The session manifest.
    pub fn manifest(&self) -> &SessionManifest {
        &self.manifest
    }
}

/// A cleanly stopped session awaiting finalize or discard.
#[derive(Debug)]
pub struct StagedSession {
    dir: PathBuf,
    manifest: SessionManifest,
}

impl StagedSession {
    /// Load a staged session back from its directory (used by recovery).
    pub fn load(dir: &Path) -> Result<Self, AudioError> {
        let manifest = SessionManifest::load(dir)?;
        if manifest.state != SessionState::Staged {
            return Err(AudioError::Parse(format!(
                "session {} is {}, not staged",
                dir.display(),
                if manifest.state == SessionState::InProgress {
                    "in-progress"
                } else {
                    "finalized"
                }
            )));
        }
        Ok(Self {
            dir: dir.into(),
            manifest,
        })
    }

    /// Delete the session directory, keeping siblings untouched.
    pub fn discard(self) -> Result<(), AudioError> {
        fs::remove_dir_all(&self.dir)?;
        Ok(())
    }

    /// Finalize to FLAC (the default). See [`finalize_as`](Self::finalize_as).
    pub fn finalize(self, tz: &jiff::tz::TimeZone) -> Result<FinalizedTake, AudioError> {
        self.finalize_as(tz, zikos_core::AudioFormat::Flac)
    }

    /// Finalize the staged session in the requested container: FLAC
    /// transcodes the staged WAV to `take.flac` and removes the staging
    /// file; WAV keeps the raw staged WAV as the take unchanged
    /// (passthrough). Both write `meta.json` with the default title
    /// derived from the persisted start time in `tz` (pass
    /// `jiff::tz::TimeZone::system()` for local-time naming) and record
    /// the chosen format.
    pub fn finalize_as(
        self,
        tz: &jiff::tz::TimeZone,
        format: zikos_core::AudioFormat,
    ) -> Result<FinalizedTake, AudioError> {
        let Self { dir, manifest } = self;
        let wav_path = dir.join(TAKE_WAV);
        let flac_path = dir.join(TAKE_FLAC);
        let meta_path = dir.join(META_JSON);

        let info = crate::wav_info(&fs::read(&wav_path)?)?;
        let meta = zikos_core::RecordingMeta {
            title: crate::default_title(manifest.started_at, tz)?,
            format,
            started_at: manifest.started_at,
            duration_secs: info.frames as f64 / info.sample_rate_hz as f64,
            sample_rate_hz: manifest.sample_rate_hz,
            channels: manifest.channels,
            bpm: None,
            comment: None,
        };
        meta.validate()
            .map_err(|e| AudioError::Meta(e.to_string()))?;

        let audio_path = match format {
            zikos_core::AudioFormat::Flac => {
                crate::flac::transcode_wav_to_flac(&wav_path, &flac_path)?;
                fs::remove_file(&wav_path)?;
                flac_path
            }
            // Passthrough: the staged WAV is already a valid, patched file.
            zikos_core::AudioFormat::Wav => wav_path,
            zikos_core::AudioFormat::Opus => {
                return Err(AudioError::Unsupported(
                    "opus finalization is not implemented".into(),
                ));
            }
        };
        fs::write(&meta_path, serde_json::to_vec_pretty(&meta)?)?;

        let mut manifest = manifest;
        manifest.state = SessionState::Finalized;
        manifest.save(&dir)?;

        Ok(FinalizedTake {
            dir,
            audio_path,
            meta_path,
            meta,
        })
    }

    /// The session directory on disk.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The session manifest.
    pub fn manifest(&self) -> &SessionManifest {
        &self.manifest
    }
}

/// The outcome of a successful finalize: audio + metadata on disk (FLAC
/// transcoded, or the raw WAV when passthrough was requested).
#[derive(Debug)]
pub struct FinalizedTake {
    dir: PathBuf,
    audio_path: PathBuf,
    meta_path: PathBuf,
    meta: zikos_core::RecordingMeta,
}

impl FinalizedTake {
    /// The take's audio file (`take.flac`, or the raw `take.wav` for WAV
    /// passthrough).
    pub fn audio_path(&self) -> &Path {
        &self.audio_path
    }

    /// The take's audio file; kept for callers finalized to FLAC.
    pub fn flac_path(&self) -> &Path {
        &self.audio_path
    }

    /// The `meta.json` file written by finalization.
    pub fn meta_path(&self) -> &Path {
        &self.meta_path
    }

    /// The take's metadata (mirrors `meta.json`).
    pub fn meta(&self) -> &zikos_core::RecordingMeta {
        &self.meta
    }

    /// The session directory holding the finalized artifacts.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{RecorderConfig, SineSource, SourceFormat, WavInfo};
    use std::collections::VecDeque;
    use std::path::Path;

    fn wav_info_on(path: &Path) -> WavInfo {
        crate::wav_info(&std::fs::read(path).unwrap()).unwrap()
    }

    /// Queue-driven source: the test keeps a handle after the source is
    /// boxed into the session, so chunks can be fed while paused — exactly
    /// how a live input keeps delivering during a live span. Shared via
    /// `Arc<Mutex>` so the source stays `Send` like a real one.
    #[derive(Clone, Default)]
    struct SharedQueue(std::sync::Arc<std::sync::Mutex<VecDeque<Vec<i16>>>>);

    impl SharedQueue {
        fn push_frames(&self, format: SourceFormat, frames: u64) {
            let channels = u64::from(format.channels);
            self.0
                .lock()
                .unwrap()
                .push_back(vec![0i16; (frames * channels) as usize]);
        }
    }

    struct ManualSource {
        format: SourceFormat,
        queue: SharedQueue,
    }

    impl ManualSource {
        fn new(sample_rate_hz: u32, channels: u16) -> (Self, SharedQueue) {
            let format = SourceFormat {
                sample_rate_hz,
                channels,
            };
            let queue = SharedQueue::default();
            (
                Self {
                    format,
                    queue: queue.clone(),
                },
                queue,
            )
        }
    }

    impl SampleSource for ManualSource {
        fn format(&self) -> SourceFormat {
            self.format
        }

        fn next_chunk(&mut self) -> Option<Vec<i16>> {
            self.queue.0.lock().unwrap().pop_front()
        }
    }

    #[test]
    fn paused_pull_drains_source_but_writes_nothing() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let (source, queue) = ManualSource::new(48_000, 2);
        let format = source.format();
        queue.push_frames(format, 24_000);
        let mut session = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(source),
            &sessions,
            1_776_000_000,
        )
        .unwrap();

        let wav = session.dir().join(TAKE_WAV);
        session.pull().unwrap();
        let size_before_pause = std::fs::metadata(&wav).unwrap().len();

        // Paused: audio keeps arriving, is drained, and nothing is
        // appended; pull() reports zero frames written.
        session.pause().unwrap();
        assert!(session.is_paused());
        queue.push_frames(format, 48_000);
        assert_eq!(session.pull().unwrap(), 0);
        assert_eq!(
            std::fs::metadata(&wav).unwrap().len(),
            size_before_pause,
            "no bytes may be appended while paused"
        );

        // Pausing twice / resuming while live are invalid-state errors.
        assert!(session.pause().is_err());
        session.resume().unwrap();
        assert!(!session.is_paused());
        assert!(session.resume().is_err());

        // After resume the same stream keeps writing into the same take.
        queue.push_frames(format, 24_000);
        assert_eq!(session.pull().unwrap(), 24_000);
        let staged = session.stop().unwrap();
        let info = wav_info_on(&staged.dir().join(TAKE_WAV));
        assert_eq!(info.frames, 48_000);
    }

    #[test]
    fn finalize_duration_excludes_paused_span() {
        // Spec scenario: record 2 s, pause 5 s, resume, record 3 s →
        // a 5-second take.
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let (source, queue) = ManualSource::new(48_000, 2);
        let format = source.format();
        queue.push_frames(format, 96_000); // 2 s
        let mut session = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(source),
            &sessions,
            1_792_656_900,
        )
        .unwrap();
        assert_eq!(session.pull().unwrap(), 96_000);

        session.pause().unwrap();
        for _ in 0..5 {
            queue.push_frames(format, 48_000); // 5 s arriving while paused
        }
        assert_eq!(session.pull().unwrap(), 0);

        session.resume().unwrap();
        for _ in 0..3 {
            queue.push_frames(format, 48_000); // 3 s more
        }
        assert_eq!(session.pull().unwrap(), 144_000);

        let take = session
            .stop()
            .unwrap()
            .finalize(&jiff::tz::TimeZone::UTC)
            .unwrap();
        assert!(
            (take.meta().duration_secs - 5.0).abs() < 1e-3,
            "duration {}",
            take.meta().duration_secs
        );
        let reader = claxon::FlacReader::open(take.flac_path()).unwrap();
        assert_eq!(reader.streaminfo().samples, Some(240_000));
    }

    #[test]
    fn start_creates_in_progress_session() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let mut session = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(SineSource::new(48_000, 2, 24_000)),
            &sessions,
            1_776_000_000,
        )
        .unwrap();
        let pulled = session.pull().unwrap();
        assert_eq!(pulled, 24_000);

        // Exactly one session dir, holding the streamed WAV and manifest.
        let mut entries = std::fs::read_dir(&sessions).unwrap();
        let dir = entries.next().unwrap().unwrap().path();
        assert!(entries.next().is_none());
        assert_eq!(dir, session.dir());
        // All frames are on disk even before the header patch:
        // 44 + 24_000 frames * 2 channels * 2 bytes.
        assert_eq!(
            std::fs::metadata(dir.join(TAKE_WAV)).unwrap().len(),
            44 + 24_000 * 4
        );
        let manifest = SessionManifest::load(&dir).unwrap();
        assert_eq!(manifest.state, SessionState::InProgress);
        assert_eq!(manifest.started_at, 1_776_000_000);
        assert_eq!(manifest.sample_rate_hz, 48_000);
        assert_eq!(manifest.channels, 2);
        assert_eq!(manifest.requested_sample_rate_hz, 48_000);
        assert_eq!(manifest.requested_channels, 2);
    }

    #[test]
    fn stop_yields_valid_staged_wav() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let session = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(SineSource::new(48_000, 2, 24_000)),
            &sessions,
            1_776_000_000,
        )
        .unwrap();
        let staged = session.stop().unwrap();
        assert_eq!(staged.dir(), session_dir_of(&sessions));

        let info = wav_info_on(&staged.dir().join(TAKE_WAV));
        assert_eq!(info.sample_rate_hz, 48_000);
        assert_eq!(info.channels, 2);
        assert_eq!(info.frames, 24_000);
        assert!((info.duration_secs - 0.5).abs() < 1e-3);
        assert_eq!(
            SessionManifest::load(staged.dir()).unwrap().state,
            SessionState::Staged
        );
    }

    #[test]
    fn session_uses_source_actual_format() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        // Recorder requests 48 kHz stereo; the source delivers 44.1 kHz
        // mono. Everything on disk reports the actual format.
        let session = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(SineSource::new(44_100, 1, 22_050)),
            &sessions,
            1_776_000_000,
        )
        .unwrap();
        let staged = session.stop().unwrap();

        let info = wav_info_on(&staged.dir().join(TAKE_WAV));
        assert_eq!(info.sample_rate_hz, 44_100);
        assert_eq!(info.channels, 1);
        assert_eq!(info.frames, 22_050);
        let manifest = SessionManifest::load(staged.dir()).unwrap();
        assert_eq!(manifest.sample_rate_hz, 44_100);
        assert_eq!(manifest.channels, 1);
    }

    fn session_dir_of(sessions: &Path) -> std::path::PathBuf {
        let mut entries = std::fs::read_dir(sessions).unwrap();
        entries.next().unwrap().unwrap().path()
    }

    #[test]
    fn finalize_produces_meta_and_flac() {
        use jiff::tz::TimeZone;
        use zikos_core::AudioFormat;

        const STARTED_AT: u64 = 1_792_656_900; // 2026-10-22T08:15:00Z
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let session = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(SineSource::new(48_000, 2, 24_000)),
            &sessions,
            STARTED_AT,
        )
        .unwrap();
        let staged = session.stop().unwrap();
        let dir = staged.dir().to_path_buf();
        let take = staged.finalize(&TimeZone::UTC).unwrap();

        // Metadata: default title from persisted start time, actual format.
        assert_eq!(take.meta().title, "take-20261022-0815");
        assert_eq!(take.meta().format, AudioFormat::Flac);
        assert_eq!(take.meta().started_at, STARTED_AT);
        assert!((take.meta().duration_secs - 0.5).abs() < 1e-3);
        assert_eq!(take.meta().sample_rate_hz, 48_000);
        assert_eq!(take.meta().channels, 2);
        assert_eq!(take.meta().bpm, None);
        assert_eq!(take.meta().comment, None);
        take.meta().validate().unwrap();

        // Artifacts on disk: FLAC + meta.json present, staging WAV gone.
        assert!(take.flac_path().is_file());
        assert!(take.meta_path().is_file());
        assert_eq!(take.flac_path(), dir.join("take.flac"));
        assert!(!dir.join(TAKE_WAV).exists());
        assert_eq!(
            SessionManifest::load(&dir).unwrap().state,
            SessionState::Finalized
        );
        // meta.json parses back to the same metadata.
        let meta_json: zikos_core::RecordingMeta =
            serde_json::from_slice(&std::fs::read(take.meta_path()).unwrap()).unwrap();
        assert_eq!(&meta_json, take.meta());
        // The FLAC holds exactly the staged frames.
        let reader = claxon::FlacReader::open(take.flac_path()).unwrap();
        assert_eq!(reader.streaminfo().samples, Some(24_000));
    }

    #[test]
    fn finalize_60s_take_duration_within_tolerance() {
        use jiff::tz::TimeZone;
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let session = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(SineSource::new(48_000, 2, 2_880_000)),
            &sessions,
            1_792_656_900,
        )
        .unwrap();
        let take = session.stop().unwrap().finalize(&TimeZone::UTC).unwrap();
        assert!((take.meta().duration_secs - 60.0).abs() <= 0.5);
        let reader = claxon::FlacReader::open(take.flac_path()).unwrap();
        let samples = reader.streaminfo().samples.unwrap();
        let duration = samples as f64 / reader.streaminfo().sample_rate as f64;
        assert!(
            (duration - 60.0).abs() <= 0.5,
            "decoded duration {duration}"
        );
    }

    #[test]
    fn finalize_as_wav_keeps_raw_wav_and_records_format() {
        use jiff::tz::TimeZone;
        use zikos_core::AudioFormat;

        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let staged = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(SineSource::new(48_000, 2, 24_000)),
            &sessions,
            1_792_656_900,
        )
        .unwrap()
        .stop()
        .unwrap();
        let dir = staged.dir().to_path_buf();
        let wav_bytes_before = fs::read(dir.join(TAKE_WAV)).unwrap();

        let take = staged
            .finalize_as(&TimeZone::UTC, AudioFormat::Wav)
            .unwrap();

        // Passthrough: the staged WAV *is* the take, byte-for-byte.
        assert_eq!(take.meta().format, AudioFormat::Wav);
        assert_eq!(take.audio_path(), dir.join(TAKE_WAV));
        assert_eq!(fs::read(take.audio_path()).unwrap(), wav_bytes_before);
        assert!((take.meta().duration_secs - 0.5).abs() < 1e-3);
        assert!(!dir.join(TAKE_FLAC).exists());
        assert_eq!(
            SessionManifest::load(&dir).unwrap().state,
            SessionState::Finalized
        );
    }

    #[test]
    fn finalize_as_rejects_unsupported_formats() {
        use jiff::tz::TimeZone;
        use zikos_core::AudioFormat;

        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let staged = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(SineSource::new(48_000, 2, 24_000)),
            &sessions,
            1_792_656_900,
        )
        .unwrap()
        .stop()
        .unwrap();

        assert!(
            staged
                .finalize_as(&TimeZone::UTC, AudioFormat::Opus)
                .is_err()
        );
    }

    #[test]
    fn finalize_as_flac_matches_default_finalize() {
        use jiff::tz::TimeZone;
        use zikos_core::AudioFormat;

        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let staged = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(SineSource::new(48_000, 2, 24_000)),
            &sessions,
            1_792_656_900,
        )
        .unwrap()
        .stop()
        .unwrap();
        let dir = staged.dir().to_path_buf();

        let take = staged
            .finalize_as(&TimeZone::UTC, AudioFormat::Flac)
            .unwrap();
        assert_eq!(take.meta().format, AudioFormat::Flac);
        assert_eq!(take.audio_path(), dir.join(TAKE_FLAC));
        assert!(take.audio_path().is_file());
        assert!(!dir.join(TAKE_WAV).exists());
    }
}

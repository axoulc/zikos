//! Interrupted-session recovery (spec: interrupted sessions).
//!
//! On start, sessions whose manifest is still `in-progress` (process died
//! or the OS killed capture) are detected; their partial WAV is repaired
//! from the bytes on disk and the take is offered for finalize or discard.

use crate::{AudioError, SessionManifest, SessionState, StagedSession, TAKE_WAV, repair_wav};
use std::fs;
use std::path::{Path, PathBuf};

/// List session directories under `sessions_root` that were left
/// `in-progress` by a crash and are recoverable. Directories without a
/// parseable manifest are not sessions and are skipped.
pub fn recoverable_sessions(sessions_root: &Path) -> Result<Vec<PathBuf>, AudioError> {
    let mut found = Vec::new();
    for entry in fs::read_dir(sessions_root)? {
        let path = entry?.path();
        if !path.is_dir() {
            continue;
        }
        match SessionManifest::load(&path) {
            Ok(m) if m.state == SessionState::InProgress => found.push(path),
            Ok(_) => {}
            // Missing manifest (or unparseable content): not a session.
            Err(_) => {}
        }
    }
    Ok(found)
}

/// Recover an interrupted session: repair the partial WAV header from the
/// bytes on disk and mark the session `staged`, ready for
/// [`StagedSession::finalize`] or [`StagedSession::discard`].
pub fn recover(dir: &Path) -> Result<StagedSession, AudioError> {
    let mut manifest = SessionManifest::load(dir)?;
    if manifest.state != SessionState::InProgress {
        return Err(AudioError::Parse(format!(
            "session {} is {}, not in-progress",
            dir.display(),
            match manifest.state {
                SessionState::InProgress => unreachable!(),
                SessionState::Staged => "staged",
                SessionState::Finalized => "finalized",
            }
        )));
    }
    repair_wav(&dir.join(TAKE_WAV))?;
    manifest.state = SessionState::Staged;
    manifest.save(dir)?;
    StagedSession::load(dir)
}

#[cfg(test)]
mod tests {
    use crate::{
        RecorderConfig, RecorderSession, SessionManifest, SessionState, SineSource, StagedSession,
        StreamingWavWriter, TAKE_WAV, recover, recoverable_sessions, repair_wav,
    };
    use std::path::{Path, PathBuf};

    /// Plant an in-progress session the way a crash would leave it:
    /// manifest `in-progress`, WAV on disk with unpatched header.
    fn plant_in_progress(
        sessions_root: &Path,
        rate: u32,
        channels: u16,
        frames: u64,
        started_at: u64,
    ) -> PathBuf {
        let dir = sessions_root.join(format!("plant-{started_at}"));
        std::fs::create_dir_all(&dir).unwrap();
        let mut w = StreamingWavWriter::create(&dir.join(TAKE_WAV), rate, channels).unwrap();
        let channels_us = usize::from(channels);
        let total = (frames * channels as u64) as usize;
        let mut i = 0;
        while i < total {
            let end = (i + 2_400 * channels_us).min(total);
            let chunk: Vec<i16> = (i..end)
                .map(|n| {
                    let f = (n / channels_us) as f64;
                    ((f / rate as f64 * 440.0 * std::f64::consts::TAU).sin() * i16::MAX as f64)
                        as i16
                })
                .collect();
            w.append(&chunk).unwrap();
            i = end;
        }
        w.flush().unwrap();
        drop(w); // crash: no close()
        SessionManifest {
            requested_sample_rate_hz: 48_000,
            requested_channels: 2,
            sample_rate_hz: rate,
            channels,
            started_at,
            state: SessionState::InProgress,
        }
        .save(&dir)
        .unwrap();
        dir
    }

    /// Plant a cleanly staged sibling session (via the real pipeline).
    fn plant_staged(sessions_root: &Path) -> PathBuf {
        let session = RecorderSession::start(
            &RecorderConfig::default(),
            Box::new(SineSource::new(48_000, 2, 100)),
            sessions_root,
            1_000,
        )
        .unwrap();
        let staged = session.stop().unwrap();
        staged.dir().to_path_buf()
    }

    #[test]
    fn scan_finds_only_in_progress() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        std::fs::create_dir_all(&sessions).unwrap();
        let in_progress = plant_in_progress(&sessions, 48_000, 2, 500, 100);
        let _staged = plant_staged(&sessions);
        // A finalized session dir, planted by hand.
        let fin = sessions.join("plant-finalized");
        std::fs::create_dir_all(&fin).unwrap();
        SessionManifest {
            requested_sample_rate_hz: 48_000,
            requested_channels: 2,
            sample_rate_hz: 48_000,
            channels: 2,
            started_at: 99,
            state: SessionState::Finalized,
        }
        .save(&fin)
        .unwrap();
        // A non-session directory must be skipped without error.
        std::fs::create_dir_all(sessions.join("not-a-session")).unwrap();

        let found = recoverable_sessions(&sessions).unwrap();
        assert_eq!(found, vec![in_progress.clone()]);
    }

    #[test]
    fn repaired_partial_has_exact_frames() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        // 30 s equivalent of stereo 48 kHz, killed mid-recording.
        let dir = plant_in_progress(&sessions, 48_000, 2, 1_440_000, 1_792_656_900);

        let staged = recover(&dir).unwrap();
        assert_eq!(staged.manifest().started_at, 1_792_656_900);
        let info = crate::wav_info(&std::fs::read(dir.join(TAKE_WAV)).unwrap()).unwrap();
        assert_eq!(info.frames, 1_440_000);
        assert!((info.duration_secs - 30.0).abs() < 1e-6);
        assert_eq!(staged.manifest().state, SessionState::Staged);

        // Repair is idempotent for an already-patched file.
        assert_eq!(repair_wav(&dir.join(TAKE_WAV)).unwrap(), 1_440_000);
    }

    #[test]
    fn recovered_finalize_uses_persisted_started_at() {
        use jiff::tz::{Offset, TimeZone};
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        // Started 2026-10-22T08:15:00Z = 10:15 in +02:00.
        let dir = plant_in_progress(&sessions, 48_000, 2, 24_000, 1_792_656_900);

        let staged: StagedSession = recover(&dir).unwrap();
        let take = staged
            .finalize(&TimeZone::fixed(Offset::constant(2)))
            .unwrap();
        assert_eq!(take.meta().title, "take-20261022-1015");
        assert_eq!(take.meta().started_at, 1_792_656_900);
        let reader = claxon::FlacReader::open(take.flac_path()).unwrap();
        assert_eq!(reader.streaminfo().samples, Some(24_000));
        assert!(!dir.join(TAKE_WAV).exists());
    }

    #[test]
    fn discard_removes_only_that_session() {
        let root = tempfile::tempdir().unwrap();
        let sessions = root.path().join("sessions");
        let victim = plant_in_progress(&sessions, 48_000, 2, 500, 100);
        let sibling = plant_staged(&sessions);

        recover(&victim).unwrap().discard().unwrap();
        assert!(!victim.exists());
        assert!(sibling.join(TAKE_WAV).exists());
    }
}

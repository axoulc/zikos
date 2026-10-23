//! The dictaphone: capture commands, live state, interrupted-session
//! recovery.

use crate::frb_generated::StreamSink;

/// Recorder lifecycle as the record screen renders it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordingStatus {
    /// Nothing captured; the record control starts a take.
    Idle,
    /// Capturing; the timer runs.
    Recording,
    /// Paused; the timer is frozen.
    Paused,
}

/// Live recorder state for the timer and controls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordingStateView {
    /// Recorder lifecycle.
    pub status: RecordingStatus,
    /// Elapsed recording time; paused spans excluded.
    pub elapsed_ms: u32,
}

/// A finalized take, ready to be filed in the tree.
#[derive(Debug, Clone, PartialEq)]
pub struct TakeView {
    /// Default `take-YYYYMMDD-HHMM` title.
    pub title: String,
    /// Lowercase format label (`flac` / `wav`).
    pub format: String,
    /// Recorded duration in seconds.
    pub duration_secs: f64,
    /// Absolute path of the take's audio file.
    pub audio_path: String,
}

/// A previous session found interrupted (crash/kill) on startup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InterruptedSessionView {
    /// Unix seconds of the moment that take started.
    pub started_at: u64,
}

/// Current recorder state.
pub fn recording_state() -> RecordingStateView {
    crate::state::with_app(|app| app.recording_state())
}

/// Recorder state as a stream; emits on every state or elapsed change.
pub fn recording_state_stream(sink: StreamSink<RecordingStateView>) {
    let mut rx = crate::state::with_app(|app| app.recording_updates());
    // Emit the current state immediately so late subscribers paint.
    let current = crate::state::with_app(|app| app.recording_state());
    if sink.add(current).is_err() {
        return;
    }
    loop {
        match rx.blocking_recv() {
            Ok(state) => {
                if sink.add(state).is_err() {
                    break; // Dart side cancelled the subscription.
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Start a take from the default input.
pub fn record_start() -> Result<(), String> {
    crate::state::with_app(|app| app.record_start()).map_err(|e| e.to_string())
}

/// Pause the active take.
pub fn record_pause() -> Result<(), String> {
    crate::state::with_app(|app| app.record_pause()).map_err(|e| e.to_string())
}

/// Resume the paused take.
pub fn record_resume() -> Result<(), String> {
    crate::state::with_app(|app| app.record_resume()).map_err(|e| e.to_string())
}

/// Stop and finalize the active take in the configured format.
pub fn record_stop() -> Result<TakeView, String> {
    crate::state::with_app(|app| app.record_stop()).map_err(|e| e.to_string())
}

/// The interrupted session awaiting a finalize-or-discard decision.
pub fn interrupted_session() -> Result<Option<InterruptedSessionView>, String> {
    crate::state::with_app(|app| app.interrupted_session()).map_err(|e| e.to_string())
}

/// Finalize the interrupted session into a take.
pub fn interrupted_finalize() -> Result<TakeView, String> {
    crate::state::with_app(|app| app.interrupted_finalize()).map_err(|e| e.to_string())
}

/// Discard the interrupted session.
pub fn interrupted_discard() -> Result<(), String> {
    crate::state::with_app(|app| app.interrupted_discard()).map_err(|e| e.to_string())
}

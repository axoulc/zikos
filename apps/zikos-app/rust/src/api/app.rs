//! App lifecycle: initialize the Rust state, observe the session.

use crate::frb_generated::StreamSink;

/// Whether a Matrix session exists, and as whom.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionView {
    /// A session exists.
    pub signed_in: bool,
    /// The user's Matrix ID when signed in.
    pub user_id: Option<String>,
}

/// What `init` hands back: session state plus effective settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppSnapshot {
    /// Session state.
    pub session: SessionView,
    /// Loaded (or default) settings.
    pub settings: super::settings::SettingsView,
}

/// Initialize the Rust app state rooted at `app_dir` (the app's
/// documents/config directory) and start the capture pump thread.
/// Idempotent: re-init keeps the existing state.
pub fn init(app_dir: String) -> AppSnapshot {
    crate::state::global_init(&app_dir)
}

/// Session snapshot as a stream. Emits the current state immediately;
/// nothing can change the session until a login flow exists, so the
/// stream then completes.
pub fn session_stream(sink: StreamSink<SessionView>) {
    // No login flow exists in this change, so the app is always signed
    // out; the login change will make this reflect a restored session.
    let _ = sink.add(SessionView {
        signed_in: false,
        user_id: None,
    });
}

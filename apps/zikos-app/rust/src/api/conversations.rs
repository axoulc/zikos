//! The conversation list: snapshots from the Matrix session, cached in
//! the app state for the Files/Record screens. The mapping from Matrix
//! conversations lives in `crate::state` (it must not appear in this
//! scanned module's signatures).

use crate::frb_generated::StreamSink;

/// One conversation row of the app's list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversationView {
    /// Room ID, e.g. `!jazz:example.org`.
    pub room_id: String,
    /// Display name from room state.
    pub display_name: String,
    /// The room's encryption state says encrypted.
    pub encrypted: bool,
    /// Sync provided enough state to decide encryption (when false the
    /// row shows an "unknown" glyph).
    pub encryption_known: bool,
    /// Sender of the newest message, if any.
    pub last_sender: Option<String>,
    /// Preview of the newest message, if any.
    pub last_preview: Option<String>,
    /// Timestamp (ms since epoch) of the newest message, if any.
    pub last_timestamp_ms: Option<u64>,
}

/// Snapshot of the cached conversation list (empty while signed out).
pub fn conversations() -> Vec<ConversationView> {
    crate::state::with_app(|app| app.conversations().to_vec())
}

/// The conversation list as a stream: the current list immediately, then
/// a new full list whenever a refresh changes it.
pub fn conversations_stream(sink: StreamSink<Vec<ConversationView>>) {
    let (mut rx, current) = crate::state::with_app(|app| {
        let rx = app.conversations_updates();
        let current = app.conversations().to_vec();
        (rx, current)
    });
    if sink.add(current).is_err() {
        return;
    }
    loop {
        match rx.blocking_recv() {
            Ok(list) => {
                if sink.add(list).is_err() {
                    break; // Dart side cancelled the subscription.
                }
            }
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
            Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
        }
    }
}

/// Refresh the list from the Matrix session (after each sync round).
/// Signed out, this is a no-op returning the cache.
pub async fn refresh_conversations() -> Result<Vec<ConversationView>, String> {
    let client = crate::state::with_app(|app| app.matrix_client());
    if let Some(client) = client {
        let list = client.conversations().await.map_err(|e| e.to_string())?;
        let views = crate::state::map_conversations(list);
        crate::state::with_app(|app| {
            app.set_conversations(views.clone());
        });
        Ok(views)
    } else {
        Ok(conversations())
    }
}

/// Select the conversation Files and Record operate on.
pub fn select_conversation(room_id: String) {
    crate::state::with_app(|app| app.select_conversation(room_id));
}

/// The selected conversation, if any.
pub fn current_conversation() -> Option<ConversationView> {
    crate::state::with_app(|app| app.current_conversation().cloned())
}

//! The Rust-side app state: settings, the (optional) Matrix session, the
//! current conversation, and the recorder. Screens reach it only through
//! the thin `api` functions; the recorder is owned here — not by any
//! screen — which is why capture survives navigation.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::api::conversations::ConversationView;
use crate::api::files::TreeNodeView;
use crate::api::recorder::{InterruptedSessionView, RecordingStateView, RecordingStatus, TakeView};
use crate::listing::folder_listing;
use crate::settings::Settings;
use zikos_audio::{RecorderConfig, RecorderSession, SampleSource, recover, recoverable_sessions};
use zikos_core::tree::ROOT_ID;
use zikos_matrix::Client;

/// Everything the bridge can fail with at the api boundary; stringified
/// for Dart there.
#[derive(Debug, thiserror::Error)]
pub enum BridgeError {
    /// The audio engine failed (open device, write, finalize…).
    #[error("{0}")]
    Audio(String),
    /// Settings failed validation or persistence.
    #[error("{0}")]
    Settings(#[from] crate::settings::SettingsError),
    /// A tree operation failed.
    #[error("{0}")]
    Tree(String),
    /// A recording is already active.
    #[error("a recording is already active")]
    AlreadyRecording,
    /// No recording is active.
    #[error("no recording is active")]
    NotRecording,
    /// An interrupted session must be finalized or discarded first.
    #[error("an interrupted session is waiting to be finalized or discarded")]
    InterruptedSessionPending,
    /// Files/Record need a selected conversation first.
    #[error("no conversation selected")]
    NoConversationSelected,
    /// A Matrix session is required.
    #[error("not signed in")]
    NotSignedIn,
    /// A node id was not a UUID.
    #[error("invalid node id {0:?}")]
    InvalidNodeId(String),
}

/// How the app talks to the microphone; injectable for tests.
pub type SourceFactory =
    Arc<dyn Fn() -> Result<Box<dyn SampleSource>, zikos_audio::AudioError> + Send + Sync>;

/// A live recording plus the frames written so far (elapsed time derives
/// from frames, so paused spans never count).
struct ActiveRecording {
    session: RecorderSession,
    frames_written: u64,
}

/// The whole application state behind the bridge.
pub struct AppState {
    app_dir: PathBuf,
    sessions_root: PathBuf,
    settings: Settings,
    /// Matrix client once a login flow exists; `None` today, which is
    /// what makes every screen render its signed-out state.
    session: Option<Arc<Client>>,
    current_room: Option<String>,
    conversations: Vec<ConversationView>,
    conversations_tx: broadcast::Sender<Vec<ConversationView>>,
    recorder: Option<ActiveRecording>,
    recording_tx: broadcast::Sender<RecordingStateView>,
    source_factory: SourceFactory,
}

fn cpal_source_factory() -> SourceFactory {
    // RecorderConfig policy: 48 kHz stereo default target.
    let requested = zikos_audio::SourceFormat {
        sample_rate_hz: 48_000,
        channels: 2,
    };
    Arc::new(move || {
        let source = zikos_audio::CpalSource::open(requested)?;
        Ok(Box::new(source) as Box<dyn SampleSource>)
    })
}

/// Map Matrix conversations to list rows, most recent activity first
/// (rooms without activity last, then by name).
pub fn map_conversations(list: Vec<zikos_matrix::Conversation>) -> Vec<ConversationView> {
    let mut views: Vec<ConversationView> = list
        .into_iter()
        .map(|c| ConversationView {
            room_id: c.room_id,
            display_name: c.display_name,
            encrypted: c.encryption == zikos_matrix::RoomEncryption::Encrypted,
            encryption_known: c.encryption != zikos_matrix::RoomEncryption::Unknown,
            last_sender: c.last_activity.as_ref().map(|a| a.sender.clone()),
            last_preview: c.last_activity.as_ref().map(|a| a.preview.clone()),
            last_timestamp_ms: c.last_activity.as_ref().map(|a| a.timestamp_ms),
        })
        .collect();
    views.sort_by(|a, b| {
        let ts_of = |v: &ConversationView| v.last_timestamp_ms.unwrap_or(0);
        ts_of(b)
            .cmp(&ts_of(a))
            .then_with(|| a.display_name.cmp(&b.display_name))
    });
    views
}

/// The process-wide app state behind the bridge.
static APP: std::sync::OnceLock<std::sync::Mutex<AppState>> = std::sync::OnceLock::new();
/// Guard so `global_init` starts exactly one pump thread.
static PUMP_STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Initialize (or return) the global app state rooted at `app_dir` and
/// ensure the capture pump thread is running.
pub fn global_init(app_dir: &str) -> crate::api::app::AppSnapshot {
    let app_dir = PathBuf::from(app_dir);
    let mutex = APP.get_or_init(|| match AppState::new(&app_dir) {
        Ok(state) => std::sync::Mutex::new(state),
        // init must not kill the app: fall back to a fresh state under a
        // temp dir; the error surfaces on the next settings operation.
        Err(_) => std::sync::Mutex::new(
            AppState::with_source_factory(
                &std::env::temp_dir().join(format!("zikos-fallback-{}", std::process::id())),
                cpal_source_factory(),
            )
            .expect("temp-dir app state is always constructible"),
        ),
    });
    if !PUMP_STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        std::thread::spawn(|| {
            loop {
                std::thread::sleep(std::time::Duration::from_millis(25));
                with_app(|app| app.pump_once());
            }
        });
    }
    let guard = mutex.lock().unwrap_or_else(|p| p.into_inner());
    // No login flow exists in this change, so the app is always signed
    // out; the login change will make this reflect a restored session.
    crate::api::app::AppSnapshot {
        session: crate::api::app::SessionView {
            signed_in: false,
            user_id: None,
        },
        settings: crate::api::settings::settings_view_of(guard.settings()),
    }
}

/// Run `f` against the global state. Locks briefly; never hold the guard
/// across an await.
pub fn with_app<T>(f: impl FnOnce(&mut AppState) -> T) -> T {
    let mutex = APP.get_or_init(|| {
        // with_app before init (unit tests use AppState directly; the api
        // must not) — construct under the OS temp dir.
        let dir = std::env::temp_dir().join("zikos-uninitialized");
        std::sync::Mutex::new(
            AppState::with_source_factory(&dir, cpal_source_factory())
                .expect("temp-dir app state is always constructible"),
        )
    });
    let mut guard = mutex.lock().unwrap_or_else(|p| p.into_inner());
    f(&mut guard)
}

impl AppState {
    /// State rooted at `app_dir` (settings live directly in it, recorder
    /// sessions under `sessions`), capturing from the default input.
    pub fn new(app_dir: &Path) -> Result<Self, BridgeError> {
        Self::with_source_factory(app_dir, cpal_source_factory())
    }

    /// Same, with an injectable microphone (tests feed deterministic
    /// fixtures).
    pub fn with_source_factory(
        app_dir: &Path,
        source_factory: SourceFactory,
    ) -> Result<Self, BridgeError> {
        let sessions_root = app_dir.join("sessions");
        std::fs::create_dir_all(&sessions_root).map_err(|e| BridgeError::Audio(e.to_string()))?;
        let settings = Settings::load(app_dir)?;
        let (recording_tx, _) = broadcast::channel(16);
        let (conversations_tx, _) = broadcast::channel(16);
        Ok(Self {
            app_dir: app_dir.to_path_buf(),
            sessions_root,
            settings,
            session: None,
            current_room: None,
            conversations: Vec::new(),
            conversations_tx,
            recorder: None,
            recording_tx,
            source_factory,
        })
    }

    /// The persisted settings.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Validate and persist new settings.
    pub fn save_settings(&mut self, settings: Settings) -> Result<(), BridgeError> {
        settings.save(&self.app_dir)?;
        self.settings = settings;
        Ok(())
    }

    /// The Matrix client, when signed in. Nothing can set it until the
    /// login change lands; the field is the seam that change grows into.
    pub fn matrix_client(&self) -> Option<Arc<Client>> {
        self.session.clone()
    }

    /// Cache a freshly listed conversation set (the api refreshes this
    /// after each sync round) and notify subscribers.
    pub fn set_conversations(&mut self, conversations: Vec<ConversationView>) {
        self.conversations_tx.send(conversations.clone()).ok();
        self.conversations = conversations;
    }

    /// Subscribe to conversation-list updates.
    pub fn conversations_updates(&self) -> broadcast::Receiver<Vec<ConversationView>> {
        self.conversations_tx.subscribe()
    }

    /// The cached conversation list, most recent activity first.
    pub fn conversations(&self) -> &[ConversationView] {
        &self.conversations
    }

    /// Select the conversation Files and Record operate on.
    pub fn select_conversation(&mut self, room_id: String) {
        self.current_room = Some(room_id);
    }

    /// The selected conversation, if any.
    pub fn current_conversation(&self) -> Option<&ConversationView> {
        let room = self.current_room.as_deref()?;
        self.conversations.iter().find(|c| c.room_id == room)
    }

    /// The tree listing of one folder of the current conversation
    /// (`None` = its root).
    pub fn files_listing(
        &self,
        folder_id: Option<String>,
    ) -> Result<Vec<TreeNodeView>, BridgeError> {
        let client = self.matrix_client().ok_or(BridgeError::NotSignedIn)?;
        let room = self
            .current_room
            .clone()
            .ok_or(BridgeError::NoConversationSelected)?;
        let folder = match folder_id {
            None => ROOT_ID,
            Some(id) => uuid::Uuid::parse_str(&id).map_err(|_| BridgeError::InvalidNodeId(id))?,
        };
        let room_tree = client.room_tree(&room);
        folder_listing(&room_tree.tree, folder, &room_tree.pending_nodes)
            .map_err(|e| BridgeError::Tree(e.to_string()))
    }

    /// The current recording state for the UI.
    pub fn recording_state(&self) -> RecordingStateView {
        match &self.recorder {
            None => RecordingStateView {
                status: RecordingStatus::Idle,
                elapsed_ms: 0,
            },
            Some(active) => {
                let rate = f64::from(active.session.manifest().sample_rate_hz);
                let status = if active.session.is_paused() {
                    RecordingStatus::Paused
                } else {
                    RecordingStatus::Recording
                };
                RecordingStateView {
                    status,
                    elapsed_ms: (active.frames_written as f64 / rate * 1000.0) as u32,
                }
            }
        }
    }

    /// Subscribe to recording-state updates.
    pub fn recording_updates(&self) -> broadcast::Receiver<RecordingStateView> {
        self.recording_tx.subscribe()
    }

    fn broadcast_recording_state(&self) {
        let _ = self.recording_tx.send(self.recording_state());
    }

    /// Start a take. Fails when one is active or an interrupted session
    /// is still waiting for a finalize-or-discard decision.
    pub fn record_start(&mut self) -> Result<(), BridgeError> {
        if self.recorder.is_some() {
            return Err(BridgeError::AlreadyRecording);
        }
        if self.interrupted_session()?.is_some() {
            return Err(BridgeError::InterruptedSessionPending);
        }
        let source = (self.source_factory)().map_err(|e| BridgeError::Audio(e.to_string()))?;
        let started_at = jiff::Zoned::now().timestamp().as_second().max(0) as u64;
        let config = RecorderConfig {
            format: self.settings.default_format,
            ..RecorderConfig::default()
        };
        let session = RecorderSession::start(&config, source, &self.sessions_root, started_at)
            .map_err(|e| BridgeError::Audio(e.to_string()))?;
        self.recorder = Some(ActiveRecording {
            session,
            frames_written: 0,
        });
        self.broadcast_recording_state();
        Ok(())
    }

    /// One pump round: drain the source into the WAV and publish the new
    /// elapsed time. Called from the bridge's worker thread while a
    /// session is active.
    pub fn pump_once(&mut self) {
        let Some(active) = self.recorder.as_mut() else {
            return;
        };
        match active.session.pull() {
            Ok(frames) => {
                active.frames_written += frames;
                self.broadcast_recording_state();
            }
            Err(_) => {
                // I/O died mid-take: drop the session without touching the
                // manifest so interrupted-session recovery offers it next
                // time.
                self.recorder = None;
                self.broadcast_recording_state();
            }
        }
    }

    /// Pause the active take.
    pub fn record_pause(&mut self) -> Result<(), BridgeError> {
        let active = self.recorder.as_mut().ok_or(BridgeError::NotRecording)?;
        active
            .session
            .pause()
            .map_err(|e| BridgeError::Audio(e.to_string()))?;
        self.broadcast_recording_state();
        Ok(())
    }

    /// Resume the paused take.
    pub fn record_resume(&mut self) -> Result<(), BridgeError> {
        let active = self.recorder.as_mut().ok_or(BridgeError::NotRecording)?;
        active
            .session
            .resume()
            .map_err(|e| BridgeError::Audio(e.to_string()))?;
        self.broadcast_recording_state();
        Ok(())
    }

    /// Stop and finalize the active take in the configured format.
    pub fn record_stop(&mut self) -> Result<TakeView, BridgeError> {
        let active = self.recorder.take().ok_or(BridgeError::NotRecording)?;
        let staged = active
            .session
            .stop()
            .map_err(|e| BridgeError::Audio(e.to_string()))?;
        let take = self.finalize_staged(staged)?;
        self.broadcast_recording_state();
        Ok(take)
    }

    fn finalize_staged(&self, staged: zikos_audio::StagedSession) -> Result<TakeView, BridgeError> {
        let tz = jiff::tz::TimeZone::system();
        let finalized = staged
            .finalize_as(&tz, self.settings.default_format)
            .map_err(|e| BridgeError::Audio(e.to_string()))?;
        Ok(TakeView {
            title: finalized.meta().title.clone(),
            format: crate::listing::format_label(finalized.meta().format),
            duration_secs: finalized.meta().duration_secs,
            audio_path: finalized.audio_path().display().to_string(),
        })
    }

    /// The interrupted session awaiting a decision, if one exists.
    pub fn interrupted_session(&self) -> Result<Option<InterruptedSessionView>, BridgeError> {
        let Some(dir) = self.interrupted_dir()? else {
            return Ok(None);
        };
        let manifest = zikos_audio::SessionManifest::load(&dir)
            .map_err(|e| BridgeError::Audio(e.to_string()))?;
        Ok(Some(InterruptedSessionView {
            started_at: manifest.started_at,
        }))
    }

    /// The directory of the recoverable session, if any.
    fn interrupted_dir(&self) -> Result<Option<PathBuf>, BridgeError> {
        let dirs = recoverable_sessions(&self.sessions_root)
            .map_err(|e| BridgeError::Audio(e.to_string()))?;
        Ok(dirs.into_iter().next())
    }

    /// Recover and finalize the interrupted session.
    pub fn interrupted_finalize(&mut self) -> Result<TakeView, BridgeError> {
        let dir = self.interrupted_dir()?.ok_or(BridgeError::NotRecording)?;
        let staged = recover(&dir).map_err(|e| BridgeError::Audio(e.to_string()))?;
        self.finalize_staged(staged)
    }

    /// Recover and discard the interrupted session.
    pub fn interrupted_discard(&mut self) -> Result<(), BridgeError> {
        let dir = self.interrupted_dir()?.ok_or(BridgeError::NotRecording)?;
        let staged = recover(&dir).map_err(|e| BridgeError::Audio(e.to_string()))?;
        staged
            .discard()
            .map_err(|e| BridgeError::Audio(e.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::SettingsError;
    use std::collections::VecDeque;
    use std::sync::Mutex;
    use zikos_audio::SourceFormat;

    /// Queue-backed microphone fixture shared with the test (see the
    /// equivalent fixture in zikos-audio's session tests).
    #[derive(Clone, Default)]
    struct SharedQueue(Arc<Mutex<VecDeque<Vec<i16>>>>);

    impl SharedQueue {
        fn push_frames(&self, format: SourceFormat, frames: u64) {
            let channels = u64::from(format.channels);
            self.0
                .lock()
                .unwrap()
                .push_back(vec![0i16; (frames * channels) as usize]);
        }
    }

    struct QueueSource {
        format: SourceFormat,
        queue: SharedQueue,
    }

    impl SampleSource for QueueSource {
        fn format(&self) -> SourceFormat {
            self.format
        }

        fn next_chunk(&mut self) -> Option<Vec<i16>> {
            self.queue.0.lock().unwrap().pop_front()
        }
    }

    fn fixture_state() -> (AppState, SharedQueue) {
        let dir = tempfile::tempdir().unwrap();
        let (queue, state) = {
            let queue = SharedQueue::default();
            let factory: SourceFactory = {
                let queue = queue.clone();
                Arc::new(move || {
                    let format = SourceFormat {
                        sample_rate_hz: 48_000,
                        channels: 2,
                    };
                    Ok(Box::new(QueueSource {
                        format,
                        queue: queue.clone(),
                    }) as Box<dyn SampleSource>)
                })
            };
            let state = AppState::with_source_factory(dir.path(), factory).unwrap();
            (queue, state)
        };
        // Leak the tempdir for the lifetime of the test run: sessions live
        // under it and AppState does not own the TempDir.
        std::mem::forget(dir);
        (state, queue)
    }

    fn conversation(room_id: &str, name: &str, ts: u64) -> ConversationView {
        ConversationView {
            room_id: room_id.into(),
            display_name: name.into(),
            encrypted: true,
            encryption_known: true,
            last_sender: Some("@bob:example.org".into()),
            last_preview: Some("hi".into()),
            last_timestamp_ms: Some(ts),
        }
    }

    #[test]
    fn settings_round_trip_through_state() {
        let (mut state, _queue) = fixture_state();
        assert_eq!(state.settings(), &Settings::default());
        state
            .save_settings(Settings {
                homeserver_url: "https://matrix.example.org".into(),
                blob_server_url: String::new(),
                default_format: zikos_core::AudioFormat::Wav,
            })
            .unwrap();
        assert_eq!(
            state.settings().default_format,
            zikos_core::AudioFormat::Wav
        );
        assert!(matches!(
            state.save_settings(Settings {
                homeserver_url: "nope".into(),
                ..Settings::default()
            }),
            Err(BridgeError::Settings(SettingsError::InvalidUrl { .. }))
        ));
    }

    #[test]
    fn recorder_state_machine_from_start_to_finalized_take() {
        let (mut state, queue) = fixture_state();
        let format = SourceFormat {
            sample_rate_hz: 48_000,
            channels: 2,
        };

        assert_eq!(state.recording_state().status, RecordingStatus::Idle);
        queue.push_frames(format, 48_000); // 1 s of audio waiting
        state.record_start().unwrap();
        assert_eq!(state.recording_state().status, RecordingStatus::Recording);
        state.pump_once();
        assert_eq!(state.recording_state().elapsed_ms, 1000);

        // Pause freezes the clock even while audio keeps arriving.
        state.record_pause().unwrap();
        assert_eq!(state.recording_state().status, RecordingStatus::Paused);
        queue.push_frames(format, 48_000);
        state.pump_once();
        assert_eq!(
            state.recording_state().elapsed_ms,
            1000,
            "paused span must not count"
        );

        // Double commands are invalid-state errors surfaced as Audio.
        assert!(state.record_pause().is_err());

        state.record_resume().unwrap();
        queue.push_frames(format, 96_000); // 2 s more
        state.pump_once();
        assert_eq!(state.recording_state().elapsed_ms, 3000);

        // Stop finalizes with the default (FLAC) format.
        let take = state.record_stop().unwrap();
        assert_eq!(take.format, "flac");
        assert!((take.duration_secs - 3.0).abs() < 1e-3);
        assert!(take.audio_path.ends_with("take.flac"));
        assert_eq!(state.recording_state().status, RecordingStatus::Idle);
    }

    #[test]
    fn record_commands_without_an_active_take_fail() {
        let (mut state, _queue) = fixture_state();
        assert!(matches!(
            state.record_pause(),
            Err(BridgeError::NotRecording)
        ));
        assert!(matches!(
            state.record_stop(),
            Err(BridgeError::NotRecording)
        ));
    }

    #[test]
    fn interrupted_session_offers_finalize_or_discard() {
        let (mut state, queue) = fixture_state();
        let format = SourceFormat {
            sample_rate_hz: 48_000,
            channels: 2,
        };
        // Simulate a crash: start a take, pump audio, then drop the
        // session without stopping (the manifest stays in-progress).
        queue.push_frames(format, 48_000);
        state.record_start().unwrap();
        state.pump_once();
        std::mem::drop(state.recorder.take());

        let interrupted = state.interrupted_session().unwrap().unwrap();
        assert!(interrupted.started_at > 0);
        // A new take cannot start until the decision is made.
        assert!(matches!(
            state.record_start(),
            Err(BridgeError::InterruptedSessionPending)
        ));

        let take = state.interrupted_finalize().unwrap();
        assert!((take.duration_secs - 1.0).abs() < 1e-3);
        assert!(state.interrupted_session().unwrap().is_none());
    }

    #[test]
    fn maps_conversations_most_recent_first() {
        use zikos_matrix::{Conversation, LastActivity, RoomEncryption};

        let conv = |room: &str, name: &str, ts: Option<u64>, enc: RoomEncryption| Conversation {
            room_id: room.into(),
            display_name: name.into(),
            encryption: enc,
            last_activity: ts.map(|t| LastActivity {
                sender: "@bob:example.org".into(),
                preview: "hi".into(),
                timestamp_ms: t,
            }),
        };
        let views = map_conversations(vec![
            conv(
                "!old:example.org",
                "Old",
                Some(100),
                RoomEncryption::Encrypted,
            ),
            conv(
                "!quiet:example.org",
                "Quiet",
                None,
                RoomEncryption::Encrypted,
            ),
            conv(
                "!new:example.org",
                "New",
                Some(900),
                RoomEncryption::Encrypted,
            ),
            conv(
                "!unknown:example.org",
                "Unknown",
                None,
                RoomEncryption::Unknown,
            ),
        ]);
        let ids: Vec<&str> = views.iter().map(|v| v.room_id.as_str()).collect();
        assert_eq!(
            ids,
            vec![
                "!new:example.org",
                "!old:example.org",
                "!quiet:example.org",
                "!unknown:example.org"
            ]
        );
        assert_eq!(views[0].last_preview.as_deref(), Some("hi"));
        assert!(views[3].last_timestamp_ms.is_none());
        assert!(views[0].encrypted);
        assert!(!views[3].encrypted && !views[3].encryption_known);
    }

    #[test]
    fn set_conversations_notifies_subscribers() {
        let (mut state, _queue) = fixture_state();
        let mut updates = state.conversations_updates();
        state.set_conversations(vec![conversation("!a:example.org", "Jazz Quartet", 200)]);
        let received = updates.blocking_recv().unwrap();
        assert_eq!(received.len(), 1);
        assert_eq!(received[0].display_name, "Jazz Quartet");
        assert_eq!(state.conversations().len(), 1);
    }

    #[test]
    fn files_and_conversation_selection_rules() {
        let (mut state, _queue) = fixture_state();
        // Signed out: conversations empty, files listing refused.
        assert!(state.conversations().is_empty());
        assert!(matches!(
            state.files_listing(None),
            Err(BridgeError::NotSignedIn)
        ));

        state.set_conversations(vec![
            conversation("!b:example.org", "Sunday Rehearsal", 100),
            conversation("!a:example.org", "Jazz Quartet", 200),
        ]);
        assert!(state.current_conversation().is_none());
        state.select_conversation("!a:example.org".into());
        assert_eq!(
            state.current_conversation().unwrap().display_name,
            "Jazz Quartet"
        );
        // Still signed out (no matrix session): listing is refused even
        // with a selected conversation.
        assert!(matches!(
            state.files_listing(None),
            Err(BridgeError::NotSignedIn)
        ));
    }
}

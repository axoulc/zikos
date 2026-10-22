//! The wrapped `matrix-sdk` client: construction, login, and session
//! lifetime. The SDK client never appears in this crate's public API.

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use matrix_sdk::send_queue::RoomSendQueueUpdate;
use tokio::sync::broadcast;

use crate::delivery::{DeliveryEvent, SendPath};
use crate::error::MatrixError;
use crate::message::{self, MessageEvent};
use crate::message_log::MessageLog;
use crate::session_store;

/// A logged-in Matrix session for one Zikos account.
///
/// The underlying `matrix-sdk` client holds the encrypted sqlite state
/// store; drop this value to close it.
pub struct Client {
    inner: matrix_sdk::Client,
    message_tx: broadcast::Sender<MessageEvent>,
    log: Arc<Mutex<MessageLog>>,
    delivery: Arc<SendPath>,
    /// Incremental-sync cursor. The SDK's own token isn't publicly
    /// readable, so we track it ourselves (plaintext: an opaque cursor,
    /// not a credential) and persist it for restart-continuation.
    sync_token: Mutex<Option<String>>,
    sync_token_path: std::path::PathBuf,
}

const SYNC_TOKEN_FILE: &str = "zikos-sync-token";

/// Encryption state of a room, mirrored from the SDK without leaking its
/// types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomEncryption {
    /// `m.room.encryption` is in the room state.
    Encrypted,
    /// The room state says the room is not encrypted.
    Unencrypted,
    /// Sync has not provided enough state to decide.
    Unknown,
}

fn read_sync_token(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join(SYNC_TOKEN_FILE))
        .ok()
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty())
}

/// Wire the send path: global watcher + delivery states for local echoes
/// already persisted in the store. Instantiating each room's send queue
/// also resumes transmitting whatever the previous session left queued;
/// echoes wedged by a past unrecoverable error are explicitly unwedged —
/// connectivity may be back.
fn attach_send_path(inner: &matrix_sdk::Client) -> Arc<SendPath> {
    let path = Arc::new(SendPath::new());
    path.watch(inner);
    for room in inner.rooms() {
        let queue = room.send_queue();
        let path = path.clone();
        tokio::spawn(async move {
            let Ok((echoes, _)) = queue.subscribe().await else {
                return;
            };
            let mut restore = Vec::new();
            for echo in &echoes {
                if let matrix_sdk::send_queue::LocalEchoContent::Event {
                    send_handle,
                    send_error,
                    ..
                } = &echo.content
                {
                    restore.push((echo.transaction_id.to_string(), send_error.is_some()));
                    if send_error.is_some() {
                        let _ = send_handle.unwedge().await;
                    }
                }
            }
            path.restore_echoes(restore);
        });
    }
    path
}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Client")
            .field("user_id", &self.user_id())
            .finish()
    }
}

impl Client {
    /// Log in to `homeserver` with username and password, storing session
    /// and room state encrypted (key derived from `passphrase`) under
    /// `store_path`.
    ///
    /// Fails with [`MatrixError::InvalidCredentials`] on `M_FORBIDDEN`
    /// without persisting anything; [`MatrixError::Connection`] when the
    /// homeserver is unreachable.
    pub async fn login(
        homeserver: &str,
        username: &str,
        password: &str,
        store_path: &Path,
        passphrase: &str,
    ) -> Result<Self, MatrixError> {
        let inner = matrix_sdk::Client::builder()
            .homeserver_url(homeserver)
            .sqlite_store(store_path, Some(passphrase))
            .build()
            .await
            .map_err(MatrixError::from)?;

        inner
            .matrix_auth()
            .login_username(username, password)
            .initial_device_display_name("Zikos")
            .await
            .map_err(MatrixError::from)?;

        // The SDK does not reload the token session from the store on
        // restart, so persist it ourselves (encrypted with `passphrase`).
        let session = inner.matrix_auth().session().ok_or_else(|| {
            MatrixError::Store("login succeeded but no session was returned".into())
        })?;
        session_store::save(store_path, passphrase, &session)?;

        let log = Arc::new(Mutex::new(MessageLog::open(store_path, passphrase)?));
        let message_tx = message::attach(&inner, log.clone(), Arc::from(passphrase));
        let delivery = attach_send_path(&inner);
        Ok(Self {
            inner,
            message_tx,
            log,
            delivery,
            sync_token: Mutex::new(read_sync_token(store_path)),
            sync_token_path: store_path.join(SYNC_TOKEN_FILE),
        })
    }

    /// Reopen a previously logged-in session from `store_path` (created by
    /// [`Client::login`], same `passphrase`), without credentials.
    ///
    /// Fails with [`MatrixError::Store`] when no session was persisted or
    /// the passphrase is wrong.
    pub async fn restore(
        homeserver: &str,
        store_path: &Path,
        passphrase: &str,
    ) -> Result<Self, MatrixError> {
        let session = session_store::load(store_path, passphrase)?;
        let inner = matrix_sdk::Client::builder()
            .homeserver_url(homeserver)
            .sqlite_store(store_path, Some(passphrase))
            .build()
            .await
            .map_err(MatrixError::from)?;
        inner
            .restore_session(session)
            .await
            .map_err(MatrixError::from)?;
        let log = Arc::new(Mutex::new(MessageLog::open(store_path, passphrase)?));
        let message_tx = message::attach(&inner, log.clone(), Arc::from(passphrase));
        let delivery = attach_send_path(&inner);
        Ok(Self {
            inner,
            message_tx,
            log,
            delivery,
            sync_token: Mutex::new(read_sync_token(store_path)),
            sync_token_path: store_path.join(SYNC_TOKEN_FILE),
        })
    }

    /// The logged-in user's Matrix ID, e.g. `@alice:example.org`, or
    /// `None` before login.
    pub fn user_id(&self) -> Option<String> {
        self.inner.user_id().map(|id| id.to_string())
    }

    /// Subscribe to messages arriving through sync. Every receiver keeps
    /// its own backlog; late subscribers see only future messages.
    pub fn messages(&self) -> broadcast::Receiver<MessageEvent> {
        self.message_tx.subscribe()
    }

    /// Subscribe to delivery-state changes of outgoing messages
    /// (`sending` → `sent`/`failed`, retries, restart recovery).
    pub fn deliveries(&self) -> broadcast::Receiver<DeliveryEvent> {
        self.delivery.subscribe()
    }

    /// Queue a text message for `room_id`. Returns immediately with the
    /// send-queue transaction ID and the message already marked `sending`
    /// (see [`DeliveryEvent`]); transmission — and retry after failure,
    /// including across an app restart — is handled by the SDK's durable
    /// send queue.
    pub async fn send_text(&self, room_id: &str, body: &str) -> Result<String, MatrixError> {
        use matrix_sdk::ruma::events::AnyMessageLikeEventContent;
        use matrix_sdk::ruma::events::room::message::RoomMessageEventContent;

        let owned_room_id: matrix_sdk::ruma::OwnedRoomId = room_id
            .try_into()
            .map_err(|e| MatrixError::Homeserver(format!("invalid room ID: {e}")))?;
        let room = self
            .inner
            .get_room(&owned_room_id)
            .ok_or_else(|| MatrixError::UnknownRoom(room_id.to_owned()))?;
        if !matches!(room.state(), matrix_sdk::RoomState::Joined) {
            return Err(MatrixError::Homeserver(format!(
                "room {room_id} is not joined"
            )));
        }

        // Serialize per room so the echo of OUR enqueue is identifiable.
        let room_send_lock = self.delivery.room_lock(room_id);
        let guard = room_send_lock.lock().await;
        let queue = room.send_queue();
        let (echoes, mut updates) = queue
            .subscribe()
            .await
            .map_err(|e| MatrixError::Store(e.to_string()))?;
        let known_before = SendPath::known_transaction_ids(&echoes);

        // A previous failure run may have disabled the whole queue.
        self.inner.send_queue().set_enabled(true).await;

        queue
            .send(AnyMessageLikeEventContent::RoomMessage(
                RoomMessageEventContent::text_plain(body),
            ))
            .await
            .map_err(|e| MatrixError::Homeserver(e.to_string()))?;

        let transaction_id = loop {
            let update = tokio::time::timeout(Duration::from_secs(5), updates.recv())
                .await
                .map_err(|_| MatrixError::Store("send queue did not echo the enqueue".into()))?
                .map_err(|_| MatrixError::Store("send queue update channel closed".into()))?;
            if let RoomSendQueueUpdate::NewLocalEvent(echo) = update {
                let txn = echo.transaction_id.to_string();
                if !known_before.contains(&txn) {
                    break txn;
                }
            }
            // Everything else is handled by the global watcher.
        };
        drop(guard);
        Ok(transaction_id)
    }

    /// The accumulated history of one room (oldest first): everything
    /// seen through sync on this store, restored after restart.
    pub fn room_messages(&self, room_id: &str) -> Vec<MessageEvent> {
        self.log
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .messages(room_id)
    }

    /// Create a new conversation: a private room with encryption enabled
    /// in the creation request itself, so the room is encrypted before
    /// the first message is sent. There is deliberately no way to create
    /// an unencrypted room through this API.
    ///
    /// Returns the new room ID.
    pub async fn create_encrypted_room(&self, name: &str) -> Result<String, MatrixError> {
        use matrix_sdk::ruma::api::client::room::create_room::v3::Request as CreateRoomRequest;
        use matrix_sdk::ruma::api::client::room::create_room::v3::RoomPreset;
        use matrix_sdk::ruma::events::InitialStateEvent;
        use matrix_sdk::ruma::events::room::encryption::RoomEncryptionEventContent;

        let mut request = CreateRoomRequest::new();
        request.name = Some(name.to_owned());
        request.preset = Some(RoomPreset::PrivateChat);
        request.initial_state = vec![
            InitialStateEvent::new(RoomEncryptionEventContent::with_recommended_defaults())
                .to_raw_any(),
        ];

        let room = self
            .inner
            .create_room(request)
            .await
            .map_err(MatrixError::from)?;
        Ok(room.room_id().to_string())
    }

    /// Run one `/sync` round, continuing incrementally from the stored
    /// cursor. First call after login does an initial sync; after a
    /// restore it picks up where the previous session stopped.
    pub async fn sync_once(&self) -> Result<(), MatrixError> {
        let mut settings = matrix_sdk::config::SyncSettings::default();
        if let Some(token) = self
            .sync_token
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        {
            settings = settings.token(token);
        }
        let response = self
            .inner
            .sync_once(settings)
            .await
            .map_err(MatrixError::from)?;

        let next = response.next_batch;
        *self.sync_token.lock().unwrap_or_else(|p| p.into_inner()) = Some(next.clone());
        let _ = std::fs::write(&self.sync_token_path, &next); // best-effort cursor persist

        // A successful sync round is our "connectivity is back" signal:
        // re-enable the send queue so offline-queued messages drain.
        self.inner.send_queue().set_enabled(true).await;
        Ok(())
    }

    /// The encryption state of one joined room, so callers can warn
    /// visibly when a room is not encrypted. Resolves `Unknown` by asking
    /// the homeserver for the `m.room.encryption` state event; `Unknown`
    /// can still surface if that lookup is inconclusive.
    pub async fn room_encryption(&self, room_id: &str) -> Result<RoomEncryption, MatrixError> {
        let room_id: matrix_sdk::ruma::OwnedRoomId = room_id
            .try_into()
            .map_err(|e| MatrixError::Homeserver(format!("invalid room ID: {e}")))?;
        let room = self
            .inner
            .get_room(&room_id)
            .ok_or_else(|| MatrixError::UnknownRoom(room_id.to_string()))?;
        match room.latest_encryption_state().await? {
            matrix_sdk::EncryptionState::Encrypted => Ok(RoomEncryption::Encrypted),
            matrix_sdk::EncryptionState::NotEncrypted => Ok(RoomEncryption::Unencrypted),
            matrix_sdk::EncryptionState::Unknown => Ok(RoomEncryption::Unknown),
        }
    }
}

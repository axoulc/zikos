//! The message model exposed to embedders, and the sync-event handler
//! that feeds it.

use std::sync::{Arc, Mutex};

use matrix_sdk::Room;
use matrix_sdk::ruma::events::AnySyncMessageLikeEvent;
use matrix_sdk::ruma::events::room::encrypted::SyncRoomEncryptedEvent;
use matrix_sdk::ruma::events::room::message::{MessageType, SyncRoomMessageEvent};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::message_log::MessageLog;

/// The body of a received message.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageBody {
    /// A decrypted (or plain) text message.
    Text(String),
    /// The event could not be decrypted (e.g. missing room key). The
    /// message is surfaced with this marker rather than dropped.
    Undecryptable,
}

/// A message received through sync.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct MessageEvent {
    /// Room the message belongs to, e.g. `!room:example.org`.
    pub room_id: String,
    /// Server-assigned event ID.
    pub event_id: String,
    /// Full Matrix ID of the sender.
    pub sender: String,
    /// Origin server timestamp, milliseconds since the Unix epoch.
    pub timestamp: u64,
    /// Decoded body.
    pub body: MessageBody,
}

impl MessageEvent {
    /// Map a synced message-like event to our model. Returns `None` for
    /// events that are not conversational text in phase 1 (reactions,
    /// redactions, edits, non-text msgtypes, …).
    pub(crate) fn from_sync(room: &Room, event: &AnySyncMessageLikeEvent) -> Option<Self> {
        let (event_id, sender, timestamp, body) = match event {
            AnySyncMessageLikeEvent::RoomMessage(SyncRoomMessageEvent::Original(m)) => {
                let MessageType::Text(text) = &m.content.msgtype else {
                    return None; // phase 1 surfaces text messages only
                };
                (
                    m.event_id.to_string(),
                    m.sender.to_string(),
                    m.origin_server_ts.0.into(),
                    MessageBody::Text(text.body.clone()),
                )
            }
            // If decryption had succeeded, we would see RoomMessage here;
            // an encrypted event reaching us still encrypted failed to
            // decrypt.
            AnySyncMessageLikeEvent::RoomEncrypted(SyncRoomEncryptedEvent::Original(m)) => (
                m.event_id.to_string(),
                m.sender.to_string(),
                m.origin_server_ts.0.into(),
                MessageBody::Undecryptable,
            ),
            _ => return None,
        };
        Some(Self {
            room_id: room.room_id().to_string(),
            event_id,
            sender,
            timestamp,
            body,
        })
    }
}

/// Create the channel embedders subscribe to and register the sync
/// handler feeding it and the persisted history log. Called once per
/// client construction.
pub(crate) fn attach(
    client: &matrix_sdk::Client,
    log: Arc<Mutex<MessageLog>>,
    passphrase: Arc<str>,
) -> broadcast::Sender<MessageEvent> {
    let (tx, _) = broadcast::channel(128);
    let handler_tx = tx.clone();
    client.add_event_handler(move |event: AnySyncMessageLikeEvent, room: Room| {
        let handler_tx = handler_tx.clone();
        let log = log.clone();
        let passphrase = passphrase.clone();
        async move {
            if let Some(message) = MessageEvent::from_sync(&room, &event) {
                let mut log = log.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                log.record(message.clone(), &passphrase);
                // A closed receiver just means nobody is listening; the
                // message is still in the persisted log.
                let _ = handler_tx.send(message);
            }
        }
    });
    tx
}

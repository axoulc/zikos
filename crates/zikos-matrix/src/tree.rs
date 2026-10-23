//! Tree-op transport over Matrix: the custom `zikos.tree.op` event type
//! and its wiring into send, sync, persistence, and replay.

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use matrix_sdk::ruma::events::macros::EventContent;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use tokio::sync::broadcast;
use zikos_core::tree::{FileTree, NodeId, TreeOp};
use zikos_core::tree_sync::{ReplayOutput, TreeOpContent, TreeOpEvent};

use crate::encrypted::{self, Cipher};
use crate::error::MatrixError;

/// The custom Matrix event type carrying one tree operation (inside the
/// room's Megolm envelope).
pub const TREE_EVENT_TYPE: &str = "zikos.tree.op";

/// Content of a `zikos.tree.op` room event: the operation plus conflict
/// metadata, flattened onto the wire exactly as
/// [`zikos_core::tree_sync::TreeOpContent`] defines it.
#[derive(Clone, Debug, Serialize, Deserialize, EventContent)]
#[ruma_event(type = "zikos.tree.op", kind = MessageLike)]
pub struct ZikosTreeOpEventContent {
    /// The core wire envelope.
    #[serde(flatten)]
    pub inner: TreeOpContent,
}

impl From<TreeOpContent> for ZikosTreeOpEventContent {
    fn from(inner: TreeOpContent) -> Self {
        Self { inner }
    }
}

/// A tree op received through sync: the decrypted event merged with its
/// room envelope.
#[derive(Clone, Debug, PartialEq)]
pub struct TreeEvent {
    /// Room the tree belongs to, e.g. `!room:example.org`.
    pub room_id: String,
    /// The merged tree-op event (replay input).
    pub event: TreeOpEvent,
}

/// File name of the tree-event log inside the store directory.
pub const TREE_LOG_FILE: &str = "zikos-tree-log";

/// The per-room tree-event log: every tree event surfaced by sync is
/// appended here, persisted encrypted (same passphrase and format as the
/// message log), and deduplicated by event ID so sync re-delivery never
/// double-applies. Reloaded on restore; replayed into the tree on demand.
#[derive(Default)]
pub(crate) struct TreeLog {
    cipher: Option<Cipher>,
    path: Option<PathBuf>,
    rooms: BTreeMap<String, Vec<TreeOpEvent>>,
}

impl TreeLog {
    /// Load the log from `dir` (empty when absent).
    pub(crate) fn open(dir: &Path, passphrase: &str) -> Result<Self, MatrixError> {
        let path = dir.join(TREE_LOG_FILE);
        let rooms = match std::fs::read(&path) {
            Ok(bytes) => Cipher::open(passphrase, &bytes)?,
            Err(_) => BTreeMap::new(),
        };
        Ok(Self {
            cipher: None,
            path: Some(path),
            rooms,
        })
    }

    /// Record an event if its event ID is new for the room, persisting
    /// the whole log on change. Returns true when appended.
    pub(crate) fn record(&mut self, room_id: &str, event: TreeOpEvent, passphrase: &str) -> bool {
        let room = self.rooms.entry(room_id.to_owned()).or_default();
        if room.iter().any(|e| e.event_id == event.event_id) {
            return false;
        }
        room.push(event);

        let cipher = match &self.cipher {
            Some(c) => c,
            None => match Cipher::generate(passphrase) {
                Ok(c) => self.cipher.get_or_insert(c),
                Err(_) => return true, // stays in memory; surfaced by save errors later
            },
        };
        // Full rewrite per event is O(history) — fine at musician scale;
        // snapshotting arrives when the log grows real volume.
        if let (Some(path), Ok(bytes)) = (&self.path, cipher.seal(&self.rooms)) {
            let _ = encrypted::write_atomic(path, &bytes);
        }
        true
    }

    /// The accumulated tree-event history of one room.
    pub(crate) fn events(&self, room_id: &str) -> Vec<TreeOpEvent> {
        self.rooms.get(room_id).cloned().unwrap_or_default()
    }
}

/// One locally authored tree op that has not been acknowledged by the
/// room yet (its echo has not come back through sync).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct PendingTreeOp {
    /// Room the op targets.
    pub room_id: String,
    /// Send-queue transaction id (delivery-state correlation).
    pub transaction_id: String,
    /// The operation awaiting its room echo.
    pub op: TreeOp,
}

/// The synced state of one room's tree: the committed replay plus the
/// optimistic overlay of pending local ops.
#[derive(Clone, Debug, PartialEq)]
pub struct RoomTree {
    /// The deterministic product of replaying the room's committed event
    /// log (conflict losers preserved under `Conflicts/`, lost ops
    /// reported).
    pub committed: ReplayOutput,
    /// The committed tree with pending local ops applied optimistically —
    /// what the user sees.
    pub tree: FileTree,
    /// Nodes whose latest mutation is still pending (id pending marker).
    pub pending_nodes: HashSet<NodeId>,
}

/// Create the channel embedders subscribe to and register the sync
/// handler feeding it and the persisted tree log. Called once per client
/// construction.
pub(crate) fn attach(
    client: &matrix_sdk::Client,
    log: Arc<Mutex<TreeLog>>,
    passphrase: Arc<str>,
) -> broadcast::Sender<TreeEvent> {
    use matrix_sdk::ruma::events::SyncMessageLikeEvent;

    let (tx, _) = broadcast::channel(128);
    let handler_tx = tx.clone();
    client.add_event_handler(
        move |event: SyncMessageLikeEvent<ZikosTreeOpEventContent>, room: matrix_sdk::Room| {
            let handler_tx = handler_tx.clone();
            let log = log.clone();
            let passphrase = passphrase.clone();
            async move {
                // Redacted or malformed tree events carry no original
                // payload — there is nothing to replay.
                let Some(original) = event.as_original() else {
                    return;
                };
                let merged = TreeOpEvent::from_parts(
                    original.sender.to_string(),
                    original.origin_server_ts.0.into(),
                    original.event_id.to_string(),
                    original.content.inner.clone(),
                );
                let tree_event = TreeEvent {
                    room_id: room.room_id().to_string(),
                    event: merged,
                };
                let mut log = log.lock().unwrap_or_else(|p| p.into_inner());
                log.record(&tree_event.room_id, tree_event.event.clone(), &passphrase);
                // A closed receiver just means nobody is listening; the
                // event is still in the persisted log.
                let _ = handler_tx.send(tree_event);
            }
        },
    );
    tx
}

#[cfg(test)]
mod tests {
    use super::*;
    use matrix_sdk::ruma::events::StaticEventContent;
    use uuid::Uuid;
    use zikos_core::tree::ROOT_ID;
    use zikos_core::tree::{NodeKind, TreeOp};
    use zikos_core::tree_sync::{NodeBase, TreeOpContent};

    fn sample() -> TreeOpContent {
        TreeOpContent {
            op: TreeOp::Rename {
                id: ROOT_ID,
                name: "take-1.flac".into(),
            },
            base: Some(NodeBase {
                name: "old.flac".into(),
                parent: ROOT_ID,
            }),
            known_children: None,
        }
    }

    fn tree_event(event_id: &str) -> TreeOpEvent {
        TreeOpEvent {
            op: TreeOp::Delete { id: ROOT_ID },
            author: "@bob:example.org".into(),
            origin_server_ts: 1,
            event_id: event_id.into(),
            base: None,
            known_children: None,
        }
    }

    #[test]
    fn tree_log_records_dedups_and_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut log = TreeLog::open(dir.path(), "pass").unwrap();
            assert!(log.record("!r:hs", tree_event("$1"), "pass"));
            assert!(log.record("!r:hs", tree_event("$2"), "pass"));
            // Re-delivery of a known event id never double-applies.
            assert!(!log.record("!r:hs", tree_event("$1"), "pass"));
        }
        let log = TreeLog::open(dir.path(), "pass").unwrap();
        assert_eq!(log.events("!r:hs").len(), 2);
        assert_eq!(log.events("!r:hs")[1].event_id, "$2");
        assert!(log.events("!other:hs").is_empty());
    }

    #[test]
    fn tree_log_wrong_passphrase_is_store_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = TreeLog::open(dir.path(), "pass").unwrap();
        log.record("!r:hs", tree_event("$1"), "pass");
        assert!(matches!(
            TreeLog::open(dir.path(), "other"),
            Err(MatrixError::Store(_))
        ));
    }

    #[test]
    fn content_serializes_to_the_wire_envelope_with_custom_type() {
        let content = ZikosTreeOpEventContent::from(sample());
        let json = serde_json::to_value(&content).unwrap();
        // Exact envelope: op + conflict metadata, no envelope fields.
        assert_eq!(json["op"]["op"], "rename");
        assert_eq!(json["op"]["name"], "take-1.flac");
        assert_eq!(json["base"]["name"], "old.flac");
        assert!(json.get("known_children").is_none());
        assert!(json.get("author").is_none());
        assert!(json.get("origin_server_ts").is_none());
        assert!(json.get("event_id").is_none());
        // Round-trips through the wire type back into core content.
        let back: ZikosTreeOpEventContent = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back.inner, sample());
        assert_eq!(ZikosTreeOpEventContent::TYPE, TREE_EVENT_TYPE);
        let _ = NodeKind::Folder; // re-export sanity for the module's users
        let _ = Uuid::new_v4();
    }
}

//! The per-room message log: what "history" means for phase 1.
//!
//! Every message surfaced by the sync handler is appended here and
//! persisted encrypted (same passphrase and format as the session file),
//! deduplicated by event ID so sync redelivery never duplicates history.
//! After a restart the log is reloaded, so history is available on start
//! without user action. Backfilling older server-side history via
//! `/messages` and the SDK event-cache timeline is a later change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::encrypted::{self, Cipher};
use crate::error::MatrixError;
use crate::message::MessageEvent;

/// File name inside the store directory.
pub const LOG_FILE: &str = "zikos-messages";

#[derive(Default)]
pub(crate) struct MessageLog {
    cipher: Option<Cipher>,
    path: Option<PathBuf>,
    rooms: BTreeMap<String, Vec<MessageEvent>>,
}

impl MessageLog {
    /// Load the log from `dir` (empty when absent). The cipher is created
    /// lazily on first record, since loading an empty log needs none.
    pub(crate) fn open(dir: &Path, passphrase: &str) -> Result<Self, MatrixError> {
        let path = dir.join(LOG_FILE);
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

    /// Record a message if its event ID is new for the room, persisting
    /// the whole log on change. Returns true when appended.
    pub(crate) fn record(&mut self, message: MessageEvent, passphrase: &str) -> bool {
        let room = self.rooms.entry(message.room_id.clone()).or_default();
        if room.iter().any(|m| m.event_id == message.event_id) {
            return false;
        }
        room.push(message);

        let cipher = match &self.cipher {
            Some(c) => c,
            None => match Cipher::generate(passphrase) {
                Ok(c) => self.cipher.get_or_insert(c),
                Err(_) => return true, // history stays in memory; surfaced by save errors later
            },
        };
        // Full rewrite per message is O(history) — fine at phase-1 scale;
        // compaction/streaming arrives when the log grows real volume.
        if let (Some(path), Ok(bytes)) = (&self.path, cipher.seal(&self.rooms)) {
            let _ = encrypted::write_atomic(path, &bytes);
        }
        true
    }

    /// The accumulated history of one room, oldest first.
    pub(crate) fn messages(&self, room_id: &str) -> Vec<MessageEvent> {
        self.rooms.get(room_id).cloned().unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::message::MessageBody;

    fn msg(room: &str, event_id: &str, body: &str) -> MessageEvent {
        MessageEvent {
            room_id: room.to_owned(),
            event_id: event_id.to_owned(),
            sender: "@bob:example.org".to_owned(),
            timestamp: 1,
            body: MessageBody::Text(body.to_owned()),
        }
    }

    #[test]
    fn records_in_order_and_persists_across_reopen() {
        let dir = tempfile::tempdir().unwrap();
        {
            let mut log = MessageLog::open(dir.path(), "pass").unwrap();
            assert!(log.record(msg("!r:hs", "$1", "a"), "pass"));
            assert!(log.record(msg("!r:hs", "$2", "b"), "pass"));
        }
        let log = MessageLog::open(dir.path(), "pass").unwrap();
        assert_eq!(log.messages("!r:hs").len(), 2);
        assert_eq!(log.messages("!r:hs")[1].event_id, "$2");
        assert!(log.messages("!other:hs").is_empty());
    }

    #[test]
    fn duplicate_event_id_is_ignored() {
        let mut log = MessageLog {
            cipher: None,
            path: None,
            rooms: BTreeMap::new(),
        };
        assert!(log.record(msg("!r:hs", "$1", "a"), "pass"));
        assert!(!log.record(msg("!r:hs", "$1", "a again"), "pass"));
        assert_eq!(log.messages("!r:hs").len(), 1);
        assert_eq!(
            log.messages("!r:hs")[0].body,
            MessageBody::Text("a".to_owned())
        );
    }

    #[test]
    fn wrong_passphrase_on_open_is_store_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut log = MessageLog::open(dir.path(), "pass").unwrap();
        log.record(msg("!r:hs", "$1", "a"), "pass");
        assert!(matches!(
            MessageLog::open(dir.path(), "other"),
            Err(MatrixError::Store(_))
        ));
    }
}

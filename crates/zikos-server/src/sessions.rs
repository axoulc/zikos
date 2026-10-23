//! Session storage: opaque tokens minted after Matrix OpenID verification.
//!
//! The registry is swappable behind [`SessionStore`]: an in-memory map for
//! tests and development, and a persistent file-backed registry so sessions
//! survive server restarts (see `specs/audio-storage-server.md`).

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum SessionError {
    #[error("session registry io error: {0}")]
    Io(#[from] std::io::Error),
}

/// Issues and resolves opaque session tokens (`token -> Matrix user id`).
pub trait SessionStore: Send + Sync {
    fn issue(&self, user_id: String) -> Result<String, SessionError>;
    fn resolve(&self, token: &str) -> Option<String>;
}

/// Volatile registry: sessions vanish when the process does. Development
/// and tests; production uses the file-backed registry.
#[derive(Default, Clone)]
pub struct InMemorySessionStore {
    sessions: Arc<Mutex<HashMap<String, String>>>, // token -> user_id
}

impl SessionStore for InMemorySessionStore {
    fn issue(&self, user_id: String) -> Result<String, SessionError> {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = B64URL.encode(bytes);
        self.sessions
            .lock()
            .expect("session registry poisoned")
            .insert(token.clone(), user_id);
        Ok(token)
    }

    fn resolve(&self, token: &str) -> Option<String> {
        self.sessions
            .lock()
            .expect("session registry poisoned")
            .get(token)
            .cloned()
    }
}

/// One JSONL record: `{"token":…,"user_id":…}`.
#[derive(Serialize, Deserialize)]
struct SessionRecord {
    token: String,
    user_id: String,
}

/// Persistent registry: an append-only JSONL file, one record per session.
///
/// Issued sessions survive restarts (the file is reloaded on open). A
/// crash mid-write can only damage an unterminated trailing line, which is
/// skipped on load; complete-but-corrupt lines are skipped as well, never
/// taking earlier sessions down with them. The file is created with
/// owner-only permissions because it holds bearer tokens.
pub struct FileSessionStore {
    sessions: Arc<Mutex<HashMap<String, String>>>, // token -> user_id
    file: Mutex<std::fs::File>,                    // append handle
}

impl FileSessionStore {
    pub fn open(path: impl AsRef<Path>) -> std::io::Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut load = String::new();
        match std::fs::read_to_string(path) {
            Ok(contents) => load = contents,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }

        let mut sessions = HashMap::new();
        for line in load.lines() {
            // A trailing fragment (crash mid-write) or a corrupt line is
            // skipped; complete valid records always load.
            if let Ok(record) = serde_json::from_str::<SessionRecord>(line) {
                sessions.insert(record.token, record.user_id);
            }
        }

        let file = open_append_handle(path)?;
        Ok(Self {
            sessions: Arc::new(Mutex::new(sessions)),
            file: Mutex::new(file),
        })
    }
}

#[cfg(unix)]
fn open_append_handle(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::unix::fs::OpenOptionsExt;
    std::fs::OpenOptions::new()
        .append(true)
        .read(true)
        .create(true)
        .mode(0o600) // bearer tokens live here
        .open(path)
}

#[cfg(not(unix))]
fn open_append_handle(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .append(true)
        .read(true)
        .create(true)
        .open(path)
}

impl SessionStore for FileSessionStore {
    fn issue(&self, user_id: String) -> Result<String, SessionError> {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = B64URL.encode(bytes);
        let record = SessionRecord {
            token: token.clone(),
            user_id: user_id.clone(),
        };
        let line = serde_json::to_string(&record)
            // Infallible: plain string fields, no maps with non-string keys.
            .expect("serializing a SessionRecord cannot fail");

        let mut file = self.file.lock().expect("session file poisoned");
        writeln!(file.by_ref(), "{line}")?;
        file.flush()?;
        drop(file);

        self.sessions
            .lock()
            .expect("session registry poisoned")
            .insert(token.clone(), user_id);
        Ok(token)
    }

    fn resolve(&self, token: &str) -> Option<String> {
        self.sessions
            .lock()
            .expect("session registry poisoned")
            .get(token)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn issue_then_resolve_roundtrip() {
        let store: Arc<dyn SessionStore> = Arc::new(InMemorySessionStore::default());
        let token = store.issue("@lisa:example.org".into()).unwrap();
        assert_eq!(store.resolve(&token), Some("@lisa:example.org".into()));
        assert_eq!(store.resolve("no-such-token"), None);
    }

    #[test]
    fn issued_tokens_are_unique() {
        let store = InMemorySessionStore::default();
        let a = store.issue("@lisa:example.org".into()).unwrap();
        let b = store.issue("@lisa:example.org".into()).unwrap();
        assert_ne!(a, b, "two sessions for one user must not share a token");
    }

    #[test]
    fn sessions_survive_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.jsonl");

        let first = FileSessionStore::open(&path).unwrap();
        let token = first.issue("@lisa:example.org".into()).unwrap();
        let token2 = first.issue("@fred:example.org".into()).unwrap();
        drop(first); // simulate a server restart

        let reopened = FileSessionStore::open(&path).unwrap();
        assert_eq!(
            reopened.resolve(&token),
            Some("@lisa:example.org".into()),
            "sessions must survive a restart"
        );
        assert_eq!(reopened.resolve(&token2), Some("@fred:example.org".into()));
    }

    #[test]
    fn corrupt_tail_is_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.jsonl");

        let store = FileSessionStore::open(&path).unwrap();
        let token = store.issue("@lisa:example.org".into()).unwrap();
        drop(store);

        // Simulate a crash mid-write: an unterminated, truncated last line.
        let mut tail = std::fs::read_to_string(&path).unwrap();
        tail.push_str(r#"{"token":"ab"#);
        std::fs::write(&path, tail).unwrap();

        let reopened = FileSessionStore::open(&path).unwrap();
        assert_eq!(
            reopened.resolve(&token),
            Some("@lisa:example.org".into()),
            "a corrupt trailing record must not lose earlier sessions"
        );
        assert_eq!(reopened.resolve("ab"), None);
    }

    #[cfg(unix)]
    #[test]
    fn registry_file_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("sessions.jsonl");
        let store = FileSessionStore::open(&path).unwrap();
        store.issue("@lisa:example.org".into()).unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o600, "session file must be owner-only");
    }
}

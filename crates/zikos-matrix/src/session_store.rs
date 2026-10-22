//! Persisted session storage.
//!
//! matrix-sdk 0.13 does not hand back the access-token session from the
//! sqlite store on restart, so zikos-matrix persists the
//! [`MatrixSession`] itself, encrypted with the same passphrase as the
//! state store. See [`encrypted`] for the file format.

use std::path::Path;

use matrix_sdk::authentication::matrix::MatrixSession;

use crate::encrypted::{self, Cipher};
use crate::error::MatrixError;

/// Name of the session file inside the store directory.
pub const SESSION_FILE: &str = "zikos-session";

/// Serialize `session` encrypted with `passphrase` into the store
/// directory. Overwrites any previous session.
pub fn save(dir: &Path, passphrase: &str, session: &MatrixSession) -> Result<(), MatrixError> {
    let bytes = Cipher::generate(passphrase)?.seal(session)?;
    std::fs::create_dir_all(dir).map_err(|e| MatrixError::Store(e.to_string()))?;
    encrypted::write_atomic(&dir.join(SESSION_FILE), &bytes)
}

/// Load a session previously written by [`save`]. Fails with
/// [`MatrixError::Store`] when no session exists, the passphrase is
/// wrong, or the file is corrupted.
pub fn load(dir: &Path, passphrase: &str) -> Result<MatrixSession, MatrixError> {
    let bytes = std::fs::read(dir.join(SESSION_FILE))
        .map_err(|_| MatrixError::Store("no persisted session".into()))?;
    Cipher::open(passphrase, &bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session() -> MatrixSession {
        serde_json::from_value(serde_json::json!({
            "user_id": "@alice:example.org",
            "device_id": "ZIKOSTEST",
            "access_token": "syt_tok",
            "refresh_token": null,
        }))
        .unwrap()
    }

    #[test]
    fn roundtrip_preserves_session() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "pass", &session()).unwrap();
        assert_eq!(load(dir.path(), "pass").unwrap(), session());
    }

    #[test]
    fn wrong_passphrase_is_store_error() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "pass", &session()).unwrap();
        assert!(matches!(
            load(dir.path(), "nope"),
            Err(MatrixError::Store(_))
        ));
    }

    #[test]
    fn missing_file_is_store_error() {
        let dir = tempfile::tempdir().unwrap();
        assert!(matches!(
            load(dir.path(), "pass"),
            Err(MatrixError::Store(_))
        ));
    }

    #[test]
    fn truncated_file_is_store_error() {
        let dir = tempfile::tempdir().unwrap();
        save(dir.path(), "pass", &session()).unwrap();
        let path = dir.path().join(SESSION_FILE);
        let full = std::fs::read(&path).unwrap();
        std::fs::write(&path, &full[..full.len() - 5]).unwrap();
        assert!(matches!(
            load(dir.path(), "pass"),
            Err(MatrixError::Store(_))
        ));
    }
}

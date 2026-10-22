//! Passphrase-encrypted file codec shared by everything zikos-matrix
//! persists next to the SDK store (session, message log).
//!
//! File format: `[u32 BE cipher_len][cipher export][value ciphertext]`.
//! The cipher export is itself passphrase-encrypted (Argon2 KDF, pure
//! RustCrypto via `matrix-sdk-store-encryption`), so a wrong passphrase
//! fails at import time.

use matrix_sdk_store_encryption::StoreCipher;
use serde::Serialize;
use serde::de::DeserializeOwned;

use crate::error::MatrixError;

use std::sync::Arc;

/// A cipher kept in memory for repeated encryption; `export` is written
/// once into every file so the file is self-contained.
pub(crate) struct Cipher {
    inner: Arc<StoreCipher>,
    export: Vec<u8>,
}

impl Cipher {
    /// Generate a fresh random cipher exportable with `passphrase`.
    pub(crate) fn generate(passphrase: &str) -> Result<Self, MatrixError> {
        let inner = StoreCipher::new().map_err(err)?;
        let export = inner.export(passphrase).map_err(err)?;
        Ok(Self {
            inner: Arc::new(inner),
            export,
        })
    }

    /// Serialize and encrypt `value` into self-contained file bytes.
    pub(crate) fn seal<T: Serialize>(&self, value: &T) -> Result<Vec<u8>, MatrixError> {
        let blob = self.inner.encrypt_value(value).map_err(err)?;
        let mut bytes = Vec::with_capacity(4 + self.export.len() + blob.len());
        bytes.extend_from_slice(&(self.export.len() as u32).to_be_bytes());
        bytes.extend_from_slice(&self.export);
        bytes.extend_from_slice(&blob);
        Ok(bytes)
    }

    /// Open self-contained file bytes sealed with `passphrase`.
    pub(crate) fn open<T: DeserializeOwned>(
        passphrase: &str,
        bytes: &[u8],
    ) -> Result<T, MatrixError> {
        let bad = || MatrixError::Store("wrong passphrase or corrupted file".into());
        let len = u32::from_be_bytes(bytes.get(..4).ok_or_else(bad)?.try_into().unwrap()) as usize;
        let export = bytes.get(4..4 + len).ok_or_else(bad)?;
        let blob = bytes.get(4 + len..).ok_or_else(bad)?;

        let cipher = StoreCipher::import(passphrase, export).map_err(|_| bad())?;
        cipher.decrypt_value(blob).map_err(|_| bad())
    }
}

/// Atomic write: temp file then rename, so a crash never truncates the
/// previous version.
pub(crate) fn write_atomic(path: &std::path::Path, bytes: &[u8]) -> Result<(), MatrixError> {
    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, bytes).map_err(|e| MatrixError::Store(e.to_string()))?;
    std::fs::rename(&tmp, path).map_err(|e| MatrixError::Store(e.to_string()))?;
    Ok(())
}

fn err(e: matrix_sdk_store_encryption::Error) -> MatrixError {
    MatrixError::Store(e.to_string())
}

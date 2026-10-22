//! Client-side encryption for audio blobs before they ever leave the device.
//!
//! Threat model: the storage server is honest-but-curious. It stores opaque
//! ciphertext and never learns file contents, file names, or conversation
//! membership. The per-file [`FileKey`] travels only inside the end-to-end
//! encrypted Matrix room, alongside the blob id.
//!
//! Format is whole-blob AES-256-GCM for now; chunked/streaming encryption for
//! multi-hundred-MB takes is specified in `specs/security.md` and will land as
//! an OpenSpec change.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Key, Nonce};
use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Symmetric key for one file. 256 bits, generated on-device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileKey(pub [u8; 32]);

impl FileKey {
    pub fn generate() -> Self {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        FileKey(bytes)
    }

    /// Base64 form, as it appears inside the encrypted room message.
    pub fn to_base64(&self) -> String {
        B64.encode(self.0)
    }

    pub fn from_base64(s: &str) -> Result<Self, CryptoError> {
        let bytes: Vec<u8> = B64
            .decode(s)
            .map_err(|_| CryptoError::MalformedKeyMaterial)?;
        let arr: [u8; 32] = bytes
            .try_into()
            .map_err(|_| CryptoError::MalformedKeyMaterial)?;
        Ok(FileKey(arr))
    }
}

/// What gets uploaded: nonce prepended is NOT done implicitly; both fields are
/// explicit so the wire format is unambiguous.
#[derive(Serialize, Deserialize)]
pub struct EncryptedBlob {
    pub nonce: [u8; 12],
    pub ciphertext: Vec<u8>,
}

#[derive(Debug, Error, PartialEq)]
pub enum CryptoError {
    #[error("decryption failed (wrong key or tampered ciphertext)")]
    DecryptFailed,
    #[error("malformed key material")]
    MalformedKeyMaterial,
}

/// Encrypt a whole file with AES-256-GCM. A fresh random nonce per call.
pub fn encrypt_blob(key: &FileKey, plaintext: &[u8]) -> EncryptedBlob {
    let mut nonce_bytes = [0u8; 12];
    rand::rng().fill_bytes(&mut nonce_bytes);
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key.0));
    let ciphertext = cipher
        .encrypt(Nonce::from_slice(&nonce_bytes), Payload::from(plaintext))
        .expect("AES-GCM encryption of in-memory buffers cannot fail");
    EncryptedBlob {
        nonce: nonce_bytes,
        ciphertext,
    }
}

/// Decrypt a blob. Fails if the key is wrong or a single bit was flipped.
pub fn decrypt_blob(key: &FileKey, blob: &EncryptedBlob) -> Result<Vec<u8>, CryptoError> {
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&key.0));
    cipher
        .decrypt(
            Nonce::from_slice(&blob.nonce),
            Payload::from(blob.ciphertext.as_slice()),
        )
        .map_err(|_| CryptoError::DecryptFailed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_preserves_plaintext() {
        let key = FileKey::generate();
        let take = vec![0u8; 1024]; // stand-in for audio bytes
        let blob = encrypt_blob(&key, &take);
        assert_eq!(decrypt_blob(&key, &blob).unwrap(), take);
    }

    #[test]
    fn gcm_appends_a_16_byte_tag() {
        let key = FileKey::generate();
        let blob = encrypt_blob(&key, &[0u8; 100]);
        assert_eq!(blob.ciphertext.len(), 100 + 16);
    }

    #[test]
    fn nonces_are_unique_per_encryption() {
        let key = FileKey::generate();
        let a = encrypt_blob(&key, &[1, 2, 3]);
        let b = encrypt_blob(&key, &[1, 2, 3]);
        assert_ne!(a.nonce, b.nonce);
        assert_ne!(a.ciphertext, b.ciphertext);
    }

    #[test]
    fn wrong_key_fails() {
        let blob = encrypt_blob(&FileKey::generate(), &vec![7u8; 512]);
        let other = FileKey::generate();
        assert_eq!(decrypt_blob(&other, &blob), Err(CryptoError::DecryptFailed));
    }

    #[test]
    fn tampered_ciphertext_fails() {
        let key = FileKey::generate();
        let mut blob = encrypt_blob(&key, &vec![7u8; 512]);
        blob.ciphertext[10] ^= 0x01;
        assert_eq!(decrypt_blob(&key, &blob), Err(CryptoError::DecryptFailed));
    }

    #[test]
    fn file_key_base64_roundtrip() {
        let key = FileKey::generate();
        assert_eq!(FileKey::from_base64(&key.to_base64()).unwrap(), key);
        assert!(FileKey::from_base64("not!!valid").is_err());
        assert!(FileKey::from_base64(&B64.encode([0u8; 5])).is_err()); // wrong length
    }
}

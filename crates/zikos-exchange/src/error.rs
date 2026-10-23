//! The typed error surface of the exchange layer: auth, transport, server,
//! metadata-validation, and crypto failures.

use zikos_crypto::CryptoError;

#[derive(Debug, thiserror::Error)]
pub enum ExchangeError {
    /// No session yet, or the session was rejected (401).
    #[error("not logged in or session rejected")]
    Unauthorized,
    /// The referenced blob does not exist on the server (404).
    #[error("blob {0} not found on the server")]
    NotFound(String),
    /// The upload exceeded the server's configured size limit (413).
    #[error("upload exceeds the server's size limit")]
    TooLarge,
    /// Any other unexpected HTTP status from the blob server.
    #[error("blob server returned status {0}")]
    Status(u16),
    /// The request could not be completed at all (DNS, connect, TLS, ...).
    #[error("transport failure: {0}")]
    Transport(#[from] reqwest::Error),
    /// The take's metadata failed validation before anything was uploaded.
    #[error("invalid recording metadata: {0}")]
    InvalidMeta(String),
    /// Decryption or blob-frame failure (wrong key, tampering, bad frame).
    #[error("crypto failure: {0}")]
    Crypto(#[from] CryptoError),
}

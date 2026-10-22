//! Error type at the crate boundary: every `matrix-sdk` failure leaving
//! this crate is translated to a [`MatrixError`], so embedders never
//! match on SDK internals.

use matrix_sdk::Error as SdkError;
use matrix_sdk::HttpError;
use matrix_sdk::ruma::api::client::error::ErrorKind;
use thiserror::Error;

/// Everything that can go wrong in a Zikos Matrix session.
#[derive(Debug, Error)]
pub enum MatrixError {
    /// The homeserver rejected the credentials (`M_FORBIDDEN`). The UI
    /// should keep the entered username and show a clear error.
    #[error("invalid username or password")]
    InvalidCredentials,

    /// The homeserver (or network) could not be reached. Retrying later
    /// may succeed; this is not a credential problem.
    #[error("could not reach the homeserver")]
    Connection,

    /// The homeserver answered with an error we don't model explicitly.
    #[error("homeserver error: {0}")]
    Homeserver(String),

    /// End-to-end encryption machinery failed (Olm/Megolm, key store).
    #[error("crypto error: {0}")]
    Crypto(String),

    /// The local (encrypted) state store failed.
    #[error("local store error: {0}")]
    Store(String),

    /// The caller referenced a room this session doesn't know.
    #[error("unknown room: {0}")]
    UnknownRoom(String),
}

impl From<SdkError> for MatrixError {
    fn from(err: SdkError) -> Self {
        match err {
            SdkError::Http(ref http) => map_http(http),
            SdkError::StateStore(e) => MatrixError::Store(e.to_string()),
            SdkError::EventCacheStore(e) => MatrixError::Store(e.to_string()),
            SdkError::CryptoStoreError(e) => MatrixError::Crypto(e.to_string()),
            SdkError::OlmError(e) => MatrixError::Crypto(e.to_string()),
            SdkError::MegolmError(e) => MatrixError::Crypto(e.to_string()),
            other => MatrixError::Homeserver(other.to_string()),
        }
    }
}

fn map_http(http: &HttpError) -> MatrixError {
    if matches!(
        http.client_api_error_kind(),
        Some(ErrorKind::Forbidden { .. })
    ) {
        return MatrixError::InvalidCredentials;
    }
    if let HttpError::Reqwest(e) = http {
        if e.is_connect() {
            return MatrixError::Connection;
        }
    }
    MatrixError::Homeserver(http.to_string())
}

impl From<matrix_sdk::ClientBuildError> for MatrixError {
    fn from(err: matrix_sdk::ClientBuildError) -> Self {
        use matrix_sdk::ClientBuildError;
        match err {
            ClientBuildError::SqliteStore(e) => MatrixError::Store(e.to_string()),
            ClientBuildError::Http(ref http) => map_http(http),
            ClientBuildError::Url(e) => MatrixError::Homeserver(e.to_string()),
            other => MatrixError::Homeserver(other.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_distinguishes_credentials_from_connection() {
        assert_eq!(
            MatrixError::InvalidCredentials.to_string(),
            "invalid username or password"
        );
        assert_eq!(
            MatrixError::Connection.to_string(),
            "could not reach the homeserver"
        );
    }
}

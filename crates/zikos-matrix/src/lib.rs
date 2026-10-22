//! Matrix transport for Zikos: end-to-end encrypted messaging over any
//! homeserver, via `matrix-sdk`.
//!
//! Phase 1 wraps login with a persisted encrypted session, `/sync`-based
//! history restore, room creation that always enables encryption before the
//! first message, and text send/receive with delivery states and offline
//! queueing that survives restarts.
//!
//! All `matrix-sdk` types stay inside this crate: embedders see only the
//! types defined here.

#![warn(missing_docs)]

pub mod error;

mod client;
mod delivery;
mod encrypted;
mod message;
mod message_log;
mod session_store;

pub use client::{Client, RoomEncryption};
pub use delivery::{DeliveryEvent, DeliveryState};
pub use error::MatrixError;
pub use message::{MessageBody, MessageEvent};

#[cfg(test)]
mod tests {
    #[test]
    fn crate_in_workspace() {
        // Smoke test: proves the crate is registered in the workspace and
        // runs under `cargo t -p zikos-matrix`.
        assert_eq!(env!("CARGO_PKG_NAME"), "zikos-matrix");
    }

    #[tokio::test]
    async fn builds_client_with_sqlite_store() {
        let dir = tempfile::tempdir().unwrap();
        let client = matrix_sdk::Client::builder()
            .homeserver_url("http://127.0.0.1:1")
            .sqlite_store(dir.path(), Some("store-pass"))
            .build()
            .await;
        assert!(client.is_ok());
    }
}

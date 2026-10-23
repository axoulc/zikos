//! Zikos audio blob server.
//!
//! One dumb, honest service: authenticated clients PUT opaque ciphertext and
//! GET it back. It never sees file contents, file names, or conversation
//! membership — those live in the Matrix room, encrypted end-to-end.
//! See `specs/audio-storage-server.md` for the contract.

pub mod access_log;
pub mod auth;
pub mod error;
pub mod routes;
pub mod sessions;
pub mod store;

pub use access_log::{AccessEntry, LogSink, StderrLogSink};
pub use auth::{MatrixOpenIdVerifier, OpenIdVerifier};
pub use error::ApiError;
pub use routes::{AppState, ServerConfig, router};
pub use sessions::{InMemorySessionStore, SessionStore};
pub use store::{BlobStore, FsBlobStore};

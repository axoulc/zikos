//! Zikos audio blob server.
//!
//! One dumb, honest service: authenticated clients PUT opaque ciphertext and
//! GET it back. It never sees file contents, file names, or conversation
//! membership — those live in the Matrix room, encrypted end-to-end.
//! See `specs/audio-storage-server.md` for the contract.

pub mod auth;
pub mod error;
pub mod routes;
pub mod store;

pub use auth::{MatrixOpenIdVerifier, OpenIdVerifier, SessionRegistry};
pub use error::ApiError;
pub use routes::{AppState, ServerConfig, router};
pub use store::{BlobStore, FsBlobStore};

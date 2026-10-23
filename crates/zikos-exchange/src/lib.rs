//! Client-side file exchange: publish finalized takes to a conversation's
//! shared drive and fetch them back on any device.
//!
//! Publishing runs entirely on-device: the audio is encrypted under a fresh
//! per-file key (`zikos-crypto`) before anything leaves, the ciphertext is
//! uploaded to the self-hosted blob server (`zikos-server`), and the
//! file-create tree event — carrying the blob id, the key, and the take's
//! metadata — travels only inside the end-to-end encrypted Matrix room.
//! Fetching is the mirror: download the referenced blob, verify and decrypt
//! it with the room-distributed key, no user key management.

mod blob;
mod error;
mod service;

pub use blob::{BlobClient, BlobSession, UploadedBlob};
pub use error::ExchangeError;
pub use service::{ExchangeService, FetchedTake, PublishedTake};

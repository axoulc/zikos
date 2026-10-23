//! Zikos domain core.
//!
//! Everything in this crate is pure, platform-independent Rust with no I/O:
//! the shared file tree, its event log, and recording metadata. The Matrix
//! sync layer and the audio engine both build on top of these types.

pub mod meta;
pub mod tree;
pub mod tree_sync;

pub use meta::{AudioFormat, RecordingMeta};
pub use tree::{FileTree, Node, NodeId, NodeKind, TreeOp};

//! The exchange orchestration: publish a finalized take (encrypt → upload →
//! file-create event) and fetch one back (download → verify → decrypt).

use uuid::Uuid;
use zikos_core::RecordingMeta;
use zikos_core::tree::{FilePayload, NodeId, NodeKind, TreeOp};
use zikos_core::tree_sync::TreeOpContent;
use zikos_crypto::{EncryptedBlob, FileKey, decrypt_blob, encrypt_blob};

use crate::{BlobClient, ExchangeError};

/// The publish outcome: everything needed to make the take appear in the
/// room's shared tree. `content` is ready to send as a `zikos.tree.op`
/// room event; it references the already-uploaded blob.
#[derive(Debug, Clone)]
pub struct PublishedTake {
    /// The id of the created file node.
    pub node_id: NodeId,
    /// The uploaded blob's server id.
    pub blob_id: String,
    /// The per-file key (also inside `content`'s payload).
    pub key: FileKey,
    /// The ready-to-send file-create event content.
    pub content: TreeOpContent,
}

/// The fetch outcome: decrypted audio plus its metadata.
#[derive(Debug, Clone)]
pub struct FetchedTake {
    pub audio: Vec<u8>,
    pub meta: RecordingMeta,
}

/// Publishes and fetches takes through one blob server.
#[derive(Debug, Clone)]
pub struct ExchangeService {
    blobs: BlobClient,
}

impl ExchangeService {
    /// Wrap an authenticated [`BlobClient`] (log in first).
    pub fn new(blobs: BlobClient) -> Self {
        Self { blobs }
    }

    /// Publish a finalized take: validate the metadata, encrypt the audio
    /// under a fresh per-file key, upload the wire frame, and return the
    /// file-create event referencing the blob. The upload happens strictly
    /// before the event is usable, so a referenced blob always exists.
    pub async fn publish(
        &self,
        audio: &[u8],
        meta: &RecordingMeta,
        parent: NodeId,
        name: &str,
    ) -> Result<PublishedTake, ExchangeError> {
        meta.validate()
            .map_err(|e| ExchangeError::InvalidMeta(e.to_string()))?;
        let key = FileKey::generate();
        let uploaded = self
            .blobs
            .upload(encrypt_blob(&key, audio).to_wire())
            .await?;
        let node_id = Uuid::new_v4();
        let payload = FilePayload {
            blob_id: uploaded.id.clone(),
            key: key.clone(),
            meta: meta.clone(),
        };
        Ok(PublishedTake {
            node_id,
            blob_id: uploaded.id,
            key,
            content: TreeOpContent {
                op: TreeOp::Create {
                    id: node_id,
                    parent,
                    kind: NodeKind::File,
                    name: name.to_owned(),
                    file: Some(payload),
                },
                base: None,
                known_children: None,
            },
        })
    }

    /// Fetch a file node's audio: download the referenced blob, verify and
    /// decrypt it with the room-distributed key. Fails closed on tampering,
    /// wrong keys, and unknown blobs — never emits plaintext.
    pub async fn fetch(&self, payload: &FilePayload) -> Result<FetchedTake, ExchangeError> {
        let wire = self.blobs.download(&payload.blob_id).await?;
        let blob = EncryptedBlob::from_wire(&wire)?;
        let audio = decrypt_blob(&payload.key, &blob)?;
        Ok(FetchedTake {
            audio,
            meta: payload.meta.clone(),
        })
    }
}

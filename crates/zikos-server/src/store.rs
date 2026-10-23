//! Blob storage: opaque ciphertext on disk, addressed by UUID.
//!
//! Bodies stream in both directions: uploads are written chunk by chunk to
//! a temp file that is atomically renamed on completion (a crash never
//! leaves a partial blob visible), downloads are served from the file
//! handle directly. Nothing here ever buffers a whole blob in memory.

use async_trait::async_trait;
use axum::body::Body;
use http_body_util::BodyExt;
use std::path::PathBuf;
use thiserror::Error;
use tokio::io::AsyncWriteExt;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("blob {0:?} not found")]
    NotFound(String),
    #[error("invalid blob id {0:?}")]
    InvalidId(String),
    #[error("blob exceeds the configured size cap")]
    TooLarge,
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Stores `id` from a streaming request body, enforcing `max_bytes`
    /// while counting every chunk. Returns the number of bytes stored.
    /// The blob is not retrievable until the entire body has landed.
    async fn put_stream(&self, id: &str, body: Body, max_bytes: u64) -> Result<u64, StoreError>;

    /// Opens a stored blob for streaming download, returning the file
    /// handle and its exact length in bytes.
    async fn open(&self, id: &str) -> Result<(tokio::fs::File, u64), StoreError>;
}

/// Filesystem-backed store. One file per blob under `root/{id}.bin`.
pub struct FsBlobStore {
    root: PathBuf,
}

impl FsBlobStore {
    pub fn new(root: impl Into<PathBuf>) -> std::io::Result<Self> {
        let root = root.into();
        std::fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    fn path_of(&self, id: &str) -> Result<PathBuf, StoreError> {
        uuid::Uuid::parse_str(id).map_err(|_| StoreError::InvalidId(id.to_string()))?;
        // Safe: id is a strict UUID, so it cannot traverse paths.
        Ok(self.root.join(format!("{id}.bin")))
    }
}

#[async_trait]
impl BlobStore for FsBlobStore {
    async fn put_stream(&self, id: &str, body: Body, max_bytes: u64) -> Result<u64, StoreError> {
        let path = self.path_of(id)?;
        let tmp = self.root.join(format!("{id}.bin.tmp"));
        let mut file = tokio::fs::File::create(&tmp).await?;
        let mut body = body;
        let mut written: u64 = 0;

        let outcome = async {
            while let Some(frame) = body.frame().await {
                let frame =
                    frame.map_err(|e| StoreError::Io(std::io::Error::other(e.to_string())))?;
                let Ok(data) = frame.into_data() else {
                    continue; // trailers and other non-data frames are ignored
                };
                written += data.len() as u64;
                if written > max_bytes {
                    return Err(StoreError::TooLarge);
                }
                file.write_all(&data).await?;
            }
            file.flush().await?;
            file.sync_all().await?;
            Ok(written)
        }
        .await;

        drop(file); // release the handle before rename/remove
        match outcome {
            Ok(written) => match tokio::fs::rename(&tmp, &path).await {
                Ok(()) => Ok(written),
                Err(e) => {
                    let _ = tokio::fs::remove_file(&tmp).await;
                    Err(e.into())
                }
            },
            Err(e) => {
                // Best-effort cleanup: a partial blob must never be visible.
                let _ = tokio::fs::remove_file(&tmp).await;
                Err(e)
            }
        }
    }

    async fn open(&self, id: &str) -> Result<(tokio::fs::File, u64), StoreError> {
        let path = self.path_of(id)?;
        let file = tokio::fs::File::open(&path)
            .await
            .map_err(|e| match e.kind() {
                std::io::ErrorKind::NotFound => StoreError::NotFound(id.to_string()),
                _ => StoreError::Io(e),
            })?;
        let len = file.metadata().await?.len();
        Ok((file, len))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use tokio_util::io::ReaderStream;

    /// A body delivered in many small chunks, like a real streaming upload.
    fn chunked_body(data: Vec<u8>, chunk: usize) -> Body {
        Body::from_stream(ReaderStream::with_capacity(
            std::io::Cursor::new(data),
            chunk,
        ))
    }

    #[tokio::test]
    async fn streamed_put_writes_exact_bytes_via_atomic_rename() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let payload: Vec<u8> = (0..10_000u32).map(|i| (i % 251) as u8).collect();

        let written = store
            .put_stream(&id, chunked_body(payload.clone(), 1024), 1 << 20)
            .await
            .unwrap();
        assert_eq!(written, payload.len() as u64);

        let on_disk = std::fs::read(dir.path().join(format!("{id}.bin"))).unwrap();
        assert_eq!(on_disk, payload);
        assert!(!dir.path().join(format!("{id}.bin.tmp")).exists());
    }

    #[tokio::test]
    async fn stream_over_cap_aborts_and_cleans_up() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();

        let err = store
            .put_stream(&id, chunked_body(vec![7u8; 64], 8), 8)
            .await
            .expect_err("stream past the cap must abort");
        assert!(matches!(err, StoreError::TooLarge), "got {err:?}");

        assert!(!dir.path().join(format!("{id}.bin")).exists());
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".tmp"))
            .collect();
        assert!(leftovers.is_empty(), "temp leftovers: {leftovers:?}");
    }

    #[tokio::test]
    async fn open_returns_seekable_file_with_exact_length() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();
        let payload: Vec<u8> = (0..5_000u32).map(|i| (i % 7) as u8).collect();
        store
            .put_stream(&id, chunked_body(payload.clone(), 512), 1 << 20)
            .await
            .unwrap();

        let (mut file, len) = store.open(&id).await.unwrap();
        assert_eq!(len, payload.len() as u64);
        let mut read_back = Vec::new();
        tokio::io::AsyncReadExt::read_to_end(&mut file, &mut read_back)
            .await
            .unwrap();
        assert_eq!(read_back, payload);

        match store.open(&uuid::Uuid::new_v4().to_string()).await {
            Err(StoreError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
        assert!(matches!(
            store.open("not-a-uuid").await,
            Err(StoreError::InvalidId(_))
        ));
    }

    #[tokio::test]
    async fn non_uuid_ids_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path()).unwrap();
        assert!(matches!(
            store.put_stream("../../etc/passwd", Body::empty(), 8).await,
            Err(StoreError::InvalidId(_))
        ));
    }
}

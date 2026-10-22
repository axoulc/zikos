//! Blob storage: opaque ciphertext on disk, addressed by UUID.

use async_trait::async_trait;
use std::path::PathBuf;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum StoreError {
    #[error("blob {0:?} not found")]
    NotFound(String),
    #[error("invalid blob id {0:?}")]
    InvalidId(String),
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait BlobStore: Send + Sync {
    async fn put(&self, id: &str, bytes: &[u8]) -> Result<(), StoreError>;
    async fn get(&self, id: &str) -> Result<Vec<u8>, StoreError>;
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
    async fn put(&self, id: &str, bytes: &[u8]) -> Result<(), StoreError> {
        let path = self.path_of(id)?;
        tokio::fs::write(path, bytes).await?;
        Ok(())
    }

    async fn get(&self, id: &str) -> Result<Vec<u8>, StoreError> {
        let path = self.path_of(id)?;
        tokio::fs::read(path).await.map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => StoreError::NotFound(id.to_string()),
            _ => StoreError::Io(e),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn put_get_roundtrip_and_404_mapping() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path()).unwrap();
        let id = uuid::Uuid::new_v4().to_string();

        store.put(&id, b"ciphertext".as_slice()).await.unwrap();
        assert_eq!(store.get(&id).await.unwrap(), b"ciphertext".to_vec());

        match store.get(&uuid::Uuid::new_v4().to_string()).await {
            Err(StoreError::NotFound(_)) => {}
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_uuid_ids_are_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let store = FsBlobStore::new(dir.path()).unwrap();
        assert!(matches!(
            store.put("../../etc/passwd", b"x".as_slice()).await,
            Err(StoreError::InvalidId(_))
        ));
    }
}

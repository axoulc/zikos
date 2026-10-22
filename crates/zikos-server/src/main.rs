//! Zikos audio blob server binary.
//!
//! Configuration via environment:
//! - `ZIKOS_BIND`            — listen address (default `0.0.0.0:8333`)
//! - `ZIKOS_BLOB_DIR`        — blob storage directory (default `./blobs`)
//! - `ZIKOS_MAX_BLOB_BYTES`  — upload cap in bytes (default 512 MiB)

use std::path::PathBuf;
use zikos_server::auth::MatrixOpenIdVerifier;
use zikos_server::store::FsBlobStore;
use zikos_server::{AppState, ServerConfig, router};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bind = std::env::var("ZIKOS_BIND").unwrap_or_else(|_| "0.0.0.0:8333".into());
    let blob_dir = std::env::var("ZIKOS_BLOB_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("blobs"));
    let max_blob_bytes = std::env::var("ZIKOS_MAX_BLOB_BYTES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or_else(|| ServerConfig::default().max_blob_bytes);

    let state = AppState {
        verifier: std::sync::Arc::new(MatrixOpenIdVerifier::new()),
        sessions: Default::default(),
        blobs: std::sync::Arc::new(FsBlobStore::new(&blob_dir)?),
        config: ServerConfig { max_blob_bytes },
    };

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!(
        "zikos-server listening on {bind}, blobs in {}",
        blob_dir.display()
    );
    axum::serve(listener, router(state)).await?;
    Ok(())
}

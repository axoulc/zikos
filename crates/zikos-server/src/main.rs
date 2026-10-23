//! Zikos audio blob server binary.
//!
//! Configuration via environment:
//! - `ZIKOS_BIND`            — listen address (default `0.0.0.0:8333`)
//! - `ZIKOS_BLOB_DIR`        — blob storage directory (default `./blobs`)
//! - `ZIKOS_MAX_BLOB_BYTES`  — upload cap in bytes (default 512 MiB)
//! - `ZIKOS_SESSION_FILE`    — persistent session registry (default
//!   `./sessions.jsonl`)

use std::path::PathBuf;
use zikos_server::access_log::StderrLogSink;
use zikos_server::auth::MatrixOpenIdVerifier;
use zikos_server::sessions::FileSessionStore;
use zikos_server::store::FsBlobStore;
use zikos_server::{AppState, ServerConfig, router};

/// Pure env parsing: where the persistent session registry lives.
fn session_file_from(env: Option<String>) -> PathBuf {
    env.map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("sessions.jsonl"))
}

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
    let session_file = session_file_from(std::env::var("ZIKOS_SESSION_FILE").ok());

    let state = AppState {
        verifier: std::sync::Arc::new(MatrixOpenIdVerifier::new()),
        sessions: std::sync::Arc::new(FileSessionStore::open(&session_file)?),
        blobs: std::sync::Arc::new(FsBlobStore::new(&blob_dir)?),
        config: ServerConfig { max_blob_bytes },
        log_sink: std::sync::Arc::new(StderrLogSink),
    };

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!(
        "zikos-server listening on {bind}, blobs in {}, sessions in {}",
        blob_dir.display(),
        session_file.display()
    );
    axum::serve(listener, router(state)).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_file_env_default_and_override() {
        assert_eq!(
            session_file_from(None),
            PathBuf::from("sessions.jsonl"),
            "unset env falls back to the default"
        );
        assert_eq!(
            session_file_from(Some("/var/lib/zikos/sessions.jsonl".into())),
            PathBuf::from("/var/lib/zikos/sessions.jsonl")
        );
    }
}

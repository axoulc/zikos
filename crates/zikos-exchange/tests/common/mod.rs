//! Shared integration harness: the real zikos-server router on an
//! ephemeral local port, so tests exercise the true HTTP path end to end.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use zikos_server::access_log::CapturingSink;
use zikos_server::auth::{AuthError, OpenIdVerifier};
use zikos_server::routes::{AppState, ServerConfig, router};
use zikos_server::sessions::InMemorySessionStore;
use zikos_server::store::FsBlobStore;

/// The one OpenID token the fake homeserver accepts.
pub const VALID_TOKEN: &str = "openid-token-1";
/// The Matrix identity behind that token.
pub const USER_ID: &str = "@lisa:matrix.example.org";
/// The Matrix server name the fake verifier answers for.
pub const SERVER_NAME: &str = "matrix.example.org";

struct FakeVerifier;

#[async_trait]
impl OpenIdVerifier for FakeVerifier {
    async fn verify(&self, access_token: &str, server_name: &str) -> Result<String, AuthError> {
        if server_name != SERVER_NAME || access_token != VALID_TOKEN {
            return Err(AuthError::Rejected);
        }
        Ok(USER_ID.to_string())
    }
}

/// Spawn the real router (fake OpenID verifier, in-memory sessions,
/// on-disk blobs) on `127.0.0.1:<ephemeral>`. Returns the base URL and the
/// blob directory (kept on disk for on-disk assertions).
pub async fn spawn_server(max_blob_bytes: usize) -> (String, PathBuf) {
    let dir = tempfile::tempdir().unwrap().keep();
    let state = AppState {
        verifier: Arc::new(FakeVerifier),
        sessions: Arc::new(InMemorySessionStore::default()),
        blobs: Arc::new(FsBlobStore::new(&dir).unwrap()),
        config: ServerConfig { max_blob_bytes },
        log_sink: Arc::new(CapturingSink::default()),
    };
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        // Infallible in practice for a test server; a failure aborts the
        // task and every dependent assertion fails loudly.
        axum::serve(listener, router(state)).await.expect("server");
    });
    (format!("http://{addr}"), dir)
}

/// A fresh logged-in client against an already-spawned server (one per
/// simulated device).
pub async fn login_on(base: &str) -> zikos_exchange::BlobClient {
    let mut client = zikos_exchange::BlobClient::new(base);
    client
        .login(VALID_TOKEN, SERVER_NAME)
        .await
        .expect("login must succeed against the fake verifier");
    client
}

/// A logged-in client against a fresh default-cap server. (Not every test
/// binary spins a fresh server per client, hence the allow.)
#[allow(dead_code)]
pub async fn logged_in_client() -> (zikos_exchange::BlobClient, PathBuf) {
    let (base, blob_dir) = spawn_server(ServerConfig::default().max_blob_bytes).await;
    (login_on(&base).await, blob_dir)
}

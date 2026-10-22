//! HTTP contract tests for the Zikos audio server.
//! These are the executable version of `specs/audio-storage-server.md`.

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use zikos_server::auth::{AuthError, OpenIdVerifier, SessionRegistry};
use zikos_server::routes::{AppState, ServerConfig, router};
use zikos_server::store::FsBlobStore;

struct FakeVerifier {
    valid_token: &'static str,
    user_id: &'static str,
}

#[async_trait]
impl OpenIdVerifier for FakeVerifier {
    async fn verify(&self, access_token: &str, server_name: &str) -> Result<String, AuthError> {
        if server_name != "matrix.example.org" || access_token != self.valid_token {
            return Err(AuthError::Rejected);
        }
        Ok(self.user_id.to_string())
    }
}

/// Build an app with a temp blob dir that outlives the test.
fn test_app(max_blob_bytes: usize) -> axum::Router {
    let dir = tempfile::tempdir().unwrap().keep(); // keep dir on disk after drop
    let state = AppState {
        verifier: std::sync::Arc::new(FakeVerifier {
            valid_token: "openid-token-1",
            user_id: "@lisa:matrix.example.org",
        }),
        sessions: SessionRegistry::default(),
        blobs: std::sync::Arc::new(FsBlobStore::new(&dir).unwrap()),
        config: ServerConfig { max_blob_bytes },
    };
    router(state)
}

async fn send(app: axum::Router, req: Request<Body>) -> (StatusCode, Option<Value>, Vec<u8>) {
    let mut res = app.oneshot(req).await.unwrap();
    let status = res.status();
    let headers = std::mem::take(res.headers_mut());
    let is_json = headers
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/json"));
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    let json = if is_json {
        Some(serde_json::from_slice(&bytes).unwrap())
    } else {
        None
    };
    (status, json, bytes.to_vec())
}

async fn login(app: &axum::Router) -> String {
    let req = Request::builder()
        .method("POST")
        .uri("/v1/session")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"token":"openid-token-1","server_name":"matrix.example.org"}"#,
        ))
        .unwrap();
    let (status, json, _) = send(app.clone(), req).await;
    assert_eq!(status, StatusCode::CREATED, "login must succeed");
    json.unwrap()["session"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn healthz_returns_ok() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let (status, json, _) = send(
        app,
        Request::builder()
            .uri("/healthz")
            .body(Body::empty())
            .unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(json.unwrap()["status"], "ok");
}

#[tokio::test]
async fn login_with_valid_matrix_openid_mints_a_session() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let session = login(&app).await;
    assert!(!session.is_empty());
}

#[tokio::test]
async fn login_with_a_bad_token_is_unauthorized() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let req = Request::builder()
        .method("POST")
        .uri("/v1/session")
        .header("content-type", "application/json")
        .body(Body::from(
            r#"{"token":"wrong","server_name":"matrix.example.org"}"#,
        ))
        .unwrap();
    let (status, _, _) = send(app, req).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn blob_endpoints_require_a_session() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .body(Body::from(vec![1, 2, 3]))
        .unwrap();
    let (status, _, _) = send(app.clone(), post).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let get = Request::builder()
        .uri("/v1/blobs/00000000-0000-0000-0000-000000000000")
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(app, get).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn upload_then_download_roundtrips_opaque_bytes() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let session = login(&app).await;

    // "Ciphertext" — the server must treat it as opaque bytes.
    let payload: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::from(payload.clone()))
        .unwrap();
    let (status, json, _) = send(app.clone(), post).await;
    assert_eq!(status, StatusCode::CREATED, "upload must succeed: {json:?}");
    let blob_id = json.unwrap()["id"].as_str().unwrap().to_string();

    let get = Request::builder()
        .uri(format!("/v1/blobs/{blob_id}"))
        .header("authorization", format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(app, get).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, payload, "downloaded bytes must match uploaded bytes");
}

#[tokio::test]
async fn oversized_uploads_are_rejected() {
    let app = test_app(8); // absurdly small cap for the test
    let session = login(&app).await;
    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::from(vec![0u8; 64]))
        .unwrap();
    let (status, _, _) = send(app, post).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn unknown_blob_is_not_found() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let session = login(&app).await;
    let get = Request::builder()
        .uri("/v1/blobs/00000000-0000-0000-0000-000000000000")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(app, get).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

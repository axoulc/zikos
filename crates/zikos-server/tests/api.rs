//! HTTP contract tests for the Zikos audio server.
//! These are the executable version of `specs/audio-storage-server.md`.

use async_trait::async_trait;
use axum::body::Body;
use axum::http::{Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::Value;
use tower::ServiceExt;
use zikos_server::access_log::CapturingSink;
use zikos_server::auth::{AuthError, OpenIdVerifier};
use zikos_server::routes::{AppState, ServerConfig, router};
use zikos_server::sessions::InMemorySessionStore;
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

/// App under test with the blob dir and log sink exposed so tests can
/// assert on-disk effects and captured access-log lines.
struct TestApp {
    app: axum::Router,
    blob_dir: std::path::PathBuf,
    sink: std::sync::Arc<CapturingSink>,
}

fn test_app(max_blob_bytes: usize) -> TestApp {
    let dir = tempfile::tempdir().unwrap().keep(); // keep dir on disk after drop
    let sink = std::sync::Arc::new(CapturingSink::default());
    let state = AppState {
        verifier: std::sync::Arc::new(FakeVerifier {
            valid_token: "openid-token-1",
            user_id: "@lisa:matrix.example.org",
        }),
        sessions: std::sync::Arc::new(InMemorySessionStore::default()),
        blobs: std::sync::Arc::new(FsBlobStore::new(&dir).unwrap()),
        config: ServerConfig { max_blob_bytes },
        log_sink: sink.clone(),
    };
    TestApp {
        app: router(state),
        blob_dir: dir,
        sink,
    }
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
        app.app,
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
    let session = login(&app.app).await;
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
    let (status, _, _) = send(app.app, req).await;
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
    let (status, _, _) = send(app.app.clone(), post).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    let get = Request::builder()
        .uri("/v1/blobs/00000000-0000-0000-0000-000000000000")
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(app.app, get).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn upload_then_download_roundtrips_opaque_bytes() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let session = login(&app.app).await;

    // "Ciphertext" — the server must treat it as opaque bytes.
    let payload: Vec<u8> = (0..4096u32).map(|i| (i % 251) as u8).collect();
    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::from(payload.clone()))
        .unwrap();
    let (status, json, _) = send(app.app.clone(), post).await;
    assert_eq!(status, StatusCode::CREATED, "upload must succeed: {json:?}");
    let blob_id = json.unwrap()["id"].as_str().unwrap().to_string();

    let get = Request::builder()
        .uri(format!("/v1/blobs/{blob_id}"))
        .header("authorization", format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(app.app, get).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, payload, "downloaded bytes must match uploaded bytes");
}

#[tokio::test]
async fn oversized_uploads_are_rejected() {
    let app = test_app(8); // absurdly small cap for the test
    let session = login(&app.app).await;
    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::from(vec![0u8; 64]))
        .unwrap();
    let (status, _, _) = send(app.app, post).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn unknown_blob_is_not_found() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let session = login(&app.app).await;
    let get = Request::builder()
        .uri("/v1/blobs/00000000-0000-0000-0000-000000000000")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, _) = send(app.app, get).await;
    assert_eq!(status, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn every_request_yields_one_line_and_x_request_id() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let res = app
        .app
        .oneshot(
            Request::builder()
                .uri("/healthz")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let header_id = res
        .headers()
        .get("x-request-id")
        .expect("response must carry x-request-id")
        .to_str()
        .unwrap()
        .to_string();

    let lines = app.sink.lines();
    assert_eq!(lines.len(), 1, "exactly one access-log line per request");
    let entry: Value = serde_json::from_str(&lines[0]).unwrap();
    assert_eq!(entry["request_id"].as_str(), Some(header_id.as_str()));
    assert_eq!(entry["method"], "GET");
    assert_eq!(entry["path"], "/healthz");
    assert_eq!(entry["status"], 200);
}

#[tokio::test]
async fn log_line_carries_required_fields() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let session = login(&app.app).await;

    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::from(b"some-opaque-bytes".to_vec()))
        .unwrap();
    let _ = send(app.app.clone(), post).await;

    // Unauthenticated request: user_id must be null, not absent or garbage.
    let noauth = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .body(Body::from(b"x".to_vec()))
        .unwrap();
    let _ = send(app.app.clone(), noauth).await;

    let mut blob_entry = None;
    let mut noauth_entry = None;
    for line in app.sink.lines() {
        let entry: Value = serde_json::from_str(&line).unwrap();
        match (entry["path"].as_str(), Some(blob_entry.is_some())) {
            (Some("/v1/blobs"), Some(false)) if entry["status"] == 201 => blob_entry = Some(entry),
            (Some("/v1/blobs"), _) if entry["status"] == 401 => noauth_entry = Some(entry),
            _ => {}
        }
    }
    let entry = blob_entry.expect("blob upload must be logged");
    assert_eq!(entry["method"], "POST");
    assert_eq!(entry["path"], "/v1/blobs");
    assert_eq!(entry["status"], 201);
    assert_eq!(entry["user_id"], "@lisa:matrix.example.org");
    assert!(
        entry["duration_ms"].is_u64(),
        "duration_ms must be a number"
    );
    let ts = entry["ts"].as_str().expect("ts must be a string");
    ts.parse::<jiff::Timestamp>().expect("ts must be RFC 3339");
    uuid::Uuid::parse_str(entry["request_id"].as_str().expect("request_id"))
        .expect("request_id must be a UUID");

    let entry = noauth_entry.expect("unauthenticated request must be logged");
    assert!(entry["user_id"].is_null(), "user_id must be null");
}

#[tokio::test]
async fn secrets_and_blob_bytes_never_logged() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let session = login(&app.app).await; // posts the OpenID token

    // Distinctive marker bytes inside the "ciphertext".
    let marker = "TOP-SECRET-TAKE-BYTES";
    let payload = format!("leading-noise-{marker}-trailing-noise").into_bytes();
    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::from(payload))
        .unwrap();
    let (status, json, _) = send(app.app.clone(), post).await;
    assert_eq!(status, StatusCode::CREATED);
    let blob_id = json.unwrap()["id"].as_str().unwrap().to_string();

    let get = Request::builder()
        .uri(format!("/v1/blobs/{blob_id}"))
        .header("authorization", format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let _ = send(app.app, get).await;

    let all = app.sink.lines().join("\n");
    assert!(
        !all.contains("openid-token-1"),
        "the Matrix OpenID token must never be logged"
    );
    assert!(
        !all.contains(&session),
        "the session bearer token must never be logged"
    );
    assert!(!all.contains(marker), "blob contents must never be logged");
}

#[tokio::test]
async fn chunked_streaming_upload_roundtrips() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let session = login(&app.app).await;

    // 8 KiB delivered in 256-byte chunks, like a live recording upload.
    let payload: Vec<u8> = (0..8192u32).map(|i| (i % 253) as u8).collect();
    let cursor = std::io::Cursor::new(payload.clone());
    let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(cursor, 256));
    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .header("authorization", format!("Bearer {session}"))
        .body(body)
        .unwrap();
    let (status, json, _) = send(app.app.clone(), post).await;
    assert_eq!(status, StatusCode::CREATED, "upload must succeed: {json:?}");
    let json = json.unwrap();
    assert_eq!(json["size"], payload.len() as u64);
    let blob_id = json["id"].as_str().unwrap().to_string();

    let get = Request::builder()
        .uri(format!("/v1/blobs/{blob_id}"))
        .header("authorization", format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let (status, _, body) = send(app.app, get).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, payload, "downloaded bytes must match uploaded bytes");
}

#[tokio::test]
async fn download_sets_octet_stream_and_exact_length() {
    let app = test_app(ServerConfig::default().max_blob_bytes);
    let session = login(&app.app).await;

    let payload: Vec<u8> = b"opaque-ciphertext-bytes".to_vec();
    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .header("authorization", format!("Bearer {session}"))
        .body(Body::from(payload.clone()))
        .unwrap();
    let (_, json, _) = send(app.app.clone(), post).await;
    let blob_id = json.unwrap()["id"].as_str().unwrap().to_string();

    let get = Request::builder()
        .uri(format!("/v1/blobs/{blob_id}"))
        .header("authorization", format!("Bearer {session}"))
        .body(Body::empty())
        .unwrap();
    let res = app.app.oneshot(get).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers().get("content-type").unwrap(),
        &"application/octet-stream"
    );
    assert_eq!(
        res.headers().get("content-length").unwrap(),
        &payload.len().to_string()
    );
    let body = res.into_body().collect().await.unwrap().to_bytes();
    assert_eq!(&body[..], &payload[..]);
}

#[tokio::test]
async fn oversize_streaming_upload_is_413_and_leaves_no_blob() {
    let app = test_app(8); // absurdly small cap for the test
    let session = login(&app.app).await;

    // 64 bytes in 4-byte chunks: the cap is exceeded mid-stream, not up front.
    let cursor = std::io::Cursor::new(vec![9u8; 64]);
    let body = Body::from_stream(tokio_util::io::ReaderStream::with_capacity(cursor, 4));
    let post = Request::builder()
        .method("POST")
        .uri("/v1/blobs")
        .header("authorization", format!("Bearer {session}"))
        .body(body)
        .unwrap();
    let (status, _, _) = send(app.app.clone(), post).await;
    assert_eq!(status, StatusCode::PAYLOAD_TOO_LARGE);

    let blobs: Vec<_> = std::fs::read_dir(&app.blob_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".bin") || n.ends_with(".tmp"))
        .collect();
    assert!(
        blobs.is_empty(),
        "aborted upload must leave nothing: {blobs:?}"
    );
}

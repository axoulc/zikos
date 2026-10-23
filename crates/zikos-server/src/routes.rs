//! HTTP surface of the Zikos audio server.

use axum::body::Body;
use axum::extract::{FromRequestParts, Path, Request, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::Response;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::access_log::{LogSink, access_log};
use crate::auth::OpenIdVerifier;
use crate::error::ApiError;
use crate::sessions::SessionStore;
use crate::store::BlobStore;

#[derive(Clone)]
pub struct ServerConfig {
    /// Hard cap on a single blob upload. Default: 512 MiB — about 50 minutes
    /// of stereo 24-bit/48 kHz FLAC, enough for a rehearsal take. Enforced
    /// incrementally by the streaming store, which counts every chunk.
    pub max_blob_bytes: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            max_blob_bytes: 512 * 1024 * 1024,
        }
    }
}

#[derive(Clone)]
pub struct AppState {
    pub verifier: Arc<dyn OpenIdVerifier>,
    pub sessions: Arc<dyn SessionStore>,
    pub blobs: Arc<dyn BlobStore>,
    pub config: ServerConfig,
    pub log_sink: Arc<dyn LogSink>,
}

pub fn router(state: AppState) -> Router {
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/session", post(create_session))
        .route("/v1/blobs", post(create_blob))
        .route("/v1/blobs/{id}", get(get_blob))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            access_log,
        ))
        .with_state(state)
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health { status: "ok" })
}

#[derive(Deserialize)]
pub struct LoginRequest {
    /// Matrix OpenID access token, freshly requested from the homeserver.
    pub token: String,
    /// The homeserver that minted it (`iss` of the OpenID object).
    pub server_name: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub session: String,
    pub user_id: String,
}

async fn create_session(
    State(state): State<AppState>,
    Json(req): Json<LoginRequest>,
) -> Result<(StatusCode, Json<LoginResponse>), ApiError> {
    let user_id = state.verifier.verify(&req.token, &req.server_name).await?;
    let session = state.sessions.issue(user_id.clone())?;
    Ok((
        StatusCode::CREATED,
        Json(LoginResponse { session, user_id }),
    ))
}

/// Extractor: a request carrying a valid `Authorization: Bearer <session>`.
pub struct AuthedUser {
    pub user_id: String,
}

impl FromRequestParts<AppState> for AuthedUser {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or(ApiError::Unauthorized)?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or(ApiError::Unauthorized)?;
        let user_id = state
            .sessions
            .resolve(token)
            .ok_or(ApiError::Unauthorized)?;
        Ok(AuthedUser { user_id })
    }
}

#[derive(Serialize)]
pub struct BlobCreated {
    pub id: String,
    pub size: u64,
}

async fn create_blob(
    _user: AuthedUser,
    State(state): State<AppState>,
    req: Request,
) -> Result<(StatusCode, Json<BlobCreated>), ApiError> {
    // The body is streamed straight to disk; it is never buffered whole.
    // The store enforces the size cap chunk by chunk (413, nothing stored).
    let id = Uuid::new_v4().to_string();
    let size = state
        .blobs
        .put_stream(&id, req.into_body(), state.config.max_blob_bytes as u64)
        .await?;
    Ok((StatusCode::CREATED, Json(BlobCreated { id, size })))
}

async fn get_blob(
    _user: AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    // Streamed straight off disk; the blob is never buffered whole.
    let (file, len) = state.blobs.open(&id).await?;
    let body = Body::from_stream(tokio_util::io::ReaderStream::new(file));
    Ok(Response::builder()
        .status(StatusCode::OK)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .header(header::CONTENT_LENGTH, len)
        .body(body)
        // Infallible: fixed header values, body is a stream.
        .expect("static response parts are always valid"))
}

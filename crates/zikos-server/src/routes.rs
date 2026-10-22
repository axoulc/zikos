//! HTTP surface of the Zikos audio server.

use axum::body::Bytes;
use axum::extract::{DefaultBodyLimit, FromRequestParts, Path, State};
use axum::http::request::Parts;
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

use crate::auth::{OpenIdVerifier, SessionRegistry};
use crate::error::ApiError;
use crate::store::BlobStore;

#[derive(Clone)]
pub struct ServerConfig {
    /// Hard cap on a single blob upload. Default: 512 MiB — about 50 minutes
    /// of stereo 24-bit/48 kHz FLAC, enough for a rehearsal take.
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
    pub sessions: SessionRegistry,
    pub blobs: Arc<dyn BlobStore>,
    pub config: ServerConfig,
}

pub fn router(state: AppState) -> Router {
    let max = state.config.max_blob_bytes;
    Router::new()
        .route("/healthz", get(health))
        .route("/v1/session", post(create_session))
        .route(
            "/v1/blobs",
            post(create_blob).route_layer(DefaultBodyLimit::max(max)),
        )
        .route("/v1/blobs/{id}", get(get_blob))
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
    let session = state.sessions.issue(user_id.clone());
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
    body: Bytes,
) -> Result<(StatusCode, Json<BlobCreated>), ApiError> {
    if body.len() > state.config.max_blob_bytes {
        return Err(ApiError::PayloadTooLarge);
    }
    let id = Uuid::new_v4().to_string();
    state.blobs.put(&id, &body).await?;
    Ok((
        StatusCode::CREATED,
        Json(BlobCreated {
            id,
            size: body.len() as u64,
        }),
    ))
}

async fn get_blob(
    _user: AuthedUser,
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, ApiError> {
    let bytes = state.blobs.get(&id).await?;
    Ok((
        [(header::CONTENT_TYPE, "application/octet-stream")],
        Bytes::from(bytes),
    )
        .into_response())
}

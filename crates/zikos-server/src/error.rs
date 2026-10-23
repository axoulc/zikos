//! Error type mapped onto HTTP responses.

use axum::Json;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use serde_json::json;

use crate::auth::AuthError;
use crate::sessions::SessionError;
use crate::store::StoreError;

#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    #[error("unauthorized")]
    Unauthorized,
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("not found")]
    NotFound,
    #[error("payload too large")]
    PayloadTooLarge,
    #[error("internal error")]
    Internal,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = match &self {
            ApiError::Unauthorized => StatusCode::UNAUTHORIZED,
            ApiError::BadRequest(_) => StatusCode::BAD_REQUEST,
            ApiError::NotFound => StatusCode::NOT_FOUND,
            ApiError::PayloadTooLarge => StatusCode::PAYLOAD_TOO_LARGE,
            ApiError::Internal => StatusCode::INTERNAL_SERVER_ERROR,
        };
        (status, Json(json!({ "error": self.to_string() }))).into_response()
    }
}

impl From<AuthError> for ApiError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::Rejected | AuthError::MissingCredentials => ApiError::Unauthorized,
            AuthError::InvalidServerName(name) => {
                ApiError::BadRequest(format!("invalid server_name {name:?}"))
            }
            AuthError::Upstream => ApiError::Unauthorized,
        }
    }
}

impl From<SessionError> for ApiError {
    fn from(_: SessionError) -> Self {
        ApiError::Internal
    }
}

impl From<StoreError> for ApiError {
    fn from(e: StoreError) -> Self {
        match e {
            StoreError::NotFound(_) => ApiError::NotFound,
            StoreError::InvalidId(id) => ApiError::BadRequest(format!("invalid blob id {id:?}")),
            StoreError::TooLarge => ApiError::PayloadTooLarge,
            StoreError::Io(_) => ApiError::Internal,
        }
    }
}

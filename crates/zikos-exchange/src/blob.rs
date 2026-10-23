//! HTTP client for the Zikos blob server (`zikos-server` v1 API): session
//! bootstrap, upload, download.

use crate::ExchangeError;
use serde::{Deserialize, Serialize};

/// A successful upload: the server-minted id and the stored size.
#[derive(Debug, Clone, Deserialize)]
pub struct UploadedBlob {
    pub id: String,
    pub size: u64,
}

/// What `login` establishes: the Matrix identity behind the session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobSession {
    pub user_id: String,
}

/// Client for one blob server. Log in once, then upload and download with
/// the stored bearer session.
#[derive(Debug, Clone)]
pub struct BlobClient {
    base_url: String,
    http: reqwest::Client,
    session: Option<String>,
    user_id: Option<String>,
}

impl BlobClient {
    /// A client for the server at `base_url` (e.g. `https://blobs.example.org`).
    /// No network access happens until a call is made.
    pub fn new(base_url: impl Into<String>) -> Self {
        Self {
            base_url: base_url.into(),
            http: reqwest::Client::new(),
            session: None,
            user_id: None,
        }
    }

    /// Exchange a Matrix OpenID token for a blob-server session. The token
    /// itself is requested from the homeserver via matrix-sdk at the app
    /// layer; this call only performs the `/v1/session` exchange.
    pub async fn login(
        &mut self,
        openid_token: &str,
        server_name: &str,
    ) -> Result<BlobSession, ExchangeError> {
        #[derive(Serialize)]
        struct LoginRequest<'a> {
            token: &'a str,
            server_name: &'a str,
        }
        #[derive(Deserialize)]
        struct LoginResponse {
            session: String,
            user_id: String,
        }
        let response = self
            .http
            .post(format!("{}/v1/session", self.base_url))
            .json(&LoginRequest {
                token: openid_token,
                server_name,
            })
            .send()
            .await?;
        let response = check_status(response, None).await?;
        let body: LoginResponse = response.json().await?;
        self.user_id = Some(body.user_id.clone());
        self.session = Some(body.session);
        Ok(BlobSession {
            user_id: body.user_id,
        })
    }

    /// The Matrix user id the current session authenticates as, if logged in.
    pub fn user_id(&self) -> Option<&str> {
        self.user_id.as_deref()
    }

    /// Upload one opaque blob; the server mints and returns its id.
    pub async fn upload(&self, body: Vec<u8>) -> Result<UploadedBlob, ExchangeError> {
        let response = self
            .http
            .post(format!("{}/v1/blobs", self.base_url))
            .bearer_auth(self.bearer()?)
            .header(reqwest::header::CONTENT_TYPE, "application/octet-stream")
            .body(body)
            .send()
            .await?;
        let response = check_status(response, None).await?;
        Ok(response.json().await?)
    }

    /// Download the exact stored bytes of one blob.
    pub async fn download(&self, id: &str) -> Result<Vec<u8>, ExchangeError> {
        let response = self
            .http
            .get(format!("{}/v1/blobs/{id}", self.base_url))
            .bearer_auth(self.bearer()?)
            .send()
            .await?;
        let response = check_status(response, Some(id)).await?;
        Ok(response.bytes().await?.to_vec())
    }

    fn bearer(&self) -> Result<&str, ExchangeError> {
        self.session.as_deref().ok_or(ExchangeError::Unauthorized)
    }
}

/// Pass a 2xx response through; map anything else onto the typed surface.
async fn check_status(
    response: reqwest::Response,
    blob_id: Option<&str>,
) -> Result<reqwest::Response, ExchangeError> {
    if response.status().is_success() {
        return Ok(response);
    }
    Err(match response.status().as_u16() {
        401 | 403 => ExchangeError::Unauthorized,
        404 => ExchangeError::NotFound(blob_id.unwrap_or("unknown").to_owned()),
        413 => ExchangeError::TooLarge,
        status => ExchangeError::Status(status),
    })
}

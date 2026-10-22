//! Authentication: Matrix OpenID tokens exchanged for Zikos session tokens.
//!
//! Flow (no user accounts on this server, ever):
//! 1. The client asks its homeserver for an OpenID token
//!    (`POST /_matrix/client/v3/user/{userId}/openid/request_token`).
//! 2. The client POSTs `{ token, server_name }` to `/v1/session`.
//! 3. We verify the token against the homeserver's federation endpoint and
//!    mint an opaque session token of our own.

use async_trait::async_trait;
use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64URL;
use rand::RngCore;
use serde::Deserialize;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("credentials rejected by homeserver")]
    Rejected,
    #[error("missing credentials")]
    MissingCredentials,
    #[error("invalid server name {0:?}")]
    InvalidServerName(String),
    #[error("homeserver unreachable or returned garbage")]
    Upstream,
}

#[derive(Debug, Deserialize)]
struct OpenIdUserinfo {
    sub: String,
}

/// Anything that can verify a Matrix OpenID token. Real impl talks to the
/// homeserver; tests install a fake.
#[async_trait]
pub trait OpenIdVerifier: Send + Sync {
    /// Returns the verified Matrix user id (`@user:server`).
    async fn verify(&self, access_token: &str, server_name: &str) -> Result<String, AuthError>;
}

pub struct MatrixOpenIdVerifier {
    client: reqwest::Client,
}

impl MatrixOpenIdVerifier {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }
}

impl Default for MatrixOpenIdVerifier {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl OpenIdVerifier for MatrixOpenIdVerifier {
    async fn verify(&self, access_token: &str, server_name: &str) -> Result<String, AuthError> {
        validate_server_name(server_name)?;
        let url = format!(
            "https://{server_name}/_matrix/federation/v1/openid/userinfo?access_token={access_token}"
        );
        let resp = self
            .client
            .get(&url)
            .send()
            .await
            .map_err(|_| AuthError::Upstream)?;
        if !resp.status().is_success() {
            return Err(AuthError::Rejected);
        }
        let claims: OpenIdUserinfo = resp.json().await.map_err(|_| AuthError::Upstream)?;
        if claims.sub.is_empty() || !claims.sub.starts_with('@') {
            return Err(AuthError::Rejected);
        }
        Ok(claims.sub)
    }
}

/// Matrix server names are DNS names (optionally `:port`, optionally IPv6 in
/// brackets). This check is an SSRF guard: it rejects anything carrying a
/// scheme, path, query, or userinfo, and rejects raw IP / numeric hosts
/// (homeservers on bare IPs are not a supported deployment).
pub fn validate_server_name(name: &str) -> Result<(), AuthError> {
    let host = name.split(':').next().unwrap_or_default();
    let host_is_dns = host
        .split('.')
        .any(|label| label.chars().any(|c| c.is_ascii_alphabetic()));
    let valid = !name.is_empty()
        && name.len() <= 253
        && name.split(':').count() <= 2
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | ':' | '[' | ']'))
        && name.contains('.')
        && host_is_dns;
    if valid {
        Ok(())
    } else {
        Err(AuthError::InvalidServerName(name.to_string()))
    }
}

/// Opaque session tokens, minted after OpenID verification.
/// In-memory for now; persistence is specified as a follow-up change.
#[derive(Default, Clone)]
pub struct SessionRegistry {
    sessions: Arc<Mutex<HashMap<String, String>>>, // token -> user_id
}

impl SessionRegistry {
    pub fn issue(&self, user_id: String) -> String {
        let mut bytes = [0u8; 32];
        rand::rng().fill_bytes(&mut bytes);
        let token = B64URL.encode(bytes);
        self.sessions
            .lock()
            .expect("session registry poisoned")
            .insert(token.clone(), user_id);
        token
    }

    pub fn resolve(&self, token: &str) -> Option<String> {
        self.sessions
            .lock()
            .expect("session registry poisoned")
            .get(token)
            .cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn server_name_validation_blocks_ssrf_vectors() {
        assert!(validate_server_name("matrix.example.org").is_ok());
        assert!(validate_server_name("matrix.example.org:8448").is_ok());
        for evil in [
            "",
            "https://matrix.example.org",
            "matrix.example.org/_matrix",
            "matrix.example.org?x=1",
            "user@matrix.example.org",
            "matrix example.org",
            "127.0.0.1",
        ] {
            assert!(
                validate_server_name(evil).is_err(),
                "{evil:?} must be rejected"
            );
        }
    }
}

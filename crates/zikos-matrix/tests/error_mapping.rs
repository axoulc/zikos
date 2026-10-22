//! Spec scenarios: wrong password (`M_FORBIDDEN`) and unreachable
//! homeserver map to distinct errors at the crate boundary.

mod common;

use zikos_matrix::MatrixError;

async fn raw_login(homeserver: &str, dir: &std::path::Path) -> matrix_sdk::Result<()> {
    let client = matrix_sdk::Client::builder()
        .homeserver_url(homeserver)
        .sqlite_store(dir, Some("store-pass"))
        .build()
        .await
        .unwrap();
    client
        .matrix_auth()
        .login_username("alice", "wonderland")
        .initial_device_display_name("zikos-test")
        .await
        .map(|_| ())
}

#[tokio::test]
async fn forbidden_maps_to_invalid_credentials() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_forbidden(&server).await;

    let dir = tempfile::tempdir().unwrap();
    let err: MatrixError = raw_login(&server.uri(), dir.path())
        .await
        .unwrap_err()
        .into();
    assert!(
        matches!(err, MatrixError::InvalidCredentials),
        "M_FORBIDDEN must surface as InvalidCredentials, got {err:?}"
    );
}

#[tokio::test]
async fn unreachable_homeserver_maps_to_connection() {
    // Reserve an ephemeral port, then free it: nothing listens there, so
    // requests fail at the connection layer, deterministically.
    let uri = {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        format!("http://127.0.0.1:{port}")
    };

    let dir = tempfile::tempdir().unwrap();
    let err: MatrixError = raw_login(&uri, dir.path()).await.unwrap_err().into();
    assert!(
        matches!(err, MatrixError::Connection),
        "connect failure must surface as Connection, got {err:?}"
    );
}

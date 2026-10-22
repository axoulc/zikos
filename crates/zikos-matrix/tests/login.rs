//! Spec scenarios: successful login returns the session user; wrong
//! password yields `InvalidCredentials` and persists no session.

mod common;

use zikos_matrix::{Client, MatrixError};

#[tokio::test]
async fn login_success_returns_session_user() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;

    let dir = tempfile::tempdir().unwrap();
    let client = Client::login(
        server.uri().as_str(),
        "alice",
        "wonderland",
        dir.path(),
        "store-pass",
    )
    .await
    .unwrap();

    assert_eq!(client.user_id().as_deref(), Some(common::USER_ID));
}

#[tokio::test]
async fn wrong_password_is_invalid_credentials_and_persists_nothing() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_forbidden(&server).await;

    let dir = tempfile::tempdir().unwrap();
    let err = Client::login(
        server.uri().as_str(),
        "alice",
        "wrong-password",
        dir.path(),
        "store-pass",
    )
    .await
    .unwrap_err();
    assert!(
        matches!(err, MatrixError::InvalidCredentials),
        "got {err:?}"
    );

    // No session may be persisted by a failed login: a fresh raw client
    // on the same store must see none.
    let raw = matrix_sdk::Client::builder()
        .homeserver_url(server.uri())
        .sqlite_store(dir.path(), Some("store-pass"))
        .build()
        .await
        .unwrap();
    assert_eq!(
        raw.matrix_auth().session(),
        None,
        "failed login must not leave a session in the store"
    );
}

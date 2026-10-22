//! Spec scenario: returning user — a persisted session restores without
//! credentials and completes a sync round; wrong passphrase fails with a
//! store error.

mod common;

use zikos_matrix::{Client, MatrixError};

#[tokio::test]
async fn returning_user_restores_without_credentials_and_syncs() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_sync(&server, common::empty_sync("s1")).await;

    let dir = tempfile::tempdir().unwrap();
    {
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

    // App restart: no username/password anywhere.
    let client = Client::restore(server.uri().as_str(), dir.path(), "store-pass")
        .await
        .unwrap();
    assert_eq!(client.user_id().as_deref(), Some(common::USER_ID));
    client.sync_once().await.unwrap();
}

#[tokio::test]
async fn wrong_passphrase_fails_with_store_error() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;

    let dir = tempfile::tempdir().unwrap();
    Client::login(
        server.uri().as_str(),
        "alice",
        "wonderland",
        dir.path(),
        "store-pass",
    )
    .await
    .unwrap();

    let err = Client::restore(server.uri().as_str(), dir.path(), "other-pass")
        .await
        .unwrap_err();
    assert!(matches!(err, MatrixError::Store(_)), "got {err:?}");
}

#[tokio::test]
async fn restore_without_persisted_session_fails_with_store_error() {
    let server = common::spawn().await;
    let dir = tempfile::tempdir().unwrap();

    let err = Client::restore(server.uri().as_str(), dir.path(), "store-pass")
        .await
        .unwrap_err();
    assert!(matches!(err, MatrixError::Store(_)), "got {err:?}");
}

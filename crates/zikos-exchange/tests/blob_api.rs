//! Blob transport contract tests against a live local axum instance —
//! the executable version of the client side of the
//! `audio-storage-server` API.

mod common;

use common::{VALID_TOKEN, spawn_server};
use zikos_exchange::{BlobClient, ExchangeError};
use zikos_server::routes::ServerConfig;

#[tokio::test]
async fn login_mints_a_usable_session() {
    let (base, _blob_dir) = spawn_server(ServerConfig::default().max_blob_bytes).await;
    let mut client = BlobClient::new(&base);
    assert_eq!(client.user_id(), None, "fresh clients hold no session");
    let session = client
        .login(VALID_TOKEN, "matrix.example.org")
        .await
        .unwrap();
    assert_eq!(session.user_id, "@lisa:matrix.example.org");
    assert_eq!(client.user_id(), Some("@lisa:matrix.example.org"));

    // The session immediately authorizes uploads and downloads.
    let uploaded = client.upload(vec![1, 2, 3, 4]).await.unwrap();
    assert_eq!(uploaded.size, 4);
    assert!(!uploaded.id.is_empty());
    assert_eq!(
        client.download(&uploaded.id).await.unwrap(),
        vec![1, 2, 3, 4]
    );
}

#[tokio::test]
async fn bad_login_token_is_typed_unauthorized() {
    let (base, _) = spawn_server(ServerConfig::default().max_blob_bytes).await;
    let mut client = BlobClient::new(&base);
    assert!(matches!(
        client.login("wrong-token", "matrix.example.org").await,
        Err(ExchangeError::Unauthorized)
    ));
}

#[tokio::test]
async fn calls_without_a_session_are_typed_unauthorized() {
    let (base, _) = spawn_server(ServerConfig::default().max_blob_bytes).await;
    let client = BlobClient::new(&base); // never logged in
    assert!(matches!(
        client.upload(vec![9]).await,
        Err(ExchangeError::Unauthorized)
    ));
    assert!(matches!(
        client
            .download("0e2d1a63-9f4e-4a2b-8c3d-4f5e6a7b8c9d")
            .await,
        Err(ExchangeError::Unauthorized)
    ));
}

#[tokio::test]
async fn unknown_blob_is_typed_not_found() {
    let (client, _) = common::logged_in_client().await;
    let id = "0e2d1a63-9f4e-4a2b-8c3d-4f5e6a7b8c9d";
    // reqwest errors are not PartialEq, so the id is asserted separately.
    match client.download(id).await {
        Err(ExchangeError::NotFound(found)) => assert_eq!(found, id),
        other => panic!("expected NotFound({id}), got {other:?}"),
    }
}

#[tokio::test]
async fn over_limit_upload_is_typed_too_large() {
    let (base, _) = spawn_server(4).await;
    let mut client = BlobClient::new(&base);
    client
        .login(VALID_TOKEN, "matrix.example.org")
        .await
        .unwrap();
    assert!(matches!(
        client.upload(vec![0u8; 10]).await,
        Err(ExchangeError::TooLarge)
    ));
}

#[tokio::test]
async fn larger_roundtrip_preserves_exact_bytes() {
    let (client, _) = common::logged_in_client().await;
    // A patterned body well past any single-chunk size, uneven length.
    let body: Vec<u8> = (0..100_003u32).map(|i| (i * 31 % 251) as u8).collect();
    let uploaded = client.upload(body.clone()).await.unwrap();
    assert_eq!(uploaded.size, body.len() as u64);
    assert_eq!(client.download(&uploaded.id).await.unwrap(), body);
}

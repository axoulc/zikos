//! Spec scenario: new conversation — the createRoom request carries the
//! m.room.encryption state event, so the room is encrypted before the
//! first message.

mod common;

use zikos_matrix::{Client, RoomEncryption};

#[tokio::test]
async fn create_room_request_enables_encryption_before_first_message() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_create_room(&server).await;
    common::mount_room_encryption_state(
        &server,
        Some(serde_json::json!({ "algorithm": "m.megolm.v1.aes-sha2" })),
    )
    .await;

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

    let room_id = client.create_encrypted_room("Band chat").await.unwrap();
    assert_eq!(room_id, common::NEW_ROOM_ID);

    // The creation request itself must include m.room.encryption.
    let body = serde_json::from_slice::<serde_json::Value>(
        &server
            .received_requests()
            .await
            .unwrap()
            .into_iter()
            .find(|r| r.url.path().ends_with("/createRoom"))
            .expect("createRoom request must have been sent")
            .body,
    )
    .unwrap();

    let initial = body
        .get("initial_state")
        .and_then(|s| s.as_array())
        .expect("initial_state must be present");
    let has_encryption = initial.iter().any(|event| {
        event.get("type").and_then(|t| t.as_str()) == Some("m.room.encryption")
            && event
                .get("content")
                .and_then(|c| c.get("algorithm"))
                .and_then(|a| a.as_str())
                .is_some()
    });
    assert!(
        has_encryption,
        "createRoom body must enable encryption: {body}"
    );

    // And the room reports encrypted (resolved via the state endpoint).
    assert_eq!(
        client.room_encryption(&room_id).await.unwrap(),
        RoomEncryption::Encrypted
    );
}

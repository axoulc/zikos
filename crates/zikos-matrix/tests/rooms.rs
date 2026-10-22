//! Spec scenario: unencrypted room detected — a joined room without
//! `m.room.encryption` reports `Unencrypted`; one with it reports
//! `Encrypted`.

mod common;

use zikos_matrix::{Client, RoomEncryption};

const ROOM: &str = "!room:example.org";

async fn logged_in_and_synced(
    server: &wiremock::MockServer,
    dir: &tempfile::TempDir,
    sync_body: serde_json::Value,
) -> Client {
    common::mount_sync(server, sync_body).await;
    let client = Client::login(
        server.uri().as_str(),
        "alice",
        "wonderland",
        dir.path(),
        "store-pass",
    )
    .await
    .unwrap();
    client.sync_once().await.unwrap();
    client
}

#[tokio::test]
async fn room_without_encryption_event_reports_unencrypted() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;

    let dir = tempfile::tempdir().unwrap();
    // A joined room with a message but no m.room.encryption state.
    common::mount_room_encryption_state(&server, None).await;
    let client = logged_in_and_synced(
        &server,
        &dir,
        common::sync_with_message("s1", ROOM, "$e1", "@bob:example.org", "hi", 1),
    )
    .await;

    assert_eq!(
        client.room_encryption(ROOM).await.unwrap(),
        RoomEncryption::Unencrypted
    );
}

#[tokio::test]
async fn room_with_encryption_event_reports_encrypted() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;

    let dir = tempfile::tempdir().unwrap();
    common::mount_room_encryption_state(
        &server,
        Some(serde_json::json!({
            "algorithm": "m.megolm.v1.aes-sha2",
        })),
    )
    .await;
    let client = logged_in_and_synced(
        &server,
        &dir,
        common::sync_with_room_state("s1", ROOM, common::encryption_state_event()),
    )
    .await;

    assert_eq!(
        client.room_encryption(ROOM).await.unwrap(),
        RoomEncryption::Encrypted
    );
}

//! Spec scenarios: a synced room message is exposed with sender, room,
//! timestamp, body; an undecryptable message is marked, not dropped.

mod common;

use std::time::Duration;

use zikos_matrix::{Client, MessageBody};

async fn logged_in(server: &wiremock::MockServer, dir: &tempfile::TempDir) -> Client {
    Client::login(
        server.uri().as_str(),
        "alice",
        "wonderland",
        dir.path(),
        "store-pass",
    )
    .await
    .unwrap()
}

async fn next_message(
    rx: &mut tokio::sync::broadcast::Receiver<zikos_matrix::MessageEvent>,
) -> zikos_matrix::MessageEvent {
    tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("message must arrive within timeout")
        .expect("channel must stay open")
}

#[tokio::test]
async fn received_message_carries_sender_room_timestamp_and_body() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_sync(
        &server,
        common::sync_with_message(
            "s1",
            "!room:example.org",
            "$e1",
            "@bob:example.org",
            "hello zikos",
            1_700_000_000_000,
        ),
    )
    .await;

    let dir = tempfile::tempdir().unwrap();
    let client = logged_in(&server, &dir).await;
    let mut rx = client.messages();
    client.sync_once().await.unwrap();

    let msg = next_message(&mut rx).await;
    assert_eq!(msg.room_id, "!room:example.org");
    assert_eq!(msg.event_id, "$e1");
    assert_eq!(msg.sender, "@bob:example.org");
    assert_eq!(msg.timestamp, 1_700_000_000_000);
    assert_eq!(msg.body, MessageBody::Text("hello zikos".to_owned()));
}

#[tokio::test]
async fn undecryptable_message_is_marked_not_dropped() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_sync(
        &server,
        common::sync_with_encrypted_message(
            "s1",
            "!room:example.org",
            "$e2",
            "@bob:example.org",
            1_700_000_000_001,
        ),
    )
    .await;

    let dir = tempfile::tempdir().unwrap();
    let client = logged_in(&server, &dir).await;
    let mut rx = client.messages();
    client.sync_once().await.unwrap();

    let msg = next_message(&mut rx).await;
    assert_eq!(msg.event_id, "$e2");
    assert_eq!(msg.body, MessageBody::Undecryptable);
}

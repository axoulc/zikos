//! Spec scenario: restart restores history — timeline messages
//! accumulate across sync rounds and survive a client restart without
//! user action.

mod common;

use std::time::Duration;

use zikos_matrix::{Client, MessageBody};

const ROOM: &str = "!room:example.org";

#[tokio::test]
async fn history_accumulates_across_sync_rounds_and_survives_restart() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_sync_chain(
        &server,
        vec![
            common::sync_with_message("s1", ROOM, "$e1", "@bob:example.org", "first", 1),
            common::sync_with_message("s2", ROOM, "$e2", "@bob:example.org", "second", 2),
            common::sync_with_message("s3", ROOM, "$e3", "@bob:example.org", "third", 3),
        ],
    )
    .await;

    let dir = tempfile::tempdir().unwrap();

    // Session 1: two sync rounds accumulate.
    let client = Client::login(
        server.uri().as_str(),
        "alice",
        "wonderland",
        dir.path(),
        "store-pass",
    )
    .await
    .unwrap();
    let mut rx = client.messages();

    client.sync_once().await.unwrap();
    let first = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.body, MessageBody::Text("first".to_owned()));

    client.sync_once().await.unwrap();
    let second = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.body, MessageBody::Text("second".to_owned()));

    let history = client.room_messages(ROOM);
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].event_id, "$e1");
    assert_eq!(history[1].event_id, "$e2");

    // Restart: history is back on the new client without user action.
    drop(client);
    let client = Client::restore(server.uri().as_str(), dir.path(), "store-pass")
        .await
        .unwrap();
    let history = client.room_messages(ROOM);
    assert_eq!(
        history
            .iter()
            .map(|m| m.event_id.as_str())
            .collect::<Vec<_>>(),
        vec!["$e1", "$e2"],
        "persisted history must survive restart, in order"
    );

    // And the restored client keeps accumulating on the next round,
    // without duplicating what it already has.
    let mut rx = client.messages();
    client.sync_once().await.unwrap();
    let third = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(third.event_id, "$e3");
    let history = client.room_messages(ROOM);
    assert_eq!(history.len(), 3);
}

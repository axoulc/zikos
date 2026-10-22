//! Spec scenarios for the send path: successful send goes `sending` →
//! `sent`; an offline send survives an app restart and is transmitted
//! when connectivity returns; a failed send is retried.

mod common;

use std::time::Duration;

use zikos_matrix::{Client, DeliveryState};

const ROOM: &str = "!room:example.org";

async fn logged_in_and_synced(server: &wiremock::MockServer, dir: &tempfile::TempDir) -> Client {
    common::mount_versions(server).await;
    common::mount_login_ok(server, common::USER_ID).await;
    common::mount_keys(server).await;
    // The send path resolves the room's encryption state before sending;
    // resolve it to plaintext so no olm machinery is needed in tests.
    common::mount_room_encryption_state(server, None).await;
    common::mount_sync(
        server,
        common::sync_with_message("s1", ROOM, "$seed", "@bob:example.org", "seed", 1),
    )
    .await;

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

/// Receive delivery events for `txn` until one matches `want`, failing
/// after `budget`.
async fn until_state(
    deliveries: &mut tokio::sync::broadcast::Receiver<zikos_matrix::DeliveryEvent>,
    txn: &str,
    want: &DeliveryState,
    budget: Duration,
) {
    let deadline = tokio::time::Instant::now() + budget;
    loop {
        let event = tokio::time::timeout_at(deadline, deliveries.recv())
            .await
            .expect("timed out waiting for delivery event")
            .expect("delivery channel closed");
        if event.transaction_id == txn && &event.state == want {
            return;
        }
    }
}

#[tokio::test]
async fn successful_send_goes_sending_then_sent() {
    let server = common::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let client = logged_in_and_synced(&server, &dir).await;
    common::mount_send_event(&server).await;

    let mut deliveries = client.deliveries();
    let txn = client.send_text(ROOM, "hello zikos").await.unwrap();

    until_state(
        &mut deliveries,
        &txn,
        &DeliveryState::Sending,
        Duration::from_secs(10),
    )
    .await;
    until_state(
        &mut deliveries,
        &txn,
        &DeliveryState::Sent {
            event_id: "$sent".to_owned(),
        },
        Duration::from_secs(10),
    )
    .await;

    // The wire carried our exact text.
    let send_body = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.method == "PUT" && r.url.path().contains("/send/"))
        .expect("send request must have been made");
    let body: serde_json::Value = serde_json::from_slice(&send_body.body).unwrap();
    assert_eq!(body["body"], "hello zikos");
    assert_eq!(body["msgtype"], "m.text");
}

#[tokio::test]
async fn offline_send_survives_restart_and_transmits() {
    // Server 1: everything works EXCEPT sending (unmatched PUT → 404,
    // an unrecoverable error that wedges the message in the queue).
    let server1 = common::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let client = logged_in_and_synced(&server1, &dir).await;

    let mut deliveries = client.deliveries();
    let txn = client.send_text(ROOM, "offline message").await.unwrap();
    until_state(
        &mut deliveries,
        &txn,
        &DeliveryState::Failed,
        Duration::from_secs(10),
    )
    .await;
    drop(client);

    // Server 2: connectivity is back (a working send endpoint).
    let server2 = common::spawn().await;
    common::mount_send_event(&server2).await;

    let client = Client::restore(server2.uri().as_str(), dir.path(), "store-pass")
        .await
        .unwrap();
    let mut deliveries = client.deliveries();
    until_state(
        &mut deliveries,
        &txn,
        &DeliveryState::Sent {
            event_id: "$sent".to_owned(),
        },
        Duration::from_secs(10),
    )
    .await;
}

#[tokio::test]
async fn failed_send_is_retried_when_connectivity_returns() {
    let server = common::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let client = logged_in_and_synced(&server, &dir).await;

    // The send endpoint starts DOWN.
    let send = common::mount_flipping_send(&server).await;

    let mut deliveries = client.deliveries();
    let txn = client.send_text(ROOM, "will make it").await.unwrap();
    until_state(
        &mut deliveries,
        &txn,
        &DeliveryState::Failed,
        Duration::from_secs(30),
    )
    .await;

    // Connectivity returns: a successful sync re-enables the queue and
    // the message goes out.
    send.restore();
    client.sync_once().await.unwrap();
    until_state(
        &mut deliveries,
        &txn,
        &DeliveryState::Sent {
            event_id: "$sent".to_owned(),
        },
        Duration::from_secs(30),
    )
    .await;
}

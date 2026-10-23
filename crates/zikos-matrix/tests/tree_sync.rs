//! Spec scenarios for tree events over Matrix: ops travel as encrypted
//! room events of a custom type, through the durable send queue with
//! delivery states (spec: file-tree — tree events over Matrix).

mod common;

use std::time::Duration;

use zikos_core::tree::{NodeKind, TreeOp};
use zikos_core::tree_sync::TreeOpContent;
use zikos_matrix::{Client, DeliveryState};

const ROOM: &str = "!room:example.org";

async fn logged_in_and_synced(server: &wiremock::MockServer, dir: &tempfile::TempDir) -> Client {
    common::mount_versions(server).await;
    common::mount_login_ok(server, common::USER_ID).await;
    common::mount_keys(server).await;
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
async fn tree_op_sends_as_custom_event_with_delivery_states() {
    let server = common::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    let client = logged_in_and_synced(&server, &dir).await;
    common::mount_send_event(&server).await;

    let folder = uuid::Uuid::new_v4();
    let content = TreeOpContent {
        op: TreeOp::Create {
            id: folder,
            parent: zikos_core::tree::ROOT_ID,
            kind: NodeKind::Folder,
            name: "Takes".into(),
            file: None,
        },
        base: None,
        known_children: None,
    };

    let mut deliveries = client.deliveries();
    let txn = client.send_tree_op(ROOM, &content).await.unwrap();
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

    // The wire carried our custom event type and the exact envelope.
    let request = server
        .received_requests()
        .await
        .unwrap()
        .into_iter()
        .find(|r| r.method == "PUT" && r.url.path().contains("/send/zikos.tree.op/"))
        .expect("custom-type send request must have been made");
    assert!(
        request.url.path().contains("/send/zikos.tree.op/"),
        "the send path carries the custom event type"
    );
    let body: serde_json::Value = serde_json::from_slice(&request.body).unwrap();
    assert_eq!(body["op"]["op"], "create");
    assert_eq!(body["op"]["name"], "Takes");
    assert_eq!(body["op"]["id"], folder.to_string());
}

#[tokio::test]
async fn sync_delivers_tree_ops_with_room_envelope() {
    let server = common::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_room_encryption_state(&server, None).await;

    let folder = uuid::Uuid::new_v4();
    let content = serde_json::json!({
        "op": {
            "op": "create",
            "id": folder.to_string(),
            "parent": zikos_core::tree::ROOT_ID.to_string(),
            "kind": "folder",
            "name": "Takes",
        },
    });
    common::mount_sync(
        &server,
        common::sync_with_tree_op(
            "s1",
            ROOM,
            "$tree1",
            "@bob:example.org",
            1_776_000_000_000,
            content,
        ),
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
    let mut tree_events = client.tree_events();
    client.sync_once().await.unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), tree_events.recv())
        .await
        .expect("tree event must arrive")
        .expect("tree channel closed");
    assert_eq!(event.room_id, ROOM);
    // Envelope fields come from the room event, not the content.
    assert_eq!(event.event.author, "@bob:example.org");
    assert_eq!(event.event.origin_server_ts, 1_776_000_000_000);
    assert_eq!(event.event.event_id, "$tree1");
    assert_eq!(
        event.event.op,
        TreeOp::Create {
            id: folder,
            parent: zikos_core::tree::ROOT_ID,
            kind: NodeKind::Folder,
            name: "Takes".into(),
            file: None,
        }
    );
}

#[tokio::test]
async fn undecryptable_events_produce_no_tree_events() {
    let server = common::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_room_encryption_state(&server, None).await;
    common::mount_sync(
        &server,
        common::sync_with_encrypted_message("s1", ROOM, "$enc1", "@bob:example.org", 42),
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
    let mut tree_events = client.tree_events();
    client.sync_once().await.unwrap();

    // An event that stays encrypted never becomes a tree op.
    assert!(
        tokio::time::timeout(Duration::from_millis(300), tree_events.recv())
            .await
            .is_err(),
        "no tree event may surface for an undecryptable event"
    );
}

#[tokio::test]
async fn room_tree_replays_synced_ops_and_restores_after_restart() {
    let server = common::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_room_encryption_state(&server, None).await;

    let takes = uuid::Uuid::new_v4();
    let file = uuid::Uuid::new_v4();
    let root = zikos_core::tree::ROOT_ID.to_string();
    let events = vec![
        serde_json::json!({
            "type": "zikos.tree.op", "event_id": "$t1",
            "sender": "@bob:example.org", "origin_server_ts": 100,
            "content": {"op": {"op": "create", "id": takes.to_string(),
                               "parent": root, "kind": "folder", "name": "Takes"}},
        }),
        serde_json::json!({
            "type": "zikos.tree.op", "event_id": "$t2",
            "sender": "@bob:example.org", "origin_server_ts": 200,
            "content": {"op": {"op": "create", "id": file.to_string(),
                               "parent": takes.to_string(), "kind": "file",
                               "name": "take-1.flac"}},
        }),
    ];
    common::mount_sync(
        &server,
        common::sync_with_timeline_events("s1", ROOM, events),
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

    let room_tree = client.room_tree(ROOM);
    assert_eq!(
        room_tree.tree.find_by_path("Takes/take-1.flac").unwrap(),
        Some(file)
    );
    assert!(room_tree.pending_nodes.is_empty());

    // Restart on the same store: the tree rebuilds from the persisted
    // log without user action (spec scenario: restart restores the
    // tree).
    drop(client);
    let server2 = common::spawn().await;
    common::mount_versions(&server2).await;
    common::mount_login_ok(&server2, common::USER_ID).await;
    common::mount_keys(&server2).await;
    common::mount_room_encryption_state(&server2, None).await;
    common::mount_sync(&server2, common::empty_sync("s1")).await;
    let client = Client::restore(server2.uri().as_str(), dir.path(), "store-pass")
        .await
        .unwrap();
    assert_eq!(client.room_tree(ROOM).tree, room_tree.tree);
}

#[tokio::test]
async fn offline_tree_op_appears_pending_until_room_echo() {
    let server = common::spawn().await;
    let dir = tempfile::tempdir().unwrap();
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_room_encryption_state(&server, None).await;

    // Round 1 joins the room; round 2 (after connectivity returns)
    // delivers the echo of our own op, authored by us.
    let folder = uuid::Uuid::new_v4();
    let root = zikos_core::tree::ROOT_ID.to_string();
    let echo = serde_json::json!({
        "op": {"op": "create", "id": folder.to_string(),
               "parent": root, "kind": "folder", "name": "Takes"},
    });
    common::mount_sync_chain(
        &server,
        vec![
            common::sync_with_message("s1", ROOM, "$seed", "@bob:example.org", "seed", 1),
            common::sync_with_tree_op("s2", ROOM, "$echo", common::USER_ID, 42, echo),
        ],
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

    // Offline: the send endpoint is down, so the op stays queued.
    let send = common::mount_flipping_send(&server).await;
    let content = TreeOpContent {
        op: TreeOp::Create {
            id: folder,
            parent: zikos_core::tree::ROOT_ID,
            kind: NodeKind::Folder,
            name: "Takes".into(),
            file: None,
        },
        base: None,
        known_children: None,
    };
    let _txn = client.send_tree_op(ROOM, &content).await.unwrap();

    // The folder appears immediately, marked pending (spec scenario:
    // offline folder creation).
    let pending_view = client.room_tree(ROOM);
    assert_eq!(
        pending_view.tree.find_by_path("Takes").unwrap(),
        Some(folder),
        "pending op applies optimistically"
    );
    assert!(pending_view.pending_nodes.contains(&folder));
    assert!(
        !pending_view.committed.tree.nodes.contains_key(&folder),
        "not committed until the room echo"
    );

    // Connectivity returns: the queued event is sent, sync delivers our
    // own echo, the pending mark clears.
    send.restore();
    client.sync_once().await.unwrap();
    let committed_view = client.room_tree(ROOM);
    assert_eq!(
        committed_view.tree.find_by_path("Takes").unwrap(),
        Some(folder)
    );
    assert!(committed_view.pending_nodes.is_empty());
    assert!(committed_view.committed.tree.nodes.contains_key(&folder));
    assert_eq!(
        committed_view.committed.tree, pending_view.tree,
        "the committed tree catches up to what the user already saw"
    );
}

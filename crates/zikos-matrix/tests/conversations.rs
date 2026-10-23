//! Spec scenario (platform-app, conversations screen): the client lists
//! the account's conversations with display name, encryption state, and
//! latest activity — from synced state and the local message log, so it
//! works offline.

mod common;

use serde_json::{Value, json};
use zikos_matrix::{Client, RoomEncryption};

const JAZZ: &str = "!jazz:example.org";
const ADMIN: &str = "!admin:example.org";

/// A sync response with several joined rooms, each carrying its own state
/// events and timeline.
fn sync_two_rooms(next_batch: &str) -> Value {
    let jazz = json!({
        "timeline": {
            "limited": false,
            "prev_batch": "p0",
            "events": [{
                "type": "m.room.message",
                "event_id": "$e1",
                "sender": "@bob:example.org",
                "origin_server_ts": 1_792_000_000_000u64,
                "content": {"msgtype": "m.text", "body": "take 3 is the keeper"},
            }],
        },
        "state": {"events": [
            {"type": "m.room.name", "state_key": "", "event_id": "$s1",
             "sender": "@alice:example.org", "origin_server_ts": 1_791_000_000_000u64,
             "content": {"name": "Jazz Quartet"}},
            {"type": "m.room.encryption", "state_key": "", "event_id": "$s1e",
             "sender": "@alice:example.org", "origin_server_ts": 1_791_000_000_000u64,
             "content": {"algorithm": "m.megolm.v1.aes-sha2"}},
        ]},
        "account_data": {"events": []},
        "ephemeral": {"events": []},
        "summary": {},
        "unread_notifications": {},
    });
    let admin = json!({
        "timeline": {"limited": false, "prev_batch": "p0", "events": []},
        "state": {"events": [
            {"type": "m.room.name", "state_key": "", "event_id": "$s2",
             "sender": "@alice:example.org", "origin_server_ts": 1_791_000_000_000u64,
             "content": {"name": "Band Admin"}},
        ]},
        "account_data": {"events": []},
        "ephemeral": {"events": []},
        "summary": {},
        "unread_notifications": {},
    });
    json!({
        "next_batch": next_batch,
        "rooms": {"join": {JAZZ: jazz, ADMIN: admin}, "invite": {}, "leave": {}},
        "presence": {"events": []},
        "account_data": {"events": []},
        "to_device": {"events": []},
        "device_lists": {"changed": [], "left": []},
        "device_one_time_keys_count": {},
    })
}

async fn logged_in_and_synced(server: &wiremock::MockServer, dir: &tempfile::TempDir) -> Client {
    common::mount_sync(server, sync_two_rooms("s1")).await;
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
async fn conversations_list_names_encryption_and_last_activity() {
    let server = common::spawn().await;
    common::mount_versions(&server).await;
    common::mount_login_ok(&server, common::USER_ID).await;
    common::mount_keys(&server).await;
    common::mount_room_encryption_state(&server, None).await;

    let dir = tempfile::tempdir().unwrap();
    let client = logged_in_and_synced(&server, &dir).await;

    let conversations = client.conversations().await.unwrap();
    assert_eq!(conversations.len(), 2, "both joined rooms are listed");

    let by_id = |id: &str| {
        conversations
            .iter()
            .find(|c| c.room_id == id)
            .unwrap_or_else(|| panic!("room {id} missing from listing"))
    };

    let jazz = by_id(JAZZ);
    assert_eq!(jazz.display_name, "Jazz Quartet");
    assert_eq!(jazz.encryption, RoomEncryption::Encrypted);
    let activity = jazz.last_activity.as_ref().unwrap();
    assert_eq!(activity.sender, "@bob:example.org");
    assert_eq!(activity.preview, "take 3 is the keeper");
    assert_eq!(activity.timestamp_ms, 1_792_000_000_000);

    let admin = by_id(ADMIN);
    assert_eq!(admin.display_name, "Band Admin");
    assert_eq!(admin.encryption, RoomEncryption::Unencrypted);
    assert!(
        admin.last_activity.is_none(),
        "a room without messages has no last activity"
    );
}

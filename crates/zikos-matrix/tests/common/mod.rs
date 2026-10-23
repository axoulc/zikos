//! Mock Matrix homeserver for zikos-matrix tests.
//!
//! Every client-facing behavior is tested against a wiremock server
//! answering the client-server API endpoints this crate touches, with
//! hand-written deterministic JSON fixtures. No live homeserver.

// Each integration-test binary links this module but uses only a subset
// of the fixtures, so unused ones are expected per-binary.
#![allow(dead_code)]

use serde_json::{Value, json};
use wiremock::Mock;
use wiremock::MockServer;
use wiremock::ResponseTemplate;
use wiremock::matchers::{method, path, path_regex, query_param, query_param_is_missing};

/// The user every login fixture authenticates as.
pub const USER_ID: &str = "@alice:example.org";
/// The room every createRoom fixture returns.
pub const NEW_ROOM_ID: &str = "!newroom:example.org";

/// Spawn an empty mock homeserver on a random local port.
pub async fn spawn() -> MockServer {
    MockServer::start().await
}

/// Mount `GET /versions`, which the SDK probes before login.
pub async fn mount_versions(server: &MockServer) {
    Mock::given(method("GET"))
        .and(path("/_matrix/client/versions"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "versions": ["r0.6.1", "v1.1", "v1.2", "v1.3", "v1.4", "v1.5", "v1.6", "v1.7", "v1.8", "v1.9", "v1.10", "v1.11"],
            "unstable_features": {},
        })))
        .mount(server)
        .await;
}

/// Mount `POST /login` accepting the password flow for `user_id`.
pub async fn mount_login_ok(server: &MockServer, user_id: &str) {
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/login"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "user_id": user_id,
            "access_token": "t0k3n",
            "device_id": "ZIKOSTEST",
        })))
        .mount(server)
        .await;
}

/// Mount `POST /login` rejecting the attempt with `M_FORBIDDEN`.
pub async fn mount_login_forbidden(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/login"))
        .respond_with(ResponseTemplate::new(403).set_body_json(json!({
            "errcode": "M_FORBIDDEN",
            "error": "Invalid password",
        })))
        .mount(server)
        .await;
}

/// Mount the key-management endpoints the E2EE machinery probes around
/// sync: upload, query, claim. Minimal empty-payload responses.
pub async fn mount_keys(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/keys/upload"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "one_time_key_counts": {},
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/keys/query"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "device_keys": {},
        })))
        .mount(server)
        .await;
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/keys/claim"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "one_time_keys": {},
        })))
        .mount(server)
        .await;
}

/// Mount `GET /sync` always answering with `body`.
pub async fn mount_sync(server: &MockServer, body: Value) {
    Mock::given(method("GET"))
        .and(path("/_matrix/client/v3/sync"))
        .respond_with(ResponseTemplate::new(200).set_body_json(body))
        .mount(server)
        .await;
}

/// Mount a sequence of `/sync` responses: the first request (no `since`
// param, initial sync) gets `rounds[0]`, then each next request gets the
/// round whose `next_batch` the previous one returned.
pub async fn mount_sync_chain(server: &MockServer, rounds: Vec<Value>) {
    assert!(rounds.len() >= 2, "a chain needs at least two rounds");
    for (i, round) in rounds.iter().enumerate() {
        let since = rounds
            .get(i.wrapping_sub(1))
            .and_then(|prev| prev.get("next_batch"))
            .and_then(|b| b.as_str())
            .map(|s| s.to_owned());
        let mut mock = Mock::given(method("GET")).and(path("/_matrix/client/v3/sync"));
        mock = match since {
            Some(token) => mock.and(query_param("since", &token)),
            None => mock.and(query_param_is_missing("since")),
        };
        mock.respond_with(ResponseTemplate::new(200).set_body_json(round.clone()))
            .mount(server)
            .await;
    }
}

/// Mount `GET /rooms/{id}/state/m.room.encryption` answering with the
/// given content (`Some`) or `M_NOT_FOUND` (`None`, room not encrypted).
pub async fn mount_room_encryption_state(server: &MockServer, content: Option<Value>) {
    let response = match content {
        Some(content) => ResponseTemplate::new(200).set_body_json(content),
        None => ResponseTemplate::new(404).set_body_json(json!({
            "errcode": "M_NOT_FOUND",
            "error": "Event not found.",
        })),
    };
    Mock::given(method("GET"))
        .and(path_regex(
            r"^/_matrix/client/v3/rooms/[^/]+/state/m\.room\.encryption/?$",
        ))
        .respond_with(response)
        .mount(server)
        .await;
}

/// Mount `PUT /rooms/{id}/send/{type}/{txn}` always succeeding.
pub async fn mount_send_event(server: &MockServer) {
    Mock::given(method("PUT"))
        .and(path_regex(
            r"^/_matrix/client/v3/rooms/[^/]+/send/[^/]+/[^/]+$",
        ))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "event_id": "$sent",
        })))
        .mount(server)
        .await;
}

/// A send endpoint that fails with 500 until connectivity is restored
/// through the handle — no mock-precedence games, one deterministic
/// endpoint.
#[derive(Debug, Clone)]
pub struct FlippingSend {
    down: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl FlippingSend {
    /// Bring the endpoint back up.
    pub fn restore(&self) {
        self.down.store(false, std::sync::atomic::Ordering::Relaxed);
    }
}

struct FlippingSendResponder {
    down: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl std::fmt::Debug for FlippingSendResponder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FlippingSendResponder").finish()
    }
}

impl wiremock::Respond for FlippingSendResponder {
    fn respond(&self, _request: &wiremock::Request) -> ResponseTemplate {
        if self.down.load(std::sync::atomic::Ordering::Relaxed) {
            ResponseTemplate::new(500).set_body_json(json!({
                "errcode": "M_UNKNOWN",
                "error": "temporarily unavailable",
            }))
        } else {
            ResponseTemplate::new(200).set_body_json(json!({
                "event_id": "$sent",
            }))
        }
    }
}

/// Mount the send endpoint starting DOWN (500); flip it up with
/// [`FlippingSend::restore`].
pub async fn mount_flipping_send(server: &MockServer) -> FlippingSend {
    let down = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    Mock::given(method("PUT"))
        .and(path_regex(
            r"^/_matrix/client/v3/rooms/[^/]+/send/[^/]+/[^/]+$",
        ))
        .respond_with(FlippingSendResponder { down: down.clone() })
        .mount(server)
        .await;
    FlippingSend { down }
}

/// Mount `POST /createRoom` succeeding with [`NEW_ROOM_ID`].
pub async fn mount_create_room(server: &MockServer) {
    Mock::given(method("POST"))
        .and(path("/_matrix/client/v3/createRoom"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!({
            "room_id": NEW_ROOM_ID,
        })))
        .mount(server)
        .await;
}

/// A `/sync` response with no rooms and no messages.
pub fn empty_sync(next_batch: &str) -> Value {
    sync_body(next_batch, json!({}))
}

/// A `/sync` response where `room_id` is joined and its timeline holds one
/// text message from `sender`.
pub fn sync_with_message(
    next_batch: &str,
    room_id: &str,
    event_id: &str,
    sender: &str,
    body: &str,
    ts: u64,
) -> Value {
    sync_body(
        next_batch,
        json!({
            room_id: {
                "timeline": {
                    "limited": false,
                    "prev_batch": "p0",
                    "events": [{
                        "type": "m.room.message",
                        "event_id": event_id,
                        "sender": sender,
                        "origin_server_ts": ts,
                        "content": {"msgtype": "m.text", "body": body},
                    }],
                },
                "state": {"events": []},
                "account_data": {"events": []},
                "ephemeral": {"events": []},
                "summary": {},
                "unread_notifications": {},
            },
        }),
    )
}

/// A `/sync` response where `room_id` is joined and carries one state
/// event in its state section (e.g. `m.room.encryption`).
pub fn sync_with_room_state(next_batch: &str, room_id: &str, state_event: Value) -> Value {
    sync_body(
        next_batch,
        json!({
            room_id: {
                "timeline": {
                    "limited": false,
                    "prev_batch": "p0",
                    "events": [],
                },
                "state": {"events": [state_event]},
                "account_data": {"events": []},
                "ephemeral": {"events": []},
                "summary": {},
                "unread_notifications": {},
            },
        }),
    )
}

/// A `/sync` response where `room_id` is joined and its timeline holds one
/// `m.room.encrypted` event whose keys this client does not have.
pub fn sync_with_encrypted_message(
    next_batch: &str,
    room_id: &str,
    event_id: &str,
    sender: &str,
    ts: u64,
) -> Value {
    sync_body(
        next_batch,
        json!({
            room_id: {
                "timeline": {
                    "limited": false,
                    "prev_batch": "p0",
                    "events": [{
                        "type": "m.room.encrypted",
                        "event_id": event_id,
                        "sender": sender,
                        "origin_server_ts": ts,
                        "content": {
                            "algorithm": "m.megolm.v1.aes-sha2",
                            "ciphertext": "not-a-real-ciphertext",
                            "sender_key": "unknown-device-key",
                            "device_id": "UNKNOWNDEVICE",
                            "session_id": "sess1",
                        },
                    }],
                },
                "state": {"events": []},
                "account_data": {"events": []},
                "ephemeral": {"events": []},
                "summary": {},
                "unread_notifications": {},
            },
        }),
    )
}

/// A `/sync` response where `room_id` is joined and its timeline holds
/// one `zikos.tree.op` event with the given content envelope.
pub fn sync_with_tree_op(
    next_batch: &str,
    room_id: &str,
    event_id: &str,
    sender: &str,
    ts: u64,
    content: Value,
) -> Value {
    sync_body(
        next_batch,
        json!({
            room_id: {
                "timeline": {
                    "limited": false,
                    "prev_batch": "p0",
                    "events": [{
                        "type": "zikos.tree.op",
                        "event_id": event_id,
                        "sender": sender,
                        "origin_server_ts": ts,
                        "content": content,
                    }],
                },
                "state": {"events": []},
                "account_data": {"events": []},
                "ephemeral": {"events": []},
                "summary": {},
                "unread_notifications": {},
            },
        }),
    )
}

/// A `/sync` response where `room_id` is joined and its timeline holds
/// the given events verbatim (each already carrying type/event_id/...).
pub fn sync_with_timeline_events(next_batch: &str, room_id: &str, events: Vec<Value>) -> Value {
    sync_body(
        next_batch,
        json!({
            room_id: {
                "timeline": {
                    "limited": false,
                    "prev_batch": "p0",
                    "events": events,
                },
                "state": {"events": []},
                "account_data": {"events": []},
                "ephemeral": {"events": []},
                "summary": {},
                "unread_notifications": {},
            },
        }),
    )
}

/// The `m.room.encryption` state event fixture.
pub fn encryption_state_event() -> Value {
    json!({
        "type": "m.room.encryption",
        "state_key": "",
        "sender": "@alice:example.org",
        "content": {"algorithm": "m.megolm.v1.aes-sha2"},
    })
}

fn sync_body(next_batch: &str, join_rooms: Value) -> Value {
    json!({
        "next_batch": next_batch,
        "rooms": {"join": join_rooms, "invite": {}, "leave": {}},
        "presence": {"events": []},
        "account_data": {"events": []},
        "to_device": {"events": []},
        "device_lists": {"changed": [], "left": []},
        "device_one_time_keys_count": {},
    })
}

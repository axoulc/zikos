# Design: implement-file-exchange

## Context

See `proposal.md` for motivation. Today: `zikos-crypto` provides whole-blob
AES-256-GCM (`FileKey`, `EncryptedBlob`), `zikos-server` exposes
`POST /v1/session`, `POST /v1/blobs`, `GET /v1/blobs/{id}` (streaming,
bearer-authenticated), and `zikos-core`/`zikos-matrix` carry an
event-sourced tree over Megolm-encrypted `zikos.tree.op` events — but
`TreeOp::Create` has no notion of a blob, so nothing links a take on one
device to audio on another. Constraints: `zikos-core` stays I/O-free;
Rust-first with pure-Rust deps; Matrix send path (`send_tree_op`) already
transports arbitrary `TreeOpContent` and must not change.

## Goals / Non-Goals

**Goals:**

- One publish call: finalized take → encrypted blob on the server + a
  file-create event that makes it downloadable by any room member.
- One fetch call: file node → decrypted audio + metadata, no user key
- management.
- Event-format change is additive: pre-payload logs and clients keep working.

**Non-Goals:** (beyond the proposal's) server-side changes; re-uploads for
conflict copies; client-side size-cap pre-checks (the server is the
enforcement point; the client surfaces 413 as a typed error).

## Decisions

**D1 — New crate `crates/zikos-exchange` for the client side.**
Alternatives rejected: `zikos-core` (forbidden I/O), `zikos-matrix` (it is
homeserver transport; the blob server is a separate service with separate
auth — pulling reqwest and blob semantics in muddies it), the Flutter app
(business logic must not live in Dart). `zikos-exchange` depends on
`zikos-core` + `zikos-crypto` + `reqwest` (rustls, already a workspace dep —
pure-Rust TLS keeps the crypto policy).

**D2 — File payload on the create op and the node.**
`TreeOp::Create` gains `file: Option<FilePayload>` and `Node` gains
`file: Option<FilePayload>` with `#[serde(default)]`:

```rust
pub struct FilePayload {
    pub blob_id: String,        // server-minted UUID, opaque to clients
    pub key: FileKey,           // zikos-crypto, serde as base64
    pub meta: RecordingMeta,
}
```

`zikos-core` takes a dependency on `zikos-crypto` (pure, I/O-free — allowed)
so the key is typed end-to-end rather than a string; alternative (base64
`String` in core, converted at the edge) rejected as stringly-typed domain
data. Payload on a folder create is a new `TreeError` variant. Legacy events
replay unchanged (`Option` default); old clients ignore the unknown `file`
field (serde struct variants skip unknown fields), degrading to
non-downloadable file nodes in mixed-version rooms — acceptable until
upgrade.

**D3 — "Wrapped key" = room E2EE, no extra wrap layer.**
The base64 `FileKey` rides inside the `zikos.tree.op` content, which only
ever travels inside the room's Megolm envelope — that envelope *is* the wrap
(`specs/security.md`: key distributed "in the same logical unit as the tree
event"). A separate key-encryption-key hierarchy would only pay off for
rotation/re-sharing, an explicit non-goal.

**D4 — Stable blob frame: `nonce ‖ ciphertext`.**
The server stores bodies verbatim, so the layout is a pure client contract:
12-byte nonce followed by AES-GCM ciphertext. `zikos-crypto` gains
`EncryptedBlob::{to_wire, from_wire}` plus `Serialize`/`Deserialize` for
`FileKey` (base64 form). JSON framing (`{nonce_b64, ciphertext_b64}`) was
rejected: ~33% size inflation for zero benefit on an opaque body.
`from_wire` fails closed on frames shorter than a nonce.

**D5 — `BlobClient` + `ExchangeService` (zikos-exchange).**
`BlobClient` wraps one server: `login(openid_token, server_name)` → bearer
session (the app layer obtains the OpenID token from matrix-sdk; exchange
owns the /v1/session dance), `upload(wire_bytes) -> {id, size}`,
`download(id) -> bytes`, mapping 401/404/413/transport to a typed
`thiserror` error. `ExchangeService::publish(audio, meta, parent, name)`
validates meta, mints the `NodeId` + fresh `FileKey`, encrypts, uploads,
and returns the ready-to-send `TreeOpContent`; `fetch(&FilePayload)`
downloads and decrypts, returning `(audio, meta)`. Ordering guarantee:
upload succeeds *before* the event is sendable, so a referenced blob always
exists (barring later server data loss — fetch fails typed).

**D6 — Conflict copies share the blob.**
`tree_sync`'s `snapshot_subtree` and `orphan_create` build fresh creates;
they must copy the original's `FilePayload` verbatim. Copies reference the
same blob and key — no re-upload, and a preserved loser stays downloadable.

**D7 — E2E test over a live local axum instance.**
`zikos-exchange/tests/e2e.rs` (dev-deps: `zikos-server`, `zikos-audio`):
router with `FakeVerifier` + `InMemorySessionStore` + `FsBlobStore`
(tempdir), served by `axum::serve` on `127.0.0.1:0` so the real reqwest
stack is exercised. Device A finalizes a real take (tiny WAV →
`StagedSession::finalize` → FLAC + meta), publishes, and serializes the
event content to JSON. Device B is a fresh client with its own login: it
deserializes the JSON, replays the op into a tree, fetches, and decrypts.
Asserts: byte-identical FLAC, equal meta, on-disk blob bytes contain no
plaintext/title, and tampered-blob/wrong-key/unknown-id paths fail typed.

## Risks / Trade-offs

- [In-memory whole-blob encryption until the tus follow-up] → accepted for
  ≤100 MB per `specs/security.md`; interim cap documented.
- [Persisted `TreeLog` from older clients deserialized by new code] →
  covered by a legacy-replay unit test; additive serde defaults.
- [Blob orphaned if upload succeeds but the event is never sent] → visible
  as a tree node that never appears; GC is a non-goal, noted for a future
  change.
- [`zikos-core` now depends on `zikos-crypto`] → crypto stays pure and
  tiny; keeps one `FileKey` type across the stack.

## Migration Plan

Additive only: no server deploy, no data migration. Rollback = revert; new
events replay on old clients as payload-less file nodes.

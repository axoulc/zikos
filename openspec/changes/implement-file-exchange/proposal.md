# Proposal: implement-file-exchange

## Why

The building blocks exist in isolation — whole-blob AES-256-GCM in
`zikos-crypto`, a hardened blob server, and tree events synced over Matrix —
but nothing connects them: a finalized take never leaves the device. The core
product loop (record → share → open on another device) is incomplete.

## What Changes

- **Tree event format**: `TreeOp::Create` for files gains an optional file
  payload — blob id, `FileKey`, `RecordingMeta` — so a file node references
  its blob and carries its key inside the encrypted room event
  (backward-compatible: optional, defaulted on deserialize).
- **Conflict copies retain blob references**: `Conflicts/` snapshots of file
  nodes keep the payload, so a preserved loser stays downloadable.
- **New client crate `zikos-exchange`**: blob-server HTTP client (Matrix
  OpenID session, streaming upload/download) plus the publish/fetch
  orchestration — encrypt under a fresh per-file key, upload, return the
  file-create event; fetch by blob id, verify, decrypt.
- **`zikos-crypto`**: stable wire framing for `EncryptedBlob`
  (`nonce ‖ ciphertext`) and base64 serde for `FileKey`, so any device can
  decode what another device uploaded.
- **End-to-end test** over a live local axum instance: finalized take →
  publish → event content crosses the "wire" as JSON → another device's
  client fetches and decrypts byte-identical audio; the stored blob is
  ciphertext with no names or metadata.

## Capabilities

### New Capabilities

(none)

### Modified Capabilities

- `file-tree`: adds the blob-referencing requirements deferred by
  `sync-tree-over-matrix` — file nodes reference blobs (id + wrapped key +
  encrypted meta), publish and download-on-another-device flows, conflict
  copies staying downloadable. Derived from `specs/file-tree.md`
  ("file nodes reference blobs", "operations", "download on another device")
  and `specs/security.md` ("file encryption at rest", "key distribution").

## Impact

- `crates/zikos-core` (`tree.rs`, `tree_sync.rs`): payload on creates/nodes,
  conflict-copy propagation; new dep on `zikos-crypto`.
- `crates/zikos-crypto`: wire framing + `FileKey` serde.
- `crates/zikos-exchange` (new): reqwest client + orchestration; dev-deps on
  `zikos-server` and `zikos-audio` for the e2e test.
- No server behavior change; no Dart/UI change; Matrix send path unchanged
  (`send_tree_op` already carries arbitrary `TreeOpContent`).

## Non-goals

- Chunked/resumable (tus) uploads for >100 MB takes — tracked by
  `specs/security.md` as a follow-up.
- Blob deletion/garbage collection; key re-wrapping or rotation.
- Flutter UI wiring and offline publish queueing beyond the existing
  tree-op send queue.

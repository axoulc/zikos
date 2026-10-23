# Tasks: implement-file-exchange

Strict TDD per `AGENTS.md`: each task writes the failing test first, then
the implementation. Gate per task: new + existing tests pass,
`cargo clippy --workspace --all-targets -- -D warnings` clean,
`cargo fmt --all` applied.

## 1. Crypto wire format (`zikos-crypto`)

- [x] 1.1 Test: `FileKey` serializes to/from its base64 string form and
  rejects malformed input (unit tests next to the type). Implement
  `Serialize`/`Deserialize` on `FileKey` via the existing base64 helpers.
- [x] 1.2 Test: `to_wire`/`from_wire` roundtrip an `EncryptedBlob`;
  wire length is `12 + ciphertext.len()`; frames shorter than 12 bytes (or
  nonce-only) fail closed with a typed error. Implement the
  `nonce ‖ ciphertext` frame helpers on `EncryptedBlob` (design D4).

## 2. File payload in the tree model (`zikos-core`)

- [x] 2.1 Test: a `TreeOp::Create` for a file with a `FilePayload`
  (`blob_id`, `FileKey`, `RecordingMeta`) applies and the resulting `Node`
  carries it; the op round-trips through JSON; a legacy create without the
  field deserializes and replays identically (old logs keep working).
  Implement `FilePayload`, the optional `file` field on `TreeOp::Create`
  and `Node` (serde default), and the `zikos-crypto` dependency in core.
- [x] 2.2 Test: a folder create carrying a file payload is rejected with a
  new typed `TreeError` variant and does not corrupt the tree. Implement
  the validation in `FileTree::apply`.

## 3. Conflict copies keep blob references (`zikos-core/tree_sync`)

- [x] 3.1 Test: in a concurrent-rename race and an orphaned create, the
  copies materialized under `Conflicts/` carry the original's
  `FilePayload` verbatim (same blob id, same key). Fix
  `snapshot_subtree` and `orphan_create` to propagate the payload
  (design D6).

## 4. Blob transport (`zikos-exchange`, new crate)

- [x] 4.1 Scaffold `crates/zikos-exchange` (workspace member, deps:
  `zikos-core`, `zikos-crypto`, `reqwest` rustls; dev-deps:
  `zikos-server`, `zikos-audio`, `tokio`, `axum`, `tower`, `http-body-util`,
  `tempfile`) with a `thiserror` `ExchangeError` covering auth, not-found,
  too-large, transport, crypto, and malformed-blob cases.
- [x] 4.2 Test (integration over a spawned local axum router —
  `FakeVerifier` + `InMemorySessionStore` + `FsBlobStore` in a tempdir,
  `axum::serve` on `127.0.0.1:0`): `login` mints a bearer session (401 on
  a bad token maps to the typed auth error); `upload` returns `{id, size}`
  and the stored bytes match; `download` returns the exact bytes; unknown
  id → typed not-found; an over-cap upload → typed too-large. Implement
  `BlobClient` (`login`/`upload`/`download`) against the v1 API.

## 5. Publish / fetch orchestration (`zikos-exchange`)

- [x] 5.1 Test: `publish(audio, meta, parent, name)` validates `meta`
  first (invalid meta → typed error, zero blobs on disk), uploads
  `to_wire(encrypt_blob(fresh_key, audio))`, and returns a file-create
  `TreeOpContent` whose payload key decrypts the uploaded blob; on-disk
  blob bytes contain no audio plaintext and no title substring
  (honest-but-curious). Implement `ExchangeService::publish` (design D5).
- [x] 5.2 Test: `fetch(&FilePayload)` returns audio byte-identical to the
  publish input plus the same meta; tampered wire bytes and a wrong key
  fail with the typed crypto error (no plaintext); a payload whose blob id
  is absent from the server fails with the typed not-found error.
  Implement `ExchangeService::fetch`.

## 6. End-to-end over a local axum instance

- [x] 6.1 Test (`tests/e2e.rs`): device A finalizes a real take with
  `zikos-audio` (tiny generated WAV → `StagedSession::finalize`), publishes
  it, and serializes the create content to JSON; device B — a fresh
  `BlobClient` with its own login — deserializes the JSON, replays the op
  into a `FileTree`, fetches, and decrypts. Assert byte-identical FLAC
  bytes, identical `RecordingMeta`, a tree node exposing the blob id, and
  ciphertext-only storage (no plaintext, no file name, no meta in the
  stored blob). Covers the spec scenarios "finalized take published",
  "take arrives on another device", and "honest-but-curious server".

## 7. Repo hygiene

- [x] 7.1 Run the full gate (`cargo t`, `cargo lint`, `cargo fmtcheck`),
  add the `CHANGELOG.md` entry under `## [Unreleased] / Added`, and commit
  (`feat: file exchange — encrypted upload, tree reference, cross-device
  fetch`).

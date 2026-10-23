# Spec Delta: file-tree (file exchange — blobs, keys, and metadata in the tree)

Derived from `specs/file-tree.md` (source of truth — "file nodes reference
blobs", "operations", the "download on another device" scenario) and
`specs/security.md` ("file encryption at rest", "key distribution"), scoped
to this change: blob-referencing file nodes, the publish flow, fetch/decrypt
on another device, and conflict copies staying downloadable. Chunked/
resumable uploads remain deferred (`specs/security.md` follow-up).

## ADDED Requirements

### Requirement: file nodes reference blobs
A file node SHALL reference its blob by server id plus the per-file key,
encoded base64 inside the create event's content, and SHALL carry its
`RecordingMeta` in the same event. The file key and `RecordingMeta` SHALL
travel only inside the end-to-end encrypted room event — never to the blob
server. A create with kind folder SHALL NOT carry a file payload; violations
SHALL be rejected with a typed error and SHALL NOT corrupt the tree. Creates
authored before this change (no file payload) SHALL keep replaying
unchanged, yielding file nodes without a blob reference.

#### Scenario: file create carries its payload
- **WHEN** a create op for a file carries `{blob id, file key, RecordingMeta}`
- **THEN** replaying it yields a file node exposing that blob id, key, and
  metadata

#### Scenario: payload on a folder create is rejected
- **WHEN** a create op with kind folder also carries a file payload
- **THEN** the op is rejected with a typed error and the tree built from the
  remaining log is unaffected

#### Scenario: legacy events replay unchanged
- **WHEN** an event log authored before file payloads existed is replayed
- **THEN** it yields the same tree as before, with file nodes carrying no
  blob reference

### Requirement: publish flow
Publishing a finalized take SHALL validate its `RecordingMeta`, encrypt the
audio under a fresh per-file 256-bit key with AES-256-GCM and a fresh nonce,
upload the ciphertext to the blob server over the authenticated API as a
stable frame (`nonce ‖ ciphertext`) decodable by any device, and return a
file-create op referencing the blob id and carrying the key and metadata.
Plaintext audio, file names, and metadata SHALL NOT be sent to the blob
server. An upload that fails (authentication, size limit, transport) SHALL
surface a typed error and produce no tree event.

#### Scenario: finalized take published
- **WHEN** a member publishes a finalized take into a folder
- **THEN** the blob server stores one opaque blob and the returned create op
  references its blob id with the key and `RecordingMeta` inside the event
  content

#### Scenario: honest-but-curious server
- **WHEN** the blob server operator inspects the stored bytes
- **THEN** they see AES-GCM ciphertext only — no audio plaintext, no file
  name, no metadata beyond an opaque blob id and session usage

#### Scenario: failed upload produces no event
- **WHEN** the blob server rejects the upload (e.g. over the size limit or
  unauthorized)
- **THEN** publishing fails with a typed error and no file-create op is
  produced

### Requirement: download and decrypt on another device
Opening a file node on a device that did not author it SHALL download the
referenced blob over the authenticated API and decrypt it with the key
distributed in the room, without user key management. A wrong key, tampered
ciphertext, or unknown blob id SHALL fail closed with a typed error and
emit no plaintext.

#### Scenario: take arrives on another device
- **WHEN** a member opens a published file node on a new device
- **THEN** the client fetches the referenced blob, decrypts it with the
  room-distributed key, and obtains audio byte-identical to the author's
  finalized take, with identical metadata

#### Scenario: tampered blob fails closed
- **WHEN** the stored blob has been modified and the client decrypts it
- **THEN** decryption fails with a typed integrity error and no plaintext is
  emitted

#### Scenario: unknown blob id fails closed
- **WHEN** a file node references a blob id the server does not have
- **THEN** fetching fails with a typed not-found error

### Requirement: conflict copies keep blob references
Copies of file nodes preserved under `Conflicts/` SHALL retain the original's
blob id, key, and `RecordingMeta`, so a losing state remains downloadable.

#### Scenario: losing rename copy stays downloadable
- **WHEN** a concurrent rename loses and the file's prior state is preserved
  under `Conflicts/`
- **THEN** the preserved copy references the same blob and key, and fetching
  it returns the same audio as the original node

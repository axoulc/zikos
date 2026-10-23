# file-tree Specification

## Purpose

Each conversation carries a shared, Drive-like folder tree that lives as an
event log inside the end-to-end encrypted Matrix room: every client sends
tree operations as encrypted room events and replays room history into the
same deterministic tree, converging identically even after concurrent or
offline edits.

## Requirements

### Requirement: event-sourced tree
The tree SHALL be the deterministic product of replaying an ordered log of
`TreeOp` events (`create`, `rename`, `move`, `delete`). Applying the same
log SHALL always yield an identical tree on every device.

#### Scenario: replay convergence
- **WHEN** two clients replay the same 50-op log
- **THEN** both compute identical trees (same nodes, names, parents)

### Requirement: tree operations
The client SHALL support create folder, create file node, rename, move, and
delete (folder delete cascades). Invalid operations — unknown ids,
non-folder parents, names containing `/`, moves creating cycles — SHALL be
rejected with typed errors and SHALL NOT corrupt the tree.

#### Scenario: invalid operation is rejected
- **WHEN** a log contains a rename targeting an unknown node id
- **THEN** the rename is rejected with a typed unknown-node error and the
  tree built from the remaining log is unaffected

### Requirement: tree events over Matrix
Tree ops SHALL be sent to the conversation's encrypted room as events of a
custom type (`zikos.tree.op`) whose content is the operation plus conflict
metadata. Applying received events SHALL be idempotent under re-delivery.
Mutations made while offline SHALL queue durably, appear immediately in the
local tree marked `pending`, and be transmitted automatically when
connectivity returns; once the room echo of a queued op is observed, its
pending mark SHALL be cleared.

#### Scenario: offline folder creation
- **WHEN** a user creates a folder while offline
- **THEN** the folder appears immediately in the local tree with a pending
  state and the event is sent when the connection returns

#### Scenario: re-delivery is idempotent
- **WHEN** the same tree event is delivered twice (e.g. by two sync rounds)
- **THEN** it is applied once and the tree is unchanged by the duplicate

### Requirement: tree history replay
Every tree event received through sync SHALL be appended to a persistent,
deduplicated per-room log stored encrypted at rest. The room's tree SHALL
be rebuilt by replaying that log on every start, without user action.

#### Scenario: restart restores the tree
- **WHEN** the app restarts with a persisted session and tree-event log
- **THEN** the room's tree is rebuilt from the log without user action

#### Scenario: fresh device converges
- **WHEN** a member's new device receives the room's tree-event history
  through sync
- **THEN** replaying it yields the same tree as the other members' devices

### Requirement: deterministic conflict convergence
Concurrent edits to the same node (rename vs rename, move vs delete)
SHALL converge deterministically on all devices by last-writer-wins,
ordered by origin server timestamp with event id as the tiebreak. A
conflict SHALL never lose data silently: the losing state SHALL be
preserved as a copy under a `Conflicts/` folder in the tree rather than
vanish. Replay SHALL be independent of the order in which events arrive.

#### Scenario: concurrent rename
- **WHEN** two devices rename the same node offline and both sync
- **THEN** all devices end with the same final name (the rename with the
  greater origin server timestamp, event id breaking ties) and the losing
  name preserved under `Conflicts/`

#### Scenario: move versus delete
- **WHEN** one device moves a node while another concurrently deletes it
- **THEN** all devices converge on the same outcome and the losing state
  is preserved under `Conflicts/`

#### Scenario: arrival order does not matter
- **WHEN** the same set of tree events is delivered to two devices in
  different orders
- **THEN** both devices compute identical trees, including `Conflicts/`
  contents

#### Scenario: create versus delete of parent
- **WHEN** a node was created under a folder that a concurrent delete
  removes, and the create loses
- **THEN** the created node is preserved under `Conflicts/` rather than
  vanishing

#### Scenario: late operation on a deleted node
- **WHEN** an event targets a node a concurrent delete already removed
- **THEN** the event is dropped from the tree deterministically and
  reported as lost rather than silently ignored

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

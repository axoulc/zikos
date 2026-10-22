# File tree (Drive-like, Matrix-synced) — source spec

## Purpose

Each conversation carries a shared folder tree — the musicians' drive. The
tree lives entirely inside the (end-to-end encrypted) Matrix room as an event
log; every client replays the log into the same tree. Audio blobs themselves
sit on the Zikos blob server, referenced by id.

Status: the deterministic model + event log + replay + path resolution are
implemented in `crates/zikos-core/src/tree.rs` with tests. This file remains
the contract.

## Requirements

### Requirement: event-sourced tree
The tree SHALL be the deterministic product of replaying an ordered log of
`TreeOp` events (`create`, `rename`, `move`, `delete`). Applying the same log
SHALL always yield an identical tree on every device.

#### Scenario: replay convergence
- **WHEN** two clients replay the same 50-op log
- **THEN** both compute identical trees (same nodes, names, parents)

### Requirement: operations
The client SHALL support create folder, upload file (create node referencing
a blob id), rename, move, and delete (folder delete cascades). Invalid
operations — unknown ids, non-folder parents, names containing `/`, moves
creating cycles — SHALL be rejected with typed errors and SHALL NOT corrupt
the tree.

### Requirement: file nodes reference blobs
A file node SHALL reference its blob by server id plus a wrapped
`FileKey` (see `security.md`). The node SHALL carry `RecordingMeta` as
encrypted content.

#### Scenario: download on another device
- **WHEN** a member opens a file node on a new device
- **THEN** the client fetches the referenced blob and decrypts it with the
  key distributed in the room, without user key management

### Requirement: tree events over Matrix
Tree ops SHALL be sent as encrypted room events (custom event type) and SHALL
be idempotent to re-delivery. Offline mutations SHALL queue locally and sync
when connectivity returns.

#### Scenario: offline folder creation
- **WHEN** a user creates a folder while offline
- **THEN** the folder appears immediately (pending state) and the event is
  sent when the connection returns

### Requirement: conflicts
Concurrent edits (rename vs rename, move vs delete) SHALL converge
deterministically on all devices (e.g. last-writer-wins by event origin
server timestamp + event id tiebreak). A conflict SHALL never lose data
silently: losing nodes move to a `Conflicts/` folder rather than vanish.

#### Scenario: concurrent rename
- **WHEN** two devices rename the same node offline and both sync
- **THEN** all devices end with the same final name and the losing name
  preserved under `Conflicts/`

### Requirement: browsing UX
The tree browser SHALL list folder contents sorted by name, show file
duration/format, allow in-app playback, and reach any recording within three
taps from the main screen.

## Non-goals (for now)

- Partial sync / sparse trees; sharing subtrees with other rooms; quotas.

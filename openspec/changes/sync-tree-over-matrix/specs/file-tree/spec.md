# Spec Delta: file-tree (new capability — sync and conflicts)

Derived from `specs/file-tree.md` (source of truth), scoped to this change:
the event-sourced model, tree events over Matrix, history replay, and
conflict convergence. Blob payloads (`FileKey`, `RecordingMeta`) in tree
events and the browsing UX are deferred to their own changes.

## Purpose

Each conversation carries a shared, Drive-like folder tree that lives as an
event log inside the end-to-end encrypted Matrix room: every client sends
tree operations as encrypted room events and replays room history into the
same deterministic tree, converging identically even after concurrent or
offline edits.

## ADDED Requirements

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

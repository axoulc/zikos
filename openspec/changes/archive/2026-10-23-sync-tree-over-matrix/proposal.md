# Proposal: sync-tree-over-matrix

Source spec: `specs/file-tree.md` (source of truth) — tree sync and
conflicts slice only.

## Why

`zikos-core` has the deterministic `FileTree` and its `TreeOp` log, but the
log never leaves the device: there is no way for a conversation to actually
share a tree. This change makes the room the transport — ops travel as
encrypted custom events, history replays into the tree on every client, and
concurrent edits converge deterministically instead of corrupting or
diverging.

## What Changes

- New `zikos-core` pure sync engine (`tree_sync`): the op-event envelope
  (op + author + origin server ts + event id + conflict metadata),
  deduplication by event id, canonical ordering, lenient replay with
  pending/orphan handling, and LWW conflict resolution — loser states
  preserved under a deterministic `Conflicts/` folder.
- `zikos-matrix` gains a tree module: custom event type `zikos.tree.op`
  sent through the SDK's durable send queue (offline queue + retry for
  free), a sync handler feeding an encrypted, deduplicated tree-event log
  persisted like the message log, and a per-room API exposing the replayed
  tree, pending (not-yet-sent) local ops, and conflict records.
- Conflict rules proven by property tests (`proptest`, new pure-Rust
  workspace dev-dependency).

## Capabilities

### New Capabilities

- `file-tree`: seeds the capability per `specs/file-tree.md`, scoped to
  what this change makes true: event-sourced tree, tree operations,
  tree events over Matrix (custom type, idempotent, offline queue with
  pending states), replay of room history into the tree, and
  deterministic LWW conflict convergence with `Conflicts/` preservation.

### Modified Capabilities

(none — `file-tree` is new to `openspec/specs/`; existing capabilities are
untouched)

## Impact

- Code: new module in `crates/zikos-core` (no I/O, proptest-friendly);
  new module + public API in `crates/zikos-matrix` (gains a `zikos-core`
  dependency). `TreeOp` itself is unchanged.
- Dependencies: `proptest` (dev-only) and `uuid` v5 feature
  (deterministic conflict-copy ids).
- Testing: unit + proptest in `zikos-core`; wiremock-harness tests in
  `zikos-matrix` for send/receive/replay/offline. Real Megolm round-trips
  are not exercisable against the mock harness (same limitation as phase 1
  messaging).

## Non-goals

- Blob upload/download and `FileKey`/`RecordingMeta` payloads in tree
  events (Phase 5 `implement-file-exchange` per `specs/security.md`).
- Browsing UX (Flutter UI phase). Partial sync/sparse trees, subtree
  sharing, quotas (source-spec non-goals).

# Tasks: sync-tree-over-matrix

Every task follows strict TDD: write the failing test named after "Test:",
watch it fail, then implement minimally. Gate before ticking any task:
`cargo t`, `cargo lint`, `cargo fmtcheck` all clean.

## 1. Workspace setup and core envelope

- [x] 1.1 Add `proptest` (dev-only) to workspace dependencies and enable
  the `v5` feature on `uuid`. Test: `uuid_v5_ids_are_deterministic()` in
  `zikos-core` proves the same (namespace, name) pair yields the same id
  across calls (fails until the feature resolves).
- [x] 1.2 New module `zikos-core::tree_sync`: `TreeOpEvent { op, author,
  origin_server_ts, event_id, base, known_children }` plus content
  serde (op + base + known_children only, lenient: unknown fields ignored).
  Test: content JSON round-trips; envelope fields excluded from content;
  unknown fields tolerated (spec: wire format, design D2).
- [x] 1.3 Dedup by event id and canonical order by (origin_server_ts,
  event_id). Test: duplicate event ids collapse to one; an unsorted
  event vec canonicalizes to the expected total order (design D3).

## 2. Core replay engine — lenient application

- [x] 2.1 Replay applies the canonical log onto `FileTree` using only its
  public API. Test: a clean 50-op log replays to the same tree as
  sequential application (spec scenario: replay convergence).
- [x] 2.2 Pending-and-retry for unknown references: a create whose parent
  appears later in canonical order (same-ms event-id inversion) still
  lands. Test: same-ts inverted create-chain (folder created after the
  file inside it in canonical order) converges to the correctly nested
  tree (design D4.2).
- [x] 2.3 Terminal unsatisfiable ops: a create whose parent never appears
  orphans the node under `Conflicts/`; rename/move/delete on a never-seen
  node is reported lost. Test: both cases produce the `Conflicts/` copy /
  `ReplayReport::lost` entries and never fail the whole replay (spec
  scenarios: create vs delete of parent; late operation on a deleted node).

## 3. Core conflict resolution — LWW with Conflicts/

- [x] 3.1 `Conflicts/` scaffold: well-known folder id, created on first
  conflict only, deterministic loser-copy ids (uuid v5 over losing event
  id). Test: conflicting replays produce byte-identical trees across two
  replay invocations; no `Conflicts/` node when no conflict occurred.
- [x] 3.2 Rename vs rename via `base`: mismatch against current state →
  snapshot the loser state into `Conflicts/`, then the later op (by
  (ts, event id)) applies; matching base → clean sequential apply, no
  copy. Test: spec scenario "concurrent rename" (winner + loser name
  under `Conflicts/`) and a sequential-rename control case with no
  `Conflicts/` entry.
- [x] 3.3 Move vs delete: racing delete snapshots the moved subtree into
  `Conflicts/` before deleting (deletion dominates); late ops on a deleted
  node are dropped and reported. Test: spec scenario "move versus delete"
  converges with the loser preserved; late rename after delete lands in
  `ReplayReport::lost`.
- [x] 3.4 Delete vs concurrent create-under via `known_children`: children
  beyond the claimed set are preserved under `Conflicts/` (with their
  subtrees) instead of cascaded. Test: delete of a folder with one known
  and one concurrently-created child removes only the known one; the extra
  survives under `Conflicts/`.

## 4. Core property tests (proptest)

- [x] 4.1 Permutation invariance: for generated event sets, every arrival
  permutation replays to an identical tree including `Conflicts/` contents
  and copy ids (spec scenario: arrival order does not matter).
- [x] 4.2 Re-delivery idempotence: replay(events + events again) ==
  replay(events) at the dedup layer (spec scenario: re-delivery is
  idempotent).
- [x] 4.3 Rename-race winner rule: for generated racing rename pairs with
  a shared base, the winner is exactly `max((ts, event_id))` and the loser
  name is under `Conflicts/`.
- [x] 4.4 No-silent-loss invariant: for generated event sets, every
  created node is in the final tree, or under `Conflicts/`, or removed by
  a delete that could observe it per its `known_children` claim (design
  D7 property 4).

## 5. Matrix — custom event send path

- [x] 5.1 Define the `zikos.tree.op` event content (ruma event macro,
  message-like) wrapping `tree_sync` content; serialize to
  `Raw<AnyMessageLikeEventContent>`. Test: content serializes to the exact
  envelope JSON and the event type string is `zikos.tree.op` (design D2).
- [x] 5.2 `Client::send_tree_op(room_id, op, base, known_children)` via
  `RoomSendQueue::send_raw`, reusing the delivery-state flow. Test:
  harness (unencrypted fixture room) captures the `/send` body and asserts
  type + content; delivery goes `sending` → `sent` (spec requirement:
  tree events over Matrix).

## 6. Matrix — receive path and persisted log

- [x] 6.1 Sync handler for the custom type → envelope merge (sender, ts,
  event id from the room event). Test: harness `/sync` fixture delivering
  `zikos.tree.op` events decodes them with correct envelope fields;
  undecryptable-encrypted fixtures produce no tree event (surfaced by the
  message layer instead).
- [x] 6.2 Encrypted per-room `TreeLog` (same `Cipher` + atomic-write
  pattern as `MessageLog`), dedup by event id, reload on restore. Test:
  record → reopen same store path → log intact; duplicate event id not
  re-appended; wrong passphrase → `Store` error (spec requirement: tree
  history replay).

## 7. Matrix — replayed tree and pending overlay

- [x] 7.1 `Client::room_tree(room_id)` returning the replayed
  `FileTree` + conflict records + lost ops from the committed log.
  Test: sync N fixture ops → `room_tree` equals the expected tree;
  after client restore from the same store, the tree rebuilds without
  user action (spec scenarios: restart restores the tree; fresh device
  converges).
- [x] 7.2 Optimistic pending overlay: local ops apply immediately on top
  of the committed replay, marked pending; the room echo (same event id)
  clears the pending mark. Test: apply op offline (send endpoint down) →
  node visible with pending state; enable endpoint + sync echo → pending
  cleared, committed tree equals optimistic one (spec scenario: offline
  folder creation).

## 8. Wrap-up

- [x] 8.1 Public-API doc pass (doc comments on all public items; no SDK
  types leaked past `zikos-matrix`), then full gate: `cargo t`,
  `cargo lint`, `cargo fmtcheck`.

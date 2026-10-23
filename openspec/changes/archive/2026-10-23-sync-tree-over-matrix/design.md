# Design: sync-tree-over-matrix

## Context

`zikos-core::tree` has the strict `FileTree`/`TreeOp` model; `zikos-matrix`
(phase 1) has login, sync, encrypted rooms, durable text sending, and an
encrypted deduplicated message log. What's missing is everything between:
transporting ops as room events, replaying them, and converging after
concurrent/offline edits. See proposal.md for motivation.

## Goals / Non-Goals

**Goals:** pure, proptestable conflict logic (no I/O) in `zikos-core`;
Matrix wiring isolated in `zikos-matrix`; SDK types never leak; all
persisted state encrypted at rest; every device converges from the same
event set regardless of arrival order.

**Non-Goals:** blob/`FileKey` payloads (Phase 5), UI, server-side history
backfill beyond what `/sync` delivers (same boundary as phase-1 messaging),
snapshot/compaction of the log.

## Decisions

### D1 — Layering
New pure module `zikos-core::tree_sync`: envelope, ordering, dedup,
replay/conflict engine, property tests — no I/O, no SDK types. New module
`zikos-matrix::tree`: custom event, send/receive wiring, persistence,
public API. `zikos-matrix` gains a `zikos-core` dependency (domain types
already flow to the UI this way). `TreeOp` itself is unchanged.

### D2 — Wire format (verified against matrix-sdk 0.13 sources)
Custom message-like event type `zikos.tree.op`. Content (inside the Megolm
envelope, so plaintext fields are fine):

```json
{ "op": {..TreeOp..}, "base": {"name": "..", "parent": ".."}?,
  "known_children": [".."]? }
```

- Send: `RoomSendQueue::send_raw(Raw<AnyMessageLikeEventContent>, type)` —
  exists in 0.13 (`send_queue/mod.rs:457`), so custom types get the durable
  offline queue, retries, and restart survival for free, reusing the phase-1
  delivery-state machinery.
- Receive: ruma `event!`-macro content type + `client.add_event_handler`
  for `SyncMessageLikeEvent<ZikosTreeOpEventContent>` (custom-event handlers
  are the SDK's documented pattern; `Raw<T>` handlers are the fallback if
  the macro path fights us in 0.13). Undecryptable tree events never reach
  the handler; the message layer already surfaces them — tree replay simply
  excludes what it cannot decode, mirroring message semantics.
- `author`, `origin_server_ts`, `event_id` come from the room event, not
  the content; the envelope merges the two at receive time.
- Serde is lenient (unknown fields ignored, missing optionals = absent) so
  older clients tolerate newer content.

### D3 — Canonical order and LWW
Replay canonicalizes first: dedup by `event_id`, sort by
`(origin_server_ts, event_id)`. This total order is why origin-server ts is
the LWW clock: two devices editing offline both get server timestamps at
the moment they sync, so true races carry (near-)identical ts and the event
id breaks ties. Arrival order cannot affect the result — the core
property proptest guards. Alternative — client wall clocks: rejected
(device skew, no total order); Lamport/vector clocks: rejected (requires
changing `TreeOp`'s published shape; ts+id suffices for convergence — the
concurrency *detection* signal is separate, see D4).

### D4 — Conflict detection and the Conflicts/ mechanics
LWW alone decides the winner; detecting *that* a race happened (so the
loser is preserved rather than overwritten) needs the author's intent:
- `base` (rename/move/delete only): the `(name, parent)` the author
  believed the target had. At apply time in canonical order: base matches
  current state → sequential edit, clean apply; base mismatches → a
  concurrent edit won/lost against this op.
- `known_children` (delete only): child ids the deleter saw. Children
  beyond that set are concurrent creations: preserved under `Conflicts/`
  instead of deleted.

Replay algorithm (pure, built on `FileTree`'s public API only):
1. Canonical sort; apply in order. `base`-mismatch on a rename/move: the
   current (loser) state — post earlier-apply — is snapshotted into
   `Conflicts/`, then the winning op applies. For a racing delete, the
   subtree is snapshotted before deletion (delete still wins: deletion
   dominates).
2. Unknown parent (create) or unknown target: the op goes to a pending set
   and is retried after each successful apply (handles same-ms event-id
   inversions of a single author's chain). Still-unsatisfied at the end:
   creates orphan into `Conflicts/`; other ops are reported lost
   (`ReplayReport::lost`) — surfaced, never silently dropped. Ops on nodes
   already removed by a delete are likewise lost-and-reported.
3. `Conflicts/` is a fixed root folder (well-known id, like `ROOT_ID`);
   loser copies get deterministic ids (`uuid` v5 over the losing event id —
   pure Rust SHA-1) so every device mints identical copies. Duplicate
   names inside `Conflicts/` are permitted (nodes are id-keyed).

Alternative — full version vectors per event: rejected (event-size and
complexity at musician scale; `base`/`known_children` are O(1)/O(children)
and cover the spec'd races).

### D5 — Offline queue, pending states, optimistic tree
Send path reuses the SDK durable queue (D2) and the phase-1
`DeliveryTracker` states. Local ops apply immediately to an optimistic
overlay: `room_tree()` = replay(committed log) + pending local ops applied
leniently; nodes whose latest mutation is still pending carry a pending
mark. When the room echo returns via sync, the event enters the committed
log (dedup by event id) and the pending entry clears — the room log is the
single source of truth, never a second tree.

### D6 — Persistence
Per-room tree-event log persisted like `MessageLog`: encrypted with the
same `Cipher` (Argon2 + AES-GCM, pure RustCrypto), atomic writes, deduped
by event id, replayed on start. Replay is O(log) at musician scale;
snapshotting is a later change.

### D7 — Property testing (proptest; pure Rust, dev-only)
Strategies generating random event sets (valid ops, racing renames/moves/
deletes, same-ts clusters). Properties: (1) permutation invariance — any
arrival order yields identical trees and `Conflicts/` contents; (2)
re-delivery idempotence; (3) rename races resolve to `max(ts, event_id)`
with the loser under `Conflicts/`; (4) no-silent-loss — every created node
is either in the tree, under `Conflicts/`, under a surviving ancestor, or
explicitly deleted by a delete that observed it. New workspace dev-dep
`proptest`; `uuid` gains the `v5` feature.

## Risks / Trade-offs

- [Same-ms event-id inversion breaks an author's create-chain] → pending
  retry (D4.2); proptest generates same-ts clusters explicitly.
- [Mock harness can't exercise real Megolm] → send tests assert the
  raw `zikos.tree.op` body in unencrypted harness rooms; encrypted-path
  coverage rides on the SDK (same stance as phase 1).
- [`known_children` omits concurrent *renames* of children] → delete of a
  folder whose child was concurrently renamed deletes that child; bounded
  loss, documented; full coverage needs per-child bases (later change if
  real usage hits it).
- [Conflicts/ could accumulate] → user-facing cleanup is UI-phase work;
  ids stay stable so copies are dedupable later.

## Migration Plan

Additive: two new modules + deps; no existing behavior changes. Rollback =
remove the modules and the two dep entries.

## Open Questions

None blocking. (Exact ruma event-macro ergonomics in 0.13 are settled in
task 1; `Raw<T>` fallback does not change the approach.)

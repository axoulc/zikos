# Proposal: bootstrap-matrix-client

Source spec: `specs/messaging.md` (source of truth) — phase 1 only.

## Why

Zikos's core promise is E2EE chat over Matrix, but the workspace contains no
Matrix client code. This change lays the foundation: a `zikos-matrix` crate
wrapping matrix-sdk that can log in, persist an encrypted session, sync,
create one encrypted room, and exchange text messages — the minimum for a
first runnable messaging loop that later phases (verification, multi-device,
UI) build on.

## What Changes

- New crate `crates/zikos-matrix` wrapping matrix-sdk (Rust), exposing:
  - username+password login to any homeserver, with typed error mapping
    (e.g. `M_FORBIDDEN` → invalid credentials);
  - session persistence encrypted at rest, restored on restart without
    re-prompting for credentials;
  - sync-based room/history restore on start;
  - creation of a room with encryption enabled before the first message;
  - send/receive of text messages with sender, timestamp, and delivery
    state (`sending`/`sent`/`failed`) plus an offline outbox that
    auto-transmits queued messages when connectivity returns.
- Root `Cargo.toml` gains matrix-sdk workspace dependencies.

## Capabilities

### New Capabilities

- `messaging`: phase 1 of Matrix messaging per `specs/messaging.md` —
  login with persisted encrypted session, history sync, E2EE-by-default
  room creation, and text send/receive with offline queueing.

### Modified Capabilities

(none — `openspec/specs/` is empty today; `messaging` is seeded here)

## Impact

- New code: `crates/zikos-matrix` (library only). No changes to existing
  crates; `apps/zikos-app` integration is out of scope.
- New workspace dependencies: matrix-sdk (+ its sqlite state store) —
  pure-Rust crypto via vodozemac; details and trade-offs in design.md.
- Testing: deterministic unit tests for pure logic (outbox, delivery
  state, session store) and mock-homeserver tests for client flows.

## Non-goals

- SSO login; emoji device verification; multi-device flows (later changes).
- Any UI work in `apps/zikos-app`.
- Voice/video calls, message search, reactions, read-receipt UI.
- Push notifications; file-tree room events; audio messages.

# Design: bootstrap-matrix-client

## Context

The workspace has domain, crypto, audio, and server crates, but no Matrix
client code; `apps/zikos-app` is a bare Slint shell. This change adds a
library-only crate `crates/zikos-matrix` that wraps matrix-sdk and exposes
the phase-1 messaging behaviors in `specs/messaging/spec.md`. See
proposal.md for motivation. The crate is a tokio I/O crate by nature (the
no-I/O rule applies to `zikos-core`, not here); embedders pass paths and
secrets in, so the crate stays testable and UI-agnostic.

## Goals / Non-Goals

**Goals:**
- Every requirement testable deterministically, without a live homeserver.
- All persisted state (session, room state, crypto material, outbox)
  encrypted at rest via a passphrase the embedder supplies.
- SDK types never leak past the crate boundary (contains 0.x API churn).

**Non-Goals:** OS keychain wiring (embedder supplies the passphrase;
keychain integration is a later app change); Sliding Sync; verification and
multi-device flows; UI integration.

## Decisions

### D1 — Dependencies (workspace-pinned)
- `matrix-sdk` **0.13.0** with `default-features = false`, features
  `e2e-encryption`, `automatic-room-key-forwarding`, `sqlite`,
  `bundled-sqlite`, `rustls-tls`. (Planning said 0.14.x; every release
  above 0.13 raises MSRV past the workspace's rustc 1.85, so 0.13 is the
  newest the workspace can take without a toolchain-wide bump. The send
  queue is core API in 0.13, not a feature flag.) Official SDK, actively
  maintained; E2EE is `vodozemac` — pure-Rust Olm/Megolm. `bundled-sqlite`
  compiles vendored C SQLite — the one C dependency, chosen for hermetic
  Android/iOS cross-builds.
- Store encryption: `matrix-sdk-store-encryption` (pulled in via the
  sqlite store) — pure RustCrypto (Argon2/PBKDF2 + AES-GCM), no SQLCipher.
- HTTP: reqwest with rustls (matches `zikos-server`; no native-tls).
- Errors: `thiserror`. Tests: `wiremock` + `matrix-sdk-test` fixtures,
  `tempfile` (already workspace deps).
- Alternative — Element's Sliding Sync stack: rejected for phase 1 (needs
  a proxy; plain `/sync` works on any homeserver incl. Continuwuity).

### D2 — Session persistence and encryption at rest
`ClientBuilder` sqlite store at an embedder-provided path with a
`SecretString` passphrase; the SDK derives store keys and encrypts session,
room state, and crypto material in the DB. Login persists the session via
the SDK; restart calls restore-from-store with no credentials.
Alternatives: default filesystem store (unencrypted — violates spec);
SQLCipher (unnecessary — store encryption is already pure Rust).

### D3 — Sync and history
Classic long-poll `/sync` (exposed as `sync_once` rounds in phase 1) with
a room-event handler that fans decrypted events out on a typed broadcast
channel. The sync cursor is tracked by zikos-matrix (the SDK's stored
token isn't publicly readable) and persisted, so a restored client
continues incrementally. History for phase 1 is zikos-matrix's own
message log: every surfaced message is appended (deduped by event ID) to
an encrypted file next to the store, reloaded on start — restored history
without user action. Backfilling older server-side history via
`/messages` and the SDK event-cache Timeline is a later change. Rooms
expose an encryption-state query so callers can warn on unencrypted
rooms.

### D4 — E2EE room creation
Create rooms with the `m.room.encryption` state event included in the
creation request, then assert `room.is_encrypted()` before reporting
success. The crate offers no API that creates an unencrypted room.

### D5 — Send path: SDK send queue (durable) + delivery-state tracker
- Source inspection of matrix-sdk 0.13 corrected a planning assumption:
  the send queue is NOT in-memory — it persists local echoes and queued
  requests in the sqlite state store (`send_queue_events` tables), so
  offline sends already survive restarts and auto-resume when the room
  queue is re-instantiated. zikos-matrix therefore does not add its own
  outbox file (that would duplicate persistence and risk double
  submission, since `send()` mints its own transaction IDs).
- zikos-matrix adds a `DeliveryTracker`: a pure state machine mapping
  queue updates (`NewLocalEvent`/`SendError`/`RetryEvent`/`SentEvent`)
  to our `sending`/`sent`/`failed` states, surfaced on the crate's event
  channel. After restart, initial states are reconstructed from the
  queue's persisted local echoes (`send_error` present → `failed`).
- After retries are exhausted the SDK disables the queue; zikos-matrix
  re-enables it on the next send attempt and after a successful sync
  round, so connectivity return implies automatic transmission. Echoes
  wedged by an unrecoverable error are unwedged on restore (their local
  echo carries a `SendHandle`), which is what makes the offline-restart
  scenario transmit. Note: sending resolves the room's encryption state
  via `GET /rooms/{id}/state/m.room.encryption` first, so that endpoint
  must answer (200 or a well-formed `M_NOT_FOUND`).
- Alternatives: our own file-backed outbox (rejected: duplicates SDK
  persistence); plain `room.send()` (rejected: no retry/offline
  semantics).

### D6 — Error model
`thiserror` enum at the boundary: `InvalidCredentials` (M_FORBIDDEN),
`Connection`, `Homeserver` (errorcode), `Crypto`, `Store`. Distinct
variants carry the wrong-password vs unreachable-homeserver scenarios.

### D7 — Test strategy (TDD backbone)
- Pure unit tests (no network): outbox state machine + store round-trip,
  delivery-state mapping, error mapping, txn-id derivation.
- Mock homeserver: `wiremock` dev-server answering `/login`, `/sync`,
  `/createRoom`, `/keys/*` with hand-written or `matrix-sdk-test` JSON
  fixtures; tests assert request bodies (e.g. `createRoom` contains
  `m.room.encryption`) and feed sync responses.
- No Synapse/docker in CI (stack policy: no non-Rust services).
- Alternative — `matrix-sdk-test`'s mockito-based server: rejected as
  primary (stale mockito); kept as a fixture source.

## Risks / Trade-offs

- [matrix-sdk 0.x API churn] → pin minor version in workspace; all SDK
  types wrapped behind our API in this one crate.
- [Mock homeserver drifts from real servers] → assert only fields we
  control; keep fixtures minimal; revisit when a real-device milestone
  lands.
- [Embedder mishandles passphrase] → crate accepts `SecretString` only and
  never persists it; document that plaintext passphrase storage in the app
  would weaken at-rest guarantees.
- [Outbox resubmit could double-send] → txn IDs derived from stable local
  IDs make room sends idempotent server-side.

## Migration Plan

New crate only; no existing behavior touched. Rollback = remove
`crates/zikos-matrix` and its workspace-dep entries.

## Open Questions

None blocking. (Exact 0.14.x feature-flag set is settled in task 1; it
does not change the approach.)

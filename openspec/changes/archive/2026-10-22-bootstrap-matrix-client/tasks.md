# Tasks: bootstrap-matrix-client

Every task follows strict TDD: write the failing test named after "Test:",
watch it fail, then implement minimally. Gate before ticking any task:
`cargo t`, `cargo lint`, `cargo fmtcheck` all clean.

## 1. Crate scaffolding and test harness

- [x] 1.1 Scaffold `crates/zikos-matrix` (empty lib with crate docs) and
  register it in the workspace (`Cargo.toml` members + internal-deps
  section). Test: a placeholder `#[test] fn crate_in_workspace()` in
  `crates/zikos-matrix` fails until `cargo t` picks the crate up.
- [x] 1.2 Add workspace-pinned `matrix-sdk` 0.14.x (features `sqlite`,
  `e2e-encryption`, `send_queue`; pin the final flag set here) plus
  `thiserror`; dev-deps `wiremock`, `matrix-sdk-test`, `tempfile`, tokio
  test macros. Test: `builds_client_with_sqlite_store()` constructs an SDK
  client with a sqlite store in a tempdir (fails until features resolve).
- [x] 1.3 Build the mock-homeserver harness in `tests/common/mod.rs`:
  wiremock server answering `/login`, `/sync`, `/createRoom`, `/send/...`,
  `/keys/*` from JSON fixtures. Test: `harness_endpoints_respond()` logs
  in through the harness and gets a session (fails until fixtures exist).

## 2. Login and persisted session

- [x] 2.1 Define the crate error enum (`InvalidCredentials`,
  `Connection`, `Homeserver`, `Crypto`, `Store`) with explicit mapping
  from SDK errors. Test: unit tests map a 403 `M_FORBIDDEN` response to
  `InvalidCredentials` and an unreachable endpoint to `Connection`.
- [x] 2.2 Implement `login(homeserver, username, password, store_path,
  passphrase)`. Test: mock `/login` success returns the user ID; wrong
  password (mock returns `M_FORBIDDEN`) yields `InvalidCredentials` and
  leaves no persisted session (spec scenarios: returning user / wrong
  password).
- [x] 2.3 Implement session restore from the encrypted sqlite store.
  Test: log in, drop the client, rebuild from the same store path +
  passphrase without credentials, and complete one `/sync` round against
  the harness (spec scenario: returning user; wrong passphrase must fail
  with `Store`).

## 3. Sync and room observation

- [x] 3.1 Typed room/message model + broadcast channel fed by the SDK
  room-event handler. Test: harness `/sync` fixture with an
  `m.room.message` produces an exposed message with sender, room,
  timestamp, body; an `m.room.encrypted` without keys yields a
  `DecryptionFailed` marker instead of an error (spec scenarios: receive
  with key / undecryptable message).
- [x] 3.2 History restore on start. Test: two consecutive `/sync` rounds
  accumulate timeline messages with no user action beyond client start
  (spec scenario: restart restores history).
- [x] 3.3 Room encryption-state query. Test: `/sync` fixture whose joined
  room lacks `m.room.encryption` reports `Unencrypted` (spec scenario:
  unencrypted room detected).

## 4. E2EE room creation

- [x] 4.1 Implement `create_encrypted_room(name)`. Test: harness captures
  the `/createRoom` request body and it contains the `m.room.encryption`
  state event; the returned room reports encrypted (spec scenario: new
  conversation; no API path may create an unencrypted room).

## 5. Send path with delivery state and offline queue

- [x] 5.1 Pure `DeliveryTracker` state machine mapping send-queue updates
  (`NewLocalEvent`/`SendError`/`RetryEvent`/`SentEvent`) to delivery
  states `sending`/`sent`/`failed`. Test: unit tests for transitions,
  including rejection of invalid ones and reconstruction from persisted
  local echoes (`send_error` present → `failed`).
- [x] 5.2 Restart survival through the SDK's durable send queue (sqlite
  state store). Test: enqueue with `/send` down, drop the client, rebuild
  from the same store, bring the endpoint up — the local echo reloads and
  is transmitted (spec scenario: offline send, restart clause).
- [x] 5.3 Wire send path: `send_text` → SDK send queue → tracker updates
  surfaced on the crate's event channel; queue re-enabled on the next
  send attempt after retries disabled it. Test: harness accepts `/send`;
  message goes `sending` → `sent` (spec scenario: successful send).
- [x] 5.4 Offline behavior and automatic retry. Test: with `/send` down,
  a message stays `sending`/`failed`; once the endpoint is up and the
  queue re-enabled, it is transmitted and becomes `sent` (spec scenarios:
  offline send / failed send retried).

## 6. Wrap-up

- [x] 6.1 Public-API doc pass (doc comments on all public items; no SDK
  types leaked), then full gate: `cargo t`, `cargo lint`,
  `cargo fmtcheck`.

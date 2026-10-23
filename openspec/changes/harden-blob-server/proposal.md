# Proposal: harden-blob-server

Source spec: `specs/audio-storage-server.md` (source of truth). This change
hardens the already-implemented `crates/zikos-server` and promotes the spec's
"persistence of sessions (follow-up)" requirement to implemented behavior.

## Why

The blob server works but is soft: the real `MatrixOpenIdVerifier` has no
test against the federation endpoint's wire shape, uploads buffer whole
bodies (default cap 512 MiB) in memory, sessions evaporate on restart (spec
follow-up), and there is no access log to audit a deployment.

## What Changes

- **Verifier integration tests**: run the real `MatrixOpenIdVerifier`
  against a wiremock homeserver serving
  `/_matrix/federation/v1/openid/userinfo` — happy path, 401 mapping,
  garbage JSON, and the SSRF scenario proven by asserting zero outbound
  requests on invalid `server_name`.
- **Streaming bodies**: upload streams chunks to a temp file (atomic rename,
  incremental cap enforcement, partial cleanup); download streams from
  disk. No full-body buffering either way; HTTP contract unchanged.
- **Persistent sessions**: `SessionStore` trait; file-backed append-only
  registry (0600, corrupt-tail tolerant) so sessions survive restarts;
  in-memory impl stays for tests/dev.
- **Structured access log**: one JSON line per request on stderr (request
  id, method, path, status, latency, user id) — never blob contents,
  bodies, bearer tokens, or query strings.

## Capabilities

### New Capabilities

- `audio-storage-server`: the blob server contract — health, Matrix OpenID
  session, streaming blob upload/download, persistent sessions, access
  logging. (`openspec/specs/audio-storage-server/` does not exist yet; this
  delta seeds it from `specs/audio-storage-server.md` plus the hardening.)

### Modified Capabilities

(none)

## Impact

- Code: `crates/zikos-server` only (auth, store, routes, new log middleware,
  main wiring). No HTTP-contract breaks.
- New workspace deps: `wiremock` (dev-only, pure Rust), `tokio-util`
  (ReaderStream); `tokio` gains `io-util`.
- Testing: integration tests spin a wiremock server; registry persistence
  tested by reopening the file as a "restart".

## Non-goals

- Session expiry/revocation, logout endpoint, multi-device semantics.
- Dedup, quotas, replication (spec non-goals).
- Tracing/metrics exporters.
- Changing the wire format of any endpoint.

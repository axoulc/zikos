# Design: harden-blob-server

## Context

`crates/zikos-server` implements `specs/audio-storage-server.md`: axum
router, `OpenIdVerifier` trait (real `MatrixOpenIdVerifier` + test fake),
in-memory `SessionRegistry`, `FsBlobStore` with `put/get` over whole
buffers, env-configured `main`. See proposal.md for why we harden now.

## Goals / Non-Goals

**Goals:**

- Regression coverage for the real federation wire format (path, query
  param, status/JSON handling) without touching a live homeserver.
- Constant-ish memory per request: bodies stream disk-ward and disk-ward
  responses stream to the client.
- Sessions that survive restarts; storage swappable behind a trait.
- One auditable JSON line per request, with a hard allow-list of fields.

**Non-Goals:** session expiry/revocation, logout, per-user quotas,
metrics/tracing exporters, HTTP wire-format changes.

## Decisions

### D1. Verifier testability — injectable endpoint base, wiremock in tests

`MatrixOpenIdVerifier` gains a constructor that overrides the federation
base URL (`https://{server_name}` in production). Integration tests
(`tests/federation.rs`) point it at a **wiremock** server and assert on the
wire: exact path `/_matrix/federation/v1/openid/userinfo`, `access_token`
query parameter (built with `reqwest`'s `.query()`, so the token is
properly encoded — today it is interpolated raw), non-2xx → `Rejected`,
2xx-garbage → `Upstream`. The SSRF scenario becomes executable: mock hit
count must be zero for invalid `server_name` values.

Alternatives: fake-verifier-only tests (current state — bypasses exactly the
wire format we want pinned); `/etc/hosts` or DNS tricks (flaky, not
portable); allowing `localhost` server names in production code (weakens the
SSRF guard).

### D2. Streaming store — temp file + atomic rename, chunk-counted cap

`BlobStore` replaces `put/get` with streaming methods:

- `put_stream(id, body: axum::body::Body, max_bytes) -> u64`: writes
  `body.into_data_stream()` chunks to `{id}.bin.tmp` via tokio's
  `AsyncWriteExt`, counting bytes; over cap → new `StoreError::TooLarge`,
  abort, delete the temp file; success → `flush`, `sync_all`, atomic rename
  to `{id}.bin`, return the byte count. A crash mid-upload leaves only a
  `.tmp` that is never a resolvable blob id.
- `open(id) -> (file: tokio::fs::File, len: u64)`: download wraps the file
  in `tokio_util::io::ReaderStream` and `axum::body::Body::from_stream`,
  with `content-length: len`.

Routes stop extracting `Bytes`/`Vec<u8>`; the upload handler takes the
`Request` and streams it. `DefaultBodyLimit` is **dropped** (adjusted
during apply): with a directly-consumed body, axum's `Limited` wrapper
turns over-limit frames into body errors before our counter sees them,
which would surface as 500 instead of 413 — the store's counted cap is
the single enforcement point. Whole-buffer methods are removed
from the trait — no external consumers exist, and keeping dead buffering
APIs invites regression.

Alternative considered: `Content-Length` precheck instead of chunk counting
— chunked uploads send none; counting is the only honest enforcement.

### D3. Sessions — `SessionStore` trait, append-only JSONL file

`SessionRegistry` becomes `InMemorySessionStore` behind a `SessionStore`
trait (`issue`, `resolve`); `AppState.sessions` widens to
`Arc<dyn SessionStore>`. Production default: `FileSessionStore`, one
`{"token":…,"user_id":…}` JSON line per session, append + flush on issue,
full load on open, skipping an unparsable trailing line (crash mid-write).
Created with owner-only permissions (0600 on Unix). Restart = reopen the
same path in tests. `ZIKOS_SESSION_FILE` (default `sessions.jsonl`) wires it
in `main`.

Alternatives: SQLite/sled (dependency + embedded DB for ~100s of rows of
two strings is disproportionate); re-verifying Matrix tokens on each
request instead of persisting (churns the homeserver; tokens are
short-lived).

### D4. Access log — custom middleware, not tower-http TraceLayer

A `from_fn_with_state` middleware beats `TraceLayer` here because it can
resolve the user id from `Authorization` via the session store and control
the exact JSON shape. It: mints a UUID request id, sets `x-request-id` on
the response, times the call, and hands a typed `AccessEntry` to a
`LogSink` trait; the production sink writes one `serde_json` line to
stderr. A capturing sink makes tests assert fields (and absence of secrets)
without scraping stderr. Field allow-list: `ts`, `request_id`, `method`,
`path`, `status`, `duration_ms`, `user_id` (null when unauthenticated).
Path is logged without query string by construction (axum's `path()`).

### D5. Dependencies

`wiremock` 0.4 (dev-only; pure Rust, tokio-native HTTP mock), `tokio-util`
0.7 (ReaderStream; pure Rust, part of the tokio stack), `tokio` gains
`io-util` + `fs` (already partially present). No new async runtimes, no C
code.

## Risks / Trade-offs

- [JSONL grows unboundedly; logout/cleanup doesn't exist] → accepted for
  now (spec non-goal); format is line-addressable, so a later vacuum task
  is trivial.
- [Temp-file rename leaves `.tmp` files after hard crashes] → startup MAY
  sweep `*.tmp`; harmless either way (never resolvable).
- [Middleware resolves the session twice per request (extractor + log)] →
  in-memory map / one JSONL lookup; negligible next to blob I/O.
- [wiremock pins verifier behavior to Matrix spec shape, not a real
  Synapse] → acceptable; the endpoint contract is stable and specified.

## Migration Plan

No stored data exists that must migrate: blobs keep their `{id}.bin` layout;
sessions restart empty and repopulate on first logins. Rollback = previous
commit; no on-disk format shared with the old code except blob files.

## Open Questions

- Sweep `*.tmp` on startup now or defer? Deferring changes nothing
  observable; revisit if crash-testing litters the blob dir.

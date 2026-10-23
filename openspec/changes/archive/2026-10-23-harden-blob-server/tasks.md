# Tasks: harden-blob-server

Every task follows strict TDD: write the failing test named after "Test:",
watch it fail, then implement minimally. Gate before ticking any task:
`cargo t`, `cargo lint`, `cargo fmtcheck` all clean. Per AGENTS.md, every
commit during apply updates `CHANGELOG.md` under `## [Unreleased]` in the
same commit. Scope: `crates/zikos-server` only.

## 1. Federation verifier integration tests (design D1)

- [x] 1.1 Add workspace deps: `wiremock` (dev-only), `tokio-util`, and
  `io-util` on tokio. Add `MatrixOpenIdVerifier::with_base_url(base)` used
  by tests; production keeps `https://{server_name}`; build the query via
  reqwest `.query()` instead of raw interpolation. Test (new
  `tests/federation.rs`): `happy_login_hits_federation_userinfo` — a
  wiremock server answering 200 `{"sub":"@lisa:example.org"}` on
  `/_matrix/federation/v1/openid/userinfo`; the verifier returns the user
  id, and `mock.verify()` pins method, path, and the `access_token` query
  parameter.
- [x] 1.2 Test: `rejected_token_is_rejected` — the mock answers 401; the
  verifier returns `AuthError::Rejected`.
- [x] 1.3 Test: `garbage_2xx_body_is_upstream_error` and
  `sub_that_is_not_a_matrix_user_is_rejected` — 2xx with non-JSON body →
  `AuthError::Upstream`; 200 with `{"sub":"lisa"}` (no `@`) → `Rejected`.
- [x] 1.4 Test: `invalid_server_name_makes_no_outbound_request` — for each
  SSRF vector from the existing validation test, `verify` fails with
  `InvalidServerName` and the wiremock server's received-request count is
  zero.

## 2. Streaming blob transfer (design D2)

- [x] 2.1 `BlobStore::put_stream(id, stream, max_bytes) -> u64` on
  `FsBlobStore`: chunks to `{id}.bin.tmp`, byte-counted cap → new
  `StoreError::TooLarge` (mapped to 413 in `error.rs`), abort deletes the
  temp file, success flushes + `sync_all` + atomic rename. Remove the old
  whole-buffer `put`/`get` from the trait (no external consumers).
  Tests: `streamed_put_roundtrips_via_open` (multi-chunk stream, size
  returned and file content exact) and
  `stream_over_cap_aborts_and_cleans_up` (cap exceeded mid-stream →
  `TooLarge`, no `{id}.bin`, no leftover `.tmp`).
- [x] 2.2 `BlobStore::open(id) -> (tokio::fs::File, len)` for downloads.
  Test: `open_returns_seekable_file_with_exact_length` — after
  `put_stream`, `open` yields the exact byte length and reading the file
  returns the stored bytes; unknown id → `NotFound`, non-UUID →
  `InvalidId`.
- [x] 2.3 Upload route streams: handler takes `Request` (no `Bytes`
  extractor), feeds `body.into_data_stream()` to `put_stream`, keeps
  `DefaultBodyLimit` as the second guard, returns `{id, size}` from the
  counted bytes. Rework the api test helper to expose the blob dir.
  Tests: `chunked_streaming_upload_roundtrips` (body built from a
  `futures`-style chunk stream or `http_body_util::StreamBody`) and
  `oversize_streaming_upload_is_413_and_leaves_no_blob` (cap 8, 64-byte
  streamed body → 413; the blob dir afterwards contains no `*.bin`).
- [x] 2.4 Download route streams: `Body::from_stream(ReaderStream)` over
  `open()`, with `content-length` and `application/octet-stream`.
  Test: `download_sets_octet_stream_and_exact_length` — headers match and
  the body still equals the upload (existing roundtrip test keeps passing).

## 3. Persistent session registry (design D3)

- [x] 3.1 `SessionStore` trait (`issue`, `resolve`); rename
  `SessionRegistry` → `InMemorySessionStore` implementing it;
  `AppState.sessions` becomes `Arc<dyn SessionStore>`. Test:
  `issue_then_resolve_roundtrip` (trait-level); existing api tests pass
  with the in-memory impl swapped in.
- [x] 3.2 `FileSessionStore`: append-only JSONL (`{"token","user_id"}` per
  line), full load on open skipping an unparsable trailing line, created
  with owner-only permissions on Unix. Tests:
  `sessions_survive_reopen` (issue, drop, reopen same path, resolve —
  the restart simulation), `corrupt_tail_is_skipped` (truncated last
  line; earlier sessions still resolve), and on Unix
  `registry_file_is_owner_only`.
- [x] 3.3 Wire `ZIKOS_SESSION_FILE` (default `sessions.jsonl`) into
  `main.rs` via a pure env-parsing helper. Test:
  `session_file_env_default_and_override` — helper returns the default
  when unset and the override value when set.

## 4. Structured access log (design D4)

- [x] 4.1 `AccessEntry` (ts, request_id, method, path, status,
  duration_ms, user_id) + `LogSink` trait with a test capturing sink and
  a production stderr sink; axum `from_fn_with_state` middleware added to
  `router()` that mints a UUID request id, sets `x-request-id`, times the
  call, resolves the user via the session store, and emits one entry.
  Test: `every_request_yields_one_line_and_x_request_id` — after one
  request the captured sink holds exactly one entry and the response
  carries the matching header.
- [x] 4.2 Test: `log_line_carries_required_fields` — captured entry for an
  authed blob request parses as JSON with ts (RFC 3339), request_id,
  method POST, path `/v1/blobs`, 201, duration_ms ≥ 0, and the logged-in
  user id; unauthenticated request logs `user_id: null`.
- [x] 4.3 Test: `secrets_and_blob_bytes_never_logged` — perform login
  (OpenID token in body), upload, and download with a `Bearer` session;
  no captured entry serializes to a string containing the OpenID token,
  the session token, or any of the uploaded bytes.
- [x] 4.4 Production sink renders one JSON line per entry to stderr with a
  pure `render(entry) -> String` (single `serde_json::to_string` + `\n`).
  Test: `render_yields_single_json_line` on the pure function; wire the
  sink in `main.rs`.

## 5. Wrap-up

- [x] 5.1 Full gate pass over the workspace (`cargo t`, `cargo lint`,
  `cargo fmtcheck`), crate-level doc comments refreshed (auth/store/log
  modules), and `CHANGELOG.md` `[Unreleased]` entries complete for every
  commit made during apply.

# Audio storage server — source spec

## Purpose

One self-hosted Rust service (axum) that stores **client-encrypted** audio
blobs and nothing else. It learns as little as possible: no file names, no
conversation membership, no plaintext. Authentication delegates to Matrix via
OpenID token exchange — the server has no user database.

Status: partially implemented by `crates/zikos-server` (health, session,
upload/download, size cap). This file remains the contract.

## Requirements

### Requirement: health endpoint
`GET /healthz` SHALL return 200 with `{"status":"ok"}` and SHALL not require
authentication.

### Requirement: Matrix OpenID session
`POST /v1/session` with `{token, server_name}` SHALL verify the OpenID token
against the homeserver federation endpoint
(`GET https://{server_name}/_matrix/federation/v1/openid/userinfo`) and return
an opaque session token plus the Matrix user id. Invalid tokens SHALL get 401.
`server_name` SHALL be validated as a Matrix server name (SSRF guard).

#### Scenario: happy login
- **WHEN** a client presents a valid Matrix OpenID token for `@lisa:example.org`
- **THEN** the server responds 201 with a session usable as `Bearer` on
  subsequent calls

#### Scenario: SSRF guard
- **WHEN** `server_name` contains a scheme, path, or userinfo (e.g. `https://x/y`)
- **THEN** the request fails validation with 400 and no outbound request is made

### Requirement: blob upload
`POST /v1/blobs` (authenticated) SHALL store the request body verbatim as an
opaque blob keyed by a fresh server-generated UUID and return 201 with
`{id, size}`. Bodies above the configured limit SHALL be rejected with 413.
The server SHALL NOT inspect, transcode, or re-encrypt blob contents.

### Requirement: blob download
`GET /v1/blobs/{id}` (authenticated) SHALL return the exact stored bytes with
`application/octet-stream`; unknown ids SHALL return 404. Ids SHALL be
validated as UUIDs.

### Requirement: deployment
The server SHALL ship as a single static binary configured by environment
variables (`ZIKOS_BIND`, `ZIKOS_BLOB_DIR`, `ZIKOS_MAX_BLOB_BYTES`), SHALL log
to stderr, and SHALL run behind any TLS-terminating reverse proxy (Caddy
recommended).

### Requirement: persistence of sessions (follow-up)
Session tokens SHALL survive server restarts (persistent registry) — tracked
as a follow-up change; the in-memory registry is accepted for development.

## Non-goals (for now)

- Deduplication, quotas per user, replication.
- Any knowledge of blobs' contents or tree structure.

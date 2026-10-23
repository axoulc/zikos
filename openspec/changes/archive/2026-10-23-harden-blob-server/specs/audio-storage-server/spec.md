# Spec Delta: audio-storage-server (new capability)

Derived from `specs/audio-storage-server.md` (source of truth), hardened by
this change: streaming blob transfer (no full-body buffering), persistent
sessions (promoted from the source spec's follow-up note to a requirement),
structured access logging, and executable coverage for the federation
verification path.

## Purpose

One self-hosted Rust service (axum) that stores client-encrypted audio blobs
and nothing else: Matrix-OpenID-authenticated, streaming in both directions,
learning no file names, no membership, no plaintext.

## ADDED Requirements

### Requirement: health endpoint
`GET /healthz` SHALL return 200 with `{"status":"ok"}` and SHALL not require
authentication.

#### Scenario: health probe
- **WHEN** any client GETs `/healthz` with no credentials
- **THEN** the response is 200 with body `{"status":"ok"}`

### Requirement: Matrix OpenID session
`POST /v1/session` with `{token, server_name}` SHALL verify the OpenID token
against the homeserver federation endpoint
(`GET https://{server_name}/_matrix/federation/v1/openid/userinfo`) and
return 201 with an opaque session token plus the Matrix user id. Any
verification failure — a rejected token, an unreachable homeserver, or a 2xx
response whose body is not valid userinfo — SHALL get 401 without
distinguishing the cause. `server_name` SHALL be validated as a Matrix
server name (SSRF guard) before any outbound request is made.

#### Scenario: happy login
- **WHEN** the homeserver's federation userinfo endpoint answers 200 with
  `{"sub":"@lisa:example.org"}` for the presented token
- **THEN** the server responds 201 with a session usable as `Bearer` on
  subsequent calls, together with the user id

#### Scenario: homeserver rejects the token
- **WHEN** the federation userinfo endpoint answers a non-2xx status
- **THEN** the server responds 401

#### Scenario: homeserver returns garbage
- **WHEN** the federation userinfo endpoint answers 2xx with a body that is
  not valid userinfo JSON
- **THEN** the server responds 401

#### Scenario: SSRF guard
- **WHEN** `server_name` contains a scheme, path, or userinfo (e.g. `https://x/y`)
- **THEN** the request fails validation with 400 and no outbound request is made

### Requirement: blob upload
`POST /v1/blobs` (authenticated) SHALL store the request body verbatim as an
opaque blob keyed by a fresh server-generated UUID and return 201 with
`{id, size}`. The request body SHALL be streamed to storage without
buffering the entire body in memory. Bodies above the configured limit SHALL
be rejected with 413; an over-limit or failed upload SHALL leave no
retrievable blob. The server SHALL NOT inspect, transcode, or re-encrypt
blob contents.

#### Scenario: chunked upload roundtrips
- **WHEN** an authenticated client uploads bytes using chunked
  transfer-encoding
- **THEN** it receives 201 with `{id, size}` and downloading `id` returns
  the exact uploaded bytes

#### Scenario: over-limit stream is aborted
- **WHEN** an upload exceeds the configured size cap mid-stream
- **THEN** the server responds 413 and no blob from that upload is
  retrievable afterwards

### Requirement: blob download
`GET /v1/blobs/{id}` (authenticated) SHALL return the exact stored bytes
with `application/octet-stream`, streamed from storage without buffering the
entire blob in memory. Unknown ids SHALL return 404. Ids SHALL be validated
as UUIDs and malformed ids SHALL return 400.

#### Scenario: exact bytes returned
- **WHEN** an authenticated client downloads a stored blob
- **THEN** the response has content type `application/octet-stream` and its
  body is byte-identical to the upload

#### Scenario: malformed id
- **WHEN** the requested id is not a UUID
- **THEN** the server responds 400

### Requirement: session persistence
Session tokens SHALL survive server restarts: a session issued before a
restart SHALL resolve to the same Matrix user id after the restart. The
persistent registry SHALL tolerate a corrupted trailing record (e.g. a crash
mid-write) without losing previously issued sessions. Session storage SHALL
be swappable behind a storage abstraction, with a file-backed registry as
the production default and configuration by environment variable.

#### Scenario: restart preserves sessions
- **WHEN** a session is issued and the server is restarted against the same
  registry location
- **THEN** the session still authenticates requests as the same user id

#### Scenario: corrupt tail is tolerated
- **WHEN** the registry's last record is truncated or corrupt
- **THEN** the registry still loads and all previously valid sessions
  resolve

### Requirement: structured access log
The server SHALL emit one JSON access-log line per completed HTTP request to
stderr with at least: timestamp, request id, method, path, status code,
latency, and the authenticated user id when known. Every response SHALL
carry its request id in an `x-request-id` header. The access log SHALL NOT
contain blob contents, request or response bodies, `Authorization`
credentials, OpenID tokens, or query strings.

#### Scenario: one line per request
- **WHEN** any HTTP request completes
- **THEN** exactly one JSON line with the required fields is emitted to
  stderr, and the response carries an `x-request-id` header matching it

#### Scenario: secrets never logged
- **WHEN** a request carries a `Bearer` session, a login posts an OpenID
  token, and blobs are uploaded and downloaded
- **THEN** no access-log line contains the session token, the OpenID token,
  or any blob bytes

### Requirement: deployment
The server SHALL ship as a single static binary configured by environment
variables (`ZIKOS_BIND`, `ZIKOS_BLOB_DIR`, `ZIKOS_MAX_BLOB_BYTES`,
`ZIKOS_SESSION_FILE`), SHALL log to stderr, and SHALL run behind any
TLS-terminating reverse proxy (Caddy recommended).

#### Scenario: env-configured startup
- **WHEN** the binary starts with the environment variables set
- **THEN** it serves on `ZIKOS_BIND`, storing blobs under `ZIKOS_BLOB_DIR`
  and sessions in `ZIKOS_SESSION_FILE`

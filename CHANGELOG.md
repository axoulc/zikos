# Changelog

All notable changes to Zikos are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and versions follow [Semantic Versioning](https://semver.org/). Entries are
written from the user's perspective (what changed, not which files moved).
Every commit adds its entry under `## [Unreleased]` — see `AGENTS.md`.

## [Unreleased]

### Added
- Main app UI (`platform-app` capability): adaptive navigation with three
  always-visible destinations — Conversations, Files, Record — as a bottom
  bar on phones and a rail on desktop, with Settings one gear-tap away.
- Conversations screen: the account's rooms with name, last-activity
  snippet, time, and a shape-differentiated encryption glyph; the list
  updates live, shows a signed-out state until a login flow exists, and
  tapping a conversation selects it (bold name + accent edge) as the one
  Files and Record operate on.
- Files screen: the current conversation's recording tree with folders
  first and names sorted, duration and format chips per recording, amber
  pending markers on unsynced changes, breadcrumb navigation, a
  pick-a-conversation prompt, and a details sheet on the third tap from
  the main screen.
- Record screen: one-tap start with a pause-aware timer, pause/resume and
  stop controls, and the finalize-or-discard prompt when a previous
  session was interrupted; recording keeps running while you switch tabs,
  with an active indicator on the Record destination.
- Settings screen: homeserver, blob server URL, and default recording
  format (FLAC by default, WAV keeps the raw capture), validated and
  persisted by the Rust core — invalid URLs are rejected with a visible
  error and never saved.
- Pause/resume in the dictaphone engine: pausing stops writing samples
  (audio arriving while paused is dropped), the elapsed timer freezes,
  and the finalized take's duration counts only recorded audio.
- Finalize-format selection in the dictaphone engine: takes finalize to
  FLAC (unchanged default) or keep the raw WAV unchanged when configured.
- The whole UI is bound to the Rust core through a new flutter_rust_bridge
  crate (`zikos-bridge`, with the generated Dart bindings compiled into
  the app via cargokit): Dart renders and routes only, every screen is
  widget-tested against an in-memory fake of the bridge.

### Added
- `file-tree` capability spec in `openspec/specs/` updated with the
  file-exchange requirements merged from the archived
  `implement-file-exchange` change: blob-referencing file nodes (id +
  per-file key + metadata inside the encrypted event, with legacy-event
  compatibility), the publish flow (validate → encrypt → upload →
  file-create event), download-and-decrypt on another device with
  fail-closed integrity errors, and downloadable `Conflicts/` copies.
- End-to-end file exchange (new `zikos-exchange` crate + `file-tree`
  payload extension): publishing a finished recording encrypts it on the
  device with a fresh per-file key (AES-256-GCM), uploads only ciphertext
  to the blob server, and shares it as a tree event carrying the blob id,
  the key, and the take's metadata inside the encrypted room — so opening
  the file on another member's device downloads and decrypts it with no
  key management. File nodes in the shared tree now show which blob they
  reference, and copies preserved under `Conflicts/` stay downloadable.
  The blob server still sees nothing but opaque ciphertext: no audio, no
  file names, no metadata (proven by an end-to-end test that moves a real
  recorded take between two simulated devices over a live local server).
- `file-tree` capability spec in `openspec/specs/`, merged from the
  archived `sync-tree-over-matrix` change: event-sourced tree, tree
  operations, tree events over Matrix (custom type, idempotent,
  offline queue with pending states), history replay, and deterministic
  last-writer-wins conflict convergence with `Conflicts/` preservation.
- Shared file tree synced over Matrix (`file-tree` capability, new
  `zikos-core` sync engine + `zikos-matrix` transport): tree operations
  travel as encrypted `zikos.tree.op` room events through the durable
  offline send queue; every client replays the room's event log (stored
  encrypted at rest, deduplicated) into the same tree on start; offline
  edits appear immediately marked pending until their room echo arrives.
  Concurrent edits converge deterministically on every device —
  last-writer-wins by origin server timestamp with event-id tiebreak —
  and losing states are preserved under a `Conflicts/` folder instead of
  vanishing (rules proven by property tests, including arrival-order
  independence).
- Hardened audio blob server (`zikos-server`): session tokens now survive
  server restarts via a persistent file-backed registry (owner-only
  permissions, crash-tolerant), every response carries an `x-request-id`
  echoed in a structured JSON access log on stderr (method, path, status,
  latency, user id — never blob contents, bodies, or credentials), and the
  Matrix OpenID verifier is now integration-tested against a mock
  homeserver speaking the real federation protocol, including the SSRF
  guard (invalid homeserver names never trigger an outbound request).
- `audio-storage-server` capability spec in `openspec/specs/`, merged from
  the archived `harden-blob-server` change: health, Matrix OpenID session,
  streaming blob upload/download, persistent sessions, structured access
  log, and deployment contract.
- Dictaphone capture engine in `zikos-audio` (desktop-first): records
  16-bit PCM from the system input (48 kHz stereo by default, falling
  back to the nearest device-supported format and reporting that
  honestly), streams every sample to disk as WAV while recording, and on
  stop finalizes the take to lossless FLAC (pure-Rust `flacenc`) with
  `take-YYYYMMDD-HHMM` local-time naming and full recording metadata.
  Takes interrupted by a crash or OS kill are detected on next start and
  repaired for finalize-or-discard — no captured audio is silently lost.
- Project bootstrap: Cargo workspace with `zikos-core` (event-sourced shared
  file tree + recording metadata), `zikos-crypto` (AES-256-GCM client-side
  file encryption), `zikos-audio` (WAV encode/parse, recorder policy),
  `zikos-server` (axum audio blob server with Matrix-OpenID auth and size
  caps), and the `zikos-app` Flutter client shell (Material 3, one-tap
  record button, widget-tested) over the Rust core — the
  `flutter_rust_bridge` wiring lands with the UI change.
- Spec- and test-driven development via OpenSpec: source specs for seven
  capabilities in `specs/`, prompt playbook in `specs/PROMPTS.md`, workflow
  mandated in `AGENTS.md`.
- CI on every push: rustfmt check, clippy `-D warnings`, full test suite.

### Changed
- Blob uploads and downloads stream: the server pipes request bodies to
  disk chunk by chunk (temp file + atomic rename, size cap enforced
  mid-stream with nothing left behind on abort) and serves downloads
  straight from the file handle — a recording upload no longer buffers a
  whole blob (up to 512 MiB) in memory.

### Fixed
- The Matrix OpenID access token is now URL-encoded when calling the
  homeserver federation endpoint instead of being interpolated raw.

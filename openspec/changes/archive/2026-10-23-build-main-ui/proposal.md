# Proposal: build-main-ui

Source spec: `specs/platform-app.md` (source of truth); record-control
interaction per `specs/audio-recording.md` ("one-tap recording").

## Why

`apps/zikos-app` is a widget-tested shell: one button toggling a boolean.
The Rust core (file tree, capture, Matrix messaging) has no surface yet —
musicians cannot browse conversations, navigate the recording tree, record
with a timer, or point the app at their servers.

## What Changes

- Main UI (Flutter/Dart, Material 3): three destinations — Conversations,
  Files, Record — bottom navigation on mobile, navigation rail on desktop.
- Conversations screen: rooms from `zikos-matrix` (name, encryption state,
  last activity), live via bridge events.
- Files screen: tree browser over the current conversation's `zikos-core`
  `FileTree` — sorted folder listing with duration/format, breadcrumb
  navigation, any recording reachable within three taps.
- Record screen: one-tap start, pause/resume, stop + finalize to FLAC via
  `zikos-audio`; elapsed timer; interrupted-session recovery prompt.
- Settings screen: homeserver, blob server URL, default recording format
  (FLAC default), persisted Rust-side.
- `zikos-audio` gains pause/resume on `RecorderSession` (spec'd
  interaction, missing today).
- New flutter_rust_bridge crate at the platform edge (`apps/zikos-app/rust`)
  exposing the core to Dart; Dart renders and routes only.

## Capabilities

### New Capabilities

- `platform-app`: the main app UI — navigation, conversations list, tree
  browser, record screen, settings, and the flutter_rust_bridge contract.

### Modified Capabilities

- `audio-recording`: add pause/resume to the capture session; elapsed time
  excludes paused spans.

## Impact

- Code: `apps/zikos-app` (Dart UI + new non-workspace bridge crate),
  `crates/zikos-audio` (pause/resume on the capture session),
  `crates/zikos-matrix` (read-only `conversations()` listing — rooms,
  names, encryption state, last activity — required by the conversations
  screen; amended during apply).
- Deps: `flutter_rust_bridge` (crate + pub package); state-management
  package chosen in design.
- Tests: widget tests for every screen; Rust unit tests for pause/resume.

## Non-goals

- Login/registration UI, message timeline and composer (follow-up change).
- Sign-out with local data wipe; Android quick-settings tile / iOS App
  Shortcut; in-app playback; upload/download progress UI.
- Mobile capture adapters (Oboe/CoreAudio); Web/WASM target.

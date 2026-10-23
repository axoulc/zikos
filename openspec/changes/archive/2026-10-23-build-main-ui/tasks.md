# Tasks: build-main-ui

Reference: spec deltas in `specs/` of this change; decisions in design.md.
Gate for every task: its tests pass, plus `flutter analyze` (Dart tasks) or
`cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --all`
(Rust tasks).

## 1. zikos-audio — pause/resume

- [x] 1.1 Test: mock-source unit test — after `pause()`, delivered samples
  append no bytes; after `resume()`, writes continue into the same take;
  `pause()` on a non-in-progress session returns an invalid-state error.
  Implement the paused flag and the `pull()` write guard on
  `RecorderSession`.
- [x] 1.2 Test: mock-source finalize test — record 2 s, pause 5 s, resume,
  record 3 s, stop, finalize; take duration is 5 s ± tolerance and
  metadata/manifest reflect only recorded audio. Adjust elapsed/duration
  derivation if the paused span leaks anywhere.

## 2. zikos-audio — finalize format selection

- [x] 2.1 Test: staged-session finalize with WAV requested keeps the raw
  `take.wav` bytes unchanged and records `Wav` in `RecordingMeta`; with no
  explicit request, FLAC output and `Flac` metadata remain as today.
  Thread the configured `AudioFormat` through `finalize` (new parameter or
  sibling method — keep the existing call sites working).

## 3. zikos-bridge — crate, settings, view models

- [x] 3.1 Test: `cargo test -p zikos-bridge` — settings round-trip through
  a temp config dir, defaults (`flac`, empty URLs), and URL validation
  (`url` crate: reject non-https/malformed). Scaffold
  `apps/zikos-app/rust` as workspace member `zikos-bridge` with the
  `Settings` model + JSON persistence (D4).
- [x] 3.2 Test: bridge unit test — a `FileTree` fixture renders a
  children snapshot with folders first, each group name-sorted, file rows
  carrying duration/format, pending markers joined from delivery state.
  Implement the pure listing/ordering/join functions over `zikos-core` +
  `zikos-matrix` types (D7).
- [x] 3.3 Test: `cargo test -p zikos-bridge` smoke test initializing the
  Rust-side app-state singleton (D3); `flutter analyze` stays clean.
  Wire flutter_rust_bridge v2 (pinned pub + Cargo deps), codegen config,
  generated module with scoped `#![allow]`s, and expose the settings,
  conversations, tree-listing, and recorder command/stream surface (D1–D3).

## 4. Flutter shell

- [x] 4.1 Test: widget test — under 600 dp the shell shows a
  `NavigationBar` with Conversations/Files/Record, above it a
  `NavigationRail`; taps switch screens; the app-bar gear opens Settings.
  Implement the adaptive shell (D9) plus the abstract Dart `ZikosApi`
  facade and an in-test fake (D2).
- [x] 4.2 Test: widget test — emitting `recording` on the api's recorder
  stream lights the active-recording indicator on the Record destination
  from any tab. Implement the indicator driven by the bridge stream.

## 5. Conversations screen

- [x] 5.1 Test: widget tests — rows render name, snippet, time, and
  encryption glyph (shape-differentiated, D10); a stream update reorders/
  updates rows without reload; with no session the signed-out state shows.
  Implement the list screen over the conversations stream.
- [x] 5.2 Test: widget test — tapping a row sets it as current
  conversation (visible via bold name + accent edge beyond the tint, D10)
  and Files/Record chips reflect it. Implement current-conversation
  selection.

## 6. Files screen (tree browser)

- [x] 6.1 Test: widget test — a folder snapshot renders folders first,
  name-sorted, files with duration + format chip, and an amber pending
  chip on unsynced entries. Implement the browser list.
- [x] 6.2 Test: widget tests — breadcrumb taps open ancestors; opening
  Files with no current conversation shows the pick-a-conversation prompt.
  Implement breadcrumb + empty-selection states.
- [x] 6.3 Test: widget test — tapping a recording (third tap from the main
  screen) opens a details sheet with title, duration, format, and tree
  path. Implement the details sheet (no playback — non-goal).

## 7. Record screen

- [x] 7.1 Test: widget tests — from idle, the record control starts and
  the medium-weight tabular timer ticks from the api's elapsed stream;
  pause freezes the timer and toggles the control to resume; stop calls
  the api command and returns the screen to idle. Implement the record
  screen per the preview (status dot, controls, FLAC destination hint).
- [x] 7.2 Test: widget test — api reporting an interrupted session makes
  the screen show the finalize-or-discard prompt before a new take; each
  action invokes the matching api command. Implement the recovery prompt.
- [x] 7.3 Test: widget test — starting a take, switching to another tab
  and back keeps the session recording with the indicator on. Verify
  capture-survives-navigation against the fake api.

## 8. Settings screen

- [x] 8.1 Test: widget tests — fields show current values; a valid save
  persists via the api; a malformed URL shows the red error state and
  keeps the previous value; the format dropdown defaults to FLAC (WAV
  selectable, Opus hidden until supported). Implement the screen over the
  bridge settings model.

## 9. Wiring and gates

- [x] 9.1 Test: the app's smoke widget test pumps the real app root with
  the fake api injected; production boot initializes FRB behind a guard.
  Swap the old shell home for the new shell, delete the replaced shell
  test, and run all gates: `flutter test`, `flutter analyze`,
  `cargo t`, `cargo lint`, `cargo fmtcheck`.

# Design: build-main-ui

## Context

The Rust core is complete enough to drive a UI: `zikos-matrix` (rooms,
messages, tree events, delivery state), `zikos-core` (`FileTree`,
`RecordingMeta`), `zikos-audio` (`RecorderSession` → `StagedSession` →
`FinalizedTake`, interrupted-session recovery). `apps/zikos-app` is a
widget-tested shell. See proposal.md for motivation; the spec deltas list
the required behavior.

Four HTML previews (Conversations, Files, Record, Settings) were rendered
and judged by the visual-judge-flutter agent: all PASS, with P3 polish
notes folded into the decisions below.

## Goals / Non-Goals

Goals: a UI whose every byte of domain state and behavior comes from Rust
over flutter_rust_bridge (FRB); widget-testable screens without a device.
Non-goals (design-level): login UI, timeline/composer, playback, upload
progress, OS-level shortcuts — unchanged from the proposal.

## Decisions

### D1 — Bridge crate: `apps/zikos-app/rust` (`zikos-bridge`), workspace member
A thin Rust crate at the platform edge wrapping the core crates into
FRB-friendly functions and streams (path deps on `crates/*`). It joins the
Cargo workspace so `cargo t` / `clippy -D warnings` / `fmt` gates cover it;
FRB-generated code is isolated in one module with targeted `#![allow]`s so
the gate stays meaningful on hand-written code. Alternative rejected:
non-member crate — escapes the mandatory gates.

### D2 — Dart `ZikosApi` facade over generated bindings
Screens depend on an abstract `ZikosApi` (typed models, streams, commands).
The production implementation delegates 1:1 to generated FRB functions;
widget tests inject a fake. This is the only seam that keeps screens
testable without a Rust dylib, and it puts zero logic in Dart (facade is
pure delegation). Generated model classes (FRB mirrors of Rust structs)
are used directly as the app's view models.

### D3 — Rust-side app state singleton
The Matrix client, recorder session, current conversation, and settings
live in a `zikos-bridge` singleton (behind `OnceLock`, on FRB's tokio
runtime). This is why capture survives tab switches: no screen owns the
recorder. Streams (conversations, tree updates, recording ticks) are FRB
streams fanned out from the existing `broadcast` receivers.

### D4 — Settings persisted in Rust
`Settings { homeserver, blob_server_url, default_format }` serialized with
serde to JSON in the OS config dir (directories-next or FRB's path helper
— decided at apply time, both pure Rust). Validation in Rust via the
`url` crate (pure Rust, ubiquitous): non-https-URL → typed error → red
field state in Dart. `shared_preferences` rejected: validation and
defaults are business logic, which is Rust's job.

### D5 — Pause/resume in `zikos-audio`
`RecorderSession` gains `pause()`/`resume()` flipping a flag; while
paused, `pull()` drains the source but writes nothing. Duration stays
derived from bytes written, so paused spans are excluded everywhere
(timer, finalize, metadata) with no clock bookkeeping. Invalid-state error
when not recording. Tests use the existing mock sample source.

### D6 — Finalize format selection
`StagedSession::finalize` takes the configured `AudioFormat` (zikos-core
enum already has Flac/Wav): Flac transcodes (current path), Wav keeps
`take.wav` as the take and records `Wav` in metadata. Opus stays
unimplemented (parse-only in settings until a later change).

### D7 — Presentation ordering in the bridge
Folders-before-files and name sorting are computed in `zikos-bridge` over
`FileTree::children()`; `zikos-core` stays I/O-free and untouched. Pending
markers come from `zikos-matrix` delivery state joined onto tree nodes in
the bridge.

### D8 — Three-tap guarantee via filing depth
The record flow files takes one folder deep (`<conversation>/<date>/`),
so tab → folder → take is exactly three taps (matches the spec scenario).
Deeper user-created nesting is allowed and may exceed three taps —
accepted trade-off; recent-takes/search are future changes.

### D9 — Navigation shell
`LayoutBuilder` breakpoint (≤600 dp): `NavigationBar` on phones,
`NavigationRail` on desktop; three destinations plus Settings reached
from the app-bar gear. Recording state is exposed as a stream so the
Record destination shows an active indicator from any tab.

### D10 — Preview polish guidance (from visual-judge review)
Medium-weight tabular timer digits; current-conversation row gets a
secondary cue beyond the tint (bold name + accent edge); encryption state
distinguished by glyph shape, not color alone; file rows show duration +
format chip; pending chip in amber with sync glyph (as in the previews).

## Risks / Trade-offs

- [FRB codegen drift / clippy noise on generated code] → regenerate in a
  dedicated task; `#![allow]` scoped to the generated module only.
- [cpal devices unavailable in CI] → all Rust audio tests keep using the
  mock source; widget tests never touch audio.
- [Deep nesting breaks the 3-tap budget] → D8 default depth + breadcrumb
  back-jumps; revisit with search if users nest deeply.
- [FRB version churn] → pin the v2 minor in both pubspec and Cargo.

## Migration Plan

Additive: bridge crate and new screens land beside the shell;
`main.dart` swaps its home to the new shell as the last wiring task;
existing shell widget test is replaced by the new shell's tests. Rollback
is a plain revert (no data, format, or protocol changes).

## Open Questions

None blocking — D4's config-dir helper choice is deferrable to apply.

# Proposal: implement-dictaphone-capture

Source spec: `specs/audio-recording.md` (source of truth) — capture-engine
scope, desktop-first.

## Why

Zikos's reason to exist is capturing a take now, yet `zikos-audio` today
only encodes WAV buffers in memory: there is no device capture, no disk
streaming, and no FLAC output. This change builds the dictaphone engine so
the app can record, finalize, and recover takes.

## What Changes

- `zikos-audio` gains a capture pipeline behind a sample-source trait:
  - PCM capture via cpal on desktop, defaulting to 48 kHz stereo i16
    (`RecorderConfig` already fixes this policy);
  - streaming WAV writer that appends PCM to disk as it arrives and
    patches the RIFF header on close (crash-safe even unpatched);
  - finalize: lossless transcode WAV → FLAC via pure-Rust `flacenc`, then
    a `zikos-core` `RecordingMeta` with default title `take-YYYYMMDD-HHMM`
    (local timezone, anchored to recording start);
  - interrupted-session recovery: detect an in-progress session on next
    start, repair its header, and expose finalize-or-discard.
- Root `Cargo.toml` gains `cpal`, `flacenc`, and a local-time crate as
  workspace dependencies.

## Capabilities

### New Capabilities

- `audio-recording`: the dictaphone capture engine — PCM capture policy,
  streaming WAV capture, FLAC finalization, `RecordingMeta` with default
  take naming, and interrupted-session recovery.

### Modified Capabilities

(none — `openspec/specs/audio-recording/` does not exist yet)

## Impact

- Code: `crates/zikos-audio` only (new modules; no breaking API changes
  to existing items). `zikos-core` is consumed, not modified.
- New workspace dependencies: `cpal` (spec-mandated; ALSA/CoreAudio under
  the hood), `flacenc` (pure Rust), plus a pure-Rust local-time crate.
- Testing: deterministic sine-wave fixtures through a mock sample source;
  no microphone and no real audio devices in tests.

## Non-goals

- Mobile (Oboe/CoreAudio) capture adapters — desktop cpal only.
- UI wiring in `apps/zikos-app` (one-tap control, timer, backgrounding);
  the <300 ms start budget is honored at engine level, UI lands later.
- Opus "voice note" mode; input monitoring; multi-input capture.
- Uploading takes to the file tree / blob server (later changes).

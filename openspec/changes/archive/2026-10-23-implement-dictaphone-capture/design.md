# Design: implement-dictaphone-capture

## Context

`zikos-audio` today holds an in-memory WAV encoder/inspector
(`pcm_to_wav`/`wav_info`, canonical 44-byte RIFF) and `RecorderConfig`
policy (48 kHz stereo FLAC by default). This change grows it into the
dictaphone engine of `specs/audio-recording.md`: capture → streamed WAV
staging → FLAC finalize → `RecordingMeta`, plus crash recovery. See
proposal.md for motivation; the spec delta lists the observable
behaviors.

## Goals / Non-Goals

**Goals:**
- Whole pipeline testable deterministically — sine-wave fixtures in, no
  microphone, no real audio device in any test.
- Crash-safety by construction: frame count always recoverable from the
  staged file, header patch is an optimization, not a correctness
  dependency.
- No async runtime in the crate (stays embeddable from
  flutter_rust_bridge and plain Rust alike).

**Non-Goals:** mobile capture adapters (Oboe/CoreAudio via cpal arrive
with the Android/iOS changes); UI wiring; Opus mode; upload to the file
tree; resampling (fallback picks a supported rate instead).

## Decisions

### D1 — Dependencies (workspace-pinned)
- `cpal` 0.15 — cross-platform audio I/O, the spec-mandated choice.
  Linux builds bind system ALSA (alsa-sys); this is the crate's one C
  dependency, same precedent as matrix-sdk's bundled SQLite. Devices are
  only touched behind the adapter (D7); tests never open one.
- `flacenc` 0.4 — pure-Rust FLAC encoder (spec-mandated), maintained,
  no C. Encode happens at finalize, so its speed is not on the realtime
  path.
- `jiff` 0.2 — pure-Rust date/time with real local-timezone support.
  Alternatives: `chrono` + `chrono-tz` (two crates, tz handling more
  awkward); `time` (local tz historically weak on Unix). The
  `embedded-tzdb` feature is deferred to the mobile change; desktop uses
  system tzdata.
- Dev-only: `claxon` (pure-Rust FLAC decoder) to verify transcodes
  against an independent implementation — hound reads WAV, not FLAC.

### D2 — Sample-source abstraction for testability
Capture logic consumes a `SampleSource` trait (pull-based: chunks of
interleaved i16 plus the source's actual sample rate/channels). Tests
drive the entire pipeline with a deterministic `SineSource`. The cpal
adapter (D7) is the only code that touches a device, and it stays thin.
Alternative — model the pipeline around cpal streams directly: rejected;
untestable in CI and it would leak cpal types into the API.

### D3 — Streaming WAV writer, crash-safe by format
`StreamingWavWriter` writes the same canonical 44-byte header as
`pcm_to_wav` (sizes zeroed), appends PCM chunks with periodic flush, and
seeks back to patch RIFF/data sizes on close. Invariant: frames =
(file_len − 44) / block_align, valid even when the process died before
patching — recovery just recomputes and patches. Alternative — raw PCM
staging with header written only at close: rejected, same recoverability
but the staged file is not inspectable mid-take.

### D4 — Session directory layout and atomic state
`<sessions_root>/<uuid>/` holds `take.wav` (staging), `session.json`
(config, actual format, `started_at`, state `in-progress`|`staged`).
`session.json` is written atomically (temp file + rename) — it is the
recovery marker and the durable record of `started_at`, so naming and
metadata survive crashes. Finalize writes `take.flac` + `meta.json` in
place, deletes `take.wav`, marks state `finalized`, and returns the
paths; moving takes into the file tree is a later change.

### D5 — Naming anchored to start time (recorded assumption)
`take-YYYYMMDD-HHMM` is derived from the persisted `started_at` in local
time. The source spec's scenario says "a take finishes at 10:15 →
take-…-1015"; we anchor to start because (a) an interrupted take has no
reliable finish moment yet must still get a stable name, and (b) the
title then agrees with `RecordingMeta.started_at`. Starts and finishes
within the same minute satisfy the source scenario verbatim.
`default_title(unix_secs, tz)` is a pure function → deterministic tests.

### D6 — Threading model
The cpal callback only pushes i16 chunks into an mpsc channel (no
allocation-heavy work, no I/O on the audio thread). A session-owned
writer thread drains the channel into the `StreamingWavWriter`. Stop =
drop/flush the source, drain the channel, patch, mark staged. Sync
`std` threads only.

### D7 — Cpal adapter and format fallback
`CpalSource` opens the default input device requesting the configured
format; device-supported configs are negotiated by a pure function
(`negotiate(requested, supported) -> actual`) that picks the nearest
supported rate and channel count. That negotiation is unit-tested
against synthetic config lists; the live-stream wrapper itself stays a
thin, device-free-untested shell (smoke-testing a real microphone is
out per AGENTS.md).

## Risks / Trade-offs

- [cpal device quirks across desktop backends] → the adapter is isolated
  behind `SampleSource`; actual format always reported, never assumed.
- [flacenc encode time on long takes] → runs at finalize off the audio
  path; block size left at encoder defaults.
- [WAV 4 GiB ceiling inherited from RIFF] → accepted for now (≈3 h at
  48 kHz stereo 16-bit); a length-prefixed raw staging format can replace
  `take.wav` later without changing the spec'd behavior.
- [Timing-based requirements (<300 ms start) not spec-tested] → kept as
  an engine design goal (start = create dir + open file, no device scan
  beyond the default device) rather than a flaky timing test.

## Migration Plan

Additive modules inside `zikos-audio`; no existing item changes. Rollback
= remove the new modules and the workspace-dep entries.

## Open Questions

None blocking. (Exact `flacenc` minor version and encoder block size are
settled in task 1 and do not change the approach.)

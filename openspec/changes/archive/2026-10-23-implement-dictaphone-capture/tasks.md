# Tasks: implement-dictaphone-capture

Every task follows strict TDD: write the failing test named after "Test:",
watch it fail, then implement minimally. Gate before ticking any task:
`cargo t`, `cargo lint`, `cargo fmtcheck` all clean. No microphone and no
real audio device in any test — sine-wave fixtures only (AGENTS.md).

## 1. Dependencies and sample-source abstraction

- [x] 1.1 Add workspace-pinned `cpal`, `flacenc`, `jiff` to the root
  `Cargo.toml`; `zikos-audio` takes them (plus dev-deps `claxon`,
  `tempfile`). Test: `sine_source_matches_reference()` — a `SineSource`
  fixture (0.5 s, 440 Hz, 48 kHz stereo) yields interleaved i16 samples
  equal to an independently computed reference sine (fails until the
  `SampleSource` trait + `SineSource` exist and deps resolve).
- [x] 1.2 Define the `SampleSource` contract (chunk of interleaved i16 +
  actual sample rate/channels, pull until exhausted). Test:
  `source_reports_actual_format()` — a source configured 44.1 kHz mono
  reports (44_100, 1) even when the recorder requested 48 kHz stereo.

## 2. Streaming WAV writer (crash-safe, design D3)

- [x] 2.1 `StreamingWavWriter::create` writes the canonical 44-byte
  header (sizes zeroed) and appends PCM chunks. Test:
  `streamed_header_is_canonical_44_bytes()` mirrors the existing
  in-memory header test; `append_grows_file_by_bytes()` checks chunk
  appends land on disk (tempfile).
- [x] 2.2 `close()` patches RIFF/data sizes. Test:
  `streamed_wav_roundtrips_through_hound()` — stream 1 s of 440 Hz sine
  (48 kHz stereo), close, `wav_info` reports exact rate/channels/frames
  and duration 1.0 s; byte-equal to `pcm_to_wav` of the same samples.
- [x] 2.3 Crash-safety repair. Test: `unpatched_file_is_repairable()` —
  simulate a crash (header sizes left zeroed, data on disk), then
  `repair` recomputes frames = (file_len − 44) / block_align and patches;
  hound parses it with the exact frame count.

## 3. Recorder session lifecycle (design D4/D6)

- [x] 3.1 `RecorderSession::start(config, source, sessions_root)`: creates
  `<sessions_root>/<uuid>/take.wav` + atomically written `session.json`
  (config, actual format, `started_at`, state `in-progress`), runs the
  drain loop. Test: `start_creates_in_progress_session()` — after
  recording a 0.5 s sine, the WAV holds all frames, `session.json` parses
  with state `in-progress` and the injected `started_at`.
- [x] 3.2 `stop()` drains, patches, marks state `staged`. Test:
  `stop_yields_valid_staged_wav()` — stop after a 0.5 s sine; staged WAV
  parses via hound with duration 0.5 s ± 1 ms.
- [x] 3.3 Actual-format fallback is honored end to end. Test:
  `session_uses_source_actual_format()` — a 44.1 kHz mono source yields a
  staged WAV and `session.json` reporting 44.1 kHz mono (spec scenario:
  device cannot match the request).

## 4. Default take naming (design D5)

- [x] 4.1 Pure `default_title(unix_secs, tz) -> take-YYYYMMDD-HHMM` via
  jiff local time. Tests: `title_uses_local_timezone()` — fixed instant
  renders `take-20261022-1015` in a +02:00 zone and
  `take-20261022-0815` in UTC; `title_flips_at_midnight_boundary()`.

## 5. FLAC finalization (design D1/D4)

- [x] 5.1 WAV → FLAC transcode via flacenc. Test:
  `transcode_is_lossless()` — transcode 1 s sine WAV, decode with claxon,
  decoded PCM is bit-identical to the captured samples and duration
  matches (spec scenario: take finalization, scaled fixture).
- [x] 5.2 `finalize()`: transcode staged WAV → `take.flac`, build
  `RecordingMeta` (format FLAC, persisted `started_at`, measured
  duration, actual rate/channels, bpm/comment none, default title from
  task 4), write `meta.json`, delete `take.wav`, mark `finalized`. Tests:
  `finalize_produces_meta_and_flac()` (meta validates, `wav_info` of the
  staging file no longer exists) and
  `finalize_60s_take_duration_within_tolerance()` — 60 s synthetic stereo
  sine, decoded duration 60 s ± 0.5 s (spec scenario verbatim).

## 6. Interrupted-session recovery (spec: interrupted sessions)

- [x] 6.1 `recoverable_sessions(sessions_root)` lists sessions whose
  `session.json` is `in-progress`. Test: `scan_finds_only_in_progress()` —
  plant one in-progress, one staged, one finalized dir; exactly the
  in-progress one is returned.
- [x] 6.2 `recover()` repairs the partial WAV header. Test:
  `repaired_partial_has_exact_frames()` — plant an in-progress session
  with unpatched header (simulated OS kill, 30 s-equivalent data);
  recovery patches it so hound parses the exact on-disk frame count.
- [x] 6.3 Recovered sessions finalize or discard. Tests:
  `recovered_finalize_uses_persisted_started_at()` (title from the
  pre-crash `started_at`, FLAC + meta produced like task 5.2) and
  `discard_removes_only_that_session()` (dir gone, sibling sessions
  untouched).

## 7. Cpal desktop adapter (design D7)

- [x] 7.1 Pure `negotiate(requested, supported_configs) -> actual`: picks
  the nearest supported sample rate and channel count, preferring
  stereo ≥ mono and rates closest to 48 kHz. Test:
  `negotiation_falls_back_to_nearest()` — requested 48 kHz stereo vs a
  device offering [44.1 kHz mono, 48 kHz mono] → 48 kHz mono; vs
  [44.1 kHz stereo] → 44.1 kHz stereo; empty support list is an error.
- [x] 7.2 `CpalSource` shell: default input device, i16 stream, callback
  pushes chunks into an mpsc channel feeding `SampleSource`. Test:
  `channel_bridge_yields_source_chunks()` — a fake callback pushing
  chunks surfaces them as interleaved source output (no device opened);
  the device path itself stays untested per AGENTS.md.

## 8. Wrap-up

- [x] 8.1 Doc pass on all public items (trait, session, writer, naming,
  recovery, adapter; no cpal types leaked beyond the adapter), add the
  `## [Unreleased]` CHANGELOG entry (feat: dictaphone capture engine in
  `zikos-audio`), then full gate: `cargo t`, `cargo lint`,
  `cargo fmtcheck`.

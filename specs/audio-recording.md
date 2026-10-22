# Audio recording (dictaphone) — source spec

## Purpose

The reason musicians open Zikos: capture a take now. Recording MUST be
reachable in one tap from the main screen, start in under 300 ms, and survive
app backgrounding on mobile. Captures land in the conversation's file tree
(see `file-tree.md`) as FLAC by default.

## Requirements

### Requirement: one-tap recording
The record control SHALL be visible on the main screen at all times and SHALL
start a new recording on the first tap, pause/resume on subsequent taps of a
dedicated control, and stop+finalize on the stop control.

#### Scenario: instant capture
- **WHEN** the app is opened and the user taps Record
- **THEN** capture starts within 300 ms and a timer is displayed

#### Scenario: backgrounding
- **WHEN** recording is active and the app goes to background or the screen locks
- **THEN** recording continues and completes normally

### Requirement: capture format
The client SHALL capture PCM audio via cpal (Oboe/AAudio on Android,
CoreAudio on iOS), defaulting to 48 kHz stereo; the raw capture SHALL be
encoded losslessly to FLAC (pure-Rust `flacenc`) on finalize. Opus MAY be
offered later as a "voice note" mode.

#### Scenario: take finalization
- **WHEN** the user stops a 60 s stereo 48 kHz recording
- **THEN** a valid FLAC file exists whose decoded PCM matches the captured
  samples and whose duration is 60 s ± 0.5 s

### Requirement: recording metadata
Each finalized take SHALL carry `RecordingMeta` (title, format, started_at,
duration, sample rate, channels, optional bpm/comment). Default title SHALL
follow `take-YYYYMMDD-HHMM` in the device's local timezone.

#### Scenario: default naming
- **WHEN** a take finishes at 2026-10-22 10:15 local
- **THEN** its default title is `take-20261022-1015`

### Requirement: interrupted sessions
If the process dies or the OS kills capture mid-recording, the client SHALL
recover the partial capture on next start and offer it to the user (finalize
or discard); no captured audio SHALL be silently lost.

#### Scenario: OS kill
- **WHEN** the app is killed while a 30 s recording is in flight
- **THEN** on next start the partial file is detected and proposed for recovery

## Non-goals (for now)

- Input monitoring / live effects.
- Multi-input (multi-mic) capture.

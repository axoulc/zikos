# Spec Delta: audio-recording (new capability)

Derived from `specs/audio-recording.md` (source of truth), scoped to the
capture engine on desktop: PCM capture, streaming WAV staging, FLAC
finalization, take naming and metadata, interrupted-session recovery.
The one-tap UI and mobile (Oboe/CoreAudio) adapters arrive in later
changes.

## Purpose

The dictaphone capture engine: turns live PCM from the system input into
finalized FLAC takes with metadata, streaming every sample to disk so a
crash never loses captured audio.

## ADDED Requirements

### Requirement: streaming capture to disk
A recording session SHALL stream captured interleaved 16-bit PCM to a WAV
file on disk while recording, under a per-session directory that also
records the session's configuration and start time. On a clean stop the
WAV header SHALL reflect the exact number of frames written.

#### Scenario: capture lands on disk
- **WHEN** a session is recording and PCM frames arrive
- **THEN** the session's WAV file on disk grows with the captured frames

#### Scenario: clean stop yields a valid WAV
- **WHEN** a session capturing a sine wave is stopped
- **THEN** the staged WAV parses with the exact sample rate, channel
  count, and frame count captured, and its duration equals the capture
  length

### Requirement: capture format policy
The session SHALL request the configured capture format (default 48 kHz
stereo 16-bit PCM). If the input device cannot provide it, the session
SHALL fall back to the nearest supported configuration and SHALL report
the actual format in all outputs, never the requested one.

#### Scenario: default capture format
- **WHEN** a session starts with default configuration
- **THEN** it targets 48 kHz stereo 16-bit PCM

#### Scenario: device cannot match the request
- **WHEN** the input device supports 44.1 kHz mono but not 48 kHz stereo
- **THEN** the session captures at 44.1 kHz mono and its WAV and metadata
  report 44.1 kHz mono

### Requirement: finalization to FLAC
On finalize the staged WAV SHALL be transcoded losslessly to FLAC (pure
Rust), producing a take whose decoded PCM is bit-identical to the
captured samples, and the staging WAV SHALL be removed only after the
FLAC is complete and verified written.

#### Scenario: take finalization
- **WHEN** the user stops a 60 s stereo 48 kHz recording and it finalizes
- **THEN** a valid FLAC file exists whose decoded PCM matches the captured
  samples and whose duration is 60 s ± 0.5 s

#### Scenario: staged WAV is cleaned up
- **WHEN** finalization completes successfully
- **THEN** the FLAC and metadata remain and the staging WAV is gone

### Requirement: take metadata
Each finalized take SHALL carry `RecordingMeta` (from `zikos-core`) with
title, format FLAC, start time, duration, sample rate, and channels
matching the actual capture; optional bpm/comment default to none. The
default title SHALL follow `take-YYYYMMDD-HHMM` computed from the
recording's start time in the device's local timezone.

#### Scenario: default naming
- **WHEN** a take starts at 2026-10-22 10:15 local
- **THEN** its default title is `take-20261022-1015`

#### Scenario: metadata reflects reality
- **WHEN** a take captured at 44.1 kHz mono finalizes
- **THEN** its metadata reports 44.1 kHz, 1 channel, and passes
  `RecordingMeta` validation

### Requirement: interrupted-session recovery
On start, sessions left in progress by a process death or OS kill SHALL
be detected and their partial capture recovered: the WAV header SHALL be
repaired from the bytes actually on disk, and the recovered take SHALL be
offered for finalize or discard. No captured audio SHALL be silently
lost.

#### Scenario: OS kill
- **WHEN** the process dies while a recording is in flight
- **THEN** on next start the session is detected and its partial WAV is
  repaired to parse with the exact frame count on disk

#### Scenario: recovered take finalizes like a normal take
- **WHEN** a recovered interrupted session is finalized
- **THEN** it produces a FLAC take with metadata whose title is derived
  from the start time persisted before the crash

#### Scenario: discard
- **WHEN** a recovered interrupted session is discarded
- **THEN** its session directory is removed without touching other
  sessions

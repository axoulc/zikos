## ADDED Requirements

### Requirement: pause and resume
The capture session SHALL support pausing an in-progress recording: while
paused, no samples SHALL be written and the session's elapsed time SHALL
exclude paused spans. Resuming SHALL continue the same capture stream, and
stopping after a pause/resume cycle SHALL finalize a take whose duration
counts only recorded audio. Pausing when no capture is in progress SHALL
return an error.

#### Scenario: paused capture writes nothing
- **WHEN** a mock source delivers samples while the session is paused
- **THEN** no additional bytes are appended to the take's WAV data

#### Scenario: duration excludes paused time
- **WHEN** a session records 2 s, pauses for 5 s, resumes, and records 3 s
  more before stopping
- **THEN** the finalized take's duration is 5 s ± tolerance

#### Scenario: pause without an active capture
- **WHEN** pause is requested on a session that is not in progress
- **THEN** the call returns an invalid-state error and nothing is written

### Requirement: finalize format selection
Finalization SHALL produce FLAC by default and SHALL support keeping the
raw WAV capture (no transcode) when the client requests WAV, per the
user's configured default recording format. The finalized take's metadata
SHALL record the chosen format.

#### Scenario: WAV passthrough
- **WHEN** a staged session is finalized with WAV requested
- **THEN** the take's audio file is the captured WAV unchanged and its
  metadata records WAV

#### Scenario: FLAC remains the default
- **WHEN** a staged session is finalized with no explicit request
- **THEN** the take is transcoded to FLAC and its metadata records FLAC

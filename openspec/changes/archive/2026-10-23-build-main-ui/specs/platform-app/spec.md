## Purpose

The Flutter client's main user interface: navigation between conversations,
the shared recording tree, and the dictaphone, plus settings — rendering
state that lives in the Rust core and reaching it only through
flutter_rust_bridge.

## ADDED Requirements

### Requirement: main navigation
The main screen SHALL expose three destinations — Conversations, Files,
and Record — with Record reachable in one tap from any destination. On
narrow screens navigation SHALL be a bottom bar; on desktop it SHALL be a
navigation rail, from the same codebase.

#### Scenario: switch destination
- **WHEN** the user selects a destination
- **THEN** the corresponding screen is shown and the active destination is
  indicated

#### Scenario: record is one tap away
- **WHEN** the user is on the Conversations or Files screen
- **THEN** the Record destination is visible and activates the record
  screen in a single tap

#### Scenario: desktop navigation
- **WHEN** the app runs on a desktop target
- **THEN** destinations appear in a navigation rail and no bottom bar is
  shown

### Requirement: conversations screen
The Conversations screen SHALL list the signed-in account's conversations
with display name, encryption state, and latest activity, and SHALL update
as conversations change. When no account is signed in, it SHALL show a
signed-out state instead of an error.

#### Scenario: live conversation list
- **WHEN** a conversation's name or latest activity changes while the
  Conversations screen is open
- **THEN** the list updates without manual refresh

#### Scenario: signed out
- **WHEN** no Matrix session exists
- **THEN** the screen shows a signed-out explanation and no conversation
  rows

### Requirement: tree browser screen
The Files screen SHALL browse the current conversation's file tree from
local (cached) state: it SHALL list a folder's contents sorted by name with
folders before files, show each recording's duration and format, and offer
breadcrumb navigation. Any recording SHALL be reachable within three taps
from the main screen. Changes not yet synced SHALL carry a visible pending
marker rather than being hidden.

#### Scenario: folder listing
- **WHEN** the user opens a folder
- **THEN** its children are listed sorted by name, folders before files,
  and each file row shows duration and format

#### Scenario: three-tap reachability
- **WHEN** the user taps the Files destination, then a folder, then a
  recording
- **THEN** the recording's details are shown — three taps total from the
  main screen

#### Scenario: breadcrumb navigation
- **WHEN** the user taps an ancestor folder in the breadcrumb
- **THEN** the browser shows that folder's contents

#### Scenario: offline browsing
- **WHEN** the device is offline
- **THEN** the cached tree remains browsable and unsynced changes show a
  pending marker

#### Scenario: no conversation selected
- **WHEN** Files is opened with no current conversation
- **THEN** the screen prompts to pick a conversation instead of showing an
  empty tree

### Requirement: record screen
The record screen SHALL start capture on the first tap of the record
control (within the 300 ms start budget), display an elapsed timer that
excludes paused spans, pause and resume on the pause control, and stop and
finalize on the stop control, after which the finalized take appears in the
current conversation's tree. Capture SHALL continue while the user
navigates between destinations. When an interrupted session is detected,
the screen SHALL offer finalize-or-discard before a new take can start.

#### Scenario: instant capture
- **WHEN** the user taps the record control from an idle screen
- **THEN** capture starts and the timer is running within 300 ms

#### Scenario: pause and resume
- **WHEN** the user pauses for 10 s then resumes and stops
- **THEN** the timer freezes while paused and the finalized take's duration
  excludes the paused span

#### Scenario: stop finalizes a take
- **WHEN** the user stops an active recording
- **THEN** the take is finalized to the configured default format and
  appears in the conversation's tree

#### Scenario: capture survives navigation
- **WHEN** recording is active and the user switches to Conversations or
  Files
- **THEN** capture continues and the Record destination shows an
  active-recording indicator

#### Scenario: interrupted session recovery prompt
- **WHEN** the record screen opens and an interrupted session exists on
  disk
- **THEN** the user is asked to finalize or discard it before starting a
  new take

### Requirement: settings screen
The settings screen SHALL let the user view and edit the homeserver URL,
the blob server URL, and the default recording format (FLAC by default),
persisted so they survive restart. Invalid values SHALL be rejected with a
visible error and not saved.

#### Scenario: persist settings
- **WHEN** the user saves a valid blob server URL and restarts the app
- **THEN** the saved value is shown and used

#### Scenario: reject invalid URL
- **WHEN** the user saves a malformed homeserver URL
- **THEN** a validation error is shown and the previous value is kept

#### Scenario: default format
- **WHEN** the user has never chosen a recording format
- **THEN** settings shows FLAC as the effective default

### Requirement: UI bound to the Rust core
Every screen SHALL obtain state and trigger behavior exclusively through
the flutter_rust_bridge API surface over the Rust crates; Dart SHALL not
compute domain values (tree ordering, elapsed time, validation) itself.

#### Scenario: screens render bridged models
- **WHEN** a screen is supplied bridge data in a widget test
- **THEN** it renders that data directly, performing no Dart-side domain
  computation beyond display formatting

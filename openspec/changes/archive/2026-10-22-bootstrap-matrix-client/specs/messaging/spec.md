# Spec Delta: messaging (new capability, phase 1)

Derived from `specs/messaging.md` (source of truth), scoped to phase 1:
login, sync, one room, text messages. Later phases (SSO, emoji device
verification, multi-device) arrive as their own changes.

## Purpose

End-to-end encrypted Matrix messaging for Zikos: login with a persisted
encrypted session, history restored by sync, conversations that are
encrypted rooms by default, and text messages with delivery states and
offline queueing.

## ADDED Requirements

### Requirement: Matrix login
The client SHALL support login to any Matrix homeserver with
username+password, persist the session across restarts, and store session
and room state encrypted at rest.

#### Scenario: returning user
- **WHEN** the app starts and a persisted session exists
- **THEN** the session is restored and sync resumes without asking for
  credentials

#### Scenario: wrong password
- **WHEN** login fails with `M_FORBIDDEN`
- **THEN** a distinct invalid-credentials error is reported (so the UI can
  show a clear error and keep the entered username) and no session is
  persisted

#### Scenario: homeserver unreachable
- **WHEN** the homeserver cannot be reached
- **THEN** a distinct connection error is reported (not invalid
  credentials) and login can be retried

### Requirement: E2EE by default
Every conversation SHALL be an encrypted Matrix room. The client SHALL NOT
allow creating unencrypted rooms, and SHALL expose each joined room's
encryption state so callers can warn visibly when a room is not encrypted.

#### Scenario: new conversation
- **WHEN** a user creates a conversation
- **THEN** the created room has encryption enabled before the first message
  is sent

#### Scenario: unencrypted room detected
- **WHEN** sync reveals a joined room without encryption enabled
- **THEN** that room's reported encryption state is `unencrypted`

### Requirement: send and receive messages
The client SHALL send and receive text messages carrying sender, room,
timestamp, and body, each outgoing message having a delivery state of
`sending`, `sent`, or `failed`, and SHALL retry failed sends.

#### Scenario: successful send
- **WHEN** a text message is sent while connected
- **THEN** its delivery state starts as `sending` and becomes `sent` once
  the homeserver accepts it

#### Scenario: offline send
- **WHEN** a message is sent while disconnected
- **THEN** it is stored locally marked `sending` and transmitted
  automatically when connectivity returns, surviving an app restart

#### Scenario: failed send retried
- **WHEN** a send fails with a transient error (e.g. network)
- **THEN** the message is marked `failed` and retried automatically, and
  becomes `sent` once a retry succeeds

#### Scenario: receive with key
- **WHEN** a message arrives in an encrypted room and the room key is
  available
- **THEN** the decrypted body is exposed with sender, timestamp, and room

#### Scenario: undecryptable message
- **WHEN** a message cannot be decrypted (e.g. missing room key)
- **THEN** it is exposed with an explicit decryption-failed marker instead
  of being dropped or causing a failure

### Requirement: history sync
The client SHALL restore conversation history via Matrix sync (including
server-side room history) on every start, without user action.

#### Scenario: restart restores history
- **WHEN** the app starts with a persisted session
- **THEN** rooms and their message history are restored via sync without
  user action

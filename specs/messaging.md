# Messaging (Matrix) — source spec

## Purpose

Conversations behave like WhatsApp: pick a person or group, type, send, done.
The transport is the Matrix protocol; encryption is end-to-end (Olm/Megolm via
matrix-sdk) and on by default with no opt-out. Zikos never operates its own
homeserver requirement — public homeservers work; self-hosting Continuwuity is
a supported option, not a dependency.

## Requirements

### Requirement: Matrix login
The client SHALL support login to any Matrix homeserver with username+password
(and, later, SSO), persist the session across restarts, and store session and
room state encrypted at rest.

#### Scenario: returning user
- **WHEN** the app starts and a persisted session exists
- **THEN** the app reconnects and syncs without asking for credentials

#### Scenario: wrong password
- **WHEN** login fails with M_FORBIDDEN
- **THEN** the app shows a clear error and keeps the entered username

### Requirement: E2EE by default
Every conversation SHALL be an encrypted Matrix room; the client SHALL not
allow creating unencrypted rooms, SHALL warn visibly when a room is not
encrypted, and SHALL support emoji-based device verification.

#### Scenario: new conversation
- **WHEN** a user creates a conversation
- **THEN** the created room has encryption enabled before the first message is sent

### Requirement: send and receive messages
The client SHALL send and render text messages with sender, timestamp, and
delivery state (sending/sent/failed), and SHALL retry failed sends.

#### Scenario: offline send
- **WHEN** the user sends a message while offline
- **THEN** the message appears locally marked `sending` and is transmitted
  automatically when connectivity returns

### Requirement: history sync
The client SHALL restore conversation history via Matrix sync (including
server-side room history) on every start, without user action.

#### Scenario: reinstall
- **WHEN** a user logs in on a fresh device and joins an existing encrypted room
- **THEN** history decrypts after key backup/verification per Matrix semantics

### Requirement: multi-device
A single account SHALL work from multiple devices simultaneously, with
per-device keys, verified through the standard Matrix verification flow.

## Non-goals (for now)

- Voice/video calls (WebRTC + coturn arrives later as its own change).
- Custom message search (Matrix search comes for free later).

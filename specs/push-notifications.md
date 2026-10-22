# Push notifications — source spec

## Purpose

Members must know a new take or message landed. Android uses UnifiedPush with
a self-hostable ntfy server (no Google dependency). iOS has no alternative to
APNs — push goes through Apple by necessity, routed by a gateway that learns
the minimum needed.

## Requirements

### Requirement: Android push via UnifiedPush
The Android client SHALL receive notifications through UnifiedPush with ntfy
as distributor (self-hostable by the user, a sane default provided). No
Google Play Services dependency SHALL exist in the client.

#### Scenario: new take while backgrounded
- **WHEN** a new file event arrives for a subscribed conversation
- **THEN** the device wakes via UnifiedPush and shows a notification

### Requirement: iOS push via APNs
The iOS client SHALL receive push through APNs via a small gateway component
(either in the blob server or the homeserver's sygnal). Phase 2 — not
required for MVP.

### Requirement: content privacy
Notifications SHALL NOT include message or file content. Sender and
conversation title only, honoring E2EE (decryption happens on open, after
background sync).

#### Scenario: notification content
- **WHEN** an encrypted message triggers a push
- **THEN** the notification shows "Lisa — VoiceMemories" and no message body

### Requirement: user control
The user SHALL be able to mute conversations and disable notifications
entirely.

## Non-goals (for now)

- Web push; notification channels beyond mute-all.

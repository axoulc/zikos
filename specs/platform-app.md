# Platform app (Flutter client) — source spec

## Purpose

One Flutter UI codebase, five targets: Android, iOS, Windows, macOS, Linux.
The UI is Flutter/Dart (Material 3); ALL logic lives in the pure-Rust crates
(`zikos-core`, `zikos-crypto`, `zikos-audio`, matrix integration) and reaches
Dart through `flutter_rust_bridge` — Dart renders and routes only.

Status: shell implemented in `apps/zikos-app` (home + record button state,
widget-tested). This file remains the contract.

## Requirements

### Requirement: single codebase, platform adapters
The app SHALL build from one Flutter UI over one Rust core; platform-specific
behavior (permissions, background audio, share sheet) SHALL be isolated
behind small adapters (Flutter platform channels / Rust cfg) with desktop
fallbacks.

#### Scenario: desktop fallback
- **WHEN** a platform capability (e.g. OS share sheet) is unavailable
- **THEN** the app degrades to a working alternative (e.g. "open containing
  folder") instead of failing

### Requirement: instant dictaphone access
From app launch, recording SHALL be reachable in one tap on the main screen.
On Android the app SHALL offer a home-screen widget / quick-settings tile;
on iOS an App Shortcut / Action Button entry that jumps straight to recording.

#### Scenario: quick tile (Android)
- **WHEN** the user taps the Zikos quick-settings tile
- **THEN** the app opens directly in recording mode and capture starts
  without further interaction

### Requirement: main navigation
The main screen SHALL expose three destinations: Conversations, Files (tree
browser for the current conversation), and Record. Any recording SHALL be
reachable within three taps.

### Requirement: offline-first
The app SHALL remain fully usable offline: browse cached tree, play
downloaded files, record, queue sends. Connectivity loss SHALL degrade with
visible pending states, never data loss.

#### Scenario: airplane mode session
- **WHEN** the device is offline for a whole rehearsal
- **THEN** the user records, organizes the tree, and everything syncs on
  reconnect without manual retry

### Requirement: settings
The app SHALL provide settings for: homeserver, blob server URL, default
recording format (FLAC default), and sign out (with local data wipe).

## Non-goals (for now)

- Web/WASM target; wearables; tablet-specific layouts.

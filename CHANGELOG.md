# Changelog

All notable changes to Zikos are documented here.

The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/)
and versions follow [Semantic Versioning](https://semver.org/). Entries are
written from the user's perspective (what changed, not which files moved).
Every commit adds its entry under `## [Unreleased]` — see `AGENTS.md`.

## [Unreleased]

### Added
- Project bootstrap: Cargo workspace with `zikos-core` (event-sourced shared
  file tree + recording metadata), `zikos-crypto` (AES-256-GCM client-side
  file encryption), `zikos-audio` (WAV encode/parse, recorder policy),
  `zikos-server` (axum audio blob server with Matrix-OpenID auth and size
  caps), and the `zikos-app` Flutter client shell (Material 3, one-tap
  record button, widget-tested) over the Rust core — the
  `flutter_rust_bridge` wiring lands with the UI change.
- Spec- and test-driven development via OpenSpec: source specs for seven
  capabilities in `specs/`, prompt playbook in `specs/PROMPTS.md`, workflow
  mandated in `AGENTS.md`.
- CI on every push: rustfmt check, clippy `-D warnings`, full test suite.

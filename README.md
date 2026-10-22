# Zikos

Cross-platform (Android, iOS, Windows, macOS, Linux) app for musicians:
**end-to-end encrypted chat over Matrix**, a **Drive-like file tree shared
per conversation**, and a **one-tap dictaphone**. Recordings are encrypted on
the device (AES-256-GCM) before touching a self-hosted Rust blob server that
stores nothing readable.

Maximum Rust-first, by policy — pure-Rust crates, matrix-sdk, axum — with a
**Flutter (Dart) UI** bound to the Rust core via `flutter_rust_bridge`.

```
┌─────────────── Zikos client (Flutter UI + Rust core, 5 platforms) ─────┐
│  messages + tree events (E2EE) ──► Matrix homeserver (public or self-  │
│  audio blobs (AES-256-GCM)  ─────► hosted Continuwuity)                │
│                                    zikos-server (your box, 1 binary)   │
└────────────────────────────────────────────────────────────────────────┘
```

## Layout

| Path | What |
|---|---|
| `crates/zikos-core` | event-sourced file tree, recording metadata (no I/O) |
| `crates/zikos-crypto` | AES-256-GCM file encryption |
| `crates/zikos-audio` | WAV encode/parse, recorder policy |
| `crates/zikos-server` | axum blob server (Matrix-OpenID auth) |
| `apps/zikos-app` | Flutter client (Dart UI, Rust core via flutter_rust_bridge) |
| `specs/` | source-of-truth specs + `PROMPTS.md` playbook |
| `openspec/` | OpenSpec workspace (spec-driven changes) |

## Commands

```bash
cargo ck          # check the Rust workspace
cargo t           # test lib crates
cargo lint        # clippy -D warnings
cargo run -p zikos-server   # blob server (ZIKOS_BIND, ZIKOS_BLOB_DIR, ZIKOS_MAX_BLOB_BYTES)
cd apps/zikos-app && flutter run   # the client (device or desktop)
```

## Development workflow

This project is **spec- and test-driven via OpenSpec** (see `AGENTS.md`):

- zcode: `/opsx:propose` → review → `/opsx:apply` → `/opsx:archive`
- Codex: `$openspec-propose` → `$openspec-apply-change` → `$openspec-archive-change`

Start with [`specs/PROMPTS.md`](specs/PROMPTS.md) — it lists the prompts that
build Zikos capability by capability, in order.

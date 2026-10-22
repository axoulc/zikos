# AGENTS.md — Zikos working agreement

Zikos is a cross-platform (Android, iOS, desktop) app for musicians to chat
end-to-end encrypted over **Matrix**, and to share recordings through a
Drive-like file tree — with a dictaphone that is always one tap away.
Messaging rides on Matrix homeservers; a single self-hosted **Rust** service
(`zikos-server`) stores only client-encrypted audio blobs.

**The stack is Rust-first by policy**: pure-Rust crates, matrix-sdk (Rust),
axum server — plus a **Flutter (Dart) UI** for an easier, prettier
cross-platform front end, bound to the Rust core via `flutter_rust_bridge`.
Dart is allowed only inside `apps/zikos-app`; do not introduce services or
tooling in other languages. FFI only at platform edges.

## Repo layout

```
crates/zikos-core     # domain: shared file tree (event-sourced), recording metadata
crates/zikos-crypto   # AES-256-GCM file encryption before upload
crates/zikos-audio    # WAV encode/parse, recorder policy (cpal+FLAC land as changes)
crates/zikos-server   # axum audio blob server (Matrix-OpenID auth, FsBlobStore)
apps/zikos-app        # Flutter client (Dart UI; Rust core via flutter_rust_bridge)
specs/                # SOURCE-OF-TRUTH capability specs (pre-OpenSpec format)
openspec/             # OpenSpec workspace (changes/, specs/ grow here)
```

## OpenSpec workflow — mandatory, and the default

**You use OpenSpec on your own initiative.** For any change beyond a trivial
fix (typo, formatting, dependency bump, fixing a broken build), the flow is:

1. **Propose** — zcode: `/opsx:propose <change>` · Codex: `$openspec-propose <change>`.
   This creates the change artifacts (proposal → specs → design → tasks).
   Stop after planning; wait for user approval.
2. **Apply** — zcode: `/opsx:apply` · Codex: `$openspec-apply-change`.
   Implement tasks in order, ticking them off in `tasks.md`.
3. **Archive** — `/opsx:archive` · `$openspec-archive-change` when all tasks
   are done; this merges the spec deltas into `openspec/specs/`.

Rules:
- When proposing a change for an existing capability, **read the matching file
  in `specs/` first** — those are the source-of-truth requirements. The
  proposal's spec delta must cover them (scoped to the change) and stay
  compatible with their terminology.
- One capability per change. Keep changes small enough to review in one sitting.
- Never implement features without an approved change; never widen, narrow, or
  silently drop a specified behavior during apply — surface it and pause.
- Validate before archiving: `openspec validate --strict`.
- The playbook in `specs/PROMPTS.md` lists the exact prompts to run, in order.

## Test-driven development — mandatory

- Every task starts with a **failing test** that encodes the requirement being
  implemented. Write it, watch it fail, then make it pass.
- No production code without a test that proves it. Tests live next to the
  code (`#[cfg(test)]`) or in `tests/` for cross-crate contracts.
- A task is done only when: new + existing tests pass, `cargo clippy
  --workspace --all-targets -- -D warnings` is clean, and `cargo fmt --all`
  has been applied.
- Never `#[ignore]`, weaken, or delete a test to make it pass — if a spec'd
  behavior is wrong, update the spec via OpenSpec first.

## Commands

```bash
cargo ck        # check --workspace --all-targets (Rust crates)
cargo t         # test --workspace (Rust crates)
cargo lint      # clippy --workspace --all-targets -- -D warnings
cargo fmtcheck  # fmt --all -- --check
cargo run -p zikos-server   # the blob server (env: ZIKOS_BIND, ZIKOS_BLOB_DIR, ZIKOS_MAX_BLOB_BYTES)

# Flutter app — run inside apps/zikos-app:
flutter run       # launch on attached device / desktop
flutter test      # widget + unit tests
flutter analyze   # static analysis (flutter_lints)
```

## Rust conventions

- `thiserror` in libraries; errors at API boundaries mapped explicitly; no
  `unwrap()`/`expect()` outside tests, build scripts, and provably
  infallible spots (say why in a comment when you do).
- `unsafe` is forbidden unless a spec demands it; pure-Rust dependencies over
  C bindings when quality allows (RustCrypto, hound, flacenc, matrix-sdk).
- Workspace dependency versions live in the root `Cargo.toml`; crates add
  `workspace = true` dependencies.
- Public items get doc comments. Modules stay free of I/O in `zikos-core`
  (it must compile anywhere).
- **Flutter UI work.** The UI lives in `apps/zikos-app` (Flutter/Dart,
  Material 3). Keep business logic out of Dart — it belongs in the Rust
  crates and reaches the UI through `flutter_rust_bridge`; Dart renders and
  routes only. Follow the project's `flutter_lints`; if the current tool
  provides a `flutter` skill, load it before writing or reviewing UI code,
  otherwise consult docs.flutter.dev for the installed Flutter version.
- Conventional commits (`feat:`, `fix:`, `spec:`, `chore:`).
- **Every commit MUST update `CHANGELOG.md` in the same commit.** Add the
  change under `## [Unreleased]` in Keep a Changelog style
  (`Added`/`Changed`/`Fixed`/`Removed`), phrased from the user's perspective.
  A commit without its changelog entry is incomplete — do not make it.

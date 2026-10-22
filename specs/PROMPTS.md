# Zikos — OpenSpec Prompt Playbook

Copy-paste prompts that drive Zikos through OpenSpec, capability by capability.
OpenSpec is already initialized (schema `spec-driven`: proposal → specs →
design → tasks), and `AGENTS.md` makes agents use it autonomously — this file
is how *you* steer it deliberately.

**Syntax per tool:**
- zcode: `/opsx:propose …`, `/opsx:apply …`, `/opsx:archive …`, `/opsx:explore …`
- Codex: `$openspec-propose …`, `$openspec-apply-change …`, `$openspec-archive-change …`

**Golden rules**
1. One prompt = one change = one capability slice. Small changes review well.
2. After a `propose`, **read the artifacts** and approve before `apply`.
   Planning and implementation are always separate steps.
3. Every prompt below says "Stop after the planning artifacts" — keep that.
4. The agent must read the referenced `specs/<capability>.md` first; if it
   forgets, reply: "Read specs/<file> first, then redo the artifacts."

---

## Phase 0 — preflight (run once)

```
/opsx:explore
Give me an overview of this repository: workspace layout, crates, test
status, and the specs/ folder capabilities. Do not change anything.
```

```
Run `openspec context` and `openspec list`, then summarize the current
OpenSpec state of this project.
```

## Phase 1 — Matrix messaging core

```
/opsx:propose bootstrap-matrix-client
Use specs/messaging.md as the source of truth. Create the change
`bootstrap-matrix-client`: a new `zikos-matrix` crate wrapping matrix-sdk
(Rust) with login + persisted encrypted session, E2EE-enabled room creation,
and send/receive of text messages with offline queueing. Scope phase 1 only
(login, sync, one room, text messages). Follow AGENTS.md TDD rules — every
task starts with a failing test. Stop after the planning artifacts are ready;
do not implement.
```

Then: review → `/opsx:apply bootstrap-matrix-client` → `/opsx:archive`.

## Phase 2 — dictaphone engine

```
/opsx:propose implement-dictaphone-capture
Use specs/audio-recording.md as the source of truth. Create the change
`implement-dictaphone-capture` in `zikos-audio`: PCM capture via cpal
(48 kHz stereo default), WAV write streaming to disk, FLAC transcode on
finalize via `flacenc`, RecordingMeta generation with take-YYYYMMDD-HHMM
naming, and interrupted-session recovery. Desktop-first; mobile capture
adapters are out of scope. Strict TDD per AGENTS.md (sinewave fixtures, no
microphone in tests). Stop after the planning artifacts.
```

## Phase 3 — blob server hardening

```
/opsx:propose harden-blob-server
Use specs/audio-storage-server.md as the source of truth. Create the change
`harden-blob-server` in `zikos-server`: integration-test the real
MatrixOpenIdVerifier against a wiremock homeserver (federation userinfo),
add streaming upload/download (no full-body buffering), persistent session
registry behind a trait, and a structured access log (no blob contents).
Strict TDD. Stop after the planning artifacts.
```

## Phase 4 — file tree over Matrix

```
/opsx:propose sync-tree-over-matrix
Use specs/file-tree.md as the source of truth. Create the change
`sync-tree-over-matrix`: send zikos-core TreeOps as encrypted room events
(custom event type), replay room history into a FileTree, offline queue with
pending states, and deterministic conflict convergence (LWW by origin
server ts + event id, losers to Conflicts/). Conflict rules need property
tests (proptest). Strict TDD. Stop after the planning artifacts.
```

## Phase 5 — end-to-end file flow

```
/opsx:propose implement-file-exchange
Use specs/security.md and specs/file-tree.md as sources of truth. Create the
change `implement-file-exchange`: encrypt finalized takes with zikos-crypto,
upload to the blob server, reference blob id + wrapped FileKey in the tree
event, and download+decrypt on another device. Include an end-to-end test
over a local axum instance. Stop after the planning artifacts.
```

## Phase 6 — app UI

```
/opsx:propose build-main-ui
Use specs/platform-app.md as the source of truth. Create the change
`build-main-ui` in apps/zikos-app (Flutter): conversations list, tree browser
(three-tap access to any recording), record screen with timer/pause, and
settings (homeserver, blob server URL, format). Wire the UI to the Rust
crates through flutter_rust_bridge (zikos-core models, zikos-audio capture) —
business logic stays in Rust; Dart only renders and routes. Widget tests for
every screen. Stop after the planning artifacts.
```

## Phase 7 — push (Android first)

```
/opsx:propose add-unified-push
Use specs/push-notifications.md as the source of truth. Create the change
`add-unified-push`: UnifiedPush registration with ntfy, notification on new
room events, content-privacy rules (sender + conversation title only),
mute controls. Android only; iOS/APNs is explicitly out of scope.
Stop after the planning artifacts.
```

---

## Routine prompts (reusable)

**New feature/change later:**
```
/opsx:propose <kebab-name>
<2–3 sentences of intent.> Source of truth: specs/<capability>.md (or: create
the spec delta from scratch — the capability is new). One capability only.
Strict TDD per AGENTS.md. Stop after the planning artifacts.
```

**A spec turned out wrong:**
```
/opsx:propose revise-<capability>-spec
Reality contradicts specs/<capability>.md on: <point>. Propose a change whose
spec delta updates the requirement first; implementation tasks come second.
Stop after the planning artifacts.
```

**Health checks:**
```
Run `openspec list`, `openspec validate --strict` and `openspec doctor`,
then summarize anything that needs attention.
```

**Explore before deciding:**
```
/opsx:explore
How does <subsystem> currently work? Cite files and tests. No changes.
```

## Tips

- If `apply` pauses on a design issue: fix the artifact (proposal/design/
  tasks) in the change folder, then re-run `/opsx:apply` — never patch code
  outside the change's scope.
- Never let a session end with an applied-but-unarchived change without
  running `openspec validate --strict`.
- Prefer rejecting an over-wide proposal ("split this into two changes")
  over reviewing a monster.

# Zikos source specifications

These files are the **source of truth** for what Zikos must do, written in
OpenSpec's Requirement/Scenario format so they can be lifted into the
OpenSpec workspace with zero rewriting. They live here *until* each
capability is picked up by an OpenSpec change — at which point the change's
spec delta implements (a slice of) the matching file, and archiving the
change merges it into `openspec/specs/`.

| File | Capability | Status |
|---|---|---|
| `messaging.md` | Matrix E2EE conversations | not started |
| `audio-recording.md` | one-tap dictaphone | not started |
| `audio-storage-server.md` | self-hosted encrypted blob server | partially implemented (zikos-server skeleton) |
| `file-tree.md` | Drive-like encrypted tree synced over Matrix | partially implemented (zikos-core tree) |
| `security.md` | E2EE + client-side file crypto | partially implemented (zikos-crypto) |
| `platform-app.md` | Flutter cross-platform client | partially implemented (app shell) |
| `push-notifications.md` | push delivery | not started |

Workflow for each capability is in [`PROMPTS.md`](PROMPTS.md). House rules:
specs are requirements, not design; technology choices live in the change's
`design.md`. When reality proves a spec wrong, fix the spec first (via an
OpenSpec change), then the code.

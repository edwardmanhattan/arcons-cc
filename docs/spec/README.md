# docs/spec — the pinned upstream contract

Vendored from the Minos repository so that a change on the server is a **deliberate** commit here, not a
silent breakage. Nothing in this directory is ours to edit.

| file | source | from |
| --- | --- | --- |
| `minos-api.yaml` | `minos/docs/openapi/minos-api.yaml` | `minos` @ `01f8fbe2ca88` (2026-11-04, branch `wip`) |
| `realtime-protocol.md` | `minos/docs/realtime-protocol.md` | same commit |

## Re-vendoring

1. Copy both files from the Minos repo at a named commit.
2. Update the table above with the new commit and date.
3. Run `scripts/verify.sh`. The drift test (`tests/spec_drift.rs`) fails when a schema we model gained a
   **required** field, or lost one we still read.
4. Fix the code, not the test. If the contract genuinely changed meaning, say what changed in the commit
   message — that message is the record.

## What the drift test looks at

Only what we consume: `FeedPosition`, `FeedFix`, `FeedPositionListEnvelope`, `AuthEnvelope`,
`TokenPair`, `ErrorResponse`, and the `game:<id>:positions` payload. It parses the snapshot as **text**
(no YAML dependency): find the schema block, read its `required:` list, compare against a const field list
in Rust. Crude on purpose — it is a tripwire, not a validator.

## Facts from this snapshot that the code depends on

- `GET /live-feed/positions` requires the `read` action on `/system/live-feed`.
- The live-feed publication is a **bare** `FeedPositionEvent` at `pub.data`, with an outer `data` level
  from Centrifugo. `game:<id>` is a `GameEvent` at `pub.data.type` + `pub.data.data.*` — **two** levels.
  `game:<id>:positions` is bare, has **no `type`**, and lives at `pub.data.positions`.
- The live-feed event carries `speed_ms` beside `speed_kn` (same fact, absent exactly when it is).
- `game:<id>:positions` **is emitting** (since 2026-09-23), contrary to an older note.
- The `game` namespace keeps `presence` and a two-message history window; `live-feed` does not. A
  `live-feed` reconnect therefore needs a re-read, not a replay.

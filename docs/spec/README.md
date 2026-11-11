# docs/spec — the pinned upstream contract

Vendored from the Minos repository so that a change on the server is a **deliberate** commit here, not a
silent breakage. Nothing in this directory is ours to edit.

| file | source | from |
| --- | --- | --- |
| `minos-api.yaml` | `minos/docs/openapi/minos-api.yaml` | `minos` @ `0e165781b3e0` (2026-11-10, branch `wip`) |
| `realtime-protocol.md` | `minos/docs/realtime-protocol.md` | `minos` @ `01f8fbe2ca88` (2026-11-04) — byte-identical at `0e165781b3e0`, so it was not re-copied |

## Re-vendoring

1. Copy both files from the Minos repo at a named commit.
2. Update the table above with the new commit and date.
3. Run `scripts/verify.sh`. The drift test (`tests/spec_drift.rs`) fails when a schema we model gained a
   **required** field, or lost one we still read.
4. Fix the code, not the test. If the contract genuinely changed meaning, say what changed in the commit
   message — that message is the record.

## What the drift test looks at

Only what we consume: `FeedPosition`, `FeedFix`, `FeedPositionListEnvelope`, `AuthEnvelope`,
`TokenPair`, `ErrorResponse`, the `game:<id>:positions` payload, and the operational resources added
in #107 — `ViewpointSummary`, `InstallationTypeSummary`, `InstallationSummary`, `TacticalObject`,
`TacticalObjectCategoryRef`, `TacticalObjectDomainRef`, `TacticalObjectManifest`,
`TacticalObjectManifestEntry`, `TacticalObjectFacets`, `TacticalObjectDomainFacet`,
`TacticalObjectCategoryFacet`, `TacticalObjectPlacement`, `UnitOrganisationSummary`,
`UnitOrganisationNodeSummary`, `UnitOrganisationDetail`, `NodeCommanderSummary`,
`TniCommanderSummary` and `TniCommandPostSummary`. It parses the snapshot as **text**
(no YAML dependency): find the schema block, read its `required:` list, compare against a const field list
in Rust. Crude on purpose — it is a tripwire, not a validator.

Two schemas do not fit that shape and are read by a dedicated test instead: `ViewpointSummary` has
**no** `required:` list (both coordinates are absent together), and `UnitOrganisationDetail` is an
`allOf` composition whose `nodes` is required inside the inline half.

## Facts from this snapshot that the code depends on

- `GET /live-feed/positions` requires the `read` action on `/system/live-feed`.
- The live-feed publication is a **bare** `FeedPositionEvent` at `pub.data`, with an outer `data` level
  from Centrifugo. `game:<id>` is a `GameEvent` at `pub.data.type` + `pub.data.data.*` — **two** levels.
  `game:<id>:positions` is bare, has **no `type`**, and lives at `pub.data.positions`.
- The live-feed event carries `speed_ms` beside `speed_kn` (same fact, absent exactly when it is).
- `game:<id>:positions` **is emitting** (since 2026-09-23), contrary to an older note.
- The `game` namespace keeps `presence` and a two-message history window; `live-feed` does not. A
  `live-feed` reconnect therefore needs a re-read, not a replay.

Operational resources (#107):

- `GET /viewpoint` is **authentication only** — no permission object. An unconfigured deployment answers
  `{}`, with `latitude` and `longitude` absent **together**; `0,0` is never sent.
- Each of `/installations`, `/tactical-objects`, `/tactical-object-placements`,
  `/unit-organisations` and `/tni-command` needs `read` on its own `/system/...` subject.
- `installation_types.symbol_code` is a **code** into the tactical-object catalogue, not an id. The
  catalogue is reconciled at startup, so a kind may name a code the library does not carry: draw a
  generic marker, never fail the read.
- `/tactical-objects/manifest` is unpaginated on purpose, and a signing failure omits `asset_url`,
  not the entry.
- `/unit-organisations/{id}.nodes` is a **forest** (several roots are normal), and a node's `children`
  is always an array.
- `/tni-commander` is one post shared by every service branch; `commander` is absent when the post is
  vacant. `/tni-commander/history` is newest first and `is_current` restates an absent `to_at`.
- `label`, `rotation_deg`, `echelon_name`, `echelon_rank`, `id_commander`, `commander`, `icon`, `note`
  and `asset_url` are all absent in real responses, and none may default (`INV-01`). Two of them,
  `label` and `rotation_deg`, are even listed as `required` on `TacticalObjectPlacement` while the
  server omits them — which is why the client models the wire, not the `required` list.

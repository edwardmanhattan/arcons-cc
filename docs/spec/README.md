# docs/spec — the pinned upstream contract

Vendored from the Minos repository so that a change on the server is a **deliberate** commit here, not a
silent breakage. Nothing in this directory is ours to edit.

| file | source | from |
| --- | --- | --- |
| `minos-api.yaml` | `minos/docs/openapi/minos-api.yaml` | `minos` @ `5758f37` (2026-11-11) — added the newsroom (`/banglinstra`), 288 lines, no other change against `0e165781b3e0` |
| `realtime-protocol.md` | `minos/docs/realtime-protocol.md` | `minos` @ `01f8fbe2ca88` (2026-11-04) — byte-identical at `0e165781b3e0` and at `5758f37`, so it was not re-copied |

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
`TniCommanderSummary`, `TniCommandPostSummary` and — added in #135 — `BanglinstraCountry`,
`BanglinstraStory`, `BanglinstraArticle`, `BanglinstraStoryDetail`. It parses the snapshot as **text**
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

The newsroom (#135): the API's word for the route is `banglinstra`, and this code calls the panel
the newsroom:

- `GET /banglinstra/stories` requires `read` on **`/system/banglinstra`**, and so does
  `GET /banglinstra/stories/{id}`. There is no write action to grant: nothing writes an article
  through this API.
- **A row is a story, not an article.** `source_count` is how many documents carried it, and on the
  detail route it always equals `articles.len()`.
- `window=latest` is the last 24 hours **of the data held** — the server anchors it to the newest
  article it has (`max(seendate)`), **never to `now()`**. A client that computed its own cutoff would
  show an empty tab on an air-gapped box holding a three-day-old corpus. `window=archive` has no lower
  bound, which "nothing is ever deleted" makes possible.
- `min_confidence` **also drops `unknown`-scope stories**: a story whose country could not be resolved
  carries no confidence, so it cannot satisfy any positive minimum. That is the intent, not a bug.
- The four scopes are predicates over the event's country: `national` is Indonesia, `regional` is the
  deployment's list, `international` is everything else with a country, and `unknown` is a real scope
  and not a null.
- `country` is **absent** when the corpus had no usable geography, and `country.place` is absent
  unless at least half the document's location rows agreed. `confidence` is a fact to show (below 0.5
  the country is not settled), not a filter to re-apply.
- An article's `persons`, `tone`, `event_code` and `quad_class` are all optional: `event_code` is
  absent for roughly 73% of documents, and those are still articles. `flagged` means the domain is on
  neither the allow nor the deny list — shown, not filtered.
- `url` is provenance. On an air-gapped wall it cannot be opened, so it is never drawn as a browsing
  affordance.

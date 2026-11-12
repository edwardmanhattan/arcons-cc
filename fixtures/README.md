# Fixtures

Wire-shaped data the tests read. **Fixtures come from the wire** — recorded from Minos, or copied
from the pinned spec — never generated from our own types. A test that round-trips our struct through
our own serializer proves only that we agree with ourselves.

## `scripts/` — input, not data

Rune's `--script` format: an array of timestamped input events (`t` in seconds). `scripts/capture.sh` hands
`fixtures/scripts/<scene>.json` to the renderer when it exists, so a capture can be **driven by a pointer**
instead of by a function call — the only kind of test that catches a gesture that reaches no handler (#80).

| file | what it does |
| --- | --- |
| `panes-pan.json` | presses the border at 50% of the `panes` panel and drags it to 58%, which is the share `panes` sets in code. The two captures are the same picture, so one asserts the other |

Coordinates are window points at `--scale 1`, and `verify` projects against the layout the gesture must produce.

## `rest/` — recorded 2026-11-06

From the dev Minos at the commit pinned in `docs/spec/`, with the wall's read-only account. The
server said `127` hulls and `126` of them had never reported, which is the case `INV-02` exists for.

| file | what it is |
| --- | --- |
| `positions.json` | `GET /live-feed/positions`, complete and unedited: every hull, one with a fix, 126 without a `position` key at all |
| `positions-speed-absent.json` | **Derived**: the one recorded fix with `speed_kn` and `speed_ms` removed. The key is gone, not null — the shape the API uses when a device reported no speed |
| `positions-speed-null.json` | **Derived**: the same fix with both set to `null`, the other encoding of the same fact |
| `auth-login.json` | `POST /auth/login` with `return_refresh_token: true`. **Both tokens replaced with `REDACTED-…`**: a token in the tree is `INV-08` broken at rest, and a fixture is not worth that. The payload also carries `must_change_password`, which the pinned `TokenPair` does not model — that is the unknown-field tolerance test |
| `unit.json` | `GET /units/1` |
| `specifications.json` | `GET /units/1/specifications` — one append-only version, every measurement nullable |
| `commander-empty.json` | `GET /units/1/commander` — an empty list, which is a valid answer: nobody has ever commanded this hull |
| `units-page-1.json`, `units-page-2.json`, `units-page-3.json` | `GET /units?page_size=50&page_number=1..3`, recorded 2026-11-09: 127 hulls over three pages, `has_next_page` true, true, false. **Trimmed to the first four rows of each page and the presigned `image_url` replaced with a stable example host** — a presigned URL is a time-limited credential (`INV-08`), and the envelope and `metadata` are exactly as recorded, which is what the paging test reads (#52) |
| `error-401.json` | `POST /auth/login` with an account that does not exist. Junk credentials, so the recording itself holds no secret |
| `error-404.json` | `GET /units/99999999` |

Two rules for adding one:

1. **Record it, do not invent it.** If a case does not exist on the wire (the feed has only one
   reported hull today), derive it from a recording by *removing or nulling a key* and say so in the
   table above. Never write a payload from a struct in `src/`.
2. **Redact tokens, passwords and hostnames** before committing. `grep -r REDACTED fixtures/` should
   find every one, and nothing else should look like a JWT.

## `rest/` — recorded 2026-11-11, the operational resources

From the same dev Minos, with the wall's read-only account, for #107. Nothing was trimmed: the symbol
library is 56 entries and the whole catalogue was recorded. Every `asset_url` is a presigned URL, so
its scheme, host and query are replaced with `https://objects.example.test/…?X-Amz-Expires=REDACTED`
(the path — the object key — is kept); the query is the credential (`X-Amz-Credential`,
`X-Amz-Signature`) and a fixture is not worth an `INV-08` break. The envelope and every key are
otherwise exactly as the wire sent them.

| file | what it is |
| --- | --- |
| `viewpoint.json` | `GET /viewpoint` — `data: {}`, because this deployment has not chosen a view. That is the recorded state: both coordinates absent together, not `0,0` |
| `installation-types.json` | `GET /installation-types` — five kinds in picker order. None carries `symbol_code` on this deployment, which is why the type models it as optional |
| `installations.json` | `GET /installations?active_only=true` — an **empty list**: the dev instance has registered no facilities. A valid answer, and the one that says "draw no facilities", not "the read failed" |
| `tactical-objects.json` | `GET /tactical-objects?page_size=200&page_number=1&active=true` — all 56 active symbols, with `metadata` (`total_records: 56`, one page) |
| `tactical-objects-manifest.json` | `GET /tactical-objects/manifest` — `version: 1`, the same 56 symbols with `asset_key` and `asset_url`, unpaginated |
| `tactical-objects-facets.json` | `GET /tactical-objects/facets` — `total: 56`, six domains (four with `count: 0`) and three categories |
| `tactical-object-placements.json` | `GET /tactical-object-placements` — three marks on the operational map, each embedding its whole `symbol` |
| `unit-organisations.json` | `GET /unit-organisations` — four structures, one per service branch |
| `unit-organisation-1.json` | `GET /unit-organisations/1` — the Navy's structure: one root, seven nodes, nested `children`. Recorded because it is the real id the list named |
| `tni-commander.json` | `GET /tni-commander` — the post, with **no `commander` key**: the post is vacant, which is a real state and not an error |
| `tni-commander-history.json` | `GET /tni-commander/history` — an empty list. Nobody has been appointed yet, so the dev instance cannot record a non-empty history; the field names are pinned by the spec and the drift test |

The empty answers (`viewpoint`, `installations`, `tni-commander/history`) are recorded rather than
worked around. A test that needs a non-empty history cannot get one from this server without appointing
an officer, and inventing the row would be worse than the gap.

## `rest/` — the newsroom, 2026-11-12 (#135)

The dev deployment **answers** `GET /banglinstra/stories` (200, `read` on `/system/banglinstra`) and
`GET /banglinstra/stories/{id}` (404 for a story that does not exist) — but its table holds **zero
stories**, because the intel ingest had put nothing in it. So one file here is a recording and two are
not, and this table says which:

| file | what it is |
| --- | --- |
| `newsroom-stories-empty.json` | **Recorded** 2026-11-12: `GET /banglinstra/stories?scope=international&window=latest&min_confidence=0.5&page_size=25&page_number=1`, complete and unedited. An **empty list** with `total_records: 0`, `total_pages: 0` — a valid answer, and the one that says "no stories", not "the read failed" |
| `newsroom-stories-page-1.json` | **Derived from the pinned spec** (`docs/spec/minos-api.yaml` @ `5758f37`), not recorded: one page of six, 128 stories in total, with the absences the ticket needs — a story with **no `country`**, and a country with **no `place`**. The deployment had no rows to record, and inventing them from a struct in `src/` is the one thing this directory forbids; the field names and the omissions are the spec's |
| `newsroom-story-1.json` | **Derived from the pinned spec**, the same way: story 412 with **three articles, oldest first**, and `source_count: 3` equal to their length. The lead article carries `tone: 0.0` (a measured zero, not an absence); the second has no `event_code` or `quad_class`; the third is `flagged: true` and has no `tone` |

**Replace the two derived files with recordings as soon as the deployment holds stories.** They are
here so the decoder and the request line can be tested today; a recording of real rows is strictly
better, and the moment one exists these two should be deleted rather than kept alongside it.

## `scenes/` and `golden/` — recorded 2026-11-07

`scenes/feed.json` is the picture the named scenes load: 40 hulls in the `GET /live-feed/positions`
envelope, so a scene parses through the same `FeedPosition` the REST client uses. It is **derived
from `rest/positions.json`**: the envelope and every key are the recorded ones, and the values are
filled in so each trap has one hull (hull 7 never reported, hull 11 is older than the stale
threshold, hull 13 is backfilled, hull 17 has no course, hull 9 is a measured `0.0`). Say so rather
than pretending a dev server produced it.

`scenes/newsroom-international-1.json`, `scenes/newsroom-international-2.json` and
`scenes/newsroom-regional-1.json` are the newsroom scenes' pages (#136): **derived from the pinned spec**,
like the list page in `rest/`, because the dev deployment holds no stories to record. Each is the
`GET /banglinstra/stories` envelope with its `metadata` as the wire sends it — 25 rows, page 1 of 6 (128
records) for International, page 2 of 6 for the same, and page 1 of 3 (61 records) for Regional — and each
carries the absences the ticket needs: stories with **no `country`**, countries with **no `place`**, and
`source_count` from 1 to 7 so the singular is covered. The first titles differ across the three files,
which is what makes "page 2 is not page 1" and "two panes, two scopes" assertable. **Replace them with
recordings** the moment the deployment's intel ingest has rows; the same rule as the `rest/` pair above.

`scenes/newsroom-story.json` and `scenes/newsroom-story-page-2.json` are the two **story details**
(#137), derived from the pinned spec like the pages above: `GET /banglinstra/stories/{id}` is
`{story, articles}`, and each file's `story` is the very row its list page carries — id, title,
`source_count`, source and both clocks — so a header and a row cannot disagree. The first has three
articles, oldest first: a lead with `tone: 0.0`, a person, `event_code` and `quad_class`; a second with
a tone and **no** CAMEO fields; and a third that is `flagged: true` with none of them. Its story has
**no `country`**, which is the dash the capture asserts. The second is the row the `newsroom-story-back`
scene presses on page 2, with two articles.

`scripts/newsroom-story-back.json` presses the detail's `back` word at (622, 86) — the pixel the
`newsroom-story` capture puts it at — so the list's return is proven by a pointer rather than by the
scene calling the function.

`scenes/objects.json` is the `objects` scene's operational layer (#110): **derived from
`rest/tactical-object-placements.json`**, the same three marks and the same coordinates, with two
labels filled in ("North Cardinal 3", "Green cone 7") and the cone's bearing set to 45°. The third
mark keeps the recording's `"label": ""`, which is the unlabelled case the capture asserts. Everything
else — the envelope, every embedded symbol, every other key — is the recorded wire shape.

`scenes/chain-of-command.json` is the `pane-tree` scene's structure (#116): **derived from
`rest/unit-organisation-1.json`** by moving the recorded node 3 (Irjenal) out of Wakasal's subtree and making
it a root — the key a root does not have, `parent_id`, is gone. That is the one thing the recording cannot
show and the forest claim needs: `nodes` is a **forest**, and more than one root is normal. Every field value
is the wire's; nothing was invented. The supreme command is the recorded `rest/tni-commander.json`, vacant,
so the capture also proves a vacant post draws its title and no name.

`scenes/buoy.png`, `scenes/cone.png` and `scenes/light.png` are **stand-ins for the presigned SVG
artwork**: the recorded `asset_url` points at object storage, and a scene may not open a socket
(`INV-11`). Each is a 32 × 32 shape in one exact colour, so the assertion can find the decoded bytes at
a placement's projected point rather than any ink at all. They are repository art, like `hull.png`,
not wire data.

`golden/<scene>.png` is one capture per scene, compared **byte for byte** (`INV-11`). It changes only
through `scripts/capture.sh --update-goldens`, which a human runs after looking at the diff. A golden
that changes in passing is a golden that has stopped being a check.

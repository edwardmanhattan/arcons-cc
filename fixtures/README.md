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

## `scenes/` and `golden/` — recorded 2026-11-07

`scenes/feed.json` is the picture the named scenes load: 40 hulls in the `GET /live-feed/positions`
envelope, so a scene parses through the same `FeedPosition` the REST client uses. It is **derived
from `rest/positions.json`**: the envelope and every key are the recorded ones, and the values are
filled in so each trap has one hull (hull 7 never reported, hull 11 is older than the stale
threshold, hull 13 is backfilled, hull 17 has no course, hull 9 is a measured `0.0`). Say so rather
than pretending a dev server produced it.

`scenes/objects.json` is the `objects` scene's operational layer (#110): **derived from
`rest/tactical-object-placements.json`**, the same three marks and the same coordinates, with two
labels filled in ("North Cardinal 3", "Green cone 7") and the cone's bearing set to 45°. The third
mark keeps the recording's `"label": ""`, which is the unlabelled case the capture asserts. Everything
else — the envelope, every embedded symbol, every other key — is the recorded wire shape.

`scenes/buoy.png`, `scenes/cone.png` and `scenes/light.png` are **stand-ins for the presigned SVG
artwork**: the recorded `asset_url` points at object storage, and a scene may not open a socket
(`INV-11`). Each is a 32 × 32 shape in one exact colour, so the assertion can find the decoded bytes at
a placement's projected point rather than any ink at all. They are repository art, like `hull.png`,
not wire data.

`golden/<scene>.png` is one capture per scene, compared **byte for byte** (`INV-11`). It changes only
through `scripts/capture.sh --update-goldens`, which a human runs after looking at the diff. A golden
that changes in passing is a golden that has stopped being a check.

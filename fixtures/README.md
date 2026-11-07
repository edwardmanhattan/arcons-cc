# Fixtures

Wire-shaped data the tests read. **Fixtures come from the wire** — recorded from Minos, or copied
from the pinned spec — never generated from our own types. A test that round-trips our struct through
our own serializer proves only that we agree with ourselves.

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
| `error-401.json` | `POST /auth/login` with an account that does not exist. Junk credentials, so the recording itself holds no secret |
| `error-404.json` | `GET /units/99999999` |

Two rules for adding one:

1. **Record it, do not invent it.** If a case does not exist on the wire (the feed has only one
   reported hull today), derive it from a recording by *removing or nulling a key* and say so in the
   table above. Never write a payload from a struct in `src/`.
2. **Redact tokens, passwords and hostnames** before committing. `grep -r REDACTED fixtures/` should
   find every one, and nothing else should look like a JWT.

## `scenes/` and `golden/` — recorded 2026-11-07

`scenes/feed.json` is the picture the named scenes load: 40 hulls in the `GET /live-feed/positions`
envelope, so a scene parses through the same `FeedPosition` the REST client uses. It is **derived
from `rest/positions.json`**: the envelope and every key are the recorded ones, and the values are
filled in so each trap has one hull (hull 7 never reported, hull 11 is older than the stale
threshold, hull 13 is backfilled, hull 17 has no course, hull 9 is a measured `0.0`). Say so rather
than pretending a dev server produced it.

`golden/<scene>.png` is one capture per scene, compared **byte for byte** (`INV-11`). It changes only
through `scripts/capture.sh --update-goldens`, which a human runs after looking at the diff. A golden
that changes in passing is a golden that has stopped being a check.

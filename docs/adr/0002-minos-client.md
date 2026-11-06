# ADR-0002 — the Minos client: permissions, credentials, and one retry rule

- **Status:** accepted (#3)
- **Date:** 2026-11-06
- **Deciders:** Edward Manhattan
- **Context:** ADR-0001 (Rune, MapLibre), `docs/spec/` at Minos `01f8fbe2ca88`

## The scope of the wall's account

The README asks "what account does the wall get?" and the answer is a permission list, not a role.
From the pinned spec, and **confirmed against the dev server on 2026-11-06** with a read-only account:

| what the wall reads | the action it needs |
| --- | --- |
| `GET /live-feed/positions`, `GET /live-feed/positions/{id}` | `read` on **`/system/live-feed`** |
| `GET /units/{id}`, `GET /units/{id}/specifications`, `GET /units/{id}/commander` | `read` on **`/system/units`** |

Both matter. The second is easy to miss: the card's identity and commander come from `/units/…`, and a
wall account holding only `/system/live-feed` would draw the map and then fail every click. The dev
account holds both and was exercised through all four routes (`--self-check` plus a manual read).

**Nothing else.** The wall never posts a fix, an order or a message; it does not need a commander
account, and it does not get one. A display that can write is a display that can change the picture
it is reporting.

## Credential mode: refresh token for the wall, password for development

The preferred deployment credential is a **long-lived refresh token**: a wall that runs for weeks
should not hold a password at rest, and a password is a thing to rotate.

The dev account is `identifier` + `password`, exchanged at `/auth/login` with
`return_refresh_token: true` (a desktop has no cookie jar, so without that flag the pair arrives only
as a cookie). Both modes are supported, and `--self-check` exercises whichever is configured.

Refresh tokens **rotate**: `/auth/refresh` invalidates the one presented and returns a new one. Two
consequences the code takes seriously:

- the new refresh token is stored, or the next refresh replays a dead token and the wall is signed
  out (that rotation is how theft is made detectable — honouring it is not optional);
- refresh is **single-flight**: five concurrent readers cause one refresh, not five. Five refreshes
  invalidate four of them, which is the same sign-out by a different route.

Refresh happens at **80% of the stated lifetime**, not at expiry: waiting for expiry means a request
can be in flight when the token dies, and a 401 costs a round trip on the wall.

## One retry rule

`401` → refresh once → retry once. **A second `401` after a successful refresh is
`Error::Unauthorized`**: the account is disabled, the token is for another server, or the clocks are
wrong. Retrying it all night is how a wall burns a rate limit and hides a real fault.

`403` is a failure to surface (authenticated, not allowed, or the account is not `Active`), `429` is
real and backs off, and both are ordinary errors the view loop will show. No status code ever reaches
the view: it gets typed values or an `Error`.

## Branching on the error, not the prose

The ticket that specified this client said errors carry a machine-readable `code`. **The pinned spec
has no such field**: `ErrorResponse` is `{status_code, message, data}` and `data.errors` maps a field
name to a human reason. So the client branches on `status_code` and on the field names in
`data.errors`, never on `message` — the prose is for a human and is free to change. Recorded here
because the ticket and the contract disagree, and the contract wins.

## Why a current-thread runtime on its own thread

The reads thread owns one `tokio` current-thread runtime and one `reqwest` client. The view loop
talks to it over an unbounded channel (`send` never blocks, `try_recv` never waits) and gets typed
replies on its next frame. Each read is spawned as its own task, so a slow picture read never delays
a card.

Rejected: `reqwest`'s blocking client on the UI thread (it *is* the freeze), and `block_on` per
command (one slow read would serialise every other read behind it). The socket actor (#4) wants the
same shape, so the two threads look alike.

## Time

Age comes from the server: `age_seconds` at read time, then `received_at` plus local **monotonic**
elapsed time between reads. This machine's clock and the server's differ (the dev feed's only fix was
38 days old by the server's own `age_seconds`), so any arithmetic from local wall-clock time against
a server timestamp produces a fix that is younger than it is — `INV-06` in one sentence. #6 owns
the display arithmetic; this module only refuses to invent a second source for it.

## What is deliberately absent

- **`POST /live-feed/fixes`** (the `--post-fix` dev flag) is not implemented. Writing a fix needs a
  commander account, which the wall must not have; a write path behind a flag that the wall's
  credential can never exercise is dead code that reads like a feature. It belongs to whoever has a
  commander account and a reason to plot a test fix.
- **A typed `GET /units/{id}/specifications`** model carries only the fields the card will draw
  (dimensions, displacement, endurance) plus `spec` as opaque JSON: the free-form note block is
  operator prose with no schema, and inventing fields for it is how a client breaks on the next entry.

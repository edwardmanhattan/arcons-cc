# ADR-0009 — Arcons Desk: a client that writes the live feed

- Status: proposed
- Date: 2026-11-20
- Ticket: epic [#275](https://github.com/edwardmanhattan/arcons-cc/issues/275), first ticket
  [#276](https://github.com/edwardmanhattan/arcons-cc/issues/276)
- Context: ADR-0002 (the Minos client), `docs/spec/minos-api.yaml`, `docs/minos-backend-postman.json`

## Context

The wall draws. It does not write, and that is not a preference: ADR-0002's account is read-only because
`POST /live-feed/fixes` is authorised by **being the vessel's current commander**, and a display standing in an
operations room commands nothing. The wall never posts a fix, an order or a message; it does not need a commander,
and giving it one would put a write credential in a process whose job is to draw.

So reporting from shore has no client. The only one that can is the handset aboard the ship: staff who learn a
position from an external source — a plot, a signal, a phone call — have to call the ship and ask it to report, and
the ship becomes the author of a fact it did not observe.

**Arcons Desk** is that client: a native app, Rune like the wall, on the desk where the operator sits. It reads the
standing picture, lets the operator place what the source said, and posts the fixes. The name is the room, not the
role: in `CONTEXT.md` "operator" remains the human at the wall, and the code says `desk`.

Three facts from the Minos contract shape it, and none of them is the payload's shape:

1. **One vessel per request.** `id_unit` is top-level, up to 60 fixes, all of one vessel. Sending five vessels is
   five requests.
2. **`recorded_at` decides the report's kind.** Within ±60 s of the server's clock the fix is *live* and
   re-stamped with server time; 60 s to 30 days old it is *backfill* and its own time is kept; older than 30 days
   it is refused, permanently, and counted; more than 60 s in the future it is re-stamped live and logged as a
   warning.
3. **A 200 is an answer.** `{accepted, rejected}` — a batch in which every fix was refused is still a 200. `403`
   is "you are not this vessel's commander", `404` is "no such vessel", and the two remedies differ.

## Decision 1 — one workspace, `crates/desk`

The app lives in this repository as a binary crate, not a second repository.

`[patch.crates-io]` is workspace-wide, and Rune is pinned there by rev along with six vendored crates. `vendor/` is
7.5 MB and holds the map engine. `src/map/geometry.rs` is the projection a drop needs, and the Minos client is the
one thing both apps must agree about. A second repository duplicates all four on the first day, and then they drift.

What that costs, stated plainly:

- `scripts/verify.sh` now builds and tests both binaries. A red desk test is a red wall PR; the reverse is also true.
- `cargo run` at the root stays the wall; `cargo run -p desk` is the desk. The ambiguity is a flag, not a guess.
- **`INV-01…12` stay claims about the wall.** The desk's claims get their own names, and
  `tests/invariant_coverage.rs` does not grow a line for them — a claim about a map's markers has no meaning for a
  form.
- The shared client grows a write path that the wall must never call. That is enforced by a test on the wall's
  account and by the client's own type, not by a comment in a module.

Rejected: a module inside the wall's binary. The wall's process should not hold a token that can write, and a form
beside a map is a different application with a different window.

## Decision 2 — the unit of work is one vessel

The operator composes a report **per vessel**, because the wire does. The review screen groups by vessel, and the
tray shows each vessel's state separately — accepted, partly rejected, refused — rather than one global "sent" that
is true of nothing.

The desk sends its own `device_id`: a stable id for the workstation, stored beside its credential. The field means
"the handset" in the contract; a desk telling Minos which workstation it is is the honest reading until Minos says
otherwise (see the open questions).

## Decision 3 — time is edited as the operator's clock, stored as UTC, and the regime is shown before submit

The field edits local civil time. The value is epoch milliseconds plus the offset that was in force, so the ISO
string sent is unambiguous and the screen can say what clock it was read from. `rune::LocalTime` gives the OS zone's
fields and offset for an instant; the inverse — a chosen wall clock back to an instant — is ours, because a DST gap
has no answer and a fold has two.

The ±60 s boundary is not a formatting detail. It is a different report, so the field says which one is being
written:

| what the operator enters | what Minos does | what the field says |
| --- | --- | --- |
| now, to 60 s old | live; the time is **replaced** with the server's | "live — re-stamped" |
| 60 s to 30 days old | backfill; the time is kept | "backfill — kept" |
| older than 30 days | refused, permanently, and counted | "refused — older than 30 days" |
| more than a few seconds ahead | re-stamped live and logged | "in the future — fix the clock" |

The first row is the one an operator hits by accident. A sighting typed as *45 seconds ago* is live, and its honest
time is discarded — the server replaces it with now. The field must say so while it is being typed, not after the
request. The last row is a refusal of our own: a future time cannot be reported faithfully, because the server will
overwrite it, so the desk blocks it beyond the clock skew it measured.

Two consequences for the control:

- **Seconds, not minutes.** The payload carries milliseconds and a backfill's value is the shape of a track.
- **Paste is a first-class input.** `2026-11-20T14:03:22Z`, `+07:00`, `20/11/2026 14:03`, `14:03` (today). The
  operator is reading times off reports; typing six segments is the slow path.

The server's clock is read from a response header and the skew is kept beside the credential. The verdict is
computed against the server's clock, because Minos decides by its own.

## Decision 4 — a drop on the map is a claim about now, and it becomes a pending row

Minos has no "replace this fix" and no planned position on the live feed: every fix is an observation. Dragging a
ship's marker therefore does not move it. It reports that the ship is there.

So a drop:

- creates a **pending row** — ship, lat/lon, time — in the tray, with the old position drawn as history and a tether
  to the new one;
- never submits by itself, because the operator is usually placing several vessels before sending;
- is undoable;
- is refused for a hull that is not in the reportable set, with the reason on screen, rather than at submit with a
  403.

The map's camera belongs to the UI, as it already does on the wall, so the drop's latitude and longitude are
`unproject(camera, viewport, x, y)` at the moment of the drop — no round trip to the render thread, no marker that
lags the pointer.

Keyboard parity: focus a row, Enter arms placement, arrows nudge, Enter commits, Escape cancels. A gesture only a
mouse can perform is not a feature this repository ships.

## Decision 5 — the UI SDK is a source of semantics, not a dependency

Vercel Labs' UI SDK cannot sit on Rune. It is a DOM behaviour library: 183 of the 392 files in `packages/core/src`
touch `HTMLElement`, `document` or `window`, and its primitive type carries `Attrs`, `HTMLElementEventMap` and
`el: HTMLElement`. Rune has no DOM, its own layout and text, and AccessKit instead of element reflection; a bridge
would need a DOM shim and a second layout engine.

What is worth taking is the split, because it is the part that survives a change of renderer. Their calendar and
date input keep every decision in a DOM-free model — `plain-date.ts`, `calendar-model.ts`, `date-input-model.ts` —
and the primitive only wires events. We port the semantics to Rust and keep Rune's rendering. The day-number
arithmetic is about 120 lines; the segment typing rules about 300; both get tests that need no window.

Rejected as borrowings: CSS-variable theming, slots and parts, SSR, the primitive factory. Each exists to work around
a DOM Rune does not have.

## What the desk does not do

- **Planned positions.** `PUT /games/{id}/units/{unit_id}/position` is the exercise plot, and it does not feed the
  live picture.
- **Editing history.** A fix is an observation and cannot be revised; a correction is another fix.
- **Fleet administration**, commander assignment, or messages.
- **An offline queue of unsent reports.** A queued report is a report whose time has moved, and the ±60 s rule makes
  a stale queue silently wrong. The draft survives a restart; the send does not.

## Rejected

- **A web app, which the UI SDK would suit.** It would be a second rendering stack, a second credential store, no
  offline map, and a second definition of "the picture" beside the wall's.
- **A second repository.** See decision 1.
- **The wall's credential with `POST` added.** One process would then hold a write token to draw a map, and a display
  in a room is the easiest place to leave one.
- **Auto-submit on drop.** The gesture says where; it does not say "send now", and a mis-drop would be a fix with no
  undo on the wire.
- **A default `recorded_at` of now, filled silently.** It converts a backfill into a live fix before the operator
  has said which they meant.
- **A hardcoded `+07:00`.** Indonesia has no DST today, which makes the hardcode correct until the first deployment
  elsewhere and then wrong for an hour a year.
- **A date/time widget taken from a crate.** None exists for Rune, and a DOM one needs a browser.

## The test that would make this wrong

- A drop that submits without review, or a report whose time the operator did not see before it left.
- Two places deciding live versus backfill: the field and the submit path must ask one function.
- A shared module through which the wall can post a fix, or a wall build that compiles the desk's write path.
- The desk needing a DOM, a browser, or a second layout engine.
- A queued report sent later with its original time and no indication that its regime changed.

## Consequences for the gate

- The desk is built and tested by `scripts/verify.sh`, and the two `map` jobs compile it too.
- Its screens are **named scenes with goldens**, like the wall's, so a layout regression is a red capture rather
  than a screenshot nobody takes.
- The desk's credential lives where the wall's does — `$XDG_CONFIG_HOME/arcons-cc/`, mode 0600 — and
  `--self-check` says which client it is checking. No secret in the tree, in a log line, or in a panic (`INV-08`
  applies to both binaries).
- `tests/notices.rs` covers any dependency the desk adds; `tests/doc_claims.rs` may grow a claim that the desk still
  groups by vessel, because that is the contract, not a layout choice.

## Open questions for Minos (blocked, not guessed)

1. **A reportable-set read for a shore account.** `/live-feed/me` returns the vessels the caller commands, and a desk
   commands none. A `GET /live-feed/reportable`, or a permission that widens `/me`, is the difference between a
   picker and a guess.
2. **`device_id` for a non-handset client.** Is a workstation id acceptable, or does the field become optional and
   typed?
3. **An idempotency key, and a `source` field.** One so a retry is not two fixes; one so a desk report is
   distinguishable from a beacon in the audit trail. Without the second, the audit cannot answer the question the
   whole app exists to make answerable — who said this, and how did they know.
4. **A future `recorded_at` beyond +60 s.** The contract says it is treated as live and re-stamped; decision 3's last
   row depends on that being the rule rather than an accident of the implementation.

# ADR-0009 — Arcons Desk: a client that writes the live feed

- Status: proposed
- Date: 2026-11-20
- Ticket: epic [#275](https://github.com/edwardmanhattan/arcons-cc/issues/275), first ticket
  [#276](https://github.com/edwardmanhattan/arcons-cc/issues/276)
- Context: ADR-0002 (the Minos client), `docs/spec/minos-api.yaml`, `docs/minos-backend-postman.json`
- Wireframe: `~/Documents/Arcons Desk — wireframe.tldraw` (outside the tree until the screens settle)

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

Four facts from the Minos contract shape it, and none of them is the payload's shape:

1. **One vessel per request.** `id_unit` is top-level, up to 60 fixes, all of one vessel. Sending five vessels is
   five requests. The desk never reaches the 60: it records **one** fix per vessel (decision 2).
2. **`recorded_at` decides the report's kind.** Within ±60 s of the server's clock the fix is *live* and
   re-stamped with server time; 60 s to 30 days old it is *backfill* and its own time is kept; older than 30 days
   it is refused, permanently, and counted; more than 60 s in the future it is re-stamped live and logged as a
   warning.
3. **A 200 is an answer.** `{accepted, rejected}` — a request in which the fix was refused is still a 200. `403`
   is "you are not this vessel's commander", `404` is "no such vessel", and the two remedies differ.
4. **Nothing says which vessels a desk may report.** `/live-feed/me` returns the vessels the caller *commands*, and
   a shore account commands none. There is no reportable-set read, so `403` is discovered by sending, not by
   filtering a list.

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

## Decision 2 — the desk records the last fix of a vessel, and a batch is many vessels

There is one **fix row per vessel**, holding that vessel's most recent observation. A hull never contributes a second
row, and the desk never composes a track: Minos already holds the last fix, and the desk's job is to say what the
picture does not yet know. A batch is therefore N vessels → N rows → **N requests**, one `id_unit` each.

This is the whole shape of the screen, and it is not a simplification of a richer model we declined to build: a
desk that collected six positions for one hull would be writing a track nobody asked for, at a precision the source
does not have, and one `403` would cost six fixes instead of one.

The batch is where the operator's leverage is: twenty hulls reported in one pass, each independently answered, each
row independently removable. So:

- the list shows every vessel from the standing picture, in age order, each with its pending state;
- selecting a vessel opens **that vessel's single row** for editing — select, then edit, never an editable grid;
- the tray is the batch: one card per vessel, its time, its regime, and a way to drop it;
- a send is a loop. Per-vessel answers are the result, and there is no global "sent" that is true of nothing.

`device_id` is a **UUIDv4 the desk mints for itself on first run**, stored beside its credential, and kept for the
life of that installation. It is not derived from the hostname, the MAC address or the login name: a value computed
from the machine would name it to Minos, and the rule that no host or username leaves this process (`INV-08`) is not
kept by a field that merely looks like an identifier. Sixteen bytes from the OS, the version and variant bits set,
written once at mode 0600, and the same id on every request from that workstation.

The field means "the handset" in the contract; a desk reporting which workstation said so is the honest reading.
Deleting the file or reinstalling mints a new id, which is the right behaviour — it is an installation identity, not a
licence, and two desks with the same id would make the audit trail lie.

The batch is a **draft, not a queue**. It survives a restart so a mis-click does not lose ten minutes of typing; it
is never sent with the time it had when it was drafted (see decision 3).

## Decision 3 — time is edited as the operator's clock, stored as UTC, and the regime is shown before submit

The field edits local civil time. The value is epoch milliseconds plus the offset that was in force, so the ISO
string sent is unambiguous and the screen can say what clock it was read from. `rune::LocalTime` gives the OS zone's
fields and offset for an instant; the inverse — a chosen wall clock back to an instant — is ours, because a DST gap
has no answer and a fold has two.

The wall clock is **WIB**, because that is what the operator is reading, with the derived UTC shown beside it as a
readout (`= 07:03:22Z`) that cannot be typed into. The wire is UTC; the arithmetic is the field's job, not the
operator's.

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

- **Seconds, not minutes.** The record is one observation, and the difference between 14:03:22 and 14:03 is the
  difference between a live fix and a backfill.
- **Paste is a first-class input.** `2026-11-20T14:03:22Z`, `+07:00`, `20/11/2026 14:03`, `14:03` (today). The
  operator is reading times off reports; typing six segments is the slow path.

The server's clock is read from a response header and the skew is kept beside the credential. The verdict is
computed against the server's clock, because Minos decides by its own. One function answers "what will this time
do", and the field, the row and the send all call it.

## Decision 4 — a drop on the map is a claim about now, and it fills that vessel's row

Minos has no "replace this fix" and no planned position on the live feed: every fix is an observation. Dragging a
ship's marker therefore does not move it. It reports that the ship is there.

So a drop:

- fills the selected vessel's row with lat/lon and a time of **now**, and draws the old position as history with a
  tether to the new one;
- never submits by itself, because the operator is usually reporting several vessels before sending;
- is undoable;
- is a *guess at the present*, so it is only as good as the clock — the regime line is what says whether the
  server will keep the time or replace it.

A vessel the account may not report is **not knowable in advance** (context fact 4). The desk lists every hull and
learns about a `403` at send, per vessel; the row stays on the board with the reason, so a granted command makes it
retryable. A picker that pre-filtered by a set Minos does not expose would be a guess wearing a permission's
clothes.

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

## Decision 6 — a source note is kept locally, per batch

The payload has no provenance field. `recorded_at` says when, `device_id` says which workstation, and nothing says
*how the desk knew*. That is the gap this application exists to close, so the desk keeps the answer itself: one free
text note per batch ("Plot 4 · phone call 14:03"), carried from the previous batch, overridable per row, stored in
the desk's own config directory, and never sent.

It is optional. A mandatory note is a note filled with "x" by the second day, and a field that is always "x" is
worse than an empty one, because it looks like evidence.

The note is the desk's own record, not Minos's. If Minos later grows a `source` field (open question 3), the note
becomes its value and stops being local-only.

## What the desk does not do

- **More than one fix per vessel.** It records the last, not a track (decision 2).
- **Import a track.** A pasted column of positions is a backlog Minos is not asking for, and a source that hands
  over a track is a source to integrate, not a form to fill.
- **Planned positions.** `PUT /games/{id}/units/{unit_id}/position` is the exercise plot, and it does not feed the
  live picture.
- **Editing history.** A fix is an observation and cannot be revised; a correction is another fix.
- **Fleet administration**, commander assignment, or messages.
- **An offline queue of unsent reports.** A queued report is a report whose time has moved, and the ±60 s rule makes
  a stale queue silently wrong. The draft survives a restart; the send does not.

## Rejected

- **A `device_id` derived from the hostname, the MAC address or the account name.** It would be a stable id with a
  free side effect: telling Minos which machine, and under whose login, reported a fix. The version-4 UUID says
  "this installation" and nothing else.
- **A web app, which the UI SDK would suit.** It would be a second rendering stack, a second credential store, no
  offline map, and a second definition of "the picture" beside the wall's.
- **A second repository.** See decision 1.
- **The wall's credential with `POST` added.** One process would then hold a write token to draw a map, and a display
  in a room is the easiest place to leave one.
- **A table of many fixes for one vessel.** It is the shape Minos's ceiling suggests and the shape the work is not:
  a vessel has one last position, and a six-row track for it is precision the source does not have.
- **A picker filtered by a reportable set.** Minos does not publish one for a shore account; inventing it locally
  would hide a vessel the operator can actually report (decision 4).
- **Auto-submit on drop.** The gesture says where; it does not say "send now", and a mis-drop would be a fix with no
  undo on the wire.
- **A default `recorded_at` of now, filled silently.** It converts a backfill into a live fix before the operator
  has said which they meant.
- **A hardcoded `+07:00`.** Indonesia has no DST today, which makes the hardcode correct until the first deployment
  elsewhere and then wrong for an hour a year.
- **A date/time widget taken from a crate.** None exists for Rune, and a DOM one needs a browser.

## The test that would make this wrong

- A drop that submits without review, or a report whose time the operator did not see before it left.
- Two places deciding live versus backfill: the field, the row and the send must ask one function.
- A vessel with two rows in one batch, or a batch sent as one payload.
- A list that hides a vessel because the desk guessed it could not report it.
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
  writes one fix per vessel and one request per vessel, because that is the contract, not a layout choice.

## Open questions for Minos (blocked, not guessed)

1. **An idempotency key, and a `source` field.** One so a retry is not two fixes; one so a desk report is
   distinguishable from a beacon in the audit trail. Without the second, the audit cannot answer the question the
   whole app exists to make answerable — who said this, and how did they know — and decision 6's local note is the
   stopgap.
2. **A future `recorded_at` beyond +60 s.** The contract says it is treated as live and re-stamped; decision 3's last
   row depends on that being the rule rather than an accident of the implementation.
3. **The intended path for a shore account's `403`.** The desk treats it as a late, per-vessel verdict because no
   reportable-set read exists. If a widened `/live-feed/me` or a `read`-style policy is planned, that changes what
   the picker can promise; until then the desk does not guess.

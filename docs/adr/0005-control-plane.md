# ADR-0005 — A client sends intents; the wall renders

- Status: accepted
- Date: 2026-11-13
- Revised: 2026-11-26 — decision 2's ownership reversed by the `ui`/`control` seam
- Ticket: [#171](https://github.com/edwardmanhattan/arcons-cc/issues/171), epic [#162](https://github.com/edwardmanhattan/arcons-cc/issues/162); revision [#305](https://github.com/edwardmanhattan/arcons-cc/issues/305), [#322](https://github.com/edwardmanhattan/arcons-cc/issues/322), [#326](https://github.com/edwardmanhattan/arcons-cc/issues/326)

## The revision

The decision's core is unchanged: **a client sends intents, and the wall renders them**. The ownership in decision 2
is reversed from the 2026-11-13 text. The original made `control` the server that owns `State` and applies intents,
and `ui` the viewer. The build showed that the wall's rules, its projection and its scene belong to the state:
`control` was forced to name `ui` types to apply anything, and the seam became a module cycle.

The seam moved the wall to `ui`, made `control` the wire and its carriers, and put the shared vocabulary in
`src/verb.rs` (#305, #307–#319); the eight glass-only presses follow through `State::local` (#326). A pointer, a
menu row, a socket job and a capture script all arrive at `State::apply`; `control::Host` is a passive mailbox and
`State::answer` settles its jobs on the thread that owns the state. Decisions 1 and 3–7 stand. The "as built"
sections below describe the current shape; the 2026-11-13 text is in git history.

## The problem, in the operator's words

> Make the server and the client architecture. The server instance should be in control of everything and run
> the UI-facing stuff, and expose APIs to control it — panes, map, objects, workspaces, the cursor, general. The
> Viewer Client and a CLI Client both talk to it, and later a mobile remote.

The wall was built the other way round, and honestly: **there is one writer, and it is the pointer.**
`State` had twenty mutators — `focus_pane`, `maximise`, `close_pane`, `split_pane_as`, `move_pane_to`,
`switch_to`, `add_workspace`, `set_camera_of`, `zoom_about`, `begin_rename`, `commit_rename`, and the menu's
`run` — and every one of them was called from an `El` handler or from `poll`. That is not a defect; it is what a
display with one hand on it looks like. It is also why a second client is not "add a socket": the socket would be
the twenty-first caller of twenty functions whose only contract is that the pointer happened to call them in a
sensible order.

`docs/adr/0004-interaction-state.md` did this once already, for the same reason: seven booleans became one
`Interaction` with one `Begin` vocabulary and a table, and the states the design did not allow stopped being
expressible. This is the same move one level up — **one input vocabulary for every input**, so the hand and a
socket are the same kind of caller.

## What the state actually holds

The wall's control-plane state, as a client would see it (the fleet, the cards and the pictures are not in this
list on purpose — see *what a client may read*):

| the wall holds | where | a client that wants it |
| --- | --- | --- |
| the workspaces, their names, which is active | `State::workspaces`, `State::active` | `#166`'s close-all, `#168`'s projection |
| each pane's kind, id, tiling position | `Workspace::panes`, `Layout` | `pane.focus`, `pane.move`, `pane.close`, `pane.drop`, `pane.resize` (#219) |
| each pane's camera and zoom | `Pane::camera`, the map thread | `map.camera`, `map.zoom` |
| which pane is focused, which is full size | `State::focused`, `maximised` | `pane.focus`, `pane.toggle_full_size` |
| what one surface is open over the wall | `State::interaction` | `surface.close`, the cursor vocabulary |
| one pane's selection, hence its card | `Pane::selected` | `object.select`, `object.follow` (#169) |

The mutators are now reached through **one door**, `State::apply`/`apply_as` in `src/ui/door.rs`. The pointer, a
menu row, a socket job and a capture script all arrive there. The glass-only presses that have no wire verb yet
(the newsroom's six controls, a tree post, a facility) enter through `State::local` (#326), which is a second
entry to the same implementation, not a second implementation.

## The decision

**1. A client sends intents. The wall renders.** The picture does not cross a process boundary, or a socket, in
v1. A client that receives pixels needs a codec, a frame clock, a latency budget and a reason to keep `INV-04`'s
per-pane projection honest on a second machine. A client that sends intents needs a list of verbs. The ask's five
API families are *control* verbs, and a phone is a **controller**, not a renderer.

**2. One process, two sides of one seam.** The **wall** (`src/ui`) owns `State` and the door: `State::apply` and
`State::apply_as` in `src/ui/door.rs` are the only places an input settles a change. The **control plane**
(`src/control`) is the wire and its carriers: `wire` (JSON), `server` (the unix listener), `relay` (the outbound
carrier), `host` (the mailbox that owns the queue, the listener and the relay) and `sessions`. It owns no wall
state. The **viewer** is a client of the wall: the display's own pointer is an in-process client, and a `ctl` or a
phone is a carrier's.

`control::Host::next_job` hands one job at a time to `State::answer(job)` on the thread that owns the state. No
carrier reads a field of the wall, and no job is applied off the UI thread (`INV-05`). `src/verb.rs` is the shared
vocabulary leaf; `control` production names no `ui` type. The wall is the server of the control plane; `control` is
the wire that reaches it.

**3. Two vocabularies, not one.**
- An **`Intent`** carries a *value*: `FocusPane(pane)`, `Rename { target, name }`, `Camera { pane, camera }`,
  `MovePane { pane, workspace }`. One intent is one settled change. The type lives in `src/verb.rs`, shared by the
  wall and the wire.
- A **cursor event** carries a *position or a key*: `CursorMove`, `CursorPress`, `CursorRelease`. It exists for
  surfaces that are menu-like, where "which row" is a spatial question the wall must answer with its own
  hit-testing.

A rename over the wire is a **name**, not nine keystrokes; a menu walk is **keys**, because a row is a place. A
protocol that carries only the first cannot drive a menu without inventing a row index; one that carries only the
second cannot rename without a keyboard model. Both, and the rule that separates them, is the decision.

**4. Transport: a unix socket, and the socket is the credential.** `$XDG_RUNTIME_DIR/arcons-cc/control.sock`
(falling back to the config directory), created `0600`, unlinked on start when stale and on exit, **one client at
a time**, line-delimited JSON with a version on every message:

```json
{"v": 1, "intent": {"verb": "pane.focus", "pane": 2}}
{"ok": true,  "changed": true}
{"ok": false, "why": "the last pane on the wall"}
```

No TCP, no port, no token, no handshake and no secret in a log line: file permissions are the authentication, and
the wall is air-gapped (`INV-08`). The version is a constant in `src/control.rs` so the ADR and the code can be
compared by a test rather than by a reader.

**5. Determinism: a capture is not a server.** A headless run opens no socket, accepts no intent except from its
own script, and answers nothing to nobody (`INV-11`). `State` knows whether it is headless; the control plane
asks it and returns `Refused("headless")` rather than binding a path a CI machine may not have. A golden that
depended on a socket would not be a golden.

**6. A refusal is named, and the rule lives once.** `ClosePane` on the last pane on the wall is
`Refused("the last pane on the wall")`, produced by the same `State::can_close` the menu row is built from
(#141). `control::REFUSALS` tabulates every refusal message and its kind, so a new refusal is a visible row and
`tests/scaffold.rs` fails when the table and the literals disagree. `INV-07`'s rule — a new case is a visible
branch, never a default — applies to verbs as it applies to wire enums: an unknown verb is not ignored, it is
`{"ok": false, "why": "unknown verb"}`.

**7. A client may read a projection, and nothing else.** The read half is a small, versioned snapshot of the
*control* state in the table above: workspaces and their panes, ids, kinds, cameras, which is focused, which is
full size, a pane's title. Not the fleet, not the cards, not the frames, not the picture. A client that reads
hull positions starts to be a renderer, and the whole point of decision 1 is that it is not. The wire shape lives
in `src/control/projection.rs`; the one function that builds it from `State` lives in `src/ui/projection.rs`, so
the projection cannot describe a wall that is not there.

## The two vocabularies, in a table

| the guesture the wall has | the intent it means | or a cursor event? |
| --- | --- | --- |
| press a chip | `FocusWorkspace(index)` | — |
| right-click another workspace's chip | `BeginRename(Workspace(index))` | — (a press *is* the input; the surface is a rename sheet) |
| right-click the active chip | the workspace menu: `BeginRename(Workspace(index))`, and `CloseWorkspace(index)` where it is empty and not the wall's last (#242) | the menu is a surface; each row is an intent |
| press a pane's title | `ToggleFullSize(pane)` | — |
| press a menu row | `SplitPane{..}`, `ClosePane`, … | the *press that opened the menu* is a position; the row is an intent once chosen |
| drag a border | `Resize{..}` (one, on release) | the drag's intermediate points are the viewer's own |
| drag the water | `Camera{..}` (one, on release) | a pan is continuous, and `INV-05` says interpolation is display-only |
| wheel over a pane | `Camera{..}` (a zoom is a camera change) | — (a wheel is already discrete) |
| hover a menu row | — | `CursorMove` (a row is a place) |
| Escape | `CloseSurfaces` | — (one surface per press; #140's rule) |

The rule the table encodes: **anything the guesture settles into a value is an intent; anything whose meaning is
"which one" is a cursor event.** The first is testable without a renderer; the second needs the wall's own
hit-testing, which is why it is a smaller vocabulary and why it came later (#167, #164).

## What this buys

- **One door for state changes.** `State::apply` is the only place `State` mutates from a settled input, so "the
  mouse can do it and the CLI cannot" stops being a category of bug, and a refused change has one definition.
- **A second client for the price of a socket.** The CLI is the same code path as the pointer; a capture driven
  by intents and one driven by a script can be compared pixel for pixel (#172).
- **The wall's rules stay the wall's.** `can_close`, "absence is not zero", the layout file's `deny_unknown_fields`:
  none of them relax because a socket asked. A remote asking for something the design refuses gets the rule's
  name back.
- **A testable seam for the next four tickets**: `#167` (cursor) and `#164` (menu keys) are new *inputs* to a
  vocabulary that exists; `#166` (close-all) is two intents; `#169` (follow) is a new mode of a pane the intent
  can set.

## The cost

- **A projection to keep true.** It will drift from `State` unless a test compares them; the builder is
  `src/ui/projection.rs` and the wire shape is `src/control/projection.rs`, one function from the state to the
  snapshot.
- **Two callers to test.** Every intent needs a test for what it does and one for what it refuses, which is more
  tests than a handler needed. The menu's table test (#170) pays it once: every row names an intent, and every
  intent has a caller.
- **One more thing to reason about at shutdown.** A socket that outlives the process, or an intent applied after
  the last frame, is a new class of bug; #165 unlinks on exit and applies between frames only.
- **The temptation to bypass it.** The moment a handler mutates `State` directly "just this once", there are two
  dialects again. `tests/scaffold.rs` reads every `src/ui/*.rs` and fails on a direct settled mutator; the
  glass-only `State::local` entry is the named place for the presses that have no wire verb yet (#326).

## Rejected

- **Pixels over the wire (a real remote renderer).** Rejected: it is a different product with a codec, a frame
  clock and a bandwidth budget, it makes `INV-04`/`INV-12` a network claim, and none of the ask's verbs need it.
  If it ever happens it is its own epic and it starts with a codec.
- **A second process now.** Rejected for v1: the picture would have to cross a boundary to a renderer, so the
  split buys nothing but a state-replication protocol to keep faithful. The test that would change it is below.
- **One RPC per mouse event.** Rejected: a drag would be 60 intents a second, the wall would apply changes
  mid-draw, and `INV-05`'s "interpolation is display only" would be broken by a client that sent interpolated
  values as state.
- **TCP, HTTP or WebSocket on localhost.** Rejected: a port is reachable by every process on the machine and by
  anything the machine's network reaches; the unix socket's 0600 is a smaller promise to keep and easier to audit
  than a token, and HTTP gives nothing a single line of JSON does not.
- **D-Bus.** Rejected, and named because it is the Linux-native answer: it brings a session bus and a generated
  interface on a wall whose deployment is one binary in a systemd unit, its permission model is policy files
  rather than a mode, and its introspection would be a second schema to keep alongside the one `docs/control.md`
  will define.
- **An in-process plugin for the "remote client".** Rejected: a plugin shares the wall's failure modes and its
  address space, so it cannot be the client that proves the seam is real.

## The remote, as a phone (#168)

`docs/mobile-remote.md` is the note. Two of its decisions touch this ADR directly, and both keep it intact:

- **The projection is the client's only truth.** A phone reads the wall on connect and on every resume, and never
  replays intents — an intent is a change, and a wall that changed while the client was away would be changed wrongly
  by a replay. This is the read half (#181) used the way it was designed to be used, not extended.
- **The wall still does not listen.** The rejected "TCP, HTTP or WebSocket" above stands: a phone reaches the wall
  through an **outbound carrier** the wall dials — the same authenticated, reconnecting connection it already holds to
  Minos — and the relay's protocol is a contract this repo pins and tests. The unix socket stays the local credential,
  and pairing (a code drawn on the wall, a hashed device key in `devices.toml`) is the phone's, not the socket's.

## The test that would make this two processes

The decision is revisited — deliberately, with a ticket — when **either** is true:

1. **A client must outlive the viewer**: something has to keep answering while the wall is not rendering (a
   service that reconfigures the wall between deployments, a remote that must work with the display asleep).
2. **A wall with no display**: a headless instance that only serves the control plane has no viewer to be the
   first client, and then the projection becomes the whole contract rather than a convenience.

Until one of those is true, a second process is a protocol to maintain with no customer, and this ADR says so
rather than leaving it to be discovered. **Both became true when the wall became a fleet** — see
[ADR-0006](0006-a-fleet-of-walls.md), which makes the plane a process of its own and keeps this ADR's reasoning: the
picture still never crosses the wire, and `State` is still applied in one place.

## As built: the vocabulary and the door (#170, #173, #326)

`src/verb.rs` is the shared leaf: `Intent`, `Edge`, `ViewId`, `PaneContent`, `RenameTarget`, `Origin`, `EDGES`,
`Intent::verb()` and `vocabulary()`. It names `map::geometry` and nothing of `ui` or `control`;
`tests/scaffold.rs` holds the leaf rule.

`src/ui/door.rs` is the door: `State::apply` and `State::apply_as` are the public entry, `settle` is the body the
menu's leaf and a cursor's derived changes reuse, and `State::answer`/`answer_request` settle a carrier's job.
The pointer, the menu, the socket and a capture's script all arrive here. `src/control/wire.rs` parses a request
into `verb::Intent`; `src/control/authority.rs` and the carriers never construct a `ui` type.

**The glass-only entry** is `State::local(Local)` for the presses that have no wire verb yet: the newsroom's six
controls, a tree post and a facility. It dispatches to the same functions those handlers called, so the change has
one implementation; `control::wire` cannot construct a `Local`. `tests/scaffold.rs` scans every `src/ui/*.rs` and
has no bypass table since #326.

**Three things are deliberately outside the settled vocabulary**, and this is the part a later reader will want
when they find a direct mutation and wonder whether the rule was broken:

- **The pointer's held gestures** — a pane in the air, a border being dragged, the menu opened *at a position*,
  the filter panel anchored to its toggle, the drag that moves a camera per event because the map thread must
  follow the hand. They are surfaces the wall draws around a pointer; a client with no pointer gets them from the
  cursor vocabulary (#167), not from a second spelling of them here.
- **Data folding** — a fix from `live-feed`, a reply from the reads thread: the wall hearing from Minos, not a
  client's ask.
- **The sheet's own editing** — `TextEdit` in the rename field is the operator's keyboard, and `commit_rename`
  and `Intent::Rename` share the one function that applies a name (`ui::rename_to`), so a typed name and a sent
  name land identically.

**The refusal names are the ADR's and the wall's, not a new set**: `ClosePane` on the last pane returns
`"the last pane on the wall"` from `State::can_close`, the predicate the menu row is built from. `"no such pane"`,
`"no such workspace"`, `"the pane is already in that workspace"`, `"the tiling refused the pane"`,
`"the pane has no frame yet"` — each is one reason a client can act on, tabulated in `control::REFUSALS`.

**The table test** is `ui::tests::every_menu_row_is_an_intent_or_a_surface`: every row a menu offers is an
intent, every waterfall leaf is `pane.move`, and the one row that is not an intent is the trigger that opens a
second surface of the same menu. `control::tests::every_intent_has_its_own_verb` enumerates every variant, so a
new one that forgets its verb is a red test rather than a document that half-describes the wall.

## As built: the read half (#181, #221)

Decision 7 said a client may read a **projection** of the control state and nothing else. It exists:
`{"v": 1, "read": "wall"}` answers with the workspaces, their names, the panes in them (id, kind, title, camera,
selection), which is on screen, which has the keys, and what is full size. The wire shape is
`src/control/projection.rs`'s `Wall`; the one builder that reads `State` is `src/ui/projection.rs`'s `of`, and
`versioned` stamps the sequence. `control` names the shape, not the state.

The line decision 7 drew is a test: the projection is serialized in a test and asserted to carry no fleet, no fix,
no card and no story vocabulary. A client that could read where the hulls are would be the renderer decision 1
refused to send.

A request carries **an intent or a read, never both**, and the same connection answers either — a client that had
to reconnect to look would be a client with two connections open on a one-client socket.

## As built: the cursor is a value, not a stream (#167)

Decision 3 said the second vocabulary carries **pointer and key events** for surfaces that are menu-like. It was
built smaller than that, and on purpose: `cursor.move` carries a **point**, not a delta, and `cursor.press` /
`cursor.release` are settled events. A remote's cursor is not a hand — it is a coordinate a phone computed from a
map it has read — so the wall needs the place it means, not the path it took to get there. Where a press *means*
something is the wall's own hit-testing (a hull within 14 points, the pane a point is in), which is why the press is
an intent the wall resolves and not an event the client interprets.

The same decision's other half is recorded where a client's author looks for it (`docs/control.md`): a press reaches
the picture panel — a hull and the water — and not the header's chips or a menu's row, because those are Rune's
layout and a synthetic press there needs an event injection Rune does not expose.

## Consequences for the gate

- `src/control.rs` holds the wire's constants — the socket's file name and mode, the protocol version — and
  `tests/doc_claims.rs` fails when this document and that code disagree. A document that ages is a red test, the
  pattern ADR-0001 already uses for the pinned rev.
- `src/verb.rs` is the vocabulary leaf; `tests/scaffold.rs` fails if it names `ui` or `control`, and scans every
  `src/ui/*.rs` for a handler that calls a settled mutator directly.
- `docs/control.md` (#165) is the reference: every verb, its arguments, what it refuses. The tripwire there is
  the same shape — a verb in the code that the document does not name is a failure.
- A headless scene never binds the socket; `INV-11` already forbids a capture reading or writing a file, and a
  socket is both.

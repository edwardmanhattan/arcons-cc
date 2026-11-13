# ADR-0005 — A client sends intents; the wall renders

- Status: accepted
- Date: 2026-11-13
- Ticket: [#171](https://github.com/edwardmanhattan/arcons-cc/issues/171), epic [#162](https://github.com/edwardmanhattan/arcons-cc/issues/162)

## The problem, in the operator's words

> Make the server and the client architecture. The server instance should be in control of everything and run
> the UI-facing stuff, and expose APIs to control it — panes, map, objects, workspaces, the cursor, general. The
> Viewer Client and a CLI Client both talk to it, and later a mobile remote.

The wall was built the other way round, and honestly: **there is one writer, and it is the pointer.**
`State` has twenty mutators — `focus_pane`, `maximise`, `close_pane`, `split_pane_as`, `move_pane_to`,
`switch_to`, `add_workspace`, `set_camera_of`, `zoom_about`, `begin_rename`, `commit_rename`, and the menu's
`run` — and every one of them is called from an `El` handler or from `poll`. That is not a defect; it is what a
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
| each pane's kind, id, tiling position | `Workspace::panes`, `Layout` | `pane.focus`, `pane.move`, `pane.close` |
| each pane's camera and zoom | `Pane::camera`, the map thread | `map.camera`, `map.zoom` |
| which pane is focused, which is full size | `State::focused`, `maximised` | `pane.focus`, `pane.toggle_full_size` |
| what one surface is open over the wall | `State::interaction` | `surface.close`, the cursor vocabulary |
| one pane's selection, hence its card | `Pane::selected` | `object.select`, `object.follow` (#169) |

Twenty mutators, one enum of what is interacting, and — today — one caller: `src/ui`. The rest of this document is
what that costs a second client and what is worth paying to avoid it.

## The decision

**1. A client sends intents. The wall renders.** The picture does not cross a process boundary, or a socket, in
v1. A client that receives pixels needs a codec, a frame clock, a latency budget and a reason to keep `INV-04`'s
per-pane projection honest on a second machine. A client that sends intents needs a list of verbs. The ask's five
API families are *control* verbs, and a phone is a **controller**, not a renderer.

**2. One process, two roles.** The **server** is the module that owns `State` and applies intents (`src/control`);
the **viewer** is the module that draws it (`src/ui`) and, in v1, is also the client with the most traffic. They
are separable now because the seam is a function call, and they become two processes **only** when a client
cannot live in the wall's process — the test is in *the test that would make this two processes*, below.

**3. Two vocabularies, not one.**
- An **`Intent`** carries a *value*: `FocusPane(pane)`, `RenamePane { pane, name }`, `Camera { pane, at, zoom }`,
  `MovePane { pane, workspace }`. One intent is one settled change.
- A **cursor event** carries a *position or a key*: `PointerMove`, `Press`, `Key(Down)`. It exists for surfaces
  that are menu-like, where "which row" is a spatial question the wall must answer with its own hit-testing.

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
(#141). `INV-07`'s rule — a new case is a visible branch, never a default — applies to verbs as it applies to
wire enums: an unknown verb is not ignored, it is `{"ok": false, "why": "unknown verb"}`.

**7. A client may read a projection, and nothing else.** The read half is a small, versioned snapshot of the
*control* state in the table above: workspaces and their panes, ids, kinds, cameras, which is focused, which is
full size, a pane's title. Not the fleet, not the cards, not the frames, not the picture. A client that reads
hull positions starts to be a renderer, and the whole point of decision 1 is that it is not.

## The two vocabularies, in a table

| the guesture the wall has | the intent it means | or a cursor event? |
| --- | --- | --- |
| press a chip | `FocusWorkspace(index)` | — |
| right-click a chip | `BeginRename(Workspace(index))` | — (a press *is* the input; the surface is a rename sheet) |
| press a pane's title | `ToggleFullSize(pane)` | — |
| press a menu row | `SplitPane{..}`, `ClosePane`, … | the *press that opened the menu* is a position; the row is an intent once chosen |
| drag a border | `SetRatio{..}` (one, on release) | the drag's intermediate points are the viewer's own |
| drag the water | `Camera{..}` (one, on release) | a pan is continuous, and `INV-05` says interpolation is display-only |
| wheel over a pane | `Zoom{pane, steps}` | — (a wheel is already discrete) |
| hover a menu row | — | `PointerMove` (a row is a place) |
| Escape | `CloseSurfaces` | — (one surface per press; #140's rule) |

The rule the table encodes: **anything the guesture settles into a value is an intent; anything whose meaning is
"which one" is a cursor event.** The first is testable without a renderer; the second needs the wall's own
hit-testing, which is why it is a smaller vocabulary and why it came later (#167, #164).

## What this buys

- **One door for state changes.** `apply` is the only place `State` mutates from an input, so "the mouse can do
  it and the CLI cannot" stops being a category of bug, and a refused change has one definition.
- **A second client for the price of a socket.** The CLI is the same code path as the pointer; a capture driven
  by intents and one driven by a script can be compared pixel for pixel (#172).
- **The wall's rules stay the wall's.** `can_close`, "absence is not zero", the layout file's `deny_unknown_fields`:
  none of them relax because a socket asked. A remote asking for something the design refuses gets the rule's
  name back.
- **A testable seam for the next four tickets**: `#167` (cursor) and `#164` (menu keys) are new *inputs* to a
  vocabulary that exists; `#166` (close-all) is two intents; `#169` (follow) is a new mode of a pane the intent
  can set.

## The cost

- **A projection to keep true.** It will drift from `State` unless a test compares them; the ticket for the read
  half must build the projection *from* `State` in one function, and keep the number of fields small enough to
  read.
- **Two callers to test.** Every intent needs a test for what it does and one for what it refuses, which is more
  tests than a handler needed. The menu's table test (#170) pays it once: every row names an intent, and every
  intent has a caller.
- **One more thing to reason about at shutdown.** A socket that outlives the process, or an intent applied after
  the last frame, is a new class of bug; #165 unlinks on exit and applies between frames only.
- **The temptation to bypass it.** The moment a handler mutates `State` directly "just this once", there are two
  dialects again. The scaffold test that forbids it is part of #173, not a convention.

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

## The test that would make this two processes

The decision is revisited — deliberately, with a ticket — when **either** is true:

1. **A client must outlive the viewer**: something has to keep answering while the wall is not rendering (a
   service that reconfigures the wall between deployments, a remote that must work with the display asleep).
2. **A wall with no display**: a headless instance that only serves the control plane has no viewer to be the
   first client, and then the projection becomes the whole contract rather than a convenience.

Until one of those is true, a second process is a protocol to maintain with no customer, and this ADR says so
rather than leaving it to be discovered.

## As built: the vocabulary, and what it does not cover (#170)

`src/control.rs` now holds the decision's shape: `Intent` (sixteen variants over panes, workspaces, the map and
surfaces), `apply(state, intent) -> Result<Changed, Refused>`, and `Intent::verb()` — the wire name, in one
place, so `docs/control.md` (#165) and the CLI cannot spell one differently. Every input the wall has goes
through `apply`: the menu's rows, `on_key`'s Escape, the chips, the pane's name plate and full-size toggle, the
header's tree/newsroom/settings words, the wheel, a press on the water, a press on a hull.

**Three things are deliberately outside it**, and this is the part a later reader will want when they find a
direct mutation and wonder whether the rule was broken:

- **The pointer's held gestures** — a pane in the air, a border being dragged, the menu opened *at a position*,
  the filter panel anchored to its toggle, the drag that moves a camera per event because the map thread must
  follow the hand. They are surfaces the wall draws around a pointer; a client with no pointer gets them from the
  cursor vocabulary (#167), not from a second spelling of them here.
- **Data folding** — a fix from `live-feed`, a reply from the reads thread: the wall hearing from Minos, not a
  client's ask.
- **The sheet's own editing** — `TextEdit` in the rename field is the operator's keyboard, and `commit_rename`
  and `Intent::Rename` share the one function that applies a name (`ui::rename_to`), so a typed name and a sent
  name land identically.

**The refusal names are the ADR's and the UI's, not a new set**: `ClosePane` on the last pane returns
`"the last pane on the wall"` from `State::can_close`, the predicate the menu row is built from. `"no such pane"`,
`"no such workspace"`, `"the pane is already in that workspace"`, `"the tiling refused the pane"`,
`"the pane has no frame yet"` — each is one reason a client can act on.

**The table test** is `ui::tests::every_menu_row_is_an_intent_or_a_surface`: every row a menu offers is an
intent, every waterfall leaf is `pane.move`, and the one row that is not an intent is the trigger that opens a
second surface of the same menu. `control::tests::every_intent_has_its_own_verb` enumerates every variant, so a
new one that forgets its verb is a red test rather than a document that half-describes the wall.

## Consequences for the gate

- `src/control.rs` holds the wire's constants — the socket's file name and mode, the protocol version — and
  `tests/doc_claims.rs` fails when this document and that code disagree. A document that ages is a red test, the
  pattern ADR-0001 already uses for the pinned rev.
- `docs/control.md` (#165) is the reference: every verb, its arguments, what it refuses. The tripwire there is
  the same shape — a verb in the code that the document does not name is a failure.
- A headless scene never binds the socket; `INV-11` already forbids a capture reading or writing a file, and a
  socket is both.

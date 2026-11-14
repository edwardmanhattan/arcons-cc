# The control plane — `docs/control.md`

**A client sends intents; the wall renders.** A phone is the second client this document is written for — the
transport it needs, the pairing, and the rules it must follow are in [`mobile-remote.md`](mobile-remote.md), and it is
the same vocabulary on both. This is the reference for the socket that lets something other than
a hand drive the wall: the `arcons-cc ctl` CLI today, a phone later. The decision behind it is
[`docs/adr/0005-control-plane.md`](adr/0005-control-plane.md); the vocabulary is
[`src/control.rs`](../src/control.rs), and `tests/doc_claims.rs` fails when this file and that code disagree about
a verb.

## The socket

| | |
| --- | --- |
| path | `$XDG_RUNTIME_DIR/arcons-cc/control.sock`, or `$XDG_CONFIG_HOME/arcons-cc/control.sock` when the run directory is missing or not absolute |
| mode | `0600`, inside a `0700` directory |
| credential | **the file's permissions.** There is no token, no handshake and no secret on the wire (`INV-08`) |
| clients | **one at a time.** A second client waits for the first to finish; the wall is one display |
| framing | one JSON object per line, in both directions |
| protocol | `1`, on every message. Another number is refused, never negotiated |
| off switch | `ARCONS_CONTROL=0` turns the plane off and says so in one log line |
| captures | a headless scene binds nothing (`INV-11`): a capture opens no socket |
| platforms | **unix only today.** Windows compiles the vocabulary and has no plane: a named pipe is a decision nobody has taken, and `--self-check` says so rather than pretending |

```json
{"v": 1, "intent": {"verb": "pane.focus", "pane": 2}}
{"v": 1, "ok": true, "changed": true}
{"v": 1, "ok": false, "why": "the last pane on the wall"}

{"v": 1, "read": "wall"}
{"v": 1, "ok": true, "wall": { … }}
```

**One or the other, never both**: a line that changed the wall and read it at once could not say which answer the
client is waiting for. A request with neither is refused by name.

A request is refused **by name** and **on the same connection**: an unknown verb, a missing field, a wrong type or
a rule the wall will not break all come back as `{"ok": false, "why": …}`, and the next line may be a fresh
request. `changed` says whether anything on screen moved — a `pane.focus` on the pane that already has the keys is
`ok` and `changed: false`.

## The intents

Every request carries `verb`, spelled exactly as the table says. Fields are listed in the right-hand column; a
field marked **required** is a refusal when it is missing.

| verb | fields | what it does, and what it refuses |
| --- | --- | --- |
| `pane.focus` | `pane` | gives that pane the keys. `no such pane` |
| `pane.full_size` | `pane` | draws it over the wall, or puts it back if it is already there (#85). `no such pane` |
| `pane.close` | `pane` | removes the pane, joining its map thread. **`the last pane on the wall`** — the same rule the menu row is built from (#141) |
| `rename.begin` | `target` | opens the rename sheet on a pane or a workspace, without switching to it (#139, #154). `no such pane`, `no such workspace` |
| `rename.set` | `target`, `name` | sets the name now, with no sheet. `name: null` clears it: a pane falls back to its number, a workspace to `Workspace N`. `no such pane`, `no such workspace` |
| `pane.split` | `pane`, `content` | a new pane beside it showing the same water. `no such pane` |
| `pane.split_at` | `pane`, `at` | the same, at a point in **that pane's frame** pixels — what the pane menu's `Focus on new pane` does, so the new pane shows the water that was clicked (#77). `the pane has no frame yet` (a map that has not drawn once) |
| `workspace.from_pane` | `pane` | a workspace of its own holding a new pane of that kind and camera (#140). `no such pane` |
| `pane.move` | `pane`, `workspace` | carries the pane — camera, thread, view, name, filter, frame — into that workspace (#140). `no such pane`, `no such workspace`, `the pane is already in that workspace`, `the tiling refused the pane` |
| `pane.close_others` | — | closes every pane in the **workspace on screen** but the one with the keys (#166), one at a time through the close a menu row uses. Never a workspace the client cannot see. `no pane has the keys` (an empty workspace) |
| `workspace.close_others` | — | closes every workspace but the one on screen (#166): the chips go, their panes go with them and join their map threads, and **the active workspace is never closed and never emptied** |
| `pane.open` | `content` | a pane of that kind beside the focused one — the header's tree and newsroom controls (#116, #136), asking their first reads |
| `object.follow` | `pane`, `unit`, `zoom` | **locks that pane's camera onto a hull** (#169): every frame the camera centres the hull's current fix, at `zoom` if one is given (once — the pane keeps it afterwards). `unit: null` lets go. `no such pane`, `the pane draws no map`, `the hull has not reported` (no fix is not a fix at zero, `INV-01`) |
| `pane.select` | `pane`, `unit` | puts that pane's card on a hull; `unit: null` clears it (#99). `no such pane` |
| `workspace.focus` | `index` | makes that workspace the wall on screen (#138). `no such workspace` |
| `workspace.new` | — | appends an empty workspace and switches to it (#138) |
| `map.camera` | `pane`, `camera` | puts that pane's camera at `{lat, lon, zoom}` — a **value**, so a pan is one request when the hand stops, not sixty. `no such pane` |
| `menu.focus` | `row` | puts the **keyboard's ring** on a row of the open menu — or of the waterfall, when one is open. `row: null` takes it off; a row past the end is clamped. `no menu is open` |
| `menu.submenu` | `open` | `true` opens the ringed row's second surface and puts the ring on its first row; `false` closes it and keeps the menu. `no menu is open`, `no row has the ring`, `the row opens nothing` |
| `menu.choose` | — | acts on the **ringed** row: a row that opens a surface opens it, a leaf runs its action (`pane.split`, `pane.close`, …). Never the row under a pointer. `no menu is open`, `no row has the ring` |
| `cursor.move` | `at` | puts the **client's cursor** at `[x, y]` in the picture panel's points, and starts its fade (#167) |
| `cursor.press` | — | presses where the cursor is: the hull under it (its card, and that pane's keys), or the pane it is over (the keys and a cleared card). A press outside every pane, or before `cursor.move`, is refused |
| `cursor.release` | — | releases; the cursor fades from here |
| `surface.settings` | — | opens the settings sheet (#82) |
| `surface.close` | — | dismisses **one** surface, in the ladder Escape walks: the waterfall, then the menu, then the interaction, then the maximised pane, then the newsroom's detail, then the selection (#140) |

### Field shapes

| field | shape | example |
| --- | --- | --- |
| `pane` | a pane id, a non-negative integer | `2` |
| `workspace`, `index` | a workspace index, from 0, in chip order | `1` |
| `unit` | a hull's id, or `null` | `512345` |
| `target` | `{"pane": 2}` or `{"workspace": 1}` | |
| `name` | a string, or `null` to clear | `"Straits watch"` |
| `at` | `[x, y]`. For `pane.split_at`, the pane's **frame** pixels; for `cursor.move`, the **picture panel's** points | `[420.0, 260.0]` |
| `camera` | `{"lat": …, "lon": …, "zoom": …}` | `{"lat": -6.108, "lon": 106.91, "zoom": 11}` |
| `content` | `{"kind": "map"}`, `{"kind": "newsroom"}`, `{"kind": "tree", "organisation": null}` or a structure id in place of `null` | |

## What a client can read

`{"v": 1, "read": "wall"}` answers with the wall's **projection** — the control state, and nothing else (#181).
It is the read half of the plane, and the first thing a client should ask for.

| field | meaning |
| --- | --- |
| `active` | the workspace on screen, as an index into `workspaces` |
| `focused` | the pane with the keys, or `null` when the workspace on screen holds none |
| `maximised` | the pane drawn over the wall (#85), or `null` |
| `workspaces[]` | in chip order: `name`, `focused` (the pane it would come back to), `empty`, `panes[]` |
| `panes[].id` | the pane's id — the thing every intent about a pane names |
| `panes[].kind` | `map`, `tree` or `newsroom` (#116, #136) |
| `panes[].title` | what the name plate draws: the operator's name, or the pane's number |
| `panes[].camera` | `{lat, lon, zoom}` for a map pane, **`null` for one that projects nothing** |
| `panes[].focused` | whether this pane has the keys — the active workspace's answer, not a remembered one |
| `panes[].selected` | the hull whose card the pane shows, or `null` (#99) |

**Absence is not zero here too** (`INV-01`): a newsroom pane's camera is `null` rather than `0, 0, 0`; a workspace
that holds no pane is `empty: true` and still in the list with its name, because closing a workspace's last pane
leaves it on the wall (#141); a pane with no card says `selected: null`.

**What it does not carry, on purpose** (`ADR-0005` decision 7): the fleet, the fixes, the ages, the cards, the
story text, the chain of command, the frames or the pixels. A client that could read where the hulls are would have
the picture, which is the one thing this protocol decided not to send.

```sh
arcons-cc ctl wall | jq '.workspaces[] | {name, panes: [.panes[].kind]}'
```

## Following a hull

`object.follow` makes a pane's camera track a hull: every frame the camera centres that hull's fix, so a remote can say
"look at 512345" and the wall keeps looking. Three rules, and each is a test:

- **A hull with no fix cannot be followed** — `INV-01`'s absence is not zero, and the refusal names the hull rather
  than leaving the camera where a guess would put it.
- **Any camera intent ends it** — a pan, a zoom, another `map.camera` — otherwise the operator pans and the next frame
  pulls the camera back, which is a wall fighting the hand driving it. A hull that stops reporting ends it too.
- **It is not saved.** `layout.toml` remembers where a pane is looking (#153); a follow is a live mode, and a hull that
  went quiet is not a place to come back to.

## The transport is a seam (#195)

The socket above is **one carrier**. Everything the plane *does* — read a line, hand it to the UI thread, write the
answer back — lives in one session loop that knows the transport only as a carrier: read a line, write a line, hang up.
A relay the wall dials out to (a phone, #196) and a network session (a viewer, #202) are the same three calls, so there
is one vocabulary, one refusal path and one place that could break `INV-05`.

The unix socket stays what it is: the local credential, one client at a time, no token. A carrier adds a way to reach the
same loop; it does not add a second way to change the wall.

## The scene: the whole arrangement (#201)

`{"v": 1, "read": "wall"}` is the projection — small, for a client that needs to know what the wall is doing. A
**viewer** on another machine needs more: everything a picture is computed from. That is a `Scene`, and it is
`ADR-0006`'s vocabulary:

| field | what it carries |
| --- | --- |
| `v` | the protocol version — `1`. A client that cannot speak it says so rather than guessing |
| `seq` | a counter that only goes up, so two scenes can be ordered and a gap is detectable |
| `active` | the workspace on screen, as an index into `workspace` |
| `focused` | the pane with the keys, or `null` on a workspace that holds none |
| `maximised` | the pane drawn over the wall, or `null` |
| `surface` | the one surface a hand can leave open: a menu (with its row and ring), a filter panel, a rename sheet, settings — never a *held* gesture |
| `cursors` | every cursor the wall draws, by point and age |
| `workspace` | the workspaces, in chip order: `name`, `focused` (an index into `pane`), `tiling` (the same shape `layout.toml` writes), `pane` |

And a pane: `id`, `content` (`map`, `tree` with its `organisation`, `newsroom`), `lat`/`lon`/`zoom`, `name`, `follow`,
`selected`, `filter` (the three layers and the hidden type, domain and category ids), `tree` (a tree pane's own camera)
and `news` (a newsroom pane's `scope`, `page` and open `selected` story).

**What is deliberately not in a scene**, and why: the frame a map thread last drew, the threads, the decoded pictures,
the textures, the layout results — they are the *drawing machine's*, and a viewer makes its own. And the **water** —
the fleet, the cards' text, the newsroom's stories — joins the scene in #203; until then a scene is the arrangement,
and a replay proves the arrangement is complete with the water held still.

`tests/scene_replay.rs` is the proof: it renders scenes the ordinary way and again with `ARCONS_REPLAY=1`, which makes
the wall rebuild itself from its own `Scene` before drawing, and compares the bytes.

## A menu, without a pointer

A menu is a place: which row means something is decided by the wall's own layout, so a client that has no pointer
sends **the row it means** and lets the wall act on it (#164). Three verbs, and they are the three things a menu
has — a ring, a second surface, and a choice:

```sh
arcons-cc ctl menu focus 2          # the ring on the third row
arcons-cc ctl menu submenu open     # the ringed row's waterfall, ring on its first row
arcons-cc ctl menu choose           # what Enter does: act on the row the ring is on
arcons-cc ctl surface close         # what Escape does: the waterfall, then the menu
```

On a keyboard the same model is `Up`/`Down` (wrapping at both ends), `Right`/`Left` for a waterfall, `Enter` to act,
and `Escape` to close one surface at a time. A ring is drawn in `focus_edge` — the wall's colour for "this one has
the keys" — and it is always the row `menu.choose` would act on: the ring, the drawing and the assistive
description are one value.

**A menu opened by a press has no ring**, because the pointer is where the operator's attention is; a ring nobody
put there would be a second answer to "which row". The first `Down` (or `menu.focus`) puts one there.

## The client's cursor

A remote has no hand on the glass, so the wall keeps a pointer for it (#167): `cursor.move` puts it at a point in
the **picture panel's points**, `cursor.press` acts where it is, `cursor.release` lets go. The wall draws it — a
ring at the point, filled while the button is down — and **fades it after four seconds of stillness**, because a
stray pointer on a wall at three in the morning is a bug that looks like a mouse. A button held down keeps it
visible: a remote mid-press is a remote still using it.

What a press reaches, today:

| the point is over | the press does |
| --- | --- |
| a hull's marker, within 14 points (a phone guesses a point on a map it has only read) | selects that hull, opening its card, and gives its pane the keys |
| a pane, but not a hull | gives that pane the keys and clears its card |
| a gutter, or outside every pane | nothing. A resize is not a point a remote can make yet |
| the header's chips, a menu's row | **nothing yet**: those are Rune's own layout, and a press there needs an event injection Rune does not expose. Recorded rather than faked |

Two things this is not: it is not the OS pointer (a hand and a remote can point at different places, and that is
fine), and it is not a renderer's cursor (it lives in the picture panel's space, which is where panes and markers
are drawn, not over the header).

## The CLI

`arcons-cc ctl` is the second client: it builds one intent, sends it, prints the answer, and exits with a code a
script can branch on.

```sh
arcons-cc ctl pane focus 2
arcons-cc ctl pane close 4          # refused on the wall's last pane, reason on stderr, exit 1
arcons-cc ctl workspace new
arcons-cc ctl rename set workspace 1 "Straits watch"
arcons-cc ctl map camera 2 -6.108,106.91,11
arcons-cc ctl surface close
arcons-cc ctl wall                    # the projection, as JSON
arcons-cc ctl pane close              # the pane that has the keys, read from `wall` first
```

| exit | meaning |
| --- | --- |
| `0` | the wall answered: `changed: true` (printed nothing) or `changed: false` (prints `unchanged`) |
| `1` | the wall refused, and the rule's name is on stderr |
| `2` | there is no wall at the socket: the display is not running, or it could not bind — `--self-check` names which |

The CLI's words **are** the vocabulary: `pane focus` is `pane.focus`. It invents no verb, and a test fails if one
of the two gains a name the other does not have.

## What a client cannot do yet

- **Read** — beyond the projection. `read: "wall"` describes the control state (#181); a client still cannot ask
  *what is on the wall's glass* (the hulls, the cards, the counts), and by `ADR-0005` decision 7 it should not be
  able to: that is the picture.
- **Point.** There is no cursor on the wall and no pointer vocabulary in this file: a client with no hand gets one
  in #167, and the menu's arrow keys in #164. `pane.split_at` is the one request that takes a point, and a client
  that has not been told the frame cannot honestly send one.
- **Hold.** A drag — a pane in the air, a border being resized, a camera panned — is a held gesture and is the
  viewer's; the settled result of each is the intent above (`pane.move`, `map.camera`). A continuous gesture over
  a socket is a later decision, not an omission.

## A client, worked through

A script that opens a newsroom on a wall with two panes and gives it a name:

```sh
arcons-cc ctl pane open newsroom                 # beside the focused pane, asking page 1
arcons-cc ctl rename set pane 3 "Feed"           # the id the wall minted, from the log or the next read
arcons-cc ctl pane move 3 1                      # carried to workspace 2 with its view and its page
arcons-cc ctl workspace focus 0                  # back to the wall
```

Every one of those is the same code path a press takes: `control::apply`. That is the point of the plane — the
hand and the socket are two callers of one door, and a refusal is the wall's own rule rather than a second copy
of it.

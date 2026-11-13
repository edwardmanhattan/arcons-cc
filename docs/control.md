# The control plane — `docs/control.md`

**A client sends intents; the wall renders.** This is the reference for the socket that lets something other than
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
| `pane.open` | `content` | a pane of that kind beside the focused one — the header's tree and newsroom controls (#116, #136), asking their first reads |
| `pane.select` | `pane`, `unit` | puts that pane's card on a hull; `unit: null` clears it (#99). `no such pane` |
| `workspace.focus` | `index` | makes that workspace the wall on screen (#138). `no such workspace` |
| `workspace.new` | — | appends an empty workspace and switches to it (#138) |
| `map.camera` | `pane`, `camera` | puts that pane's camera at `{lat, lon, zoom}` — a **value**, so a pan is one request when the hand stops, not sixty. `no such pane` |
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
| `at` | `[x, y]` in the pane's **frame** pixels, not the window's | `[420.0, 260.0]` |
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

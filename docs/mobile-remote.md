# The mobile remote

**A phone that controls the wall.** It is a **controller**: it sends intents and shows a small projection of what the
wall is doing. It **does not receive the picture** — that is `ADR-0005`'s first decision, and a phone that could draw
the water would be a renderer, with a codec, a frame clock and a bandwidth budget, and `INV-04`/`INV-12` would become
claims about a network.

This note exists because the control plane was designed with the phone in view rather than retrofitted for it. What it
records is the **state of the ground**: what the epic (#162) actually delivered, the three decisions the phone needs,
and what is still open. The mobile client's own epic is
[#193](https://github.com/edwardmanhattan/arcons-cc/issues/193).

The note is [issue #168](https://github.com/edwardmanhattan/arcons-cc/issues/168).

## What the wall already answers

The epic was planned against a table of "what the phone does / what it needs". Every row but the last is now true, and
the last one is deliberate:

| the phone does | what it uses | today |
| --- | --- | --- |
| shows the wall's shape — workspaces, panes, which is on screen, which has the keys | `{"v": 1, "read": "wall"}` (#181) | **yes** |
| switches workspace, picks a pane, closes one, closes all but one | `workspace.focus`, `pane.focus`, `pane.close`, `pane.close_others`, `workspace.close_others` | yes |
| zooms, moves a camera, locks onto a hull | `map.camera`, `object.follow` (#169) | yes |
| opens the chain of command, the newsroom, settings; makes a pane | `pane.open`, `surface.settings`, `pane.split` | yes |
| walks a menu and picks a row | `menu.focus`, `menu.submenu`, `menu.choose` (#164) | yes |
| presses a thing it can name but not see | `cursor.move`, `cursor.press`, `cursor.release` (#167) | yes |
| learns *what* a pane is looking at | the projection: a pane's `title`, `kind`, `camera`, `selected` | yes |
| reaches the wall from another machine | **a network transport** | **no, on purpose** |

The wire is one JSON object per line, in and out: a request is a verb with its fields, and the answer is either
`{"ok": true, "changed": true}` or `{"ok": false, "why": "…"}` or `{"read": "wall", "wall": {…}}` — one or the other,
never both (`docs/control.md` is the reference).

## The three decisions

### 1. The projection is the phone's only truth

The phone reads the wall when it connects and **again every time it comes back** — from sleep, from a dropped socket,
from a network change. It never replays intents it sent before, and it keeps no state it did not just read.

Why: an intent is a *change*, and a wall that changed while the phone was away would be changed wrongly by a replay.
"I closed pane 4" is a fact about a wall that no longer exists; "read what is on the wall, then act" is the only rule
that survives a discontinuity. This is also why the read is a **projection** and not an event log: a client that needs
a cursor to catch up is a client that can fall behind, and a phone has nothing to catch up to.

What the phone shows when it cannot read: **"not connected"**. Never a labelled picture of a wall it last saw — a
stale projection with a confident label is the failure mode this rule exists to forbid.

### 2. The wall dials out; a phone never dials the wall

`ADR-0005` rejected a TCP listener on the wall, and this note does not reopen it. The wall keeps its unix socket (0600
inside a 0700 directory) as the local credential, and gains **one outbound carrier**: it dials the deployment's relay
over the same kind of authenticated connection it already holds to Minos, and the phone talks to the relay.

Why:

- **A wall that listens is a port.** On a deployment's network that is a firewall change, a certificate and a token to
  invent — the three things this wall does not have and `ADR-0005` refused to invent for the local case.
- **The wall already reaches out.** It holds an authenticated socket to Minos today; a control channel is one more
  message type on a connection that already reconnects. A machine in a cupboard needs no inbound path to be reachable.
- **The credential stays a file.** The wall authenticates *to the relay* with what it already has; the phone
  authenticates to the relay with a device key. No wall-side secret is invented, and `INV-08` is unchanged.

What this costs: the relay is somebody else's process, so its protocol is a contract this repo must pin and test
(`docs/control.md` names it, and a fake relay is what the tests drive). The wall does not listen; if the relay is
unreachable, `--self-check` says so and the wall works locally as it always did.

### 3. Pairing: a code on the wall, a key on the phone, a hash in a file

A phone is paired the way a device is paired by people standing in the same room:

1. Someone at the wall opens pairing — an intent (`pair.begin`), so it can be done from the wall's own settings surface
   or from `arcons-cc ctl` at the terminal.
2. **The wall draws a code**: six digits, on the wall, for two minutes. It is a picture on a screen in the room, not a
   message, so it is not somewhere an attacker on the network can read it.
3. The phone sends the code and a name for itself (`pair.claim`). The wall writes a row to
   `$XDG_CONFIG_HOME/arcons-cc/devices.toml` — **a hash of the key, the name, when it was paired** — mode 0600 like
   every other file there, and hands the key back through the carrier.
4. Revocation is deleting the row (`pair.forget`), which is the same surface that lists the rows.

`INV-08` holds by construction: the code lives for two minutes and is never written to a log line, the key is never in
the tree and never in this repository's state, and the file holds a hash — a leaked `devices.toml` is a list of names,
not a set of keys. A wrong code and an expired code are refused **by name**, because a client that cannot tell them
apart retries the wrong one forever.

## Decisions this note takes, so they are not discovered later

- **A cursor belongs to a client.** This note first said *one cursor*, because the wall drew one pointer (#167) and
  two arrows that neither client could explain seemed worse than one they shared. A fleet made that wrong
  ([ADR-0006](adr/0006-a-fleet-of-walls.md) decision 5): a pointer is a hand, every viewer draws every cursor it is
  told about, and the wall's own pointer is the `wall` origin. A phone's press is therefore visible where it lands,
  which is the property a remote actually needs.
- **The projection does not grow a field for the phone.** In particular it does not carry `follow`: a follow is ended
  by any camera intent (#169), so a phone that wants to stop one sends `map.camera`, and the camera it reads is where
  the pane is looking, which is what a controller needs. A `follow` field would be a fact a client acts on, and acting
  on facts about the water is the renderer's job.
- **A phone cannot choose a hull.** The projection excludes the fleet (`ADR-0005` decision 7) — a client that could
  list hulls would have the picture. So the phone can zoom, pan, follow an id the operator read off the wall's card,
  and press a point it has been told; it cannot browse. That is the honest cost of "a controller, not a renderer".

## Where the work is

[Epic #193](https://github.com/edwardmanhattan/arcons-cc/issues/193), five tickets, in this order:

| ticket | what it makes real |
| --- | --- |
| [#195](https://github.com/edwardmanhattan/arcons-cc/issues/195) | a **carrier** seam, so a relay is a transport and not a second control plane |
| [#194](https://github.com/edwardmanhattan/arcons-cc/issues/194) | **pairing**: the code on the wall, the hashed device key, `devices.toml` |
| [#196](https://github.com/edwardmanhattan/arcons-cc/issues/196) | the **relay**: a pinned contract, an outbound carrier, a fake relay in the tests |
| [#197](https://github.com/edwardmanhattan/arcons-cc/issues/197) | a phone's rules **as tests**: read, act, drop, resume, read again |
| [#198](https://github.com/edwardmanhattan/arcons-cc/issues/198) | a refusal says its **kind**, so a client need not read prose |

## Still open

- **The phone's framework, design and identity provider** — not this repository's business, and not decided here.
- **Whether the relay is Minos or a separate deployment service.** The wall's side of the contract is the same either
  way, which is why it can be built first.
- **Whether two remotes may be paired at once.** The cursor question is answered by ADR-0006 (per-`origin`); the one
  that remains is who wins a refusal when two clients ask for contradictory things at once — the authority applies them
  in order, and the second is refused or wins, which a fleet makes a rule rather than a coincidence.
- **Anything about the picture.** If that changes it is a different epic, and it starts with a codec.

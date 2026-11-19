# A development relay — `tools/dev-relay/`

**`docs/control.md`'s relay frames, and nothing else.** The wall dials out and a phone dials in; this process carries
their lines between them, so that a phone can be paired and driven on a laptop with no deployment, and so that the two
*real* ends can finally talk to each other over a socket instead of each meeting a fake.

**It is not the contract.** No authentication, no persistence, no TLS, no rate limiting. `docs/control.md` says the
relay's own process is somebody else's; what production needs is a real relay somewhere both the wall and the phone can
reach, and that is a deployment decision this tool does not take.

```sh
dart run tools/dev-relay/bin/main.dart --port 7000
```

It prints the two commands that use it. The shape of a session:

```sh
# 1. the relay
dart run tools/dev-relay/bin/main.dart --host 0.0.0.0 --port 7000 --verbose

# 2. the wall, on this machine
ARCONS_RELAY_URL=tcp://127.0.0.1:7000 cargo run

# 3. the phone (a device on the same network), from clients/phone/app
flutter run --dart-define=ARCONS_RELAY=tcp://<this machine's LAN address>:7000
```

`docs/runbook.md` §10 is the checklist a person with a phone runs against exactly those three commands.

## What it routes, and the one case that is not a lookup

| frame | where it goes |
| --- | --- |
| the wall's `hello` | answered `{"v": 1, "welcome": true}`; that connection is the wall from then on |
| a phone's frame | forwarded to the wall **byte for byte**, and the phone is remembered as the asker |
| the wall's answer with `"to": "<name>"` | to the connection that name was learnt on — the relay learns `name → connection` from the wall's own answers, so it never hashes a key or reads `devices.toml` |
| the wall's answer **with no `to`** | back to the connection that asked. This is `pair.claim`: the phone has no key, so its frame carries no `device`, and the wall has no name, so its answer carries no `to` (#262) |
| anything it cannot route | **dropped and logged**, never broadcast and never guessed |

## The log redacts

`--verbose` prints frames with the **device key** and the **pairing key** replaced by `…`, by field name, and nothing
else — a dev tool that prints a key is how a key ends up in a paste (`INV-08`). A test asserts it over a real frame.

## Tests

```sh
cd tools/dev-relay && dart analyze --fatal-infos && dart test
```

Real loopback sockets (`127.0.0.1:0`), no fakes: the welcome; a frame arriving at the wall byte for byte; an answer with
`to` reaching the asker and a later publish reaching the same device; the `pair.claim` answer going back on the
connection that asked; an unroutable frame dropped and reported; the log carrying no key. CI runs them in the `phone`
job, because they are Dart and need no display.

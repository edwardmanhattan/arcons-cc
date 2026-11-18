# The phone — `clients/phone/`

**The phone: a Dart package and the screen that draws it.** It is the phone that shows the wall's arrangement and rearranges it
(epic [#217](https://github.com/edwardmanhattan/arcons-cc/issues/217)), and the framework decision is
[ADR-0008](../docs/adr/0008-the-phones-framework.md): **Flutter**, one codebase for both phones.

This package is what comes before a screen. It exists so that the protocol, the tiling arithmetic and the client's rules
are proved by a command, in milliseconds, with no device and no UI toolkit in the toolchain:

```sh
cd clients/phone                      # the wire, the tiling, the client's rules: no UI toolkit needed
dart pub get && dart analyze --fatal-infos && dart test

cd clients/phone/app                  # the screen: Flutter, one golden per state
flutter pub get && flutter analyze && flutter test

cd clients/phone/app                  # the app, against a deployment's relay
flutter run --dart-define=ARCONS_RELAY=tcp://HOST:PORT
```

**The relay is deployment configuration and never a secret.** It is read from `ARCONS_RELAY` at build time
(`tcp://` or `tls://`), never written in the tree; a build without it starts in `not connected · no relay is
configured`, which is a state and not a crash. The **device key** is the platform's business (#265): until the
Keychain/Keystore lands, `MemoryKeyStore` is a phone that pairs again after a restart, and never a key in a file this
repository can read.

`app/` has its own `analysis_options.yaml` on purpose: the analyzer looks **upward** for the nearest options file, and
the package's excludes `app/**`, so without one `flutter analyze` in the app directory would analyse nothing.

CI runs both as the `phone` job. It is **not** the wall's gate and does not run it: the two are held together by
`fixtures/control/`, which this package reads **by path** — the same recording `scripts/verify.sh` replays, read as the
client's truth rather than the wall's.

## What is here

| file | what it is |
| --- | --- |
| `lib/wire.dart` | the answers a client acts on: `changed`, a projection (with or without the watch it was published under), a watch's grant, and a refusal with one of the four kinds. `src/control/wire.rs` is the encoder and `docs/control.md` the reference; neither is duplicated here |
| `lib/cells.dart` | the tiling laid into a rectangle the caller gives it: `Layout::cells`, `view::panes_on`, `Layout::drop_for`, `Layout::apply` (the tiling a drop previews), `Layout::borders` (the gutters) and the clamp a resize is previewed through — as ratios and never as the wall's pixels (#249, #257) |
| `lib/client.dart` | **a client's rules**, over a `Carrier` of three methods: it reads before it acts, a connection that comes back knows nothing, a wall changed elsewhere is read rather than corrected, and a version it cannot speak is a stop (#250). The wall's own client is `src/control/client.rs` (#197) |
| `lib/relay.dart` | **the carrier** (#256): a socket to the relay — one JSON object per line, `tcp://` or `tls://`, the device key in `device` — with a dial that backs off, a watch asked only after a read has landed ([`RelayCarrier.watch`]) and publishes delivered as projections that know their `watch`. The key arrives through a `KeyStore` this package does not implement: the Keychain and the Keystore are #260's |
| `test/corpus_test.dart` | the recording, line by line: every answer parses, every tiling leaf is a pane id the workspace holds, every ratio is inside the wall's clamp, every refusal kind has an action, and the watch's stream is **not** in it (a stream is a race, not a fixture) |
| `test/cells_test.dart` | the nine cases in `fixtures/control/tiling/`: the recorded cells, the recorded rectangles and every recorded release, compared with `==` because the arithmetic is the wall's `f32` — plus the shapes a preview draws and the gutters a hand grabs |
| `test/client_test.dart` | the four rules as four named tests over a fake carrier, and the fifth: **a refusal is mapped from its `kind`, never from its sentence** (#198, #250). No socket, no network, no clock it does not control |
| `test/relay_test.dart` | the carrier over a **fake relay on loopback**: the frames, a watch granted with two publishes, a drop that reads first and asks again (and replays nothing), a refusal's kind, a version that stops the carrier, a revoked phone's silence, and the key that never reaches a printed string (#256) |
| `app/lib/overview.dart` | **the screen** (#255): the chips, the canvas and the state line, drawn from the projection. One `layOut` walk gives the rects *and* the words, so a pane the canvas draws is a pane a reader hears. The drag, the gutter and **Move to…** are drawn here (#257); the update screen carries the wall's words, the app's one sentence, one button, and a refusal's `detail` when a wall sends one (#264) |
| `app/lib/controller.dart` | **the four states** (#257, #264): the projection the last read gave, the lift, the landing and the gutter — decided by `cells.dart` — and the one place this app puts a change on the wire. `offline` is a phone with nowhere to dial; `published` takes a watch's projection one `seq` after the one it holds and reads on a gap |
| `app/lib/intents.dart` | the two verbs the canvas sends — `pane.drop` and `pane.resize` — and the **one** place their fields are written, so the hand and the list cannot produce different request lines |
| `app/lib/shell.dart` | **the app's join** (#264): `ARCONS_RELAY` parsed, the `KeyStore` read on start, a `RelayCarrier` over `PhoneRelay`, the read and then the watch, and the publish stream the watch grants. No corpus: a phone with no relay is `not connected`, not a replay |
| `app/lib/pairing.dart` | **the pairing screen** (#264): six digit boxes over one field, `Name this phone`, one `Pair`, and the three refusals' steps as `docs/mobile-ui.md` gives them. A burnt code is `no pairing is open` and takes that row's step |
| `app/lib/main.dart` | the app: `PhoneApp`, the relay from `String.fromEnvironment('ARCONS_RELAY')`, the key store, and `runApp` |
| `app/test/overview_test.dart` | the corpus's own projection, held to `cells.dart`'s rects, read out loud in the drawn order, and rendered as two goldens — `live` and `not connected` |
| `app/test/drag_test.dart` | the drag against a fake carrier: one release is one intent, nothing is optimistic, the refusal table's four kinds, **the hand and the list produce the same request line**, the gutter's clamp, and a golden of a card mid-drag |
| `app/test/states_test.dart` | **the four states as the app's** (#264): `connecting` with no diagram from a previous life, `live` silent, `not connected` dimmed under its age and sending nothing, `update required` one screen with the wall's words and one button — plus the watch's `seq` rule. Three goldens: `connecting`, `not connected`, `update required` |
| `app/test/pairing_test.dart` | **the pairing screen** (#264): six boxes, the defaulted name, one Pair; the claim's code and name, the key's one write, and the overview after it; each of the three refusals and its step; the key never in a widget's text. Goldens: empty and with a burnt code |
| `app/test/shell_test.dart` | **the shell** (#264): no relay is a state, the app starts in `connecting`, asks for the watch only after the read, sends the screen's intents, dims on a dropped read, shows the update screen, draws a publish — and `lib/` reads no fixture |

## What is not here yet, and why

- **The key's platform home** (#265) — the carrier asks a `KeyStore` and writes the claimed key to it, but the Keychain
  and the Keystore are #265's: this app's store is memory, so a restarted app pairs again. ADR-0008 decision 4 rejects a
  file, and a store that survived in one would be `INV-08` broken at rest.
- **`Forget this wall`.** `docs/mobile-ui.md` puts the row on the overview's settings; the overview has no settings
  surface yet, and `KeyStore` has no `forget` until #265 gives it the platform's three methods. The row is not drawn,
  because a phone cannot forget a key it has no way to delete.
- **A redial after a drop.** The shell dials, reads and watches once; a read that does not land draws `not connected`
  and stays there. The carrier can reconnect ([`RelayCarrier.connect`]) and the app does not ask it to yet — the retry
  loop is the next ticket's, and inventing one here would be a retry the note has not decided.
- **The gutter's accessible path** (#257 leaves it open, deliberately): a share is a number and no recorded vocabulary
  names one, so the gutter's *drag* is in and its words are not (`docs/mobile-ui.md`, *accessibility*).
- **The update screen's store page.** The one button is drawn and its action is the shell's: this package has no store.
- **A bundled typeface.** The goldens draw text in the test harness's box font, so the pictures check *layout and
  colour*; the words are the semantics test's. A typeface is an asset decision and arrives with the app's assets.
- **TLS end to end.** `tls://` is parsed and would dial `SecureSocket.connect`; no certificate is in the tree, so no test
  proves the handshake — and a fixture written by a test is not a certificate.

## Rules this package keeps

- **The corpus is the spec.** A field this package names and the wall does not send is a red test; a shape the recording
  grows is a red test until it is typed.
- **The wall's numbers are cited, then checked.** `MIN_PANE_PT`, `PANE_EDGE_BAND`, `GAP` and `RATIO_RANGE` are the
  wall's (`src/ui/tokens.rs`, `src/config/layout.rs`); `Numbers.wall` names where each came from and a test holds it to
  the numbers every recorded tiling case carries, so a token that moved is red here and not found on somebody's phone.
- **Rectangles are `f32`.** The wall's are, and the recording writes them that way (`742.39996`, not `742.4`), so
  `CellRect` rounds once and the tests compare with `==` instead of a tolerance.
- **No default arms.** `INV-07` one language over: an unknown refusal kind or watch word is an error, not a category.
- **Nothing secret is printed.** `INV-08`'s cousin: a device key that can be interpolated into a string will be — so
  `lib/relay.dart` takes it from a `KeyStore`, writes it into `device` and into nothing else, and a test formats every
  error path and every `toString` to assert the key's characters appear in none of them.

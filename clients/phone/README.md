# The phone — `clients/phone/`

**A Dart package, and no screen yet.** It is the phone that shows the wall's arrangement and rearranges it
(epic [#217](https://github.com/edwardmanhattan/arcons-cc/issues/217)), and the framework decision is
[ADR-0008](../docs/adr/0008-the-phones-framework.md): **Flutter**, one codebase for both phones.

This package is what comes before a screen. It exists so that the protocol, the tiling arithmetic and the client's rules
are proved by a command, in milliseconds, with no device and no UI toolkit in the toolchain:

```sh
cd clients/phone
dart pub get
dart analyze --fatal-infos
dart test
```

CI runs exactly that as the `phone` job. It is **not** the wall's gate and does not run it: the two are held together by
`fixtures/control/`, which this package reads **by path** — the same recording `scripts/verify.sh` replays, read as the
client's truth rather than the wall's.

## What is here

| file | what it is |
| --- | --- |
| `lib/wire.dart` | the answers a client acts on: `changed`, a projection, a refusal with one of the four kinds. `src/control/wire.rs` is the encoder and `docs/control.md` the reference; neither is duplicated here |
| `lib/cells.dart` | the tiling laid into a rectangle the caller gives it: `Layout::cells`, `view::panes_on`, `Layout::drop_for` and the clamp a resize is previewed through, as ratios and never as the wall's pixels (#249) |
| `lib/client.dart` | **a client's rules**, over a `Carrier` of three methods: it reads before it acts, a connection that comes back knows nothing, a wall changed elsewhere is read rather than corrected, and a version it cannot speak is a stop (#250). The wall's own client is `src/control/client.rs` (#197) |
| `test/corpus_test.dart` | the recording, line by line: every answer parses, every tiling leaf is a pane id the workspace holds, every ratio is inside the wall's clamp, every refusal kind has an action |
| `test/cells_test.dart` | the nine cases in `fixtures/control/tiling/`: the recorded cells, the recorded rectangles and every recorded release, compared with `==` because the arithmetic is the wall's `f32` |
| `test/client_test.dart` | the four rules as four named tests over a fake carrier, and the fifth: **a refusal is mapped from its `kind`, never from its sentence** (#198, #250). No socket, no network, no clock it does not control |

## What is not here yet, and why

- **Flutter.** A screen needs a UI toolkit; proving the protocol does not. Flutter enters CI with the first golden
  image, and not before.
- **A watch** (#221). `lib/client.dart` asks for none and resumes none: `lib/wire.dart` refuses a grant by name until
  the relay's fixtures type one, so "never resumes a watch" is proved as "never asks for one" here.

## Rules this package keeps

- **The corpus is the spec.** A field this package names and the wall does not send is a red test; a shape the recording
  grows is a red test until it is typed.
- **The wall's numbers are cited, then checked.** `MIN_PANE_PT`, `PANE_EDGE_BAND`, `GAP` and `RATIO_RANGE` are the
  wall's (`src/ui/tokens.rs`, `src/config/layout.rs`); `Numbers.wall` names where each came from and a test holds it to
  the numbers every recorded tiling case carries, so a token that moved is red here and not found on somebody's phone.
- **Rectangles are `f32`.** The wall's are, and the recording writes them that way (`742.39996`, not `742.4`), so
  `CellRect` rounds once and the tests compare with `==` instead of a tolerance.
- **No default arms.** `INV-07` one language over: an unknown refusal kind is an error, not a category.
- **Nothing secret is printed.** `INV-08`'s cousin: a device key that can be interpolated into a string will be.

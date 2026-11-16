# ADR-0008 — the phone's framework, and why the UI is not in Rust

*Accepted 2026-11-16. [Issue #223](https://github.com/edwardmanhattan/arcons-cc/issues/223), epic
[#217](https://github.com/edwardmanhattan/arcons-cc/issues/217). ADR-0005 decision 1 and ADR-0006 stand unchanged.*

## Context

The client exists in prose before it exists in code: [`mobile-remote.md`](../mobile-remote.md) is the carrier, the
client's four rules and the pairing, and [`mobile-ui.md`](../mobile-ui.md) is the screen — five of them, one of them a
diagram that is also a control.

What it is, in one sentence: **a controller that reads a projection, sends intents, and draws the arrangement at its
own size.** It never receives the picture (`ADR-0005` decision 1), so it has no codec, no frame clock and no bandwidth
budget — the three costs that would have decided this ADR on their own.

Why the framework is this repository's business at all, when #193 said it was not: the choice silently decides four
things that are this repository's.

| what it decides | where the answer is already written |
| --- | --- |
| whether a client can drift from the vocabulary | `docs/control.md` is the reference; `Intents` is the code |
| whether the device key has somewhere safe to live | `pair.claim` hands it out once (#194) |
| whether a drag is accessible | `mobile-ui.md`, *Accessibility*: the words are the path, the drag is a shortcut |
| whether the corpus is the only thing shared with the wall | `fixtures/control/` (#220, #224) |

**The evidence that shapes the answer.** Discord is rebuilding its client in Rust and its phone's UI is not in Rust:
"Mobile keeps [the] native UI, but shares app code with desktop via Rust" — Vishnevskiy
([The Verge](https://www.theverge.com/news/1027379/discord-ditching-electron-web-app),
[Heise](https://www.heise.de/en/news/Discord-is-rebuilding-in-Rust-leaving-React-and-Electron-behind-11516113.html)).
Their shared surface is messaging, sync, voice and media; ours is one protocol version, a vocabulary and a projection.
That asymmetry **is** decision 1, and it is why decision 3 can afford to be "the wire" rather than "a crate".

**The state of the two candidates, as of this writing.** Compose Multiplatform's iOS target has been stable since 1.8.0
(May 2025) and is at 1.12.x, with iOS 14+ and Android API 21+ as the floor —
[JetBrains](https://blog.jetbrains.com/kotlin/2025/05/compose-multiplatform-1-8-0-released-compose-multiplatform-for-ios-is-stable-and-production-ready/),
[compatibility](https://kotlinlang.org/docs/multiplatform/compose-compatibility-and-versioning.html). Flutter 3.50
(November 2026) has Impeller default on iOS and Android API 29+, **direct Dart-to-native interop stable**, and the
design systems split out as standalone packages —
[Flutter](https://flutter.dev/blog/whats-new-in-flutter-3-50). Neither candidate is a risk on capability. The decision
is therefore not "which can do it", and this ADR does not pretend otherwise.

## Decision 1 — the phone's UI is not in Rust, and the wall's renderer does not draw it

A Rust UI on a phone means porting a desktop framework and then owning the five things a remote *is*: accessibility,
text input, gestures, secure storage, and background/resume. This repository would be inventing a platform to avoid
choosing one, and the platform would be the part users touch.

Rejected by name:

- **Rune.** A desktop wgpu framework, pinned by rev for the wall's own glass. Its mobile story is a port of a desktop
  runtime — and this repository's pin is not a phone's pin.
- **egui, Slint, Makepad.** Each is real, each is a renderer with a mobile *port*, and each would put the five platform
  integrations above on us.

What Discord's example rules out is Rust drawing the UI. It does **not** say the UI uses the platform's own widgets, and
this ADR does not claim that: see decision 2, which takes a render-own framework deliberately.

## Decision 2 — Flutter (Dart), one codebase for both phones

**Compose Multiplatform is the runner-up, not a mistake.** It would work here, and if Kotlin were already a language
this team keeps, it would win. The tie broke on three things, and each is checkable.

**What the phone needs**, from `mobile-ui.md`, so the comparison is not generic:

| need | what it is |
| --- | --- |
| the canvas | the tiling laid into the phone's own rectangle: ratios, `RATIO_RANGE`, edge bands, a gap (#220) |
| the drag | a lifted card released over another card's band → `pane.drop`; a gutter dragged → `pane.resize` |
| the words | `Move to…`, five rows per other pane, and one spoken description produced by the same walk that draws it |
| the socket | one line of JSON at a time: read → act → read, a watch frame at 4 Hz, `kind: "version"` is a stop |
| the key | one device key, in the platform's store, and nowhere else |
| the states | live / connecting / not connected / update required, and never a stale diagram labelled current |

Nothing in that list is hard in either framework. So the decision is **where the proof runs** and **what the second year
costs**.

### Why Flutter

1. **One engine means one behaviour on both phones.** The product here *is* the gesture: a card that lands where it was
   dropped, a gutter that stops where the wall says it may. Two rendering backends is two sets of gesture edge cases,
   and the iOS one is the younger code in that stack.
2. **The proof can run on a Linux runner.** `flutter test` plus `matchesGoldenFile` renders the canvas from a fixture
   projection, asserts the cells, the drop bands and the spoken description, and writes a PNG — in seconds, with no
   emulator and no Mac. That is the same idea as the wall's own gate (49 scenes, byte-identical, no display), and it is
   the one thing the two candidates do not share: Compose Multiplatform can screenshot-test its desktop target on Linux,
   but **its iOS rendering can only be rendered on a Mac**, so the phone's one screen would be a surface CI cannot see.
3. **One version matrix.** Flutter's SDK pins the engine and the toolchain together. The Kotlin side is a four-way
   alignment — Kotlin × AGP × Compose Multiplatform × Xcode — re-made on every upgrade. For one human and a set of
   agents, fewer alignment surfaces is not a matter of taste.
4. **The key can be written by hand.** Flutter 3.50's direct interop is stable, so decision 4 does not need a plugin.

### What it costs, stated rather than hidden

- **The iOS app is a Flutter app.** Its settings screen will look like this app's settings screen and not like Settings.
  For an operator tapping a diagram in a cupboard that is free; if this ever ships to a customer whose accessibility
  audit demands platform-native widgets, it is a liability, and that is the honest flip condition below.
- **Dart is a third language** with no leverage anywhere else in this repository. Kotlin would have been a third
  language too, so this is a cost against doing nothing, not against Compose Multiplatform.
- **Roughly 20 MB and a slower cold start** than a native shell.
- **`mobile-ui.md` said the phone cuts a title "with its own platform's ellipsis".** It is now Flutter's ellipsis, and
  that note is amended rather than left to age.

### Why not Compose Multiplatform, in its strongest form

The strongest case for it is that its UI code is Kotlin, the platform's own language on Android, so a screen that must
be genuinely At Home on Android has no second language between it and the platform — and the escape hatch to SwiftUI
shares Kotlin logic through Kotlin/Native. That is a good case. It loses here on (2): the one screen that carries this
app's risk is the canvas, and its iOS pixels would be unverifiable in this repository's gate. It also asks a one-human
team to hold the alignment matrix in (3) forever.

### Rejected: two native UIs (Compose for Android, SwiftUI for iOS)

Twice the surface for a controller with five screens, two test strategies, and — the part that decides it — the iOS half
is invisible to a Linux runner, so the two halves would drift with nothing to catch them. A screen is not a second
vocabulary, and two UIs are two places a vocabulary can go wrong.

### Rejected: a progressive web app

A remote's socket must outlive a tab's visibility, and the device key would live in a browser store. Flutter is not that
failure: it is an app that owns a process and a Keystore/Keychain entry.

## Decision 3 — the contract is the wire, and the corpus is how a Dart client obeys it

`PROTOCOL = 1`. A request is one JSON object per line; an answer is `{"ok": …}`, a refusal with a `kind`, or a
projection — one of those, never two. The phone's first read is `{"v": 1, "read": "wall"}`.

What holds a client written in another language to that, without a crate:

- **`fixtures/control/session.jsonl`** — the recorded exchanges (#224, #232), replayed by `scripts/conformance.sh`. A
  Dart runner replays the same file, so the phone's CI can fail the same way the wall's does.
- **`fixtures/control/tiling/`** — nine cases of `Layout::cells`' arithmetic pinned as data (#220). The phone
  re-implements that arithmetic in Dart; the fixtures are what stops it from being a second, quietly different layout.
- **The four client rules** in `src/control/client.rs` (#197), restated as the phone's own named tests: it reads before
  it acts; a connection that comes back knows nothing; a wall changed by another client is read, never corrected; a
  version the phone does not speak is a stop and an update, not a retry.

### UniFFI is the second choice, and this is the test that makes it the first

A Rust core shipped through UniFFI is how the wall's *logic* would reach a phone, and it is not taken: the phone parses
a projection rather than holding a picture, and a corpus holds a client to the protocol in the client's own CI — which
an FFI crate cannot do for a client written in another language, because it would replace the test with itself.

It becomes the first choice when either of these is true, and neither is true today:

1. **A client must hold a picture** — the fleet, the water, the cards, a frame clock.
2. **Two platforms must share state rather than parse it** — a client that reconciles two histories rather than reading
   one.

## Decision 4 — the key lives in Keystore or Keychain, written by hand

`pair.claim` hands a device a key exactly once (#194). One place on a phone can keep it: the platform's store. Not a
file, not shared preferences, not a `Map` in memory that a crash dump can read — a secret that can be printed is a
secret that will be.

**By hand, not through a plugin.** Sixty to a hundred lines against two well-documented APIs; it is the one part of the
phone an auditor will read; and every transitive crate between the key and the store is a link nobody reviewed. Flutter
3.50's stable native interop is what makes this a reasonable ask of Dart.

## Decision 5 — versioning is the client's

`kind: "version"` means *update the app*: never retry, never negotiate (#197, #198). This ADR records it again because
the app store now belongs to the protocol's lifetime — the wall ships, the phone ships later, and a wall that speaks a
newer protocol must refuse an old app **by name**, which is what `kind: "version"` is for.

## The escape hatch, and how narrow it is

A screen Flutter cannot make accessible **at all** is SwiftUI, and that is a defect in the choice above that we pay for
— not a design preference. It is explicitly *not* "a screen that would feel better native":
**a second UI is a second vocabulary**, which is why the first time one screen moves to SwiftUI for polish, this ADR
is repealed by habit.

## Where the app lives — recorded as the next decision, not taken here

Two shapes, and this ADR does not pick one:

- **`clients/phone/` in this repository.** The corpus is consumed by path, and a fourth CI job (`flutter test` on
  ubuntu) runs the phone's suite beside the wall's.
- **Its own repository.** The corpus is vendored or fetched at a pinned tag, which is a second copy of the truth.

The recommendation is the first, for the reason the corpus was built here at all: `fixtures/control/` is the phone's
specification *and* the wall's recording, and a copy of it in another repository is a specification that can drift.

## Consequences

- **The wall does not change.** Nothing on its side learns what the phone is written in; the socket, the verbs, the
  projection and the watch are the whole interface.
- **The gate stays Rust.** `scripts/verify.sh` gains nothing and loses nothing; the phone's command is `flutter test`,
  and it is the phone's gate, not the wall's. The two are held together by the corpus, not by a shared test runner.
- **A toolchain becomes part of the project**: the Flutter SDK pinned for the app, Xcode for iOS builds, the Android SDK
  for the store. That is a toll on every machine and every CI run, and it is the price of one UI for two phones.
- **`mobile-ui.md` gains one honest sentence**: its controls are drawn, so each one carries its own semantics node, and
  the spoken path in that note is not a fallback.

## Proof

```sh
cargo test --test doc_claims   # this ADR's protocol number is src/control.rs's, its corpus is on disk, and it names
                               # every alternative it rejects — including the one it took
```

## Falsified by

- a phone that ships a screen a screen reader cannot reach;
- a second UI, or a second vocabulary, for one platform;
- a device key in a file;
- a Flutter screen that re-implements `Layout::cells` because the corpus was not enough;
- a `PROTOCOL` bump that reaches a phone as a retry rather than as an update;
- an ADR that recommends a framework without naming the options it rejected.

/// **The wall, as a file, and a fake carrier** — what every test of this screen is held to (#257).
///
/// `docs/mobile-ui.md` is the screen's specification, and its first rule is that the phone's only truth is the
/// projection it read. So the projections here are **the recording's** — `fixtures/control/session.jsonl`, read by path
/// and never copied (#224, #248) — and the carrier is twenty lines with no socket, no thread and no clock a test does
/// not control. A screen proved against a projection made up in a test is proved against itself.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:arcons_phone/cells.dart';
import 'package:arcons_phone/client.dart';
import 'package:arcons_phone/wire.dart';
import 'package:flutter/gestures.dart';
import 'package:material_ui/material_ui.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:arcons_phone_app/controller.dart';
import 'package:arcons_phone_app/overview.dart';

/// The recording, by path from this package: `clients/phone/app/` → the repository's root.
const String corpusPath = '../../../fixtures/control/session.jsonl';

/// One recorded exchange: the request a client sent, and the wall's answer **verbatim**.
final class Exchange {
  const Exchange(this.send, this.expect);

  final Map<String, Object?> send;
  final Map<String, Object?> expect;

  /// The answer as it goes on the wire: one JSON object, no whitespace.
  String get line => jsonEncode(expect);
}

/// The recording, line by line. A missing file is an error and never a skipped test: the fixture is the wall's.
List<Exchange> readCorpus() {
  final file = File(corpusPath);
  if (!file.existsSync()) {
    throw StateError(
      'no corpus at $corpusPath: the screen is held to the wall\'s recording (`cargo run -- --record`), and a screen '
      'proved against a projection made up in a test is proved against itself',
    );
  }
  final out = <Exchange>[];
  for (final line in const LineSplitter().convert(file.readAsStringSync())) {
    if (line.trim().isEmpty) {
      continue;
    }
    final exchange = jsonDecode(line);
    if (exchange is! Map<String, Object?>) {
      continue;
    }
    final send = exchange['send'];
    final expect = exchange['expect'];
    if (send is! Map<String, Object?> || expect is! Map<String, Object?>) {
      continue;
    }
    out.add(Exchange(send, expect));
  }
  return out;
}

/// Every projection the recording answers a read with, in the wall's order.
List<Wall> projections(List<Exchange> corpus) => [
      for (final exchange in corpus)
        if (isRead(exchange.send)) _projection(exchange.line),
    ];

/// Every projection the recording answers a read with, as the lines the wall wrote, in its order.
List<String> projectionLines(List<Exchange> corpus) => [
      for (final exchange in corpus)
        if (isRead(exchange.send)) exchange.line,
    ];

/// The first projection the wall recorded — the wall a test starts from.
String projectionLine(List<Exchange> corpus) {
  final lines = projectionLines(corpus);
  if (lines.isEmpty) {
    throw StateError('the recording answers no read with a projection: $corpusPath');
  }
  return lines.first;
}

/// Whether a send is exactly a read: the version and `"read": "wall"`, and nothing else.
bool isRead(Map<String, Object?> send) =>
    send.length == 2 && send['v'] == protocol && send['read'] == 'wall';

/// The answer a landed change is given — `{"ok": true, "changed": true}`, the recording's own line.
String landedAnswer(List<Exchange> corpus) {
  for (final exchange in corpus) {
    if (exchange.expect['changed'] == true) {
      return exchange.line;
    }
  }
  throw StateError('the recording holds no landed change: $corpusPath');
}

/// **A recorded refusal**, by the verb that provoked it and the kind the wall gave it.
///
/// Looked up rather than written, so a test's "the wall said no" is the file's — the wall's words and the wall's
/// category, exactly as `src/control.rs` wrote them (`fixtures/README.md`).
String refusalTo(List<Exchange> corpus, String verb, RefusalKind kind) {
  for (final exchange in corpus) {
    final intent = exchange.send['intent'];
    if (exchange.expect['ok'] != false || exchange.expect['kind'] != kind.word) {
      continue;
    }
    if (intent is Map<String, Object?> && intent['verb'] == verb) {
      return exchange.line;
    }
  }
  throw StateError(
    'the recording holds no ${kind.word} refusal to $verb: fixtures/control/session.jsonl is the wall, and a test that '
    'writes its own answer proves a wall nobody has (#224)',
  );
}

/// The version refusal the recording holds. It is answered to a request at another protocol version, which is what a
/// build older than the wall is.
String versionRefusal(List<Exchange> corpus) {
  for (final exchange in corpus) {
    if (exchange.expect['kind'] == RefusalKind.version.word) {
      return exchange.line;
    }
  }
  throw StateError('the recording holds no version refusal: $corpusPath');
}

/// **A `busy` refusal**, and the one line in these tests that is not a recorded answer.
///
/// The recording has none: it is written by the wall's session loop, which always answers, and `fixtures/README.md`
/// refuses a timed exchange. So the words are the wall's own — `the wall did not answer`, in `src/control.rs`'s
/// `REFUSALS` table with `RefusalKind::Busy` — and only the envelope is this file's.
String busyAnswer({String why = 'the wall did not answer'}) =>
    jsonEncode({'v': protocol, 'ok': false, 'kind': RefusalKind.busy.word, 'why': why});

/// The projection under an answer line, through the client's own decoder.
Wall projectionOf(String line) => _projection(line);

Wall _projection(String line) {
  final answer = decodeAnswer(line);
  if (answer is Projection) {
    return answer.wall;
  }
  throw StateError('$line is not a projection: the recording changed shape (#181)');
}

/// **The wall with no socket under it.** Every line the client sends is recorded, and every answer is either the
/// recording's or held by the test's hand — no clock, no network, no thread.
final class FakeCarrier implements Carrier {
  FakeCarrier(this._answers);

  /// What the wall will answer, in order. `null` is the connection ending — the end of file a closed socket gives.
  final List<String?> _answers;

  /// Every line the client has sent since the test last looked: the tap on the client's side of the wire, so a test
  /// says what the client sent rather than inferring it from what the wall happened to answer.
  final List<String> sent = <String>[];

  bool closed = false;

  /// Whether the wall answers on its own. A test that wants to look at the screen **before** the wall answers sets this
  /// false and hands over one answer at a time ([answer]) — the only way to see `moving…` at all.
  bool automatic = true;

  final List<Completer<String?>> _parked = [];

  /// **Hold every answer** from here on: a request that is out and has not landed.
  void holdAll() => automatic = false;

  /// Answer the oldest request that is waiting, with the next recorded line.
  void answer() {
    if (_parked.isEmpty) {
      throw StateError('no request is waiting for an answer: the screen sent nothing to answer');
    }
    _parked.removeAt(0).complete(_next());
  }

  /// Let every parked request — and every later one — be answered on its own.
  void resume() {
    automatic = true;
    while (_parked.isNotEmpty) {
      _parked.removeAt(0).complete(_next());
    }
  }

  /// The intents sent, without the reads: what "one release is one intent" is a claim about.
  List<String> get intents => [
        for (final line in sent)
          if (line.contains('"intent"')) line,
      ];

  String? _next() {
    if (_answers.isEmpty) {
      throw StateError('the fake wall has no answer left: the screen sent a line this test did not script');
    }
    return _answers.removeAt(0);
  }

  @override
  Future<void> send(String line) async {
    if (closed) {
      throw StateError('the fake wall has hung up');
    }
    sent.add(line);
  }

  @override
  Future<String?> receive() async {
    if (automatic) {
      return _next();
    }
    final gate = Completer<String?>();
    _parked.add(gate);
    return gate.future;
  }

  @override
  Future<void> close() async {
    closed = true;
  }
}

/// The phone-sized surface every test draws on: a golden that sizes itself from the window moves on the runner.
const Size phone = Size(360, 640);

/// **A controller that has read the recording's first projection**, with a clock a test owns.
Future<OverviewController> liveController(
  WidgetTester tester,
  FakeCarrier carrier, {
  DateTime Function()? clock,
  Duration retryDelay = Duration.zero,
  void Function(String message)? log,
}) async {
  final controller = OverviewController(
    clock: clock ?? () => DateTime.utc(2026, 11, 6, 12),
    retryDelay: retryDelay,
    log: log,
  );
  await controller.connect(carrier);
  return controller;
}

/// Pump the screen, at phone size.
Future<void> pumpScreen(
  WidgetTester tester,
  OverviewController controller, {
  VoidCallback? onUpdate,
}) async {
  await tester.binding.setSurfaceSize(phone);
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: OverviewScreen(controller: controller, onUpdate: onUpdate),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

/// The rectangle the canvas drew into, in its own points — the one a finger is hit-tested against.
CellRect canvasRect(WidgetTester tester) {
  final size = tester.getSize(find.byKey(canvasKey));
  return CellRect(0, 0, size.width, size.height);
}

/// A point in the canvas's own points, as a finger's global position.
Offset onCanvas(WidgetTester tester, double x, double y) =>
    tester.getTopLeft(find.byKey(canvasKey)) + Offset(x, y);

/// **A hand's drag**: hold [from] until the long press lifts, move to [to], and let go.
///
/// Every intermediate state is the phone's own drawing, so the pump between the steps is not slack — it is where a test
/// looks at what the canvas drew before the release.
Future<void> dragHand(
  WidgetTester tester, {
  required Offset from,
  required Offset to,
}) async {
  final gesture = await liftCard(tester, from: from, to: to);
  await gesture.up();
  await tester.pumpAndSettle();
}

/// **A hand's gutter drag, still holding**: the grip at [from] has been dragged to [to], and the test decides when it
/// is let go.
Future<TestGesture> grabGutter(
  WidgetTester tester, {
  required Offset from,
  required Offset to,
}) async {
  final gesture = await tester.startGesture(from);
  await tester.pump();
  await gesture.moveTo(to);
  await tester.pump();
  return gesture;
}

/// **The first line a session sends** — `{"v": 1, "read": "wall"}`, and nothing else with it.
final String readLine = jsonEncode({'v': protocol, 'read': 'wall'});

/// The line a `pane.drop` is, written from `docs/control.md`'s fields **independently of the app's encoder**: a test
/// that compared two sends to each other would prove they agree, and not that either is the wire's.
String dropLine({required int pane, required int onto, required String edge}) => jsonEncode({
      'v': protocol,
      'intent': {'verb': 'pane.drop', 'pane': pane, 'onto': onto, 'edge': edge},
    });

/// The line a `pane.resize` is, the same way.
String resizeLine({required int pane, required int other, required double ratio}) => jsonEncode({
      'v': protocol,
      'intent': {'verb': 'pane.resize', 'pane': pane, 'other': other, 'ratio': ratio},
    });

/// The projection whose **drawn** workspace holds the most panes, as the line the wall wrote: a golden of one card is a
/// golden of a tiling nobody drew.
String busiestProjectionLine(List<Exchange> corpus) {
  var best = projectionLine(corpus);
  var most = -1;
  for (final exchange in corpus) {
    if (!isRead(exchange.send)) {
      continue;
    }
    final wall = projectionOf(exchange.line);
    if (wall.active < 0 || wall.active >= wall.workspaces.length) {
      continue;
    }
    final count = wall.workspaces[wall.active].panes.length;
    if (count > most) {
      most = count;
      best = exchange.line;
    }
  }
  return best;
}

/// **A hand's long press, still holding**: the card is in the air at [to], and the test decides when it is released.
///
/// Every intermediate state is the phone's own drawing, so the pump between the steps is not slack — it is where a test
/// looks at what the canvas drew before the release.
Future<TestGesture> liftCard(
  WidgetTester tester, {
  required Offset from,
  required Offset to,
}) async {
  final gesture = await tester.startGesture(from);
  await tester.pump(kLongPressTimeout + const Duration(milliseconds: 50));
  await gesture.moveTo(to);
  await tester.pump();
  return gesture;
}

/// The projection whose **active** workspace is the emptied one — the wall's own `No panes yet.`, from its own file.
String emptiedProjectionLine(List<Exchange> corpus) {
  for (final exchange in corpus) {
    if (!isRead(exchange.send)) {
      continue;
    }
    final wall = projectionOf(exchange.line);
    if (wall.active >= 0 && wall.active < wall.workspaces.length) {
      if (wall.workspaces[wall.active].empty) {
        return exchange.line;
      }
    }
  }
  throw StateError('the recording no longer shows an emptied workspace on screen (#141)');
}

/// The cell of a pane, from the wall's walk: **the rectangle a finger is hit-tested against**, never the drawn card.
CellRect cellOf(List<Cell> laid, int pane) =>
    laid.firstWhere((cell) => cell.pane == pane).rect;

/// The wall's words in a recorded refusal, by the verb that provoked it and the kind the wall gave it — so a test can
/// assert what the screen does **not** say as well as what it does.
String refusalWord(List<Exchange> corpus, String verb, RefusalKind kind) {
  final answer = decodeAnswer(refusalTo(corpus, verb, kind));
  if (answer is Refused) {
    return answer.why;
  }
  throw StateError('the recording\'s ${kind.word} answer to $verb is not a refusal');
}

/// A development shell: the screen, showing the wall recorded in `fixtures/control/session.jsonl`.
///
/// It exists so the canvas can be looked at without a wall, a relay or a socket. **The projection comes from the
/// recording**, never from a value made up here — the fixture is the wall's own answer to `{"v": 1, "read": "wall"}`
/// (`cargo run -- --record`), and a screen drawn from a hand-built projection would be a screen proved against itself.
///
/// The real app reads the wall over a carrier (#256) and this file becomes its shell.
library;

import 'dart:convert';
import 'dart:io';

import 'package:arcons_phone/wire.dart';
import 'package:material_ui/material_ui.dart';

import 'overview.dart';

/// The recording, by path from this package: `clients/phone/app/` → the repository's root.
const String corpusPath = '../../../fixtures/control/session.jsonl';

void main() {
  final wall = firstProjection();
  runApp(
    MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: OverviewScreen(wall: wall, state: PhoneState.live),
      ),
    ),
  );
}

/// The first projection in the corpus — the wall's answer to the first read it ever recorded.
Wall firstProjection() {
  final file = File(corpusPath);
  if (!file.existsSync()) {
    throw StateError(
      'no corpus at $corpusPath: run this from the repository (the recording is `cargo run -- --record`)',
    );
  }
  for (final line in const LineSplitter().convert(file.readAsStringSync())) {
    if (line.trim().isEmpty) {
      continue;
    }
    final exchange = jsonDecode(line);
    if (exchange is! Map<String, Object?>) {
      continue;
    }
    final expect = exchange['expect'];
    if (expect is! Map<String, Object?>) {
      continue;
    }
    final answer = decodeAnswer(jsonEncode(expect));
    if (answer is Projection) {
      return answer.wall;
    }
  }
  throw StateError('the corpus holds no projection: $corpusPath');
}

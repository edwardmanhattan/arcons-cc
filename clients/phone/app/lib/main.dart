/// A development shell: the screen, showing the wall recorded in `fixtures/control/session.jsonl`.
///
/// It exists so the canvas can be looked at — and now dragged — without a wall, a relay or a socket. **The projection
/// comes from the recording**, never from a value made up here: the fixture is the wall's own answer to
/// `{"v": 1, "read": "wall"}` (`cargo run -- --record`), and a screen drawn from a hand-built projection would be a
/// screen proved against itself. The carrier below is the recording, replayed: one request out, the answer the wall
/// wrote for it, in the file's order.
///
/// The real app reads the wall over a socket or a relay (#256) and this file becomes its shell.
library;

import 'dart:convert';
import 'dart:developer' as developer;
import 'dart:io';

import 'package:arcons_phone/client.dart';
import 'package:material_ui/material_ui.dart';

import 'controller.dart';
import 'overview.dart';

/// The recording, by path from this package: `clients/phone/app/` → the repository's root.
const String corpusPath = '../../../fixtures/control/session.jsonl';

Future<void> main() async {
  WidgetsFlutterBinding.ensureInitialized();
  final controller = OverviewController();
  await controller.connect(RecordingCarrier.fromCorpus(File(corpusPath)));
  runApp(
    MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: OverviewScreen(
          controller: controller,
          onUpdate: () => developer.log(
            'this build cannot speak the wall\'s protocol: its store page is not in this shell',
            name: 'arcons-phone',
          ),
        ),
      ),
    ),
  );
}

/// **The recording as a wall.** Every answer is a line the wall wrote into `session.jsonl`, handed out in the file's
/// order; when the file runs out the connection does too, which is what a closed socket looks like to a client.
final class RecordingCarrier implements Carrier {
  RecordingCarrier(this._answers);

  /// Every recorded answer, verbatim, in the order the wall gave them.
  factory RecordingCarrier.fromCorpus(File file) {
    if (!file.existsSync()) {
      throw StateError(
        'no corpus at ${file.path}: run this from the repository (the recording is `cargo run -- --record`)',
      );
    }
    final answers = <String>[];
    for (final line in const LineSplitter().convert(file.readAsStringSync())) {
      if (line.trim().isEmpty) {
        continue;
      }
      final exchange = jsonDecode(line);
      if (exchange is! Map<String, Object?>) {
        continue;
      }
      final expect = exchange['expect'];
      if (expect is Map<String, Object?>) {
        answers.add(jsonEncode(expect));
      }
    }
    if (answers.isEmpty) {
      throw StateError('the corpus holds no answer: ${file.path}');
    }
    return RecordingCarrier(answers);
  }

  final List<String> _answers;
  int _next = 0;

  @override
  Future<void> send(String line) async {
    // The recording holds the answers and not the questions: a request is one line out, and what the wall said is what
    // this shell draws. A real carrier is #256.
  }

  @override
  Future<String?> receive() async => _next < _answers.length ? _answers[_next++] : null;

  @override
  Future<void> close() async {}
}

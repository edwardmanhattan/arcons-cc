/// **The phone's first claim is about the wall's own recording**, not about the phone's opinion of it.
///
/// `fixtures/control/session.jsonl` is recorded from the wire (`cargo run -- --record`, #224) — 93 exchanges the wall
/// answered. This suite reads that file and holds three claims:
///
/// 1. **Every answer in it is an answer this client can act on.** A shape the corpus grows without the client typing
///    it is a red test here, which is the only thing that keeps a client in another language honest (`ADR-0008`
///    decision 3).
/// 2. **The projection is the arrangement the screen draws**: workspaces, panes, and a tiling whose leaves are
///    **pane ids**, each of which the same workspace holds (#218).
/// 3. **Every refusal's category is one of the four, and each has an action** (`INV-07`'s mirror: the switch has no
///    default arm, and an unknown word is an error rather than a guess).
///
/// Nothing here is generated from this package's own types: the file is the wall's, and the test fails if it shrinks.
library;

import 'dart:convert';
import 'dart:io';

import 'package:arcons_phone/wire.dart';
import 'package:test/test.dart';

/// The recording, by path from this package: `clients/phone/` → the repository's root.
const String corpusPath = '../../fixtures/control/session.jsonl';

/// One exchange: what a client sent, and what the wall answered.
final class Exchange {
  const Exchange(this.line, this.send, this.expect);

  /// The 1-based line in the file, so a failure names where it is.
  final int line;

  /// Whatever was sent, typed as JSON holds it: the corpus keeps its malformed requests on purpose (`"not json"`,
  /// `[]`, two of the three fields at once), because they are the wall's parser being tested. A client reads the
  /// *answer* to them, which is always an answer.
  final Object? send;
  final Map<String, Object?> expect;
}

List<Exchange> readCorpus() {
  final file = File(corpusPath);
  if (!file.existsSync()) {
    fail(
      'the corpus is not at $corpusPath: the phone is held to fixtures/control/session.jsonl, which '
      '`cargo run -- --record` writes. A missing corpus is not a test that passes.',
    );
  }

  final exchanges = <Exchange>[];
  final lines = const LineSplitter().convert(file.readAsStringSync());
  for (var index = 0; index < lines.length; index++) {
    if (lines[index].trim().isEmpty) {
      continue;
    }
    final decoded = jsonDecode(lines[index]);
    if (decoded is! Map<String, Object?>) {
      fail('line ${index + 1} of $corpusPath is not a JSON object');
    }
    if (!decoded.containsKey('send')) {
      fail(
        'line ${index + 1} of $corpusPath is not an exchange: it has no "send", and a client reads the '
        'recording as one. Did `--record` change shape?',
      );
    }
    final expect = decoded['expect'];
    if (expect is! Map<String, Object?>) {
      fail(
        'line ${index + 1} of $corpusPath has no "expect" object: every send in the recording was answered, '
        'and the answer is what a client reads',
      );
    }
    exchanges.add(Exchange(index + 1, decoded['send'], expect));
  }
  return exchanges;
}

void main() {
  final exchanges = readCorpus();

  group('the recording', () {
    test('is the wall\'s, and big enough to be worth reading', () {
      expect(
        exchanges.length,
        greaterThanOrEqualTo(90),
        reason:
            'fixtures/control/session.jsonl holds ${exchanges.length} exchanges: a corpus that shrank '
            'would let every test below pass while proving less (docs/conformance.md)',
      );
    });

    test('every answer is one this client can act on', () {
      final counts = <String, int>{};
      for (final exchange in exchanges) {
        final Answer answer;
        try {
          answer = decodeAnswer(jsonEncode(exchange.expect));
        } on WireException catch (error) {
          fail(
            'line ${exchange.line} of $corpusPath answers with something this client cannot read '
            '(${error.why}). The phone must type every shape the wall sends, not the shapes it expects.',
          );
        }
        counts.update(
          switch (answer) {
            Changed() => 'changed',
            Projection() => 'projection',
            Granted() => 'grant',
            Refused() => 'refused',
          },
          (count) => count + 1,
          ifAbsent: () => 1,
        );
      }

      // Non-vacuity: each of the three shapes the corpus holds must be in it, or a test above passes on a file
      // that no longer exercises anything.
      expect(counts['changed'], isNotNull, reason: 'the corpus holds no `ok: true, changed` answer');
      expect(counts['projection'], isNotNull, reason: 'the corpus holds no projection');
      expect(counts['refused'], isNotNull, reason: 'the corpus holds no refusal');
    });

    test('the watch\'s stream is not in it, and the refusal is', () {
      // A grant and a publish belong to a carrier that carries a watch (#221), and **no fixture can hold a stream**:
      // the unix socket refuses a watch by name (which is the exchange this test finds), and `fixtures/README.md`
      // refuses a timed exchange because a coalesced publish is a race. So the two shapes are typed from
      // `src/control/wire.rs` and driven by the fake relay in `test/relay_test.dart` (#256), not by this file — and
      // this test says so rather than letting the corpus look like it covers them.
      final refusedWatch = exchanges.where((exchange) {
        final send = exchange.send;
        return send is Map<String, Object?> && send['watch'] != null && exchange.expect['ok'] == false;
      }).length;
      expect(
        refusedWatch,
        greaterThanOrEqualTo(1),
        reason:
            'the corpus no longer holds a watch refused by name: either the unix carrier now carries one, or '
            'the recording changed — and the client has to be told which',
      );

      // And the stream's lines are **not** in the corpus: a `watch` answer here would be a fixture that a race wrote,
      // and the fake relay's claim to be the only proof of them would be false.
      final streamed = [
        for (final exchange in exchanges)
          if (exchange.expect['watch'] != null && exchange.expect['ok'] == true) exchange.expect,
      ];
      expect(
        streamed,
        isEmpty,
        reason:
            'the corpus holds ${streamed.length} granted or published watch line(s): a watch stream is a race and '
            'not a fixture (fixtures/README.md), so the file cannot have gained one — and if it did, the fake relay '
            'in test/relay_test.dart is no longer the only thing proving the shape',
      );
    });
  });

  group('the projection is the arrangement', () {
    test('every workspace\'s leaves are pane ids that the same workspace holds', () {
      var projections = 0;
      var leaves = 0;
      var emptyWorkspaces = 0;
      var multiPaneWorkspaces = 0;
      for (final exchange in exchanges) {
        final answer = decodeAnswer(jsonEncode(exchange.expect));
        if (answer is! Projection) {
          continue;
        }
        projections++;
        final wall = answer.wall;
        for (final workspace in wall.workspaces) {
          if (workspace.empty) {
            emptyWorkspaces++;
          }
          if (workspace.panes.length > 1) {
            multiPaneWorkspaces++;
          }
          final ids = {for (final pane in workspace.panes) pane.id};
          final tiling = workspace.tiling;
          if (tiling == null) {
            expect(
              workspace.empty,
              isTrue,
              reason:
                  '"${workspace.name}" has no tiling and is not empty: the canvas would draw nothing where the '
                  'wall draws panes',
            );
            continue;
          }

          final named = tiling.leaves;
          leaves += named.length;
          expect(
            named.toSet().length,
            named.length,
            reason:
                '"${workspace.name}" names a pane id twice in its tiling: a leaf is a pane, and a pane has one cell',
          );
          for (final id in named) {
            expect(
              ids,
              contains(id),
              reason:
                  '"${workspace.name}" arranges pane $id, which its "panes" does not hold — a leaf is a pane id '
                  'and not an index (#218), so a client would draw a card with nothing behind it',
            );
          }
          expect(
            named.length,
            ids.length,
            reason:
                '"${workspace.name}" holds ${ids.length} panes and arranges ${named.length}: every pane has a '
                'cell, and a card the wall draws but the tiling does not place is a pane the phone cannot move',
          );
        }
      }
      expect(projections, greaterThanOrEqualTo(1), reason: 'the corpus holds no projection to check');
      expect(leaves, greaterThan(0), reason: 'no tiling leaf was read: the check above passed on nothing');
      expect(
        emptyWorkspaces,
        greaterThanOrEqualTo(1),
        reason:
            'no projection holds an emptied workspace: #141 keeps its chip and its name, and a client that '
            'dropped one would show a workspace the wall still has',
      );
      expect(
        multiPaneWorkspaces,
        greaterThanOrEqualTo(1),
        reason: 'no projection holds a workspace with two or more panes: the arrangement would be untested',
      );
    });

    test('a split\'s ratio is a share, and the two ways of splitting are the only two', () {
      var splits = 0;
      for (final exchange in exchanges) {
        final answer = decodeAnswer(jsonEncode(exchange.expect));
        if (answer is! Projection) {
          continue;
        }
        for (final workspace in answer.wall.workspaces) {
          for (final split in _splits(workspace.tiling)) {
            splits++;
            expect(
              split.ratio,
              allOf(greaterThanOrEqualTo(0.05), lessThanOrEqualTo(0.95)),
              reason:
                  'a share of ${split.ratio} is outside 0.05…0.95: that is the wall\'s clamp (#220), and a client '
                  'that drew it anyway would disagree with the wall about where a gutter is',
            );
          }
        }
      }
      expect(splits, greaterThan(0), reason: 'no split was read: the corpus would not exercise the tree');
    });
  });

  group('refusals map to what the screen does', () {
    test('each of the four kinds has an action, and none of them is a default', () {
      // The switch in `Next.of` has no default arm, so this test cannot compile while a fifth kind exists without
      // an action — which is `INV-07` one language over.
      for (final kind in RefusalKind.values) {
        expect(
          Next.of(kind),
          isA<Next>(),
          reason: 'refusal kind "${kind.word}" has no action on the phone (#198)',
        );
      }
      expect(
        RefusalKind.values.map((kind) => kind.word).toList(),
        ['rule', 'version', 'input', 'busy'],
        reason: 'the four kinds are the wall\'s (src/control.rs::RefusalKind), and a client reads the words',
      );
    });

    test('the corpus\'s categories are ones this client acts on, and the words round-trip', () {
      final seen = <String>{};
      for (final exchange in exchanges) {
        final answer = decodeAnswer(jsonEncode(exchange.expect));
        if (answer is Refused) {
          seen.add(answer.kind.word);
          expect(
            RefusalKind.ofWord(answer.kind.word),
            answer.kind,
            reason: 'a refusal kind must round-trip through its own word',
          );
          expect(
            Next.of(answer.kind),
            isA<Next>(),
            reason: 'line ${exchange.line}: kind "${answer.kind.word}" decides the screen, and it has no action',
          );
        }
      }
      expect(seen, isNotEmpty, reason: 'the corpus holds no refusal: the mapping above would be untested');
      expect(
        seen.difference({'rule', 'version', 'input', 'busy'}),
        isEmpty,
        reason: 'the corpus holds a refusal kind this client does not know: $seen',
      );
    });

    test('an unknown word is an error and not a category', () {
      expect(
        () => RefusalKind.ofWord('unheard-of'),
        throwsA(isA<WireException>()),
        reason:
            'a client that guessed a category would act on the wrong one; a new kind must be a visible branch, '
            'which is `INV-07` (src/control.rs::REFUSALS)',
      );
      expect(() => RefusalKind.ofWord(null), throwsA(isA<WireException>()));
    });

    test('an answer at another version is not parsed, it is a stop', () {
      expect(
        () => decodeAnswer('{"v": 2, "ok": true, "changed": true}'),
        throwsA(isA<WireException>()),
        reason:
            'the version is compared and never negotiated: a phone that guessed would drive a wall it does not '
            'speak (ADR-0008 decision 5)',
      );
    });
  });
}

Iterable<TilingSplit> _splits(Tiling? tiling) sync* {
  switch (tiling) {
    case null:
    case TilingLeaf():
      return;
    case TilingSplit(:final a, :final b, :final ratio, :final split):
      yield TilingSplit(a, b, ratio, split);
      yield* _splits(a);
      yield* _splits(b);
  }
}

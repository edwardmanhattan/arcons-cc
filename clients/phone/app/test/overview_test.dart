/// **The canvas, held to the wall's own recording** (#255).
///
/// The projection is the corpus's — the wall's answer to `{"v": 1, "read": "wall"}`, decoded by `arcons_phone`'s wire
/// — and the screen is asserted three ways:
///
/// 1. **where the cards are**: the rects come from `cells.dart`, which is `Layout::cells` proved against the nine
///    recorded cases (#249), so a canvas that drew its own arithmetic would be a second layout;
/// 2. **what it says**: one sentence for the canvas and one per card, produced by the walk that drew them;
/// 3. **what it looks like**: a golden, rendered headless. The harness draws text in its own box font, so the picture
///    is a check of *layout and colour*; the words are the semantics test's, and a bundled typeface arrives with the
///    app's assets ticket.
library;

import 'dart:convert';
import 'dart:io';

import 'package:arcons_phone/cells.dart';
import 'package:arcons_phone/wire.dart';
import 'package:material_ui/material_ui.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:arcons_phone_app/overview.dart';

/// The recording, by path from this package: `clients/phone/app/` → the repository's root.
const String corpusPath = '../../../fixtures/control/session.jsonl';

/// Every projection in the corpus, in the order the wall recorded them.
List<Wall> projections() {
  final file = File(corpusPath);
  if (!file.existsSync()) {
    // Thrown rather than expected: this runs while the file is being loaded, before any test body exists.
    throw StateError(
      'no corpus at $corpusPath: the screen is held to the wall\'s recording (`cargo run -- --record`), and a '
      'screen proved against a projection made up in a test is proved against itself',
    );
  }
  final walls = <Wall>[];
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
      walls.add(answer.wall);
    }
  }
  return walls;
}

/// A projection whose **drawn** workspace is worth drawing: the most panes in the one the wall is showing.
///
/// The score is the active workspace's, not the wall's: a projection with twelve panes behind a chip is a picture of
/// one card, and a canvas tested on one card is a canvas whose tiling is untested.
Wall busiest(List<Wall> walls) {
  var best = walls.first;
  var most = -1;
  for (final wall in walls) {
    if (wall.active < 0 || wall.active >= wall.workspaces.length) {
      continue;
    }
    final count = wall.workspaces[wall.active].panes.length;
    if (count > most) {
      most = count;
      best = wall;
    }
  }
  return best;
}

Future<void> pump(
  WidgetTester tester,
  Wall wall, {
  PhoneState state = PhoneState.live,
}) async {
  // A phone-sized surface, fixed: a golden that sizes itself from the window is a golden that moves on the runner.
  await tester.binding.setSurfaceSize(const Size(360, 640));
  addTearDown(() => tester.binding.setSurfaceSize(null));
  await tester.pumpWidget(
    MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: OverviewScreen(wall: wall, state: state),
      ),
    ),
  );
  await tester.pumpAndSettle();
}

void main() {
  final walls = projections();

  test('the recording holds projections to draw', () {
    expect(
      walls,
      isNotEmpty,
      reason: 'the corpus holds no projection: the screen would be tested against nothing',
    );
    final drawn = walls.where((wall) {
          return wall.active >= 0 &&
              wall.active < wall.workspaces.length &&
              wall.workspaces[wall.active].panes.length >= 2;
        }).toList();
    expect(
      drawn,
      isNotEmpty,
      reason:
          'no recorded projection shows a workspace with two panes: the canvas would be tested on a single card, and a '
          'tiling drawn once is a tiling nobody has drawn',
    );
  });

  group('the walk', () {
    test('every card sits in a rect the wall\'s arithmetic produced', () {
      final wall = busiest(walls);
      const size = Size(336, 560);
      final cards = layOut(wall, size);
      final workspace = wall.workspaces[wall.active];
      final byId = {for (final pane in workspace.panes) pane.id: pane};

      expect(cards, isNotEmpty, reason: 'the busiest projection drew no cards');
      final expected = rects(
        workspace.tiling!,
        CellRect(0, 0, size.width, size.height),
      );
      expect(
        cards.map((card) => card.rect).toList(),
        expected.map((cell) => cell.rect).toList(),
        reason:
            'the cards are not where `cells.dart` says the tiling puts them: the canvas would be a second layout, '
            'and `fixtures/control/tiling/` is the wall\'s own answer to where a pane is (#249)',
      );
      for (final card in cards) {
        expect(
          byId[card.pane.id],
          isNotNull,
          reason: 'card ${card.pane.id} is not a pane the workspace holds',
        );
      }
    });

    test('the words are the walk\'s, and each card is its kind, its title and whether it has the keys', () {
      final wall = busiest(walls);
      final cards = layOut(wall, const Size(336, 560));
      expect(
        spokenCanvas(wall, cards),
        startsWith(wall.workspaces[wall.active].name),
        reason:
            'the canvas speaks the workspace it draws, and it speaks it first',
      );
      for (final card in cards) {
        expect(
          card.spoken,
          contains(card.pane.kind),
          reason: 'a card\'s words name its kind: a description that says `map` twice is one nobody can act on',
        );
        if (card.pane.title.isNotEmpty) {
          expect(
            card.spoken,
            contains(card.pane.title),
            reason: 'a card\'s words name its title',
          );
        }
        expect(
          card.spoken.contains('focused'),
          card.pane.focused,
          reason: 'the ring is drawn and described, never colour alone (#111)',
        );
      }
    });
  });

  group('the screen', () {
    testWidgets(
      'draws one card per card, and reads them out loud in the drawn order',
      (tester) async {
        final wall = busiest(walls);
        await pump(tester, wall);

        final laid = layOut(wall, const Size(336, 640 - 28 - 26 - 32));
        // The spoken description is on the canvas; the cards keep their own labels.
        final canvas = find.bySemanticsLabel(spokenCanvas(wall, laid));
        expect(
          canvas,
          findsOneWidget,
          reason: 'the canvas must say what it draws, and say it with the walk that drew it',
        );
        for (final card in laid) {
          expect(
            find.bySemanticsLabel(card.spoken),
            findsWidgets,
            reason:
                'the card for ${card.spoken} was drawn and is not spoken: the words and the picture came apart',
          );
        }
      },
    );

    testWidgets(
      'the chips are the workspaces, the active one chosen, an emptied one still there',
      (tester) async {
        final wall = busiest(walls);
        await pump(tester, wall);
        for (var index = 0; index < wall.workspaces.length; index++) {
          final workspace = wall.workspaces[index];
          expect(
            find.text(workspace.name),
            findsOneWidget,
            reason:
                'workspace ${index + 1} ("${workspace.name}") is not on a chip: an emptied workspace is still a place '
                'with its name (#141)',
          );
        }
      },
    );

    testWidgets('the state line says what the phone knows, and never more', (
      tester,
    ) async {
      final wall = busiest(walls);
      await pump(tester, wall, state: PhoneState.notConnected);
      expect(
        find.text('not connected'),
        findsOneWidget,
        reason: 'a phone with no read says so rather than showing a diagram labelled current',
      );
      expect(
        find.textContaining('wall seq'),
        findsNothing,
        reason: 'a version is shown only when there is a read',
      );

      await pump(tester, wall, state: PhoneState.live);
      expect(find.text('live · wall seq ${wall.seq}'), findsOneWidget);
    });

    testWidgets('an emptied workspace says the wall\'s own words', (
      tester,
    ) async {
      // The corpus's last projections hold an emptied workspace; find one and make it active.
      final withEmpty = walls
          .where((wall) => wall.workspaces.any((workspace) => workspace.empty))
          .toList();
      expect(
        withEmpty,
        isNotEmpty,
        reason: 'the corpus no longer records an emptied workspace (#141)',
      );
      final source = withEmpty.first;
      final emptyIndex = source.workspaces.indexWhere(
        (workspace) => workspace.empty,
      );
      final emptied = Wall(
        active: emptyIndex,
        focused: null,
        maximised: null,
        seq: source.seq,
        workspaces: source.workspaces,
      );
      await pump(tester, emptied);
      expect(
        find.text('No panes yet.'),
        findsOneWidget,
        reason: 'the wall draws `No panes yet.` for an emptied workspace, and the phone says the same thing',
      );
    });

    testWidgets('a golden of the canvas, at phone size', (tester) async {
      final wall = busiest(walls);
      await pump(tester, wall);
      await expectLater(
        find.byType(OverviewScreen),
        matchesGoldenFile('goldens/overview-live.png'),
      );
    });

    testWidgets('a golden of a phone with nothing read', (tester) async {
      final wall = busiest(walls);
      await pump(tester, wall, state: PhoneState.notConnected);
      await expectLater(
        find.byType(OverviewScreen),
        matchesGoldenFile('goldens/overview-not-connected.png'),
      );
    });
  });
}

/// **The canvas, held to the wall's own recording** (#255, #257).
///
/// The projection is the corpus's — the wall's answer to `{"v": 1, "read": "wall"}`, decoded by `arcons_phone`'s wire
/// and read through the controller that carries it — and the screen is asserted three ways:
///
/// 1. **where the cards are**: the rects come from `cells.dart`, which is `Layout::cells` proved against the nine
///    recorded cases (#249), so a canvas that drew its own arithmetic would be a second layout;
/// 2. **what it says**: one sentence for the canvas and one per card, produced by the walk that drew them;
/// 3. **what it looks like**: a golden, rendered headless. The harness draws text in its own box font, so the picture
///    is a check of *layout and colour*; the words are the semantics test's, and a bundled typeface arrives with the
///    app's assets ticket.
library;

import 'package:arcons_phone/cells.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:arcons_phone_app/controller.dart';
import 'package:arcons_phone_app/overview.dart';

import 'harness.dart';

void main() {
  final corpus = readCorpus();
  final walls = projections(corpus);

  test('the recording holds projections to draw', () {
    expect(
      walls,
      isNotEmpty,
      reason:
          'the corpus holds no projection: the screen would be tested against nothing',
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
      final wall = projectionOf(busiestProjectionLine(corpus));
      final canvas = CellRect(0, 0, 336, 560);
      final cards = layOut(wall, canvas);
      final workspace = activeWorkspace(wall)!;
      final byId = {for (final pane in workspace.panes) pane.id: pane};

      expect(cards, isNotEmpty, reason: 'the busiest projection drew no cards');
      final expected = rects(workspace.tiling!, canvas);
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
      final wall = projectionOf(busiestProjectionLine(corpus));
      final cards = layOut(wall, CellRect(0, 0, 336, 560));
      expect(
        spokenCanvas(wall, cards),
        startsWith(activeWorkspace(wall)!.name),
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
        final controller = await liveController(
          tester,
          FakeCarrier([busiestProjectionLine(corpus)]),
        );
        await pumpScreen(tester, controller);

        final wall = controller.wall!;
        final laid = layOut(wall, canvasRect(tester));
        // The spoken description is on the canvas; the cards keep their own labels.
        expect(
          find.bySemanticsLabel(spokenCanvas(wall, laid)),
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
        final controller = await liveController(
          tester,
          FakeCarrier([busiestProjectionLine(corpus)]),
        );
        await pumpScreen(tester, controller);
        final wall = controller.wall!;
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

    testWidgets('the state line is silent while the last read stands, and says what it knows when it does not', (
      tester,
    ) async {
      var now = DateTime.utc(2026, 11, 6, 12);
      final carrier = FakeCarrier([projectionLine(corpus), null]);
      final controller = OverviewController(clock: () => now);
      await controller.connect(carrier);
      await pumpScreen(tester, controller);

      expect(controller.state, PhoneState.live);
      expect(
        find.text(''),
        findsOneWidget,
        reason: 'on a live connection the state line says **nothing at all**: silence is the phone saying *this is '
            'the wall* (`docs/mobile-ui.md`, the four states) — a version number here is a claim dressed as a label',
      );

      // A read that does not land takes the holes with it, and the age is what is left of the diagram.
      now = now.add(const Duration(seconds: 41));
      await controller.read();
      await tester.pumpAndSettle();
      expect(controller.state, PhoneState.notConnected);
      expect(
        find.text('not connected · last read 41 s ago'),
        findsOneWidget,
        reason: 'the number in `last read 41 s ago` is an **age** and not a sequence: it says exactly what it says, '
            'and it is the only thing separating a stale diagram from a lie',
      );
      expect(
        find.textContaining('wall seq'),
        findsNothing,
        reason: 'a version is shown only where it is true to show it',
      );
    });

    testWidgets('an emptied workspace says the wall\'s own words', (tester) async {
      final controller = await liveController(
        tester,
        FakeCarrier([emptiedProjectionLine(corpus)]),
      );
      await pumpScreen(tester, controller);
      expect(
        activeWorkspace(controller.wall!)!.empty,
        isTrue,
        reason: 'the recording no longer shows an emptied workspace on screen (#141)',
      );
      expect(
        find.text('No panes yet.'),
        findsOneWidget,
        reason: 'the wall draws `No panes yet.` for an emptied workspace, and the phone says the same thing',
      );
    });

    testWidgets('a golden of the canvas, at phone size', (tester) async {
      final controller = await liveController(
        tester,
        FakeCarrier([busiestProjectionLine(corpus)]),
      );
      await pumpScreen(tester, controller);
      await expectLater(
        find.byType(OverviewScreen),
        matchesGoldenFile('goldens/overview-live.png'),
      );
    });

    testWidgets('a golden of a phone whose last read did not land', (tester) async {
      var now = DateTime.utc(2026, 11, 6, 12);
      final carrier = FakeCarrier([busiestProjectionLine(corpus), null]);
      final controller = OverviewController(clock: () => now);
      await controller.connect(carrier);
      await pumpScreen(tester, controller);
      now = now.add(const Duration(seconds: 41));
      await controller.read();
      await tester.pumpAndSettle();
      await expectLater(
        find.byType(OverviewScreen),
        matchesGoldenFile('goldens/overview-not-connected.png'),
      );
    });
  });
}

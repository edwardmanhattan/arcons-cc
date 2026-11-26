/// **The drag, held to `docs/mobile-ui.md`** (#257).
///
/// The claims this file makes, and each is one test:
///
/// 1. **one release is one intent** — `pane.drop { pane, onto, edge }`, sent on the release and not before;
/// 2. **nothing is optimistic** — the card is drawn in the slot the preview gave it, under `moving…`, and the next
///    projection is what moves it; an `ok` is not a projection;
/// 3. **a refusal returns the card and re-reads** — `rule` and `input` restore and read, `busy` keeps the pending card
///    and retries the **read**, `version` stops;
/// 4. **the hand and the list produce the same request line** — the drag and the **Move to…** row, compared as the
///    bytes the wall sees;
/// 5. **the gutter** — one `pane.resize` on release, previewed through `previewResize`, never outside `RATIO_RANGE`.
///
/// The wall is a fake carrier of three methods and the projections are `fixtures/control/session.jsonl`'s, read by
/// path: no socket, no thread, and no clock the test does not control.
library;

import 'dart:convert';

import 'package:arcons_phone/cells.dart';
import 'package:arcons_phone/wire.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:arcons_phone_app/controller.dart';
import 'package:arcons_phone_app/intents.dart';
import 'package:arcons_phone_app/overview.dart';

import 'harness.dart';

/// Where a finger goes to ask for [edge] of [pane]: ten per cent into that edge's band, or the middle for a swap.
///
/// The *test's* finger, not the app's arithmetic: the app asks `dropFor`, and this is where a hand is put to make it
/// answer.
({double x, double y}) probe(List<Cell> laid, int pane, Edge edge) {
  final rect = cellOf(laid, pane);
  return switch (edge) {
    Edge.left => (x: rect.x + rect.w * 0.1, y: rect.y + rect.h / 2),
    Edge.right => (x: rect.right - rect.w * 0.1, y: rect.y + rect.h / 2),
    Edge.top => (x: rect.x + rect.w / 2, y: rect.y + rect.h * 0.1),
    Edge.bottom => (x: rect.x + rect.w / 2, y: rect.bottom - rect.h * 0.1),
    Edge.centre => (x: rect.x + rect.w / 2, y: rect.y + rect.h / 2),
  };
}

({double x, double y}) middle(List<Cell> laid, int pane) {
  final rect = cellOf(laid, pane);
  return (x: rect.x + rect.w / 2, y: rect.y + rect.h / 2);
}

void main() {
  final corpus = readCorpus();
  final lines = projectionLines(corpus);
  // The wall every drag here starts from: three panes, `side 0.5 (1 | stack 0.5 (2 / 3))` — pane 1 on the left, 2 over
  // 3 on the right. Its second projection is the same arrangement with the ring moved, and the fourth is a wall of one
  // pane, which is what a client that assumed its own answer would be caught by.
  final first = lines.first;
  final next = lines[1];
  final elsewhere = lines[3];

  /// A controller that has read [first] and is on screen, with the canvas's rectangle.
  Future<(OverviewController, FakeCarrier)> start(
    WidgetTester tester,
    List<String?> answers, {
    DateTime Function()? clock,
    void Function(String message)? log,
    Duration retryDelay = const Duration(seconds: 2),
  }) async {
    final carrier = FakeCarrier(answers);
    final controller = await liveController(
      tester,
      carrier,
      clock: clock,
      log: log,
      retryDelay: retryDelay,
    );
    await pumpScreen(tester, controller);
    return (controller, carrier);
  }

  group('a release is one intent', () {
    testWidgets('the hand lands pane 3 on pane 1\'s left half, once', (tester) async {
      final (controller, carrier) = await start(tester, [first, landedAnswer(corpus), next]);
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);

      await dragHand(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );

      expect(
        carrier.sent.first,
        readLine,
        reason: 'the first line a session sends is the read: the projection is the phone\'s only truth, and a drop '
            'composed against a wall nobody has read is a drop composed wrongly (mobile-remote.md decision 1)',
      );
      expect(
        carrier.intents,
        [dropLine(pane: 3, onto: 1, edge: 'left')],
        reason: 'one release is **one** `pane.drop`, with the pane that moved, the pane it landed on and the edge of '
            'that pane — and no second intent, no move of the phone\'s own (#257)',
      );
      expect(
        controller.drawn,
        same(controller.projected),
        reason: 'after the read landed the canvas draws the projection again: the preview is a question, and the '
            'answer replaces it',
      );
      expect(
        find.text('moving…'),
        findsNothing,
        reason: 'a card that says `moving…` after the projection landed is a phone still asking a question it has the '
            'answer to',
      );
    });

    testWidgets('released where it started, nothing is sent at all', (tester) async {
      final (controller, carrier) = await start(tester, [first]);
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);

      // A short move **inside** the lifted card's own cell: `dropFor` answers nothing over the pane it is dragging.
      await dragHand(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, cellOf(laid, 3).x + 12, cellOf(laid, 3).y + 12),
      );

      expect(
        carrier.intents,
        isEmpty,
        reason: 'a release over the lifted card\'s own cell sent an intent: on the wall it is no drop at all, and a '
            'phone that asked anyway would be asking the wall to swap a pane with itself (`the pane cannot be swapped '
            'with itself`)',
      );
      expect(controller.moving, isFalse, reason: 'nothing was asked, so nothing is moving');
      expect(
        find.text(''),
        findsOneWidget,
        reason: 'cancel is silent: **nothing happened**, and a phone that said something would be reporting an event '
            'that did not occur (docs/mobile-ui.md, *cancel is a place, not a button*)',
      );
      expect(carrier.sent, [readLine], reason: 'the only line a cancelled drag leaves behind is the read that opened '
          'the session');
    });

    testWidgets('while a card is moving the canvas takes no second lift', (tester) async {
      final (controller, carrier) = await start(tester, [first, landedAnswer(corpus), next]);
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);
      carrier.holdAll();

      final gesture = await liftCard(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );
      await gesture.up();
      await tester.pump();
      expect(controller.moving, isTrue, reason: 'the intent is out and its answer is held');

      expect(
        controller.lift(
          x: middle(laid, 2).x,
          y: middle(laid, 2).y,
          canvas: canvas,
        ),
        isFalse,
        reason: 'a second card came up while one was moving: two drops in flight are two answers to "where is that '
            'pane", and the projection will only settle one of them (docs/mobile-ui.md, *one gesture at a time*)',
      );
      expect(
        carrier.intents,
        hasLength(1),
        reason: 'the second lift put a second intent on the wire',
      );

      carrier.resume();
      await tester.pumpAndSettle();
    });
  });

  group('nothing is optimistic', () {
    testWidgets('the card waits in the slot, says moving…, and the projection is what moves it', (tester) async {
      final (controller, carrier) = await start(tester, [first, landedAnswer(corpus), elsewhere]);
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);
      carrier.holdAll();

      final gesture = await liftCard(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );

      // **Before release**: the landing is drawn — pane 1 split in half, pane 3 in the half it was dropped against —
      // and nothing is on the wire.
      final preview = controller.drawn!;
      expect(
        carrier.intents,
        isEmpty,
        reason: 'a drag sent an intent before it was released: `docs/control.md` says the wall sees one intent on '
            'release and no sixty a second, and the hold is the phone\'s',
      );
      expect(
        controller.landing,
        Retile(1, Side.left),
        reason: 'the pointer is in pane 1\'s left band and the phone did not read it that way: `PANE_EDGE_BAND` is the '
            'wall\'s 0.3 of the pane\'s own span (src/tokens.rs), and a client that guessed sends `centre` where '
            'the wall\'s own hand sends `left`',
      );
      final advertised = cells(preview, canvas);
      expect(
        cellOf(advertised, 3),
        CellRect(
          cellOf(laid, 1).x,
          cellOf(laid, 1).y,
          cellOf(laid, 1).w / 2,
          cellOf(laid, 1).h,
        ),
        reason: 'the preview did not draw the landing it is advertising: `Layout::apply` gives pane 3 half of pane 1\'s '
            'cell, and a picture computed another way is two layouts that disagree',
      );

      await gesture.up();
      await tester.pump();

      // **After release, before the answer**: the same picture, and now it says what it is waiting for.
      expect(controller.moving, isTrue, reason: 'the intent is out and the wall has not answered');
      expect(
        cells(controller.drawn!, canvas),
        cells(preview, canvas),
        reason: 'the canvas redrew between the release and the answer: `moving…` is the phone saying *this is where '
            'it will go if the wall agrees*, and a picture that changed underneath it would be a claim',
      );
      expect(
        find.text('moving…'),
        findsOneWidget,
        reason: 'the card is drawn in a slot the wall has not agreed to and the screen says nothing: a snap-back with '
            'no sentence, or a move with no question, is the operator being told nothing about their own gesture',
      );

      carrier.answer(); // the intent's answer: `{"ok": true, "changed": true}`
      await tester.pump();
      expect(
        controller.moving,
        isTrue,
        reason: 'an `ok` moved the card: an `ok` is not a projection (`ADR-0006` decision 4), and a client that drew '
            'its own answer would be drawing a wall it could not know',
      );
      expect(
        cells(controller.drawn!, canvas),
        cells(preview, canvas),
        reason: 'the answer to the intent moved the picture: an `ok` is not a projection',
      );

      carrier.answer(); // the read: a wall of one pane, which is *not* what was asked for
      await tester.pumpAndSettle();
      expect(controller.moving, isFalse);
      expect(
        controller.drawn,
        same(controller.projected),
        reason: 'the preview outlived the projection: after the read lands the canvas draws what the wall says, and '
            'nothing of the phone\'s own',
      );
      expect(
        activeWorkspace(controller.wall!)!.panes,
        hasLength(1),
        reason: 'the recorded projection this test answers with is a wall of one pane: a test that expected its own '
            'drop to have landed would be testing its own arithmetic',
      );
      expect(
        find.text('moving…'),
        findsNothing,
        reason: 'the question was answered, so the phone stops asking it',
      );
    });
  });

  group('the canvas only sends what it has read for', () {
    testWidgets('a press on a card sends pane.focus, and the ring moves with the projection', (tester) async {
      final (controller, carrier) = await start(tester, [first, landedAnswer(corpus), next]);
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);
      final at = middle(laid, 1);
      await tester.tapAt(onCanvas(tester, at.x, at.y));
      await tester.pumpAndSettle();
      expect(
        carrier.intents,
        [
          jsonEncode({
            'v': protocol,
            'intent': {'verb': 'pane.focus', 'pane': 1},
          }),
        ],
        reason: 'a press on a card is `pane.focus` and nothing else: the ring is shared, so a press is how a client '
            'says *this one has the keys* (docs/mobile-ui.md, the verbs it uses)',
      );
      expect(
        controller.wall!.focused,
        projectionOf(next).focused,
        reason: 'the ring moved on the `ok` and not on the projection: an answer is not a wall',
      );
    });

    testWidgets('not connected: the diagram stays, dimmed, and the canvas sends nothing', (tester) async {
      final carrier = FakeCarrier([first, null]);
      final controller = await liveController(tester, carrier);
      await pumpScreen(tester, controller);
      await controller.read();
      await tester.pumpAndSettle();
      expect(controller.state, PhoneState.notConnected);

      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);
      await dragHand(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );
      expect(
        carrier.intents,
        isEmpty,
        reason: 'the canvas sent an intent while the phone had no read to compose it from: every intent on this '
            'screen names a pane by id, and a stale id is `no such pane` — a rule, and *do not retry until something '
            'else changes* (docs/mobile-ui.md, *not connected*)',
      );
      expect(
        find.bySemanticsLabel(spokenCanvas(controller.wall!, layOut(controller.wall!, canvas))),
        findsOneWidget,
        reason: 'the diagram the operator was looking at is the only thing on the screen that says which pane is '
            'which: it stays, dimmed, with the age under it',
      );
    });

    testWidgets('connecting: no chips, no canvas, and one honest line', (tester) async {
      final controller = OverviewController();
      await pumpScreen(tester, controller);
      expect(find.text('connecting…'), findsOneWidget);
      expect(find.text('live'), findsNothing);
      expect(
        find.byKey(canvasKey),
        findsNothing,
        reason: 'a phone that has read nothing drew a diagram: `connecting` is not `not connected`, and a shape from a '
            'previous life is the stale arrangement the four states exist to forbid',
      );
    });
  });

  group('a refusal returns the card', () {
    testWidgets('rule: the wall\'s words, the projection\'s cell, and a read — never the same intent', (tester) async {
      final (controller, carrier) = await start(tester, [
        first,
        refusalTo(corpus, 'pane.drop', RefusalKind.rule),
        next,
      ]);
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);

      await dragHand(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );

      expect(
        carrier.sent,
        [
          readLine,
          dropLine(pane: 3, onto: 1, edge: 'left'),
          readLine,
        ],
        reason: 'a `rule` refusal was answered with the same intent again, or with nothing: the table says *return the '
            'card, then re-read* — a rule usually means the premise moved, and the retry is a read '
            '(docs/mobile-ui.md, refusals → what the phone does)',
      );
      expect(
        controller.state,
        PhoneState.live,
        reason: 'a refusal is not an error state: the wall answered, the connection is up, and the next line may be a '
            'fresh request',
      );
      expect(
        cells(controller.drawn!, canvas),
        cells(controller.projected!, canvas),
        reason: 'the card did not come back to the cell the last projection gave it',
      );
      expect(
        find.text(refusalWord(corpus, 'pane.drop', RefusalKind.rule)),
        findsOneWidget,
        reason: 'the wall\'s words are shown **verbatim**: the rules are the wall\'s, and a paraphrase is a second copy '
            'of a rule',
      );
    });

    testWidgets('input: the phone\'s own sentence, one log line and a read', (tester) async {
      final logs = <String>[];
      final (controller, carrier) = await start(
        tester,
        [first, refusalTo(corpus, 'pane.drop', RefusalKind.input), next],
        log: logs.add,
      );
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);

      await dragHand(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );

      expect(
        carrier.sent.last,
        readLine,
        reason: 'an `input` refusal is re-read: the phone restores the cell and asks what the wall really has',
      );
      expect(
        find.text(ownBugSentence),
        findsOneWidget,
        reason: 'an operator was shown the wall\'s words for the phone\'s own bug: `unknown verb` is not something '
            'they can fix, and the table gives this kind the app\'s sentence instead',
      );
      expect(
        find.text(refusalWord(corpus, 'pane.drop', RefusalKind.input)),
        findsNothing,
        reason: 'the wall\'s words leaked into a screen whose sentence is the app\'s',
      );
      expect(logs, hasLength(1), reason: 'an `input` refusal writes exactly one log line');
      expect(
        logs.single,
        allOf(
          contains(RefusalKind.input.word),
          isNot(contains('"intent"')),
          isNot(contains('"pane"')),
        ),
        reason: 'the log line carries the category and no request: **no secret, no host, no token** (`INV-08`), and a '
            'line that interpolated the fields would be one device key away from being one',
      );
    });

    testWidgets('busy: the pending card stays, and the retry is the read', (tester) async {
      final (controller, carrier) = await start(
        tester,
        [first, busyAnswer(), next],
        retryDelay: const Duration(milliseconds: 20),
      );
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);
      carrier.holdAll();

      final gesture = await liftCard(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );
      await gesture.up();
      await tester.pump();
      final preview = controller.drawn;

      carrier.answer(); // the intent's answer: busy
      await tester.pump();
      await tester.pump();

      expect(
        controller.moving,
        isTrue,
        reason: '`busy` was treated as a refusal: the wall did not answer, which means **the change may have landed**, '
            'so the card keeps its slot and the phone keeps asking (`docs/mobile-ui.md`, the refusal table)',
      );
      expect(
        controller.drawn == null ? null : cells(controller.drawn!, canvas),
        cells(preview!, canvas),
        reason: 'the pending card was taken away on a `busy`: a phone that dropped it would show the operator an '
            'arrangement the wall may already have changed',
      );
      expect(
        find.text(busySentence),
        findsOneWidget,
        reason: 'the phone said nothing while the wall was not answering',
      );

      // The wait is over: the read is the retry. It is answered with the wall's next projection.
      await tester.pump(const Duration(milliseconds: 20));
      expect(
        carrier.sent,
        [readLine, dropLine(pane: 3, onto: 1, edge: 'left'), readLine],
        reason: 'a `busy` was retried with the **change**: `the wall did not answer` means the change may have landed, '
            'so the only honest retry is the read that says whether it did',
      );
      carrier.answer();
      await tester.pumpAndSettle();
      expect(controller.moving, isFalse, reason: 'the read landed, so the question is answered');
      expect(controller.state, PhoneState.live);
    });

    testWidgets('version: one screen, the wall\'s words, and nothing more sent', (tester) async {
      final (controller, carrier) = await start(tester, [first, versionRefusal(corpus)]);
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);

      await dragHand(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );

      expect(controller.state, PhoneState.updateRequired);
      expect(
        find.text('unsupported protocol version'),
        findsOneWidget,
        reason: 'the update screen carries the wall\'s words, verbatim',
      );
      expect(
        find.text('This app is older than this wall. Update it to control the wall.'),
        findsOneWidget,
        reason: 'the update screen carries one sentence in the app\'s own words beside the wall\'s',
      );
      expect(
        find.text('Update'),
        findsOneWidget,
        reason: 'one button, and it is not a retry',
      );
      expect(
        carrier.sent,
        [readLine, dropLine(pane: 3, onto: 1, edge: 'left')],
        reason: 'a version refusal was followed by a read or by another intent: a client that retries a version it '
            'cannot speak loops, and `docs/control.md` says *send nothing more, update it*',
      );

      // A press after the update screen sends nothing either.
      await controller.focusPane(1);
      expect(
        carrier.sent,
        hasLength(2),
        reason: 'the canvas sent something after the version refusal',
      );
    });
  });

  group('the hand and the list', () {
    testWidgets('a drag and a Move to… row produce the same request line', (tester) async {
      // The hand.
      final (handController, handCarrier) = await start(tester, [first, landedAnswer(corpus), next]);
      final canvas = canvasRect(tester);
      final laid = cells(handController.projected!, canvas);
      await dragHand(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );
      expect(handCarrier.intents, hasLength(1));

      // The list, on a fresh session: the same pane, the same slot, the same words the card offers.
      final (listController, listCarrier) = await start(tester, [first, landedAnswer(corpus), next]);
      final title = activeWorkspace(listController.wall!)!.panes
          .firstWhere((pane) => pane.id == 1)
          .title;
      listController.openPlaces(3);
      await tester.pumpAndSettle();
      expect(
        find.text('Move "pane-03"…'),
        findsOneWidget,
        reason: 'the list is opened by the card\'s own action, and its header names the card it lists places for',
      );
      final row = find.text(placeWords(Edge.left, title));
      expect(
        row,
        findsOneWidget,
        reason: 'a row saying `left half of "$title"` was not drawn: the list is a pane\'s slots, in the wall\'s order '
            'of edges, and its words are the phone\'s',
      );
      await tester.tap(row);
      await tester.pumpAndSettle();

      expect(
        listCarrier.intents,
        [handCarrier.intents.single],
        reason: 'the hand and the list produced **different request lines**: a voice user and a hand user would land in '
            'different places, which is the whole reason the list exists (`docs/mobile-ui.md`, *accessibility*)',
      );
      expect(
        listCarrier.intents.single,
        dropLine(pane: 3, onto: 1, edge: 'left'),
        reason: 'the shared line is not the wall\'s `pane.drop { pane, onto, edge }`',
      );
    });

    test('the list offers exactly the slots the band can land in', () {
      final wall = projectionOf(first);
      final workspace = activeWorkspace(wall)!;
      final canvas = CellRect(0, 0, 336, 554);
      final laid = cells(workspace.tiling!, canvas);
      for (final pane in workspace.panes) {
        final rows = placesFor(workspace, pane.id);
        expect(
          rows,
          hasLength((workspace.panes.length - 1) * Edge.values.length),
          reason: 'a pane was offered ${rows.length} places: five rows for every **other** pane on the canvas, in the '
              'wall\'s order of edges (left, right, top, bottom, centre)',
        );
        for (final row in rows) {
          final where = probe(laid, row.onto, row.edge);
          final landing = dropFor(laid, dragged: pane.id, x: where.x, y: where.y);
          expect(
            landing == null ? null : DropIntent.of(pane.id, landing),
            row,
            reason: 'the row $row is not the slot a release at that band makes: the list and the drag would be two '
                'vocabularies for one landing, and `pane.drop` names a slot **of a pane** — "top right" of the canvas is '
                'not a name this vocabulary has',
          );
        }
      }
    });

    testWidgets('the list is cancelled by a row, and a cancel sends nothing', (tester) async {
      final (controller, carrier) = await start(tester, [first]);
      controller.openPlaces(3);
      await tester.pumpAndSettle();
      await tester.tap(find.text('cancel'));
      await tester.pumpAndSettle();
      expect(controller.placesPane, isNull, reason: 'cancel did not close the list');
      expect(carrier.intents, isEmpty, reason: 'cancel sent an intent');
      expect(
        find.byKey(canvasKey),
        findsOneWidget,
        reason: 'the canvas did not come back after the list was cancelled',
      );
    });
  });

  group('the gutter', () {
    testWidgets('one pane.resize on release, inside RATIO_RANGE', (tester) async {
      final (controller, carrier) = await start(tester, [first, landedAnswer(corpus), next]);
      final canvas = canvasRect(tester);
      final projected = controller.projected!;
      final root = gutters(projected, canvas).first;
      expect(root.split, Split.side, reason: 'the first projection is side by side at the root');

      // A drag to 60% of the split, which is the share the wall will be asked for.
      final at = root.start + root.span * 0.6;
      final share = Numbers.wall.shareAlong(at: at, start: root.start, span: root.span);
      final gesture = await grabGutter(
        tester,
        from: onCanvas(tester, root.at, root.rect.y + root.rect.h / 2),
        to: onCanvas(tester, at, root.rect.y + root.rect.h / 2),
      );

      expect(
        carrier.intents,
        isEmpty,
        reason: 'a gutter drag sent an intent before it was released: a drag is the phone\'s own drawing and the wall '
            'sees one `pane.resize` on release',
      );
      expect(
        (controller.drawn! as TilingSplit).ratio,
        share,
        reason: 'the preview drew a share the pointer did not ask for: `previewResize` is `Layout::set_ratio`, and the '
            'picture before release has to be the picture after',
      );
      expect(
        share,
        inInclusiveRange(Numbers.wall.ratioMin, Numbers.wall.ratioMax),
        reason: 'a share read off a gutter left RATIO_RANGE: `pane.resize` is clamped by the wall, and the preview may '
            'never name a ratio the wall refuses',
      );

      await gesture.up();
      await tester.pumpAndSettle();

      expect(
        carrier.intents,
        [resizeLine(pane: root.a, other: root.b, ratio: share)],
        reason: 'a gutter release is one `pane.resize`, naming **the two panes the gutter separates** and the share the '
            'preview drew — a phone that sent a ratio of its own would be a second clamp',
      );
    });

    testWidgets('a share past the end of the split is the clamp, not a number of the phone\'s', (tester) async {
      final (controller, carrier) = await start(tester, [first, landedAnswer(corpus), next]);
      final canvas = canvasRect(tester);
      final projected = controller.projected!;
      final root = gutters(projected, canvas).first;
      final gesture = await grabGutter(
        tester,
        from: onCanvas(tester, root.at, root.rect.y + root.rect.h / 2),
        to: onCanvas(tester, -60, root.rect.y + root.rect.h / 2),
      );
      expect(
        controller.gutter!.share,
        Numbers.wall.ratioMin,
        reason: 'a drag past the left edge of the canvas asked for ${controller.gutter!.share}: RATIO_RANGE is '
            '(src/config/layout.rs), and a pane with no share has no cell to be pressed in',
      );
      await gesture.up();
      await tester.pumpAndSettle();
      expect(
        carrier.intents,
        [resizeLine(pane: root.a, other: root.b, ratio: Numbers.wall.ratioMin)],
        reason: 'the release did not carry the clamped share the preview showed',
      );
    });

    testWidgets('a golden of a card in the air over the slot it will land in', (tester) async {
      final (controller, _) = await start(tester, [first]);
      final canvas = canvasRect(tester);
      final laid = cells(controller.projected!, canvas);
      final gesture = await liftCard(
        tester,
        from: onCanvas(tester, middle(laid, 3).x, middle(laid, 3).y),
        to: onCanvas(tester, probe(laid, 1, Edge.left).x, probe(laid, 1, Edge.left).y),
      );
      expect(controller.slotShaded, isTrue, reason: 'the golden would be a picture of a drag with no landing');
      await expectLater(
        find.byType(OverviewScreen),
        matchesGoldenFile('goldens/overview-dragging.png'),
      );
      // Let go without asking: the card settles back and nothing goes on the wire.
      controller.cancel();
      await tester.pump();
      await gesture.up();
      await tester.pumpAndSettle();
    });
  });
}

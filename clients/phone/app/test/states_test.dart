/// **The four states**, as `docs/mobile-ui.md` defines them (#264).
///
/// The screen (#255) already drew a projection; this file makes the *states* the app's, and each is one test:
///
/// | state | how the phone knows | what it draws |
/// | --- | --- | --- |
/// | `connecting` | it has never had a successful read | the chips absent, the canvas empty, `connecting…` — and **no diagram from a previous life** |
/// | `live` | the last read landed | the canvas, and the state line **says nothing at all** |
/// | `not connected` | a read did not land | the diagram it last read, dimmed, under its age |
/// | `update required` | a refusal with `kind: "version"` | one screen, the wall's words verbatim, the app's sentence, one button |
///
/// The projections are `fixtures/control/session.jsonl`'s, read by path through the harness: a screen proved against a
/// projection made up in a test is proved against itself. The clock is the test's, so no age here depends on the wall
/// clock.
library;

import 'dart:convert';

import 'package:arcons_phone/wire.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import 'package:arcons_phone_app/controller.dart';
import 'package:arcons_phone_app/overview.dart';
import 'package:arcons_phone_app/theme.dart';

import 'harness.dart';

/// The recording's version refusal, **with the wall's longer words on it**. The corpus sends none (`docs/control.md`
/// spells a refusal as `why` and `kind`), so the detail is the one field this file adds to a recorded answer — and it is
/// added to the wall's own sentence rather than written beside it.
String versionWithDetail(List<Exchange> corpus) {
  final object = jsonDecode(versionRefusal(corpus)) as Map<String, Object?>;
  return jsonEncode(<String, Object?>{
    ...object,
    'detail': 'this wall speaks version 2',
  });
}

void main() {
  final corpus = readCorpus();
  final lines = projectionLines(corpus);
  final now = DateTime.utc(2026, 11, 6, 12);

  group('connecting', () {
    testWidgets(
      'no chips, no canvas, `connecting…` — and never the diagram the last connection read',
      (tester) async {
        final live = FakeCarrier([lines.first]);
        final controller = OverviewController(clock: () => now);
        await controller.connect(live);
        await pumpScreen(tester, controller);
        expect(controller.state, PhoneState.live);
        // The names the previous life drew: every one of them must be gone while the new connection has read nothing.
        final before = controller.wall!.workspaces.map((workspace) => workspace.name).toList();
        expect(before, isNotEmpty);

        // A new connection, whose read is held: this is the moment a phone that drew its memory would draw the old
        // wall under a new socket.
        final next = FakeCarrier([lines.first])..holdAll();
        final connecting = controller.connect(next);
        await tester.pump();

        expect(controller.state, PhoneState.connecting);
        expect(
          find.text('connecting…'),
          findsOneWidget,
          reason: 'a phone that has read nothing on this connection says `connecting…` (`docs/mobile-ui.md`)',
        );
        expect(
          find.byKey(canvasKey),
          findsNothing,
          reason: '**a diagram from a previous life** was drawn while connecting: the arrangement on the glass belonged '
              'to a connection that is gone, and the four states exist to forbid exactly that',
        );
        for (final name in before) {
          expect(
            find.text(name),
            findsNothing,
            reason: 'the workspace chip "$name" was drawn while connecting: a phone that has read nothing has no '
                'workspaces to choose between, and a chip from a previous connection is that diagram in miniature',
          );
        }

        await expectLater(
          find.byType(OverviewScreen),
          matchesGoldenFile('goldens/overview-connecting.png'),
        );

        next.answer();
        await connecting;
        await tester.pumpAndSettle();
        expect(controller.state, PhoneState.live);
      },
    );
  });

  group('live', () {
    testWidgets('the canvas, and the state line says nothing at all', (tester) async {
      final controller = await liveController(
        tester,
        FakeCarrier([lines.first]),
        clock: () => now,
      );
      await pumpScreen(tester, controller);
      expect(controller.state, PhoneState.live);
      expect(
        controller.sentence,
        isNull,
        reason: 'on a live connection silence is the phone saying *this is the wall*: a state line with words in it is '
            'a label where there is nothing to label',
      );
      expect(
        find.text(''),
        findsOneWidget,
        reason: 'the state line drew something on a live connection: `docs/mobile-ui.md` gives `live` no words',
      );
      expect(find.byKey(canvasKey), findsOneWidget);
    });
  });

  group('not connected', () {
    testWidgets('the last diagram, dimmed, under its age — and this is not `connecting`', (tester) async {
      var clock = now;
      final controller = OverviewController(clock: () => clock);
      await controller.connect(FakeCarrier([lines.first, null]));
      await pumpScreen(tester, controller);
      clock = now.add(const Duration(seconds: 41));
      await controller.read();
      await tester.pumpAndSettle();

      expect(controller.state, PhoneState.notConnected);
      expect(
        find.text('connecting…'),
        findsNothing,
        reason: '`not connected` is not `connecting`: a phone that read a wall and lost it says so, and does not '
            'pretend it has never heard from one',
      );
      expect(
        find.text('not connected · last read 41 s ago'),
        findsOneWidget,
        reason: 'the age is the number a person can act on, and it is what separates a stale diagram from a lie',
      );
      expect(
        find.byKey(canvasKey),
        findsOneWidget,
        reason: 'the diagram was taken away: it is the only thing on the screen that says which pane is which, and '
            '`not connected` keeps it',
      );
      final dim = tester.widget<Opacity>(
        find.descendant(
          of: find.byKey(canvasKey),
          matching: find.byType(Opacity),
        ),
      );
      expect(
        dim.opacity,
        PhoneTokens.dimmed,
        reason: 'the last diagram was drawn at full strength under `not connected`: the dimming is half of what keeps '
            'it from passing for this moment\'s (`docs/mobile-ui.md`)',
      );
    });

    testWidgets('the canvas sends nothing until a read lands', (tester) async {
      final controller = OverviewController(clock: () => now);
      final carrier = FakeCarrier([lines.first, null]);
      await controller.connect(carrier);
      await pumpScreen(tester, controller);
      await controller.read();
      await tester.pumpAndSettle();
      expect(controller.state, PhoneState.notConnected);

      final before = carrier.sent.length;
      controller.openPlaces(controller.workspace!.panes.first.id);
      await controller.focusPane(controller.workspace!.panes.first.id);
      expect(
        carrier.sent.length,
        before,
        reason: 'the screen sent an intent while the phone had no read to compose it from: every intent names a pane '
            'by id, and a stale id is `no such pane` — a rule, and *do not retry until something else changes*',
      );
    });
  });

  group('update required', () {
    testWidgets('one screen: the wall\'s words, the app\'s sentence, one button', (tester) async {
      final controller = OverviewController(clock: () => now);
      final carrier = FakeCarrier([versionRefusal(corpus)]);
      await controller.connect(carrier);
      await pumpScreen(tester, controller, onUpdate: () {});

      expect(controller.state, PhoneState.updateRequired);
      expect(
        find.text('unsupported protocol version'),
        findsOneWidget,
        reason: 'the update screen must carry the wall\'s words **verbatim**: the rules are the wall\'s, and a '
            'paraphrase is a second copy of a rule',
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
        find.byType(Text),
        findsNWidgets(3),
        reason: 'the update screen drew more than the wall\'s words, the app\'s one sentence and the button: it is one '
            'screen, not a dialog over a diagram',
      );
      expect(
        find.byKey(canvasKey),
        findsNothing,
        reason: 'the diagram was drawn under the update screen: a version refusal replaces the phone\'s whole UI',
      );
      expect(
        carrier.sent,
        [readLine],
        reason: 'a version refusal was followed by another line: `docs/control.md` says send nothing more, update it',
      );

      await expectLater(
        find.byType(OverviewScreen),
        matchesGoldenFile('goldens/overview-update-required.png'),
      );
    });

    testWidgets('a detail the wire carries is drawn verbatim under the wall\'s words', (tester) async {
      final controller = OverviewController(clock: () => now);
      await controller.connect(FakeCarrier([versionWithDetail(corpus)]));
      await pumpScreen(tester, controller);
      expect(
        find.text('this wall speaks version 2'),
        findsOneWidget,
        reason: 'the refusal\'s `detail` was dropped: the wall said more than one sentence and the phone drew only '
            'the first (#264)',
      );
      expect(
        find.text('unsupported protocol version'),
        findsOneWidget,
        reason: 'the detail replaced the wall\'s own sentence instead of standing under it',
      );
    });
  });

  group('the watch', () {
    test('a publish is taken when it is the version after the one held', () async {
      final carrier = FakeCarrier([lines[0]]);
      final controller = OverviewController(clock: () => now);
      await controller.connect(carrier);
      final held = controller.wall!.seq;
      final publish = decodeAnswer(publishedLine(corpus, 1));
      expect(publish, isA<Projection>());

      await controller.published(publish as Projection);
      expect(
        controller.wall!.seq,
        held + 1,
        reason: 'the publish was not taken: a watch that draws nothing is a phone asking for a stream it ignores',
      );
      expect(controller.state, PhoneState.live);
      expect(
        carrier.sent,
        [readLine],
        reason: 'a publish that is the next version was answered with a read: the projection is the phone\'s truth, '
            'and it had just arrived',
      );
    });

    test('a gap is a read, not a reconstruction', () async {
      final carrier = FakeCarrier([lines[0], lines[3]]);
      final controller = OverviewController(clock: () => now);
      await controller.connect(carrier);
      // The corpus's last projection is seq 4 and the phone holds 0: a publish that skipped versions is read, not
      // patched (`docs/mobile-ui.md`: *one version after the one it holds, or it reads*).
      final publish = decodeAnswer(publishedLine(corpus, 5));
      await controller.published(publish as Projection);
      expect(
        carrier.sent,
        [readLine, readLine],
        reason: 'a publish that jumped versions was taken as a patch: `seq` is what the phone compares, and a gap is '
            'a read',
      );
      expect(controller.wall!.seq, projectionOf(lines[3]).seq);
    });
  });
}

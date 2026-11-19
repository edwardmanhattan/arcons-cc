/// **The app's shell**, held to #264: the relay, the key read on start, the read-then-watch, and the state that is not a
/// crash.
///
/// The `main` this replaces replayed `fixtures/control/session.jsonl`; the falsifier this file is written against is
/// **the corpus still on the runtime path**, so one test reads `lib/` and says no file there opens a fixture. The
/// carrier is [FakeRelay]: the recording's projections, the claim's two lines from `docs/control.md`, and a clock the
/// test owns.
library;

import 'dart:convert';
import 'dart:io';

import 'package:arcons_phone/wire.dart' show Projection, decodeAnswer, protocol;
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import 'package:arcons_phone_app/controller.dart' show noRelayConfigured;
import 'package:arcons_phone_app/main.dart' show PhoneApp;
import 'package:arcons_phone_app/overview.dart';
import 'package:arcons_phone_app/pairing.dart' show pairRelayKey;
import 'package:arcons_phone_app/shell.dart' show parseRelay;

import 'harness.dart';

/// A relay URL the fake never dials: nothing in this file opens a socket.
final Uri appRelay = Uri.parse('tcp://relay.invalid:7000');

void main() {
  final corpus = readCorpus();
  final first = projectionLine(corpus);
  final now = DateTime.utc(2026, 11, 6, 12);

  group('the relay the build dials', () {
    test('tcp:// or tls:// with an address, and anything else is no relay at all', () {
      expect(
        parseRelay(''),
        isNull,
        reason: 'an empty ARCONS_RELAY is a phone with no relay configured, which is a state and not a crash',
      );
      expect(parseRelay('not a relay'), isNull);
      expect(
        parseRelay('wss://relay.example:7000'),
        isNull,
        reason: 'a scheme this build cannot dial was accepted: `RelayCarrier` dials `tcp://` and `tls://`, and a '
            'third is a ticket rather than a guess',
      );
      expect(
        parseRelay('tcp://relay.example'),
        isNull,
        reason: 'a relay with no port was accepted: `RelayCarrier` refuses one by name, and a phone that passed it on '
            'would crash where the note says it should draw `not connected`',
      );
      expect(parseRelay('tcp://relay.example:7000'), isNotNull);
      expect(parseRelay('tls://relay.example:7000'), isNotNull);
      expect(
        parseRelay('tcp://relay.example:7000'),
        isA<Uri>(),
        reason: 'a relay URL is deployment configuration, never a secret (`INV-08`), and it is parsed rather than '
            'printed',
      );
    });
  });

  group('no relay configured', () {
    testWidgets('is `not connected` with one sentence, not a crash and not a pairing screen', (tester) async {
      final keys = RecordingKeyStore(testDeviceKey);
      await pumpApp(
        tester,
        PhoneApp(
          relay: null,
          keys: keys,
          clock: () => now,
          openRelay: (relay, keys) => throw StateError(
            'the app dialled a relay it does not have: with no relay there is nothing to dial',
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.text('not connected · $noRelayConfigured'),
        findsOneWidget,
        reason: 'a phone with no relay did not draw `not connected · no relay is configured`: the line names the '
            'field, says what the phone knows, and never prints the value (`INV-08`)',
      );
      expect(
        find.byKey(canvasKey),
        findsNothing,
        reason: 'a phone that has read nothing drew a diagram, or a canvas: `connecting` and `not connected` both draw '
            'nothing when there is nothing to draw',
      );
      expect(
        find.text('Pair with a wall'),
        findsNothing,
        reason: 'a phone with no relay was shown the pairing screen: a claim needs a wire, and a screen that offered '
            'one would promise a pairing it cannot make',
      );
      for (final text in screenText(tester)) {
        expect(
          text.contains('relay.invalid'),
          isFalse,
          reason: 'INV-08: the relay\'s address reached a widget. A deployment\'s host is a measurement of somebody\'s '
              'network, and the sentence names the field instead',
        );
      }
    });
  });

  group('the relay field, before a key (#271)', () {
    testWidgets('no key and no relay is still a pairing screen, with an empty field and a typed path', (tester) async {
      await pumpApp(
        tester,
        PhoneApp(
          relay: null,
          keys: RecordingKeyStore(),
          clock: () => now,
          openRelay: (relay, keys) => throw StateError(
            'the pairing screen dialled before a claim: no relay has been chosen yet',
          ),
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.text('Pair with a wall'),
        findsOneWidget,
        reason: 'a phone with no key and no compiled relay was refused a pairing screen: #271 gives it a field for the '
            'address, and the typed six digits still have to reach a wall',
      );
      final field = tester.widget<TextField>(find.byKey(pairRelayKey));
      expect(
        field.controller!.text,
        isEmpty,
        reason: 'a build with no compiled relay did not start with an empty field',
      );
    });

    testWidgets('a remembered relay is the field\'s value and the relay a paired phone dials', (tester) async {
      final remembered = RecordingRelayPreferences('tcp://remembered.invalid:7000');
      final fake = FakeRelay([first]);
      Uri? dialled;
      await pumpApp(
        tester,
        PhoneApp(
          relay: null,
          keys: RecordingKeyStore(testDeviceKey),
          preferences: remembered,
          clock: () => now,
          openRelay: (relay, keys) {
            dialled = relay;
            fake.keys = keys;
            return fake;
          },
        ),
      );
      await tester.pumpAndSettle();

      expect(
        dialled,
        Uri.parse('tcp://remembered.invalid:7000'),
        reason: 'a paired phone ignored the relay the operator scanned or typed: the remembered value is the field\'s '
            'value on the next run (#271)',
      );
      expect(find.byKey(canvasKey), findsOneWidget);
    });
  });

  group('the app', () {
    testWidgets('starts in `connecting`, draws after the read, and asks for the watch only then', (tester) async {
      final fake = FakeRelay([first])..holdAll();
      final keys = RecordingKeyStore(testDeviceKey);
      await pumpApp(
        tester,
        PhoneApp(
          relay: appRelay,
          keys: keys,
          clock: () => now,
          openRelay: (relay, keys) {
            fake.keys = keys;
            return fake;
          },
        ),
      );
      await tester.pump();
      await tester.pump();

      expect(
        find.text('connecting…'),
        findsOneWidget,
        reason: 'the app did not start in `connecting`: the four states begin there, and a phone that has read '
            'nothing says so',
      );
      expect(find.byKey(canvasKey), findsNothing);
      expect(
        fake.watches,
        0,
        reason: 'a watch was asked for before the read landed: a watch is a connection\'s baseline, and the carrier '
            'refuses the ask on a connection it has not read on',
      );

      fake.answer();
      await tester.pumpAndSettle();

      expect(find.text('connecting…'), findsNothing);
      expect(find.byKey(canvasKey), findsOneWidget);
      expect(
        fake.sent.first,
        readLine,
        reason: 'the first line the app sent was not the read: `mobile-remote.md` decision 1 says read, then act',
      );
      expect(
        fake.watches,
        1,
        reason: 'the app read the wall and never asked for the watch: the shell is the join, and the watch is half of '
            'it (#221, #264)',
      );
    });

    testWidgets('sends what the screen sends, through the client', (tester) async {
      final fake = FakeRelay([first, first]);
      await pumpApp(
        tester,
        PhoneApp(
          relay: appRelay,
          keys: RecordingKeyStore(testDeviceKey),
          clock: () => now,
          openRelay: (relay, keys) {
            fake.keys = keys;
            return fake;
          },
        ),
      );
      await tester.pumpAndSettle();

      final wall = projectionOf(first);
      final card = layOut(wall, canvasRect(tester)).first;
      await tester.tapAt(
        onCanvas(
          tester,
          card.rect.x + card.rect.w / 2,
          card.rect.y + card.rect.h / 2,
        ),
      );
      await tester.pumpAndSettle();

      expect(
        fake.intents,
        [
          jsonEncode(<String, Object?>{
            'v': protocol,
            'intent': <String, Object?>{'verb': 'pane.focus', 'pane': card.pane.id},
          }),
        ],
        reason: 'the press on a card did not reach the wall as `pane.focus`: the screen\'s intents go through the '
            'controller #257 built, and a shell that wired its own would be a second door',
      );
    });

    testWidgets('a dropped read dims the last diagram under its age', (tester) async {
      var clock = now;
      final fake = FakeRelay([first, null]);
      await pumpApp(
        tester,
        PhoneApp(
          relay: appRelay,
          keys: RecordingKeyStore(testDeviceKey),
          clock: () => clock,
          openRelay: (relay, keys) {
            fake.keys = keys;
            return fake;
          },
        ),
      );
      await tester.pumpAndSettle();
      expect(find.byKey(canvasKey), findsOneWidget);

      final card = layOut(projectionOf(first), canvasRect(tester)).first;
      clock = now.add(const Duration(seconds: 41));
      await tester.tapAt(
        onCanvas(
          tester,
          card.rect.x + card.rect.w / 2,
          card.rect.y + card.rect.h / 2,
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.text('not connected · last read 41 s ago'),
        findsOneWidget,
        reason: 'a read that did not land left the state line saying nothing, or an age that did not come from the '
            'test\'s clock',
      );
      expect(
        find.byKey(canvasKey),
        findsOneWidget,
        reason: 'the last diagram was taken away on a drop: it is the only thing on the screen that says which pane '
            'is which, and the note keeps it, dimmed',
      );
    });

    testWidgets('a version refusal is one screen, and nothing more is sent', (tester) async {
      final fake = FakeRelay([versionRefusal(corpus)]);
      await pumpApp(
        tester,
        PhoneApp(
          relay: appRelay,
          keys: RecordingKeyStore(testDeviceKey),
          clock: () => now,
          openRelay: (relay, keys) {
            fake.keys = keys;
            return fake;
          },
        ),
      );
      await tester.pumpAndSettle();

      expect(find.text('unsupported protocol version'), findsOneWidget);
      expect(find.text('Update'), findsOneWidget);
      expect(
        fake.watches,
        0,
        reason: 'a watch was asked for on a wall whose version this build cannot speak: a version refusal ends the '
            'session',
      );
      final sent = List<String>.of(fake.sent);
      await tester.tap(find.text('Update'));
      await tester.pump();
      expect(
        fake.sent,
        sent,
        reason: 'the update screen\'s one button sent something: it is not a retry, and `docs/control.md` says send '
            'nothing more in the session',
      );
    });

    testWidgets('draws the wall\'s publishes, one version after the one it holds', (tester) async {
      final fake = FakeRelay([first]);
      await pumpApp(
        tester,
        PhoneApp(
          relay: appRelay,
          keys: RecordingKeyStore(testDeviceKey),
          clock: () => now,
          openRelay: (relay, keys) {
            fake.keys = keys;
            return fake;
          },
        ),
      );
      await tester.pumpAndSettle();

      final publish = decodeAnswer(publishedLine(corpus, 1));
      expect(publish, isA<Projection>());
      fake.publish(publish as Projection);
      await tester.pumpAndSettle();

      final next = projectionOf(publishedLine(corpus, 1));
      expect(
        find.bySemanticsLabel(spokenCanvas(next, layOut(next, canvasRect(tester)))),
        findsOneWidget,
        reason: 'the watch was granted and its publish never reached the canvas: the phone asked for the wall\'s '
            'changes and drew the one it had',
      );
    });
  });

  group('the runtime path', () {
    test('the app reads the wire, not the corpus', () {
      final files = Directory('lib')
          .listSync(recursive: true)
          .whereType<File>()
          .where((file) => file.path.endsWith('.dart'))
          .toList();
      expect(files, isNotEmpty, reason: 'no Dart sources under lib/: this test would pass on nothing');
      for (final file in files) {
        final source = file.readAsStringSync();
        expect(
          source.contains('dart:io'),
          isFalse,
          reason: '${file.path} imports dart:io: the app\'s runtime reads a relay, and a file on the runtime path is '
              'the corpus back under another name (#264, falsified by *the corpus still on the app\'s runtime path*)',
        );
        expect(
          source.contains('readAsStringSync'),
          isFalse,
          reason: '${file.path} reads a file at runtime: the recording in fixtures/control/ is the tests\' fixture, and '
              'a phone that replayed it would draw a wall nobody has',
        );
        expect(
          source.contains('RecordingCarrier'),
          isFalse,
          reason: '${file.path} still holds the corpus-replaying carrier: #264 replaces it with the relay',
        );
      }
    });

    test('`main` reads the relay from the environment and nothing else', () {
      final source = File('lib/main.dart').readAsStringSync();
      expect(
        source.contains("String.fromEnvironment(relayVariable)") ||
            source.contains("String.fromEnvironment('ARCONS_RELAY')"),
        isTrue,
        reason: 'the relay is not read from `ARCONS_RELAY`: deployment configuration is read from the environment or '
            'not at all (`AGENTS.md`, and a URL in the tree is how one becomes committed configuration)',
      );
      expect(
        source.contains('http://') || source.contains('https://'),
        isFalse,
        reason: 'a relay URL was written into the tree: the address is the deployment\'s, and a test can pass one',
      );
    });
  });
}

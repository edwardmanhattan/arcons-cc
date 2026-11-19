/// **The pairing screen**, held to `docs/mobile-ui.md`'s section (#264, #271).
///
/// A phone with no key has no wall to read: the six digits are a picture on the wall in the room, the relay is where
/// the claim goes, the name is the row the wall will write, and the one button sends `pair.claim` — the only frame a
/// phone sends before it has a key (#262). The QR is additive: a scan fills the code and the relay, and typing is still
/// the path for a phone with no camera.
///
/// The key the claim hands back is a secret, and this file is as careful as the carrier's own tests: [testDeviceKey] is
/// a test literal that reaches no wall, and one test formats every string the screen would draw to assert it never
/// appears (`INV-08`). The scanner is [FakeScanner]: **no test here constructs a camera**.
library;

import 'package:arcons_phone/client.dart' show Refusal;
import 'package:arcons_phone/pairing.dart' show PairingRefusal;
import 'package:arcons_phone/wire.dart' show Paired, RefusalKind;
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import 'package:arcons_phone_app/main.dart' show PhoneApp;
import 'package:arcons_phone_app/overview.dart' show canvasKey;
import 'package:arcons_phone_app/pairing.dart';
import 'package:arcons_phone_app/scanner.dart'
    show CameraRefusal, CameraUnavailable;

import 'harness.dart';

/// A relay URL the fake never dials: the pairing screen's claim is answered by [FakeRelay], and no socket is opened.
final Uri pairingRelay = Uri.parse('tcp://relay.invalid:7000');

/// The payload #270 draws on the wall's glass, verbatim: the `pairing` scene's relay and digits.
const String wallPayload =
    'arcons://pair?relay=tcp://relay.arcons.test:7030&code=481902';

void main() {
  final corpus = readCorpus();
  final projection = projectionLine(corpus);
  final now = DateTime.utc(2026, 11, 6, 12);

  /// A fresh screen, a fresh store, a fake camera and a relay whose claim answers are scripted by the test.
  Future<
    ({
      FakeRelay relay,
      RecordingKeyStore keys,
      FakeScanner scanner,
      RecordingRelayPreferences preferences,
    })
  >
  pumpPairing(
    WidgetTester tester, {
    Uri? relay,
    List<Object> answers = const [],
    FakeScanner? scanner,
    RecordingRelayPreferences? preferences,
  }) async {
    final keys = RecordingKeyStore();
    final fake = FakeRelay(const [])..claimAnswers.addAll(answers);
    final camera = scanner ?? FakeScanner();
    final remembered = preferences ?? RecordingRelayPreferences();
    await pumpApp(
      tester,
      MaterialApp(
        debugShowCheckedModeBanner: false,
        home: Scaffold(
          body: PairingScreen(
            relay: relay,
            keys: keys,
            scanner: camera,
            preferences: remembered,
            openRelay: (relay, keys) {
              fake.keys = keys;
              return fake;
            },
            onPaired: (_) {},
          ),
        ),
      ),
    );
    return (relay: fake, keys: keys, scanner: camera, preferences: remembered);
  }

  group('the screen', () {
    testWidgets(
      'six digit boxes, the relay, the name, a scan and one Pair — and no seventh control',
      (tester) async {
        final screen = await pumpPairing(tester, relay: pairingRelay);

        expect(find.text('Pair with a wall'), findsOneWidget);
        for (var index = 0; index < 6; index++) {
          expect(
            find.byKey(pairDigitKey(index)),
            findsOneWidget,
            reason:
                'the pairing screen draws six digit boxes (`docs/mobile-ui.md`): box $index is missing, and five or '
                'seven is not the code a wall draws',
          );
        }
        expect(
          find.text('Relay'),
          findsOneWidget,
          reason: 'the relay field has no label: the field is the address a claim goes to, and the note gives it a row',
        );
        expect(
          relayField(tester).controller!.text,
          pairingRelay.toString(),
          reason: 'the relay field was not prefilled from the compiled default (#271)',
        );
        expect(
          find.text('Name this phone'),
          findsOneWidget,
          reason: 'the name field says what it is for; the wall\'s row will hold what is typed under it',
        );
        expect(
          nameField(tester).controller!.text,
          defaultDeviceName,
          reason:
              'the name is **defaulted**: an operator with a code in hand should not have to invent a device name '
              'before the Pair button works',
        );
        expect(
          find.byKey(pairScanKey),
          findsOneWidget,
          reason:
              'the pairing screen has no Scan affordance (#271): the wall draws a QR and a phone with a camera '
              'should be able to read it',
        );
        expect(find.text("Scan the wall's QR"), findsOneWidget);
        expect(find.byKey(pairButtonKey), findsOneWidget);
        expect(find.text('Pair'), findsOneWidget);
        expect(
          find.byType(TextField),
          findsNWidgets(3),
          reason:
              'the screen has one field for the code, one for the relay and one for the name: a seventh digit or a '
              'second button is a control the note does not give',
        );
        expect(
          screen.scanner.scans,
          0,
          reason:
              'the screen opened a camera without a press: a widget test would have to own a camera for that, and an '
              'operator did not ask for one',
        );

        await expectLater(
          find.byType(PairingScreen),
          matchesGoldenFile('goldens/pairing-empty.png'),
        );
      },
    );

    testWidgets('the Pair button sends nothing until the code is six digits', (
      tester,
    ) async {
      final screen = await pumpPairing(
        tester,
        relay: pairingRelay,
        answers: [Paired(name: defaultDeviceName, key: testDeviceKey)],
      );
      await enterCode(tester, '48190');
      await tester.tap(find.byKey(pairButtonKey));
      await tester.pump();
      expect(
        screen.relay.claims,
        isEmpty,
        reason:
            'a claim went out with five digits: the wall draws six, `pair.claim` takes six, and a phone that sent '
            'a short code would spend an attempt on a code it could not have read',
      );
    });

    testWidgets(
      'the Pair button sends nothing until the relay can be dialled',
      (tester) async {
        final screen = await pumpPairing(
          tester,
          relay: pairingRelay,
          answers: [Paired(name: defaultDeviceName, key: testDeviceKey)],
        );
        await enterCode(tester, '481902');
        await tester.enterText(
          find.byKey(pairRelayKey),
          'wss://relay.invalid:7000',
        );
        await tester.pump();
        await tester.tap(find.byKey(pairButtonKey));
        await tester.pump();
        expect(
          screen.relay.claims,
          isEmpty,
          reason: 'a claim went to a relay this build cannot dial: `parseRelay` refuses it, and the button must too',
        );
      },
    );
  });

  group('a scan', () {
    testWidgets(
      'fills the code and the relay, and `pair.claim` sends exactly the digits the wall drew',
      (tester) async {
        final screen = await pumpPairing(
          tester,
          relay: null,
          scanner: FakeScanner(wallPayload),
          answers: [Paired(name: defaultDeviceName, key: testDeviceKey)],
        );
        expect(
          relayField(tester).controller!.text,
          isEmpty,
          reason: 'a build with no compiled relay did not start with an empty field',
        );

        await tester.tap(find.byKey(pairScanKey));
        await tester.pumpAndSettle();

        expect(screen.scanner.scans, 1);
        expect(
          codeField(tester).controller!.text,
          '481902',
          reason: 'the scan did not fill the six digits: the wall\'s QR carries the code beside the relay (#270)',
        );
        final expectedRelay = Uri.parse('tcp://relay.arcons.test:7030');
        expect(
          relayField(tester).controller!.text,
          expectedRelay.toString(),
          reason: 'the scan did not fill the relay it names: a code without its relay is a code with no address',
        );
        expect(
          screen.preferences.remembered,
          contains(expectedRelay.toString()),
          reason: 'the scanned relay was not kept in preferences: it is a URL, not a secret, and the next run needs it',
        );

        await tester.tap(find.byKey(pairButtonKey));
        await tester.pumpAndSettle();

        expect(
          screen.relay.claims,
          [(code: '481902', name: defaultDeviceName)],
          reason: 'the claim did not carry exactly the digits the wall drew (#270 holds the other end)',
        );
        expect(
          screen.keys.remembered,
          [testDeviceKey],
          reason: 'the key the wall handed back did not reach the key store, or reached it twice',
        );
      },
    );

    testWidgets(
      'a scan wins over the field, and the field wins over the compiled default',
      (tester) async {
        final compiled = Uri.parse('tcp://compiled.invalid:7000');
        final screen = await pumpPairing(
          tester,
          relay: compiled,
          scanner: FakeScanner(wallPayload),
          answers: [
            Paired(name: defaultDeviceName, key: testDeviceKey),
            Paired(name: defaultDeviceName, key: testDeviceKey),
          ],
        );

        // First: the field is edited away from the compiled default, and the claim goes where the field says.
        const typed = 'tcp://typed.invalid:7001';
        await tester.enterText(find.byKey(pairRelayKey), typed);
        await enterCode(tester, '111111');
        await tester.tap(find.byKey(pairButtonKey));
        await tester.pumpAndSettle();
        expect(
          screen.relay.claims,
          [(code: '111111', name: defaultDeviceName)],
          reason: 'the claim did not go to the relay in the field: the field wins over the compiled default',
        );

        // Second: a scan overwrites both fields, so the next claim goes to the wall's own relay and digits.
        await tester.tap(find.byKey(pairScanKey));
        await tester.pumpAndSettle();
        expect(codeField(tester).controller!.text, '481902');
        expect(
          relayField(tester).controller!.text,
          'tcp://relay.arcons.test:7030',
        );
        await tester.tap(find.byKey(pairButtonKey));
        await tester.pumpAndSettle();
        expect(
          screen.relay.claims.last,
          (code: '481902', name: defaultDeviceName),
          reason: 'a scan did not win over the field: the QR\'s own digits and relay must replace what was there',
        );
      },
    );

    testWidgets(
      'a payload the parser refuses names the refusal and fills nothing',
      (tester) async {
        final scan = FakeScanner(
          'arcons://pair?relay=wss%3A%2F%2Frelay.arcons.test%3A7000&code=481902',
        );
        await pumpPairing(tester, relay: pairingRelay, scanner: scan);
        await enterCode(tester, '123456');
        await tester.tap(find.byKey(pairScanKey));
        await tester.pumpAndSettle();

        expect(
          find.textContaining(PairingRefusal.relayCannotBeDialled.name),
          findsOneWidget,
          reason: 'a refused payload did not say which name refused it: the screen would be guessing at what the QR was',
        );
        expect(
          codeField(tester).controller!.text,
          '123456',
          reason: 'a refused payload changed the code: a bad QR must not spend or clear the digits the operator typed',
        );
        expect(
          relayField(tester).controller!.text,
          pairingRelay.toString(),
          reason: 'a refused payload changed the relay: a bad QR must not point the claim somewhere else',
        );

        await expectLater(
          find.byType(PairingScreen),
          matchesGoldenFile('goldens/pairing-payload-refused.png'),
        );
      },
    );

    testWidgets(
      'a denied camera says so in words and leaves the typed path working',
      (tester) async {
        final scan = FakeScanner('unreached')
          ..failure = const CameraUnavailable(CameraRefusal.permissionDenied);
        final screen = await pumpPairing(
          tester,
          relay: pairingRelay,
          scanner: scan,
          answers: [Paired(name: defaultDeviceName, key: testDeviceKey)],
        );
        await tester.tap(find.byKey(pairScanKey));
        await tester.pumpAndSettle();

        expect(
          find.text(CameraRefusal.permissionDenied.sentence),
          findsOneWidget,
          reason: 'a denied camera did not say so in words: the operator would think the QR was wrong',
        );
        expect(
          relayField(tester).controller!.text,
          pairingRelay.toString(),
          reason: 'a denied camera changed the relay field',
        );

        await enterCode(tester, '481902');
        await tester.tap(find.byKey(pairButtonKey));
        await tester.pumpAndSettle();
        expect(
          screen.relay.claims,
          [(code: '481902', name: defaultDeviceName)],
          reason: 'the typed path stopped working after the camera was denied: typing is the fallback the note keeps',
        );

        await expectLater(
          find.byType(PairingScreen),
          matchesGoldenFile('goldens/pairing-camera-denied.png'),
        );
      },
    );
  });

  group('pairing', () {
    testWidgets(
      'the code and the name go to the wall, the key goes to the store, and the overview follows',
      (tester) async {
        final keys = RecordingKeyStore();
        final fake = FakeRelay([projection])
          ..claimAnswers.add(
            Paired(name: defaultDeviceName, key: testDeviceKey),
          );
        await pumpApp(
          tester,
          PhoneApp(
            relay: pairingRelay,
            keys: keys,
            scanner: FakeScanner(),
            clock: () => now,
            openRelay: (relay, keys) {
              fake.keys = keys;
              return fake;
            },
          ),
        );
        await tester.pumpAndSettle();
        expect(
          find.text('Pair with a wall'),
          findsOneWidget,
          reason:
              'a phone with no device key was not shown the pairing screen: it has nothing to read and the overview '
              'would be a diagram it never had',
        );

        await enterCode(tester, '481902');
        await tester.tap(find.byKey(pairButtonKey));
        await tester.pumpAndSettle();

        expect(
          fake.claims,
          [(code: '481902', name: defaultDeviceName)],
          reason: 'the claim did not carry the six digits and the name the screen showed: `pair.claim {code, name}`, once',
        );
        expect(
          keys.remembered,
          [testDeviceKey],
          reason:
              'the key the wall handed back did not reach the key store, or reached it twice: `claim` is the one '
              'write of a secret the phone makes (#262, INV-08)',
        );
        expect(
          find.text('Pair with a wall'),
          findsNothing,
          reason:
              'the pairing screen stayed up after a successful claim: the phone is paired, and the overview is the '
              'next screen',
        );
        expect(find.byKey(canvasKey), findsOneWidget);
        expect(
          fake.watches,
          1,
          reason:
              'the shell did not ask for the watch after the read that followed the claim: a watch is asked only '
              'after a read has landed (`mobile-ui.md`, the four states)',
        );

        for (final text in screenText(tester)) {
          expect(
            text.contains(testDeviceKey),
            isFalse,
            reason:
                'INV-08: the device key reached a widget\'s text. The key lives in the key store, in `device` on the '
                'wire, and in nothing a screenshot can read',
          );
          expect(
            text.contains('481902'),
            isFalse,
            reason:
                'the six digits are not shown again after the claim: a code that survives its two minutes is a code '
                'that outlives them (`docs/mobile-ui.md`, pairing)',
          );
        }
      },
    );
  });

  group('the three refusals, and their steps', () {
    testWidgets(
      '`no pairing is open` empties the code field — and a code burnt by five wrong guesses is that same refusal',
      (tester) async {
        final screen = await pumpPairing(
          tester,
          relay: pairingRelay,
          answers: [const Refusal(noPairingOpen, RefusalKind.rule)],
        );
        await enterCode(tester, '481902');
        await tester.tap(find.byKey(pairButtonKey));
        await tester.pumpAndSettle();

        expect(screen.relay.claims.single.code, '481902');
        expect(
          codeField(tester).controller!.text,
          isEmpty,
          reason:
              '`no pairing is open` left the code in the field: nothing is open, the two minutes are gone, the '
              'code was spent or it was burnt — and the operator has to ask the wall for a fresh one',
        );
        expect(
          codeField(tester).focusNode!.hasFocus,
          isTrue,
          reason: 'the code field was emptied but not returned to: the step is *returns to the code field, emptied*',
        );
        expect(
          find.text(codeExpiredSentence),
          findsOneWidget,
          reason:
              'the screen did not say the code expired. A burnt code is `no pairing is open` (#262) and takes '
              'this same step: the phone invents no sentence for it',
        );

        await expectLater(
          find.byType(PairingScreen),
          matchesGoldenFile('goldens/pairing-burnt-code.png'),
        );
      },
    );

    testWidgets('`that code is not the code` keeps it and selects the digits', (
      tester,
    ) async {
      await pumpPairing(
        tester,
        relay: pairingRelay,
        answers: [const Refusal(notTheCode, RefusalKind.rule)],
      );
      await enterCode(tester, '481902');
      await tester.tap(find.byKey(pairButtonKey));
      await tester.pumpAndSettle();

      expect(
        codeField(tester).controller!.text,
        '481902',
        reason: 'the wrong code was emptied: the digits were the operator\'s, and the wall said one of them is wrong',
      );
      final selection = codeField(tester).controller!.selection;
      expect(
        selection.baseOffset,
        0,
        reason:
            'the digits were not selected after `that code is not the code`: the step is *keeps the field, selects '
            'the digits*',
      );
      expect(selection.extentOffset, 6);
      expect(codeField(tester).focusNode!.hasFocus, isTrue);
      expect(find.text(wrongCodeSentence), findsOneWidget);
    });

    testWidgets(
      '`that device is already paired` keeps the name field focused',
      (tester) async {
        await pumpPairing(
          tester,
          relay: pairingRelay,
          answers: [const Refusal(alreadyPaired, RefusalKind.rule)],
        );
        await enterCode(tester, '481902');
        await tester.enterText(find.byKey(pairNameKey), 'second phone');
        await tester.tap(find.byKey(pairButtonKey));
        await tester.pumpAndSettle();

        expect(
          nameField(tester).focusNode!.hasFocus,
          isTrue,
          reason: 'a taken name left the focus on the code: the code was right, and the name is what has to change',
        );
        expect(
          codeField(tester).controller!.text,
          '481902',
          reason: 'the code was cleared on `that device is already paired`: the code was not the problem',
        );
        expect(find.text(nameTakenSentence), findsOneWidget);
      },
    );

    testWidgets(
      'a refusal this screen has no row for is the wall\'s words verbatim, and no step',
      (tester) async {
        await pumpPairing(
          tester,
          relay: pairingRelay,
          answers: [
            const Refusal('the wall is busy pairing', RefusalKind.rule),
          ],
        );
        await enterCode(tester, '481902');
        await tester.tap(find.byKey(pairButtonKey));
        await tester.pumpAndSettle();

        expect(
          find.text('the wall is busy pairing'),
          findsOneWidget,
          reason:
              'a refusal outside the note\'s three was given a sentence of the phone\'s own: a step nobody wrote is a '
              'rule invented at the screen',
        );
        expect(codeField(tester).controller!.text, '481902');
      },
    );
  });
}

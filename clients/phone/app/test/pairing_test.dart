/// **The pairing screen**, held to `docs/mobile-ui.md`'s section (#264).
///
/// A phone with no key has no wall to read: the six digits are a picture on the wall in the room, the name is the row
/// the wall will write, and the one button sends `pair.claim` — the only frame a phone sends before it has a key
/// (#262). The refusals are **steps**, not prose: the wall names what is wrong, and the note names the field that moves.
///
/// The key the claim hands back is a secret, and this file is as careful as the carrier's own tests: [testDeviceKey] is
/// a test literal that reaches no wall, and one test formats every string the screen would draw to assert it never
/// appears (`INV-08`).
library;

import 'package:arcons_phone/client.dart' show Refusal;
import 'package:arcons_phone/wire.dart' show Paired, RefusalKind;
import 'package:flutter_test/flutter_test.dart';
import 'package:material_ui/material_ui.dart';

import 'package:arcons_phone_app/main.dart' show PhoneApp;
import 'package:arcons_phone_app/overview.dart' show canvasKey;
import 'package:arcons_phone_app/pairing.dart';

import 'harness.dart';

/// A relay URL the fake never dials: the pairing screen's claim is answered by [FakeRelay], and no socket is opened.
final Uri pairingRelay = Uri.parse('tcp://relay.invalid:7000');

void main() {
  final corpus = readCorpus();
  final projection = projectionLine(corpus);
  final now = DateTime.utc(2026, 11, 6, 12);

  /// A fresh screen, a fresh store and a relay whose claim answers are scripted by the test.
  Future<(FakeRelay, RecordingKeyStore)> pumpPairing(
    WidgetTester tester, {
    List<Object> answers = const [],
  }) async {
    final keys = RecordingKeyStore();
    final fake = FakeRelay(const [])..claimAnswers.addAll(answers);
    await pumpApp(
      tester,
      MaterialApp(
        debugShowCheckedModeBanner: false,
        home: Scaffold(
          body: PairingScreen(
            relay: pairingRelay,
            keys: keys,
            openRelay: (relay, keys) {
              fake.keys = keys;
              return fake;
            },
            onPaired: (_) {},
          ),
        ),
      ),
    );
    return (fake, keys);
  }

  group('the screen', () {
    testWidgets('six digit boxes, the name defaulted, one Pair button — and no seventh control', (tester) async {
      await pumpPairing(tester);

      expect(find.text('Pair with a wall'), findsOneWidget);
      for (var index = 0; index < 6; index++) {
        expect(
          find.byKey(pairDigitKey(index)),
          findsOneWidget,
          reason: 'the pairing screen draws six digit boxes (`docs/mobile-ui.md`): box $index is missing, and five or '
              'seven is not the code a wall draws',
        );
      }
      expect(
        find.text('Name this phone'),
        findsOneWidget,
        reason: 'the name field says what it is for; the wall\'s row will hold what is typed under it',
      );
      expect(
        nameField(tester).controller!.text,
        defaultDeviceName,
        reason: 'the name is **defaulted**: an operator with a code in hand should not have to invent a device name '
            'before the Pair button works',
      );
      expect(find.byKey(pairButtonKey), findsOneWidget);
      expect(find.text('Pair'), findsOneWidget);
      expect(
        find.byType(TextField),
        findsNWidgets(2),
        reason: 'the screen has one field for the code and one for the name: a seventh digit or a second button is a '
            'control the note does not give',
      );

      await expectLater(
        find.byType(PairingScreen),
        matchesGoldenFile('goldens/pairing-empty.png'),
      );
    });

    testWidgets('the Pair button sends nothing until the code is six digits', (tester) async {
      final (fake, _) = await pumpPairing(
        tester,
        answers: [Paired(name: defaultDeviceName, key: testDeviceKey)],
      );
      await enterCode(tester, '48190');
      await tester.tap(find.byKey(pairButtonKey));
      await tester.pump();
      expect(
        fake.claims,
        isEmpty,
        reason: 'a claim went out with five digits: the wall draws six, `pair.claim` takes six, and a phone that sent '
            'a short code would spend an attempt on a code it could not have read',
      );
    });
  });

  group('pairing', () {
    testWidgets('the code and the name go to the wall, the key goes to the store, and the overview follows', (tester) async {
      final keys = RecordingKeyStore();
      final fake = FakeRelay([projection])
        ..claimAnswers.add(Paired(name: defaultDeviceName, key: testDeviceKey));
      await pumpApp(
        tester,
        PhoneApp(
          relay: pairingRelay,
          keys: keys,
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
        reason: 'a phone with no device key was not shown the pairing screen: it has nothing to read and the overview '
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
        reason: 'the key the wall handed back did not reach the key store, or reached it twice: `claim` is the one '
            'write of a secret the phone makes (#262, INV-08)',
      );
      expect(
        find.text('Pair with a wall'),
        findsNothing,
        reason: 'the pairing screen stayed up after a successful claim: the phone is paired, and the overview is the '
            'next screen',
      );
      expect(find.byKey(canvasKey), findsOneWidget);
      expect(
        fake.watches,
        1,
        reason: 'the shell did not ask for the watch after the read that followed the claim: a watch is asked only '
            'after a read has landed (`mobile-ui.md`, the four states)',
      );

      for (final text in screenText(tester)) {
        expect(
          text.contains(testDeviceKey),
          isFalse,
          reason: 'INV-08: the device key reached a widget\'s text. The key lives in the key store, in `device` on the '
              'wire, and in nothing a screenshot can read',
        );
        expect(
          text.contains('481902'),
          isFalse,
          reason: 'the six digits are not shown again after the claim: a code that survives its two minutes is a code '
              'that outlives them (`docs/mobile-ui.md`, pairing)',
        );
      }
    });
  });

  group('the three refusals, and their steps', () {
    testWidgets(
      '`no pairing is open` empties the code field — and a code burnt by five wrong guesses is that same refusal',
      (tester) async {
        final (fake, _) = await pumpPairing(
          tester,
          answers: [const Refusal(noPairingOpen, RefusalKind.rule)],
        );
        await enterCode(tester, '481902');
        await tester.tap(find.byKey(pairButtonKey));
        await tester.pumpAndSettle();

        expect(fake.claims.single.code, '481902');
        expect(
          codeField(tester).controller!.text,
          isEmpty,
          reason: '`no pairing is open` left the code in the field: nothing is open, the two minutes are gone, the '
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
          reason: 'the screen did not say the code expired. A burnt code is `no pairing is open` (#262) and takes '
              'this same step: the phone invents no sentence for it',
        );

        await expectLater(
          find.byType(PairingScreen),
          matchesGoldenFile('goldens/pairing-burnt-code.png'),
        );
      },
    );

    testWidgets('`that code is not the code` keeps it and selects the digits', (tester) async {
      await pumpPairing(
        tester,
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
        reason: 'the digits were not selected after `that code is not the code`: the step is *keeps the field, selects '
            'the digits*',
      );
      expect(selection.extentOffset, 6);
      expect(codeField(tester).focusNode!.hasFocus, isTrue);
      expect(find.text(wrongCodeSentence), findsOneWidget);
    });

    testWidgets('`that device is already paired` keeps the name field focused', (tester) async {
      await pumpPairing(
        tester,
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
    });

    testWidgets('a refusal this screen has no row for is the wall\'s words verbatim, and no step', (tester) async {
      await pumpPairing(
        tester,
        answers: [const Refusal('the wall is busy pairing', RefusalKind.rule)],
      );
      await enterCode(tester, '481902');
      await tester.tap(find.byKey(pairButtonKey));
      await tester.pumpAndSettle();

      expect(
        find.text('the wall is busy pairing'),
        findsOneWidget,
        reason: 'a refusal outside the note\'s three was given a sentence of the phone\'s own: a step nobody wrote is a '
            'rule invented at the screen',
      );
      expect(codeField(tester).controller!.text, '481902');
    });
  });
}

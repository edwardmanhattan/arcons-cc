/// **The wall's pairing QR, parsed** (#271).
///
/// The payloads are the wall's own: the exact string `src/control/pair.rs` writes for the `pairing` capture (#270) and
/// the percent-encoded form the ticket's proof gives. Nothing here is invented, and every refusal is a **name** rather
/// than a `null` — a payload the phone cannot act on has to say which part refused it.
library;

import 'package:arcons_phone/pairing.dart';
import 'package:test/test.dart';

/// The payload the wall's `pairing` golden decodes to: `src/scene.rs`'s relay and digits, through `pair::payload`.
const String wallPayload =
    'arcons://pair?relay=tcp://relay.arcons.test:7030&code=481902';

void main() {
  group('a wall\'s payload', () {
    test('the golden\'s own string parses to its relay and six digits', () {
      final result = parsePairingUri(wallPayload);
      expect(
        result,
        isA<Pairing>(),
        reason:
            'the exact payload #270 draws beside `PAIR 481 902` did not parse: the QR and the phone disagree about '
            'the shape of a pairing',
      );
      final pairing = result as Pairing;
      expect(
        pairing.relay,
        Uri.parse('tcp://relay.arcons.test:7030'),
        reason: 'the relay the wall drew was not the relay the phone parsed',
      );
      expect(
        pairing.code,
        '481902',
        reason: 'the code in the payload was not the six digits the wall drew beside it (#270)',
      );
    });

    test('a percent-encoded relay is decoded by the standard URL parser', () {
      final result = parsePairingUri(
        'arcons://pair?relay=tcp%3A%2F%2F10.2.1.20%3A7000&code=481902',
      );
      expect(result, isA<Pairing>());
      final pairing = result as Pairing;
      expect(
        pairing.relay,
        Uri.parse('tcp://10.2.1.20:7000'),
        reason: 'the phone did not undo the wall\'s percent-encoding (`docs/control.md`, *The QR beside the digits*)',
      );
      expect(pairing.code, '481902');
    });

    test('a tls relay is a relay this build dials', () {
      final result = parsePairingUri(
        'arcons://pair?relay=tls%3A%2F%2Frelay.arcons.test%3A7443&code=000001',
      );
      expect(result, isA<Pairing>());
      expect((result as Pairing).relay.scheme, 'tls');
    });
  });

  group('the six refusals, each by name', () {
    test('not an arcons:// URI', () {
      final result = parsePairingUri(
        'https://pair?relay=tcp://relay:7000&code=481902',
      );
      expect(result, isA<PairingRefused>());
      expect((result as PairingRefused).name, 'notAnArconsUri');
      expect(result.refusal.sentence, isNotEmpty);
    });

    test('`pair` is not the host', () {
      final result = parsePairingUri(
        'arcons://wall?relay=tcp://relay:7000&code=481902',
      );
      expect(result, isA<PairingRefused>());
      expect((result as PairingRefused).name, 'pairIsNotTheHost');
    });

    test('no relay', () {
      final result = parsePairingUri('arcons://pair?code=481902');
      expect(result, isA<PairingRefused>());
      expect((result as PairingRefused).name, 'noRelay');

      final empty = parsePairingUri('arcons://pair?relay=&code=481902');
      expect(empty, isA<PairingRefused>());
      expect((empty as PairingRefused).name, 'noRelay');
    });

    test('no code', () {
      final result = parsePairingUri('arcons://pair?relay=tcp://relay:7000');
      expect(result, isA<PairingRefused>());
      expect((result as PairingRefused).name, 'noCode');

      final empty = parsePairingUri(
        'arcons://pair?relay=tcp://relay:7000&code=',
      );
      expect(empty, isA<PairingRefused>());
      expect((empty as PairingRefused).name, 'noCode');
    });

    test('a code that is not exactly six digits', () {
      for (final code in ['48190', '4819020', '48190a', '48 1902']) {
        final result = parsePairingUri(
          'arcons://pair?relay=tcp://relay:7000&code=${Uri.encodeQueryComponent(code)}',
        );
        expect(
          result,
          isA<PairingRefused>(),
          reason: 'code ${code.length} was accepted',
        );
        expect(
          (result as PairingRefused).name,
          'codeIsNotSixDigits',
          reason: 'a code that is not six digits was refused by the wrong name',
        );
      }
    });

    test('a relay this build cannot dial', () {
      for (final relay in [
        'wss%3A%2F%2Frelay.arcons.test%3A7000',
        'tcp%3A%2F%2F',
        'tcp%3A%2F%2Frelay.arcons.test',
        'not%20a%20relay',
      ]) {
        final result = parsePairingUri(
          'arcons://pair?relay=$relay&code=481902',
        );
        expect(
          result,
          isA<PairingRefused>(),
          reason: 'relay $relay was accepted',
        );
        expect(
          (result as PairingRefused).name,
          'relayCannotBeDialled',
          reason:
              'a relay this build cannot dial was refused by the wrong name',
        );
      }
    });
  });

  group('nothing secret is printed', () {
    test('a pairing and a refusal do not quote the payload', () {
      final pairing = parsePairingUri(wallPayload) as Pairing;
      final refused = parsePairingUri(
        'arcons://pair?relay=wss%3A%2F%2Fsecret.example%3A7000&code=481902',
      ) as PairingRefused;

      for (final text in [
        pairing.toString(),
        refused.toString(),
        refused.refusal.sentence,
      ]) {
        expect(
          text.contains('481902'),
          isFalse,
          reason: 'INV-08: the six digits are the code, and a `toString` that quotes them puts them in a log',
        );
        expect(
          text.contains('relay.arcons.test') || text.contains('secret.example'),
          isFalse,
          reason:
              'INV-08: a relay is somebody\'s deployment address, and a message that quotes it is how it reaches a '
              'log',
        );
      }
    });
  });
}

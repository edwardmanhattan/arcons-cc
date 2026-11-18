/// **The claim's answer, typed** (#262): `{"v": 1, "ok": true, "paired": {"name": "phone", "key": "…64 hex…"}}`.
///
/// This is the one shape a phone acts on that `fixtures/control/session.jsonl` **cannot** hold: a successful
/// `pair.claim` hands back a freshly generated key, so a recording of one would differ on every run — and a key kept in
/// a fixture is a secret in the tree (`INV-08`). The shape is `docs/control.md`'s (*The one frame a stranger may send*)
/// and `wire::reply`'s (`src/control/wire.rs`); this file holds the phone's decoder to it, and to the rule that makes
/// the key a secret: it is a field, never a printed string.
///
/// The rest of the wire is held to the wall's own recording by `test/corpus_test.dart`, and the frames by
/// `test/relay_test.dart`, which drives the claim over a fake relay. This file is the answer on its own, because a
/// shape with no fixture needs one place that says what it is.
library;

import 'dart:convert';

import 'package:arcons_phone/wire.dart';
import 'package:test/test.dart';

void main() {
  // A key of the shape `pair.claim` hands back, and **not a secret**: a test literal that reaches no wall.
  final claimed = 'cd' * 32;

  /// A pairing as the wall writes it: the version, `ok`, and the two fields under `paired` — and nothing else.
  Map<String, Object?> pairing({
    String name = 'phone',
    String? key,
  }) =>
      <String, Object?>{
        'v': protocol,
        'ok': true,
        'paired': <String, Object?>{'name': name, 'key': key ?? claimed},
      };

  test('a "paired" answer is a Paired, carrying the name the wall wrote and the key', () {
    final answer = decodeAnswer(jsonEncode(pairing()));
    expect(
      answer,
      isA<Paired>(),
      reason:
          'a `paired` answer was not typed: the phone cannot pair if this shape is a WireException (#262)',
    );
    final paired = answer as Paired;
    expect(
      paired.name,
      'phone',
      reason: 'the name is what `pair.forget` and a cursor origin use, so the answer must carry it',
    );
    expect(
      paired.key,
      claimed,
      reason:
          'the key the wall handed over is not the key the phone holds: the one copy would be lost at the decode '
          '(INV-08, #262)',
    );
  });

  test('a pairing never prints its key, and still names the device', () {
    final paired = Paired(name: 'phone', key: claimed);
    for (final text in <String>['$paired', paired.toString()]) {
      expect(
        text.contains(claimed),
        isFalse,
        reason:
            'INV-08: the key reached a printed string. A pairing that lists its fields is a key in every log '
            'line that formats an answer',
      );
    }
    expect(
      paired.toString(),
      contains('phone'),
      reason:
          'the name is not a secret, and a redaction that hid the whole pairing would hide which phone paired',
    );
  });

  test('a pairing with a field missing is refused, never guessed at', () {
    for (final broken in <Map<String, Object?>>[
      <String, Object?>{'v': protocol, 'ok': true, 'paired': <String, Object?>{'key': claimed}},
      <String, Object?>{'v': protocol, 'ok': true, 'paired': <String, Object?>{'name': 'phone'}},
      <String, Object?>{'v': protocol, 'ok': true, 'paired': 3},
    ]) {
      expect(
        () => decodeAnswer(jsonEncode(broken)),
        throwsA(isA<WireException>()),
        reason:
            'a `paired` answer missing a field decoded anyway: a pairing without a key is a phone that thinks it '
            'paired, and one without a name is a row the wall cannot have written (#262)',
      );
    }
  });

  test('a refusal\'s detail is carried when the wall sends one, and absent when it does not', () {
    final bare = decodeAnswer(
      jsonEncode(<String, Object?>{
        'v': protocol,
        'ok': false,
        'why': 'unsupported protocol version',
        'kind': RefusalKind.version.word,
      }),
    );
    expect(bare, isA<Refused>());
    expect(
      (bare as Refused).detail,
      isNull,
      reason:
          'a refusal without `detail` was given one: the field is optional on the wire, and the corpus sends no '
          'detail with any of its refusals (#264)',
    );

    final withDetail = decodeAnswer(
      jsonEncode(<String, Object?>{
        'v': protocol,
        'ok': false,
        'why': 'unsupported protocol version',
        'kind': RefusalKind.version.word,
        'detail': 'this wall speaks version 2',
      }),
    );
    expect(
      (withDetail as Refused).detail,
      'this wall speaks version 2',
      reason: 'the wall\'s longer words were dropped at the decode, so the update screen cannot draw them',
    );
  });

  test('the client types the four "ok" answers it can be given, and a fifth is a wall it cannot speak', () {
    // The four, one line each: a change, a projection, a grant, a pairing. A fifth `ok` shape is a `WireException`
    // rather than a default — `INV-07`'s rule applied to the answer a client acts on.
    expect(decodeAnswer(jsonEncode(<String, Object?>{'v': protocol, 'ok': true, 'changed': false})), isA<Changed>());
    expect(
      decodeAnswer(jsonEncode(<String, Object?>{'v': protocol, 'ok': true, 'watch': Watch.wall.word})),
      isA<Granted>(),
    );
    expect(decodeAnswer(jsonEncode(pairing())), isA<Paired>());
    expect(
      () => decodeAnswer(jsonEncode(<String, Object?>{'v': protocol, 'ok': true, 'something': 1})),
      throwsA(isA<WireException>()),
      reason: 'an `ok` answer outside the four was guessed at instead of named (#198, INV-07)',
    );
  });
}

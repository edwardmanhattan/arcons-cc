/// **A client's rules, as the phone's own named tests** (#250).
///
/// `src/control/client.rs` holds four rules over the wall's own session loop (#197); these are the same four claims in
/// Dart, over a fake carrier, because a claim proved in one language is a claim in one language. The fifth is the one
/// the ticket that wrote them added: **a refusal is mapped from its `kind`, never from its sentence** (#198).
///
/// **Nothing here opens a socket, waits on a clock it does not control, or starts a thread.** The carrier is twenty
/// lines at the bottom of this file, and the answers it carries are the wall's **own recording** —
/// `fixtures/control/session.jsonl`, read by path (#224, #248) — so a test's "wall" is the file the wall wrote itself
/// into rather than a projection this file invented.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:arcons_phone/client.dart';
import 'package:arcons_phone/wire.dart';
import 'package:test/test.dart';

/// The recording, by path from this package: `clients/phone/` → the repository's root.
const String corpusPath = '../../fixtures/control/session.jsonl';

/// **The first line a session sends**, byte for byte — `src/control/wire.rs`'s `request_line_at(&Request::Wall)`, and
/// the object the recording's first line holds. The ticket spells it `{"v": 1, "read": "wall"}`: a JSON object has no
/// whitespace on a wire.
const String readLine = '{"v":$protocol,"read":"wall"}';

void main() {
  final corpus = readCorpus();
  final reads = recordedReads(corpus);
  final changes = recordedChanges(corpus);
  final versionRefusal = refusalIn(
    corpus,
    (send) => send is Map<String, Object?> && send['v'] is int && send['v'] != protocol,
    'a request at another protocol version',
  );
  final missingPane = refusalIn(
    corpus,
    (send) => verbIn(send) == 'pane.close' && paneIn(send) == 99,
    'a close of a pane the wall does not have',
  );
  final unknownVerb = refusalIn(
    corpus,
    (send) => verbIn(send) == 'pane.teleport',
    'a verb the wall does not know',
  );

  group('the recording is enough to drive these rules', () {
    test('it holds the reads and the one answer a landed change is given', () {
      expect(
        reads.length,
        greaterThanOrEqualTo(2),
        reason:
            'the recording holds ${reads.length} projections, and rules 1 and 3 are a read, a change and a read: a '
            'corpus that shrank would let a test below pass while proving less (fixtures/README.md)',
      );
      expect(
        changes,
        isNotEmpty,
        reason: 'no exchange in the recording is a change that landed, so a client that acts has no recorded answer',
      );
      expect(
        changes.toSet().length,
        1,
        reason:
            'the recording answers a landed change with ${changes.toSet().length} different lines '
            '(${changes.toSet()}): a test cannot then say which one the wall gives',
      );
    });
  });

  group('a client\'s rules, one test each', () {
    /// **Rule 1 — it reads before it acts** (`mobile-remote.md` decision 1): the first line a session sends is the
    /// read, and nothing is sent until that read has landed.
    ///
    /// The falsifier is an intent sent before a read lands. The tap on the client's side of the carrier is what makes
    /// that visible: a test that watched the wall's answers would see the read look perfectly normal either way.
    test('a read, an act and a read move exactly one pane', () async {
      final carrier = FakeCarrier([reads[0], changes.first, reads[1]]);
      final client = Client();
      await client.connect(carrier);

      // The read goes out, and the wall holds its answer.
      carrier.hold();
      final reading = client.read();
      await pumpEventQueue();
      expect(
        carrier.sent,
        [readLine],
        reason: 'the first line a session sends is the read, and nothing else goes with it',
      );
      await expectLater(
        client.act('pane.close', {'pane': 4}),
        throwsA(isA<NotRead>()),
        reason:
            'an intent sent before the read landed was accepted: the projection is the only truth a client has, and '
            'a change composed against a wall nobody has read is a change made wrongly',
      );
      expect(
        carrier.sent,
        [readLine],
        reason: 'the refused act wrote a line: "nothing is sent until that read has landed" is about the carrier',
      );
      carrier.release();

      final before = await reading;
      expect(
        before.focused,
        3,
        reason:
            'the recording opens on a wall with the keys on pane 3: a test that asserts on anything else is reading '
            'a different file (fixtures/control/session.jsonl, `cargo run -- --record`)',
      );
      expect(client.focused, before.focused, reason: 'the projection a read returned is what the client holds');

      // One settled change, and the read after it says what the change did.
      final answer = await client.act('pane.focus', {'pane': 2});
      expect(answer.changed, isTrue, reason: 'the wall answered the focus with a change that landed');
      // **Nothing is optimistic** (`ADR-0006` decision 4, `docs/mobile-ui.md`): an `ok` is not the new projection, so the
      // ring the phone draws does not move until a read says it did. The client holds what it read and no more.
      expect(
        client.focused,
        3,
        reason:
            'the client moved its own ring from the answer to its intent: that is a wall it could not know, and the '
            'one thing a second writer in the phone would look like',
      );
      final after = await client.read();

      expect(
        after.seq,
        before.seq + 1,
        reason: 'a change is one version of the projection, and `seq` is how a client knows what it holds',
      );
      expect(after.focused, 2, reason: 'the read after the act must show the pane the intent gave the keys');
      expect(
        panesOf(after),
        panesOf(before),
        reason: 'focusing a pane is not a layout change: no pane came, went or was renamed',
      );
      expect(focusedIn(after), [2], reason: 'exactly one pane has the keys, and it is the one the intent named');
      expect(focusedIn(before), [3], reason: 'the wall before the act had the keys on the other pane');

      expect(carrier.sent.length, 3, reason: 'the session is read, act, read — one line each');
      expect(carrier.sent.first, readLine);
      expect(carrier.sent.last, readLine);
      expect(
        asAnswer(carrier.sent[1]),
        {
          'v': protocol,
          'intent': {'verb': 'pane.focus', 'pane': 2},
        },
        reason: 'an intent is `src/control/wire.rs`\'s shape: the version, and the intent inside it with its verb',
      );
    });

    /// **Rule 2 — a connection that comes back knows nothing** (`mobile-remote.md` decision 1): after a drop it
    /// re-reads, and it never replays a queued intent.
    ///
    /// The falsifier is the obvious implementation: keep the intent in a queue, flush it when the wire comes back. The
    /// tap is what catches it; the projection the read returns cannot, because a replay would land *after* the read.
    test('a client that drops sends nothing it had queued', () async {
      final first = FakeCarrier([reads[0]]);
      final client = Client();
      await client.connect(first);
      final before = await client.read();
      expect(first.sent, [readLine], reason: 'the first read is one request');
      expect(before.focused, 3, reason: 'the keys are on pane 3 when the client goes away');

      // The connection goes with an intent still in the client's hand.
      await client.disconnect();
      expect(client.isConnected, isFalse);
      expect(
        client.wall,
        isNull,
        reason:
            'the projection went with the wire: a client that kept it would draw a wall it can no longer see '
            '(mobile-remote.md decision 1)',
      );
      await expectLater(
        client.act('pane.focus', {'pane': 3}),
        throwsA(isA<NoConnection>()),
        reason: 'a client that cannot send must say so, not keep the intent for later',
      );
      expect(first.sent, [readLine], reason: 'the client wrote on a connection it had dropped');

      // **It comes back knowing nothing.** A wall that changed while the client was away would be changed wrongly by a
      // replay, so the first thing on the new wire is a read and nothing else.
      final resumed = FakeCarrier([reads[1]]);
      await client.connect(resumed);
      expect(
        client.wall,
        isNull,
        reason:
            'a connection that comes back knows nothing: the projection the last one read is not knowledge, however '
            'recent it is',
      );
      expect(
        resumed.sent,
        isEmpty,
        reason: 'the client sent something on reconnecting: a drop is not a memory',
      );

      final after = await client.read();
      expect(resumed.sent, [readLine], reason: 'the first line after a resume is the read and only the read');
      expect(
        after.focused,
        2,
        reason:
            'the client replayed the intent it had queued: the keys are on the pane it remembered (3) instead of the '
            'one the wall has (2)',
      );
      expect(
        resumed.sent.where((line) => asAnswer(line).containsKey('intent')),
        isEmpty,
        reason:
            'a queued intent was sent on the connection that came back: a drop is not a memory, and "I focused pane '
            '3" is a fact about a wall that may no longer exist',
      );
      expect(
        resumed.sent.where((line) => asAnswer(line).containsKey('watch')),
        isEmpty,
        reason:
            'a watch was resumed on the connection that came back: a watch belongs to a connection, and a phone that '
            'resumed one would be watching nothing (#221)',
      );
      // A phone that wakes up holds a projection and a socket that is gone, and the app hands it the one it just
      // opened. **A new connection is not a new memory**: the projection the client was holding goes with the wire it
      // came from, before the first read on this one lands.
      final wokeUp = FakeCarrier([reads[0], null]);
      await client.connect(wokeUp);
      expect(
        client.wall,
        isNull,
        reason:
            'the client kept the projection it read on the last connection, so a screen would draw a wall it can no '
            'longer see: a connection that comes back knows nothing',
      );
      expect(wokeUp.sent, isEmpty, reason: 'the client sent something before reading on the connection it just got');
      expect(resumed.closed, isTrue, reason: 'the client kept a carrier it had replaced: it lets go of the wire it left');
      expect(
        (await client.read()).focused,
        3,
        reason: 'the first read on the new connection is what the client knows, and it is not the last one\'s answer',
      );

      // And a wire can end without the client asking. `null` is end of file, and the projection goes with it.
      await expectLater(
        client.read(),
        throwsA(isA<Lost>()),
        reason: 'end of file is a wall that hung up, never a blank answer',
      );
      expect(client.wall, isNull, reason: 'a lost connection left the client holding a projection');
      expect(client.isConnected, isFalse, reason: 'a lost connection left the client connected');
    });

    /// **Rule 3 — a wall changed by another client is read, never corrected**: the projection that disagrees replaces
    /// what the phone drew, and the next intent is composed from it. There is no second writer.
    ///
    /// The falsifier is a client that composes from its remembered focus: it changes the pane it left behind, which is
    /// a wall corrected back to a state that no longer exists.
    test('a wall that changed while a client was away is read, not corrected', () async {
      final away = FakeCarrier([reads[0]]);
      final client = Client();
      await client.connect(away);
      final start = await client.read();
      expect(start.focused, 3, reason: 'the client leaves with the keys on pane 3');

      // The first client goes away mid-sequence.
      await client.disconnect();

      // Another client gives another pane the keys while it is gone.
      final elsewhere = FakeCarrier([reads[0], changes.first]);
      final other = Client();
      await other.connect(elsewhere);
      await other.read();
      await other.act('pane.focus', {'pane': 2});
      await other.disconnect();
      expect(elsewhere.sent.length, 2, reason: 'the other client read before it acted, and sent the one intent');

      // It comes back and reads. What it holds is the wall another client made, not the one it remembered.
      final resumed = FakeCarrier([reads[1], changes.first]);
      await client.connect(resumed);
      expect(
        client.focused,
        isNull,
        reason: 'a connection that comes back knows nothing, so it has no focus of its own to correct the wall with',
      );
      final seen = await client.read();
      expect(
        seen.focused,
        2,
        reason:
            'the client read the wall and must see the pane another client focused — not the one it remembered (3)',
      );
      expect(
        client.wall?.focused,
        2,
        reason: 'the projection replaced what the phone drew: a wall changed by another client is read, never corrected',
      );
      expect(client.wall?.seq, seen.seq, reason: 'the client holds the version it just read, and no other');

      // Its next intent is composed from what it just read: it names pane 2, not pane 3.
      await client.act('pane.close', {'pane': client.focused!});
      expect(
        asAnswer(resumed.sent.last)['intent'],
        {'verb': 'pane.close', 'pane': 2},
        reason:
            'the client closed the pane it remembered (3) instead of the one it just read (2): that is a wall '
            'corrected from a memory',
      );
    });

    /// **Rule 4 — a version the wall does not speak is a stop** (`ADR-0008` decision 5): the session ends, and it is
    /// never retried and never negotiated.
    ///
    /// The falsifier is a client that resends after `kind: version`.
    test('a version the wall does not speak stops the client', () async {
      final carrier = FakeCarrier([versionRefusal]);
      final client = Client.speaking(protocol + 1);
      await client.connect(carrier);

      await expectLater(
        client.read(),
        throwsA(
          isA<Refusal>()
              .having((refusal) => refusal.why, 'why', 'unsupported protocol version')
              .having((refusal) => refusal.kind, 'kind', RefusalKind.version)
              .having((refusal) => refusal.next, 'next', Next.update),
        ),
        reason:
            'a version the wall does not speak is refused by name, and the client decides from the kind: the action '
            'is an update, not a retry (#198)',
      );
      expect(
        carrier.sent,
        ['{"v":${protocol + 1},"read":"wall"}'],
        reason: 'the refused request is the only line the client wrote',
      );
      expect(client.stop?.kind, RefusalKind.version, reason: 'the stop is what the update screen is drawn from');
      expect(client.wall, isNull, reason: 'a client the wall stopped must not hold a projection');

      // **A stop**: sending again is a client retrying a wall that can never answer it.
      await expectLater(
        client.read(),
        throwsA(isA<Refusal>().having((refusal) => refusal.kind, 'kind', RefusalKind.version)),
        reason: 'a stopped client keeps saying so, and finds out nothing new',
      );
      await expectLater(client.act('pane.focus', {'pane': 1}), throwsA(isA<Refusal>()));
      expect(
        carrier.sent.length,
        1,
        reason: 'the stopped client sent another line: a version it cannot speak is a stop, not a negotiation',
      );
    });
  });

  group('a refusal is mapped from its kind', () {
    /// The fifth rule. **The wall's words with a kind the wall did not give them**: `REFUSALS` (`src/control.rs`) maps
    /// each sentence to exactly one kind, so no wall sends this — which is the test. A client that parsed prose would
    /// stop on words that say `version`; the wall said `rule`, and a `rule`'s recourse is a read.
    test('the same words are a stop or not by the kind, never by the sentence', () async {
      final asARule = withKind(versionRefusal, 'rule');
      final carrier = FakeCarrier([reads[0], asARule, reads[0]]);
      final client = Client();
      await client.connect(carrier);
      await client.read();

      await expectLater(
        client.act('pane.close', {'pane': 4}),
        throwsA(
          isA<Refusal>()
              .having((refusal) => refusal.why, 'why', 'unsupported protocol version')
              .having((refusal) => refusal.kind, 'kind', RefusalKind.rule)
              .having((refusal) => refusal.next, 'next', Next.reread),
        ),
        reason: 'the client read the kind: the same sentence is a `rule` when the wall says `rule`',
      );
      expect(
        client.stop,
        isNull,
        reason: 'a sentence that sounds like a version stopped the client: the words decided, not the kind',
      );
      expect(
        client.wall,
        isNotNull,
        reason: 'a refusal moved nothing, so the projection the intent was composed from is still the wall\'s',
      );

      // The action the kind names is a read, and a read is what happens next — never the refused intent again.
      await client.read();
      expect(carrier.sent.length, 3, reason: 'the client sent something other than the read that a rule asks for');
      expect(carrier.sent.last, readLine);
      expect(
        carrier.sent.where((line) => line.contains('pane.close')),
        hasLength(1),
        reason: 'the client retried an intent the wall refused: a rule says do not send it again',
      );

      // And the other direction: the words of a rule, with `version` as its kind, is a stop.
      final asVersion = withKind(missingPane, 'version');
      final stoppedCarrier = FakeCarrier([reads[0], asVersion]);
      final stopped = Client();
      await stopped.connect(stoppedCarrier);
      await stopped.read();
      await expectLater(
        stopped.act('pane.close', {'pane': 99}),
        throwsA(isA<Refusal>().having((refusal) => refusal.kind, 'kind', RefusalKind.version)),
        reason: 'the client acted on the sentence: "no such pane" is a stop when the wall says `version`',
      );
      expect(
        stopped.stop?.why,
        'no such pane',
        reason: 'the wall\'s words are kept verbatim for the screen, and the kind is what the client acts on',
      );
      expect(stopped.wall, isNull, reason: 'a stopped client must not hold a projection');
      await expectLater(stopped.read(), throwsA(isA<Refusal>()));
      expect(
        stoppedCarrier.sent.length,
        2,
        reason: 'the stopped client sent more than the read and the one refused intent',
      );
    });

    /// **A refusal is not an error state** (`docs/mobile-ui.md`, the two rules under its refusal table): an `input` or
    /// a `rule` refusal leaves the client live, because the wall answered. Only `version` ends a session.
    test('an input refusal is this app\'s bug, and the client stays live', () async {
      final carrier = FakeCarrier([reads[0], unknownVerb, reads[1]]);
      final client = Client();
      await client.connect(carrier);
      await client.read();

      await expectLater(
        client.act('pane.teleport', {'pane': 1}),
        throwsA(
          isA<Refusal>()
              .having((refusal) => refusal.kind, 'kind', RefusalKind.input)
              .having((refusal) => refusal.next, 'next', Next.ownBug),
        ),
        reason: 'the phone sent a verb the wall does not know, which is this app\'s bug and never the operator\'s',
      );
      expect(
        client.stop,
        isNull,
        reason: 'a refusal is not an error state: the connection is up and the next line may be a fresh request',
      );
      expect(
        client.focused,
        3,
        reason: 'the projection the failed intent was composed from is still the wall\'s, because nothing moved',
      );

      // The answer is a read, not the verb again: a client that resent its own bug would loop on it.
      final after = await client.read();
      expect(after.seq, 1, reason: 'the read after the refusal did not land');
      expect(carrier.sent.last, readLine);
      expect(
        carrier.sent.where((line) => line.contains('pane.teleport')),
        hasLength(1),
        reason: 'the client retried the verb the wall called unknown',
      );
    });

    /// **A grant is the carrier's answer, never a read's** (#221, #256). A hand-made carrier could hand one to a
    /// client; the client must say it cannot read it rather than treat the existence of a stream as a projection.
    test('a watch grant is not an answer a client can act on', () async {
      final grant = jsonEncode({'v': protocol, 'ok': true, 'watch': 'wall'});
      final carrier = FakeCarrier([grant]);
      final client = Client();
      await client.connect(carrier);

      await expectLater(
        client.read(),
        throwsA(isA<Unreadable>()),
        reason:
            'a read answered with a grant was taken for a projection: a grant says a stream exists, and a stream is '
            'not the arrangement the read asked for',
      );
      expect(client.wall, isNull, reason: 'a client that cannot read its answer must not hold a projection');
      await expectLater(
        client.act('pane.focus', {'pane': 1}),
        throwsA(isA<NotRead>()),
        reason: 'a read that did not land leaves the client knowing nothing, so nothing may be sent after it',
      );
      expect(carrier.sent, [readLine], reason: 'the unreadable answer provoked a line the client did not mean to send');
    });

    /// `INV-07` one language over: four kinds, four actions, and no default arm anywhere in the path.
    test('every refusal kind names one action, and none of them is a default', () {
      // `Next.of`'s switch has no default arm, so a fifth kind is a compile error there rather than a screen with no
      // answer; this asserts the four answers are the ones the table gives, in the wall's own order.
      expect(
        RefusalKind.values.map(Next.of),
        [Next.reread, Next.update, Next.ownBug, Next.retryRead],
        reason: 'INV-07: every refusal kind names exactly one action, and a client decides from the kind (#198)',
      );
      expect(
        const Refusal('no such pane', RefusalKind.rule).next,
        Next.reread,
        reason: 'a denial carries its kind\'s action, so a client never maps one twice',
      );
      for (final denied in const <Denied>[
        NoConnection(),
        NotRead(),
        Lost(),
        Unreadable('a line this protocol does not have'),
      ]) {
        expect(
          denied.next,
          isNull,
          reason:
              '${denied.runtimeType} names an action: the client\'s own reasons are not the wall\'s no, and the '
              'wall said nothing to decide for the phone',
        );
      }
    });
  });
}

/// The recording, read: one exchange per line, and a corpus that is missing is a failed test rather than a skipped
/// one.
List<Map<String, Object?>> readCorpus() {
  final file = File(corpusPath);
  if (!file.existsSync()) {
    fail(
      'the corpus is not at $corpusPath: the phone is held to fixtures/control/session.jsonl, which '
      '`cargo run -- --record` writes. A missing corpus is not a test that passes.',
    );
  }
  final exchanges = <Map<String, Object?>>[];
  for (final line in const LineSplitter().convert(file.readAsStringSync())) {
    if (line.trim().isEmpty) {
      continue;
    }
    final decoded = jsonDecode(line);
    if (decoded is! Map<String, Object?>) {
      fail('an exchange in $corpusPath is not a JSON object: ${line.trim()}');
    }
    exchanges.add(decoded);
  }
  return exchanges;
}

/// **The answer an exchange records**, as the wall wrote it. The corpus's `expect` is the line the wall sent, so a test
/// that hands it back to the client is a test against the wall's own answer.
Map<String, Object?> answerOf(Map<String, Object?> exchange) {
  final expect = exchange['expect'];
  if (expect is Map<String, Object?>) {
    return expect;
  }
  fail('an exchange in $corpusPath has no "expect" object: every send in the recording was answered (#224)');
}

/// An exchange's answer as one line, which is what a carrier carries.
String answerLine(Map<String, Object?> exchange) => jsonEncode(answerOf(exchange));

/// A line, as the JSON object it is.
Map<String, Object?> asAnswer(String line) {
  final decoded = jsonDecode(line);
  if (decoded is! Map<String, Object?>) {
    fail('$line is not a JSON object');
  }
  return decoded;
}

/// **The same refusal with a kind the wall did not give it.**
///
/// `REFUSALS` (`src/control.rs`) maps each sentence to exactly one kind, so no wall sends this: it is a fake wall
/// whose words and whose category disagree, and a client that decided from the prose is caught by it and nothing else.
String withKind(String refusal, String kind) => jsonEncode({...asAnswer(refusal), 'kind': kind});

/// The `intent` object of a send, when it has one.
Map<String, Object?>? intentIn(Object? send) {
  if (send is! Map<String, Object?>) {
    return null;
  }
  final intent = send['intent'];
  return intent is Map<String, Object?> ? intent : null;
}

/// The verb a send names, when it names one. A line that is not JSON (the recording's own `not json` probe) names none.
String? verbIn(Object? send) {
  final verb = intentIn(send)?['verb'];
  return verb is String ? verb : null;
}

/// The pane id a send's intent names, when it names one.
Object? paneIn(Object? send) => intentIn(send)?['pane'];

/// The pane ids with the keys, in the wall's order: the ring the phone draws (`docs/mobile-ui.md`).
List<int> focusedIn(Wall wall) => [
      for (final workspace in wall.workspaces)
        for (final pane in workspace.panes)
          if (pane.focused) pane.id,
    ];

/// Every pane a projection names, as a card draws it: the id, the kind and the title, in the wall's order.
List<(int, String, String)> panesOf(Wall wall) => [
      for (final workspace in wall.workspaces)
        for (final pane in workspace.panes) (pane.id, pane.kind, pane.title),
    ];

/// Whether a send is exactly a read: the version and `"read": "wall"`, and nothing else. A line carrying an intent as
/// well is the recording's own malformed probe, and the wall refused it.
bool isRead(Object? send) =>
    send is Map<String, Object?> && send.length == 2 && send['v'] == protocol && send['read'] == 'wall';

/// Every projection the recording answers a read with, in the order the wall gave them.
List<String> recordedReads(List<Map<String, Object?>> corpus) => [
      for (final exchange in corpus)
        if (isRead(exchange['send'])) answerLine(exchange),
    ];

/// Every answer that says a change landed, in the order the wall gave them.
List<String> recordedChanges(List<Map<String, Object?>> corpus) => [
      for (final exchange in corpus)
        if (answerOf(exchange)['changed'] == true) answerLine(exchange),
    ];

/// **A recorded refusal**, by the send that provoked it: the wall's words and the wall's kind, as the wall wrote them.
///
/// A refusal is looked up rather than written, so that a test's "the wall said no" is the file's. The version refusal
/// is recorded against a request at another protocol version, and its answer — the same words and the same kind — is
/// what a session at another version is answered with.
String refusalIn(
  List<Map<String, Object?>> corpus,
  bool Function(Object? send) matches,
  String what,
) {
  for (final exchange in corpus) {
    if (answerOf(exchange)['ok'] == false && matches(exchange['send'])) {
      return answerLine(exchange);
    }
  }
  fail(
    'the recording holds no refusal where $what: fixtures/control/session.jsonl is the wall, and a client test that '
    'invents an answer proves a wall nobody has (#224)',
  );
}

/// **The wall, with no socket under it.** Every line the client sends is recorded, and every answer is either already
/// on the wire or held by the test's hand — no clock, no network, no thread.
final class FakeCarrier implements Carrier {
  FakeCarrier(this._answers);

  /// What the wall will answer, in order. `null` is the connection ending — the end of file a closed socket gives.
  final List<String?> _answers;

  /// Every line the client has sent since the test last looked: the tap on the client's side of the wire, so a test
  /// says what the client sent rather than inferring it from what the wall happened to answer.
  final List<String> sent = <String>[];

  /// Whether the client let go of this carrier.
  bool closed = false;

  Completer<void>? _held;

  /// Hold the next answer until [release]: a request that is out and has not landed.
  void hold() => _held = Completer<void>();

  void release() {
    final held = _held;
    _held = null;
    held?.complete();
  }

  @override
  Future<void> send(String line) async {
    if (closed) {
      throw StateError('the fake wall has hung up');
    }
    sent.add(line);
  }

  @override
  Future<String?> receive() async {
    final held = _held;
    if (held != null) {
      await held.future;
    }
    if (_answers.isEmpty) {
      throw StateError('the fake wall has no answer left: the client sent a line this test did not script');
    }
    return _answers.removeAt(0);
  }

  @override
  Future<void> close() async {
    closed = true;
  }
}

/// **The phone's carrier, over a fake relay on loopback** (#256).
///
/// There is **no fixture for the watch's three lines**, and this file does not pretend otherwise: the unix socket
/// refuses a watch by name (the corpus's own refusal is replayed in `test/corpus_test.dart`), and `fixtures/README.md`
/// refuses a timed exchange because a coalesced publish is a race and a race is not a fixture. So the relay's frames are
/// the ones `docs/control.md` and `src/control/wire.rs` spell out, written by the fake relay below, and **the
/// projections and refusals it hands back are the wall's own recording** — `fixtures/control/session.jsonl`, read by
/// path (#224). The tests build no projection of their own: the only thing added to a recorded answer is `to`, which is
/// exactly the field `src/control/relay.rs` adds to it (`RelayCarrier::write_line`: *the relay's answer is the socket's
/// answer with one field on it*).
///
/// What is proved, one test each:
///
/// 1. **the frames** — a read, an act and a read over a real socket, with the device key in `device` and nowhere else;
/// 2. **the watch** — the ask, the grant, two publishes, and a publish that lands while a read is in flight is *not*
///    taken for the read's answer;
/// 3. **a drop mid-stream reads first and asks again** — nothing is replayed, and the carrier refuses to ask for a watch
///    or to write an intent on a connection it has not seen a read land on;
/// 4. **a refusal is mapped by `kind`**, and a version refusal stops the carrier: no dial, no ask, no frame;
/// 5. **a revoked phone** sees silence — the wall's `that device is not paired` has no `to`, so nothing reaches the
///    phone — and, if a relay routed it back anyway, the phone sees a `rule` and not a stop;
/// 6. **the key is never printed**: not in a message, not in an exception, not in a `toString` (`INV-08`).
///
/// **Not proved here, and said out loud**: `tls://` is only parsed. A TLS relay needs a certificate, and a certificate a
/// test made up is a fixture nobody recorded; the scheme is all this build's one `SecureSocket.connect` would need.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:arcons_phone/client.dart';
import 'package:arcons_phone/relay.dart';
import 'package:arcons_phone/wire.dart';
import 'package:test/test.dart';

/// The recording, by path from this package: `clients/phone/` → the repository's root.
const String corpusPath = '../../fixtures/control/session.jsonl';

/// A device key that is **not a secret**: 64 lowercase hex characters, the shape `pair.claim` hands back. A test literal
/// is the only kind of key that may be in the tree, and this one never reaches a `devices.toml` or a wall.
final String deviceKey = 'ab' * 32;

/// The recorded projections, in the wall's order.
final List<Map<String, Object?>> reads = <Map<String, Object?>>[];

void main() {
  final corpus = readCorpus();
  reads.addAll([
    for (final exchange in corpus)
      if (isRead(exchange.send)) exchange.expect,
  ]);
  final landed = firstWhere(
    corpus,
    (e) => e.expect['ok'] == true && e.expect['changed'] == true,
  );
  final ruleRefusal = firstWhere(corpus, (e) => e.expect['kind'] == 'rule');
  final versionRefusal = firstWhere(
    corpus,
    (e) => e.expect['kind'] == 'version',
  );

  setUp(() {
    expect(
      reads.length,
      greaterThanOrEqualTo(2),
      reason:
          'the recording holds ${reads.length} projections, and a watch is one projection after another: a corpus '
          'that shrank would leave this suite proving a stream of one line (fixtures/control/session.jsonl)',
    );
  });

  group('the relay\'s frames', () {
    /// **A request is the socket's request with the key on it**, and an answer is the socket's answer with `to` on it.
    ///
    /// The falsifier is a second vocabulary: a phone that spelled `{"type": "read"}` or put the key in another field
    /// would be a client the wall has to learn (`docs/control.md`, *The relay's frames*).
    test('a read, an act and a read, over tcp:// — the key in "device" and in nothing else', () async {
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(deviceKey),
      );
      await carrier.connect();
      final client = Client();
      await client.connect(carrier);
      final wire = await relay.next;

      final reading = client.read();
      final read = await wire.frame();
      expect(
        read,
        {'v': protocol, 'device': deviceKey, 'read': 'wall'},
        reason:
            'a relay request is `{"v": 1, "device": "<64 lowercase hex>", "read": "wall"}` — the key rides in '
            '"device", and the version and the request are the client\'s own fields (docs/control.md)',
      );
      expect(
        read.keys.toSet(),
        {'v', 'device', 'read'},
        reason:
            'the carrier added a field the relay does not define: the relay frame is the socket\'s line with '
            '`device` on it, and the wall has one decoder (src/control/relay.rs)',
      );
      await wire.send(relayAnswer(reads[0]));
      expect(
        (await reading).focused,
        3,
        reason: 'the read landed on the recording\'s first projection',
      );

      final acting = client.act('pane.focus', {'pane': 2});
      expect(
        await wire.frame(),
        {
          'v': protocol,
          'device': deviceKey,
          'intent': {'verb': 'pane.focus', 'pane': 2},
        },
        reason: 'an intent is the client\'s line with the key on it, and the intent inside it with its verb',
      );
      await wire.send(relayAnswer(landed));
      expect(
        (await acting).changed,
        isTrue,
        reason: 'the relay\'s answer is decoded as the wall\'s own: `changed: true`',
      );
      expect(
        client.focused,
        3,
        reason: 'an `ok` is not a projection: nothing moved until a read says it did (ADR-0006 decision 4)',
      );

      final again = client.read();
      expect(await wire.frame(), {
        'v': protocol,
        'device': deviceKey,
        'read': 'wall',
      });
      await wire.send(relayAnswer(reads[1]));
      expect(
        (await again).seq,
        wallOf(reads[1])['seq'],
        reason: 'the second read is the recording\'s second projection, not a reconstruction of the first',
      );

      await carrier.close();
      await relay.stop();
    });

    /// **One JSON object per line, both directions.** A frame split across packets is still one frame: the socket is a
    /// byte stream, and a reader that took half a line for a frame would answer nonsense.
    test('a frame is one line, even when its bytes arrive in pieces', () async {
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(deviceKey),
      );
      await carrier.connect();
      final client = Client();
      await client.connect(carrier);
      final wire = await relay.next;

      final reading = client.read();
      await wire.frame();
      final line = jsonEncode(relayAnswer(reads[0]));
      final half = line.length ~/ 2;
      wire.writeBytes(utf8.encode(line.substring(0, half)));
      await Future<void>.delayed(const Duration(milliseconds: 20));
      wire.writeBytes(utf8.encode('${line.substring(half)}\n'));
      expect(
        (await reading).seq,
        wallOf(reads[0])['seq'],
        reason: 'a frame that arrived in two pieces was not read as one line',
      );

      await carrier.close();
      await relay.stop();
    });
  });

  group('the watch', () {
    /// **The ask, the grant, and two publishes** (#221): the projection arriving without being asked for, as a
    /// `Projection` that knows its watch.
    test('a watch is granted and two publishes arrive as projections that know their watch', () async {
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(deviceKey),
      );
      await carrier.connect();
      final client = Client();
      await client.connect(carrier);
      final wire = await relay.next;

      final reading = client.read();
      await wire.frame();
      await wire.send(relayAnswer(reads[0]));
      await reading;

      final published = <Projection>[];
      final listening = carrier.published.listen(published.add);

      final watching = carrier.watch();
      expect(
        await wire.frame(),
        {'v': protocol, 'device': deviceKey, 'watch': 'wall'},
        reason: 'the ask is an ordinary paired frame with "watch" on it (docs/control.md, #221)',
      );
      await wire.send(grant());
      expect(
        (await watching).watch,
        Watch.wall,
        reason: 'the grant says which stream is open, and the grant is the answer to the ask',
      );

      // **Two publishes, and their bodies are recorded projections**: a publish is a read's body with its watch on it,
      // so a body this test made up would be a projection nobody sent.
      await wire.send(publishOf(reads[0]));
      await wire.send(publishOf(reads[1]));
      await until(
        () => published.length == 2,
        'two publishes to reach the published stream',
      );

      expect(
        published.length,
        2,
        reason:
            'the wall published two projections and the carrier delivered ${published.length}: a publish is not an '
            'answer, so a client that received one as an answer would lose its next read',
      );
      for (final projection in published) {
        expect(
          projection.watch,
          Watch.wall,
          reason: 'a publish carries the watch it belongs to — the field that tells it from a read\'s answer',
        );
      }
      expect(
        [for (final projection in published) projection.wall.seq],
        [wallOf(reads[0])['seq'], wallOf(reads[1])['seq']],
        reason: 'the publishes are the wall\'s versions, in its order, and not a reconstruction of a difference',
      );

      await listening.cancel();
      await carrier.close();
      await relay.stop();
    });

    /// **A publish is not the answer to a read in flight** — the ticket's falsifier, both ways: the publish that lands
    /// first goes to [RelayCarrier.published], and the read is answered by its own line (the one with no `watch`).
    ///
    /// The falsifier is a carrier that hands every line to `receive()`: the read would return the published projection,
    /// and the phone would draw a wall as the answer to a question it had not yet had answered.
    test(
      'a publish and a read\'s answer are told apart, in either order',
      () async {
        final relay = await FakeRelay.start();
        final carrier = RelayCarrier(
          relay: relay.url,
          keys: MemoryKeyStore(deviceKey),
        );
        await carrier.connect();
        final client = Client();
        await client.connect(carrier);
        final wire = await relay.next;

        final first = client.read();
        await wire.frame();
        await wire.send(relayAnswer(reads[0]));
        await first;

        final published = <Projection>[];
        final listening = carrier.published.listen(published.add);
        final watching = carrier.watch();
        await wire.frame();
        await wire.send(grant());
        await watching;

        // **The publish lands while the read is in flight**, and its body is the projection the read will not be.
        final reading = client.read();
        await wire.frame();
        await wire.send(publishOf(reads[1]));
        await until(
          () => published.isNotEmpty,
          'the publish that arrived during the read',
        );
        expect(
          [for (final projection in published) projection.watch],
          [Watch.wall],
          reason: 'the publish that arrived during the read did not reach the published stream',
        );
        expect(
          client.wall?.seq,
          wallOf(reads[0])['seq'],
          reason:
              'the publish moved the projection the client holds before its read was answered: a publish is not an '
              'answer',
        );

        await wire.send(relayAnswer(reads[0]));
        expect(
          (await reading).seq,
          wallOf(reads[0])['seq'],
          reason: 'the read was answered by the read\'s own line, and not by the publish that landed first',
        );

        await listening.cancel();
        await carrier.close();
        await relay.stop();
      },
    );

    /// **A grant nobody asked for is an answer to no request**, and it must not become the next read's answer: the
    /// carrier pairs a request with its answer, and a line with no request has no client waiting for it.
    test(
      'a grant with no ask behind it opens nothing and answers nothing',
      () async {
        final relay = await FakeRelay.start();
        final carrier = RelayCarrier(
          relay: relay.url,
          keys: MemoryKeyStore(deviceKey),
        );
        await carrier.connect();
        final client = Client();
        await client.connect(carrier);
        final wire = await relay.next;

        final reading = client.read();
        await wire.frame();
        await wire.send(relayAnswer(reads[0]));
        await reading;

        await wire.send(grant());
        await settle();
        expect(
          carrier.isWatching,
          isFalse,
          reason: 'a grant nobody asked for opened a watch on a connection that asked for none',
        );

        // The next read is answered by its own line, not by the grant that was sitting on the wire.
        final again = client.read();
        await wire.frame();
        await wire.send(relayAnswer(reads[1]));
        expect(
          (await again).seq,
          wallOf(reads[1])['seq'],
          reason: 'an unsolicited grant was handed to a read as its answer',
        );

        await carrier.close();
        await relay.stop();
      },
    );
  });

  group('a connection that comes back', () {
    /// **A drop mid-stream reads first, then asks again** (`mobile-remote.md` decision 1), and **nothing it queued is
    /// sent**: not the intent that was in flight, not the watch that belonged to the connection that went.
    ///
    /// The falsifier is a carrier that redials behind the client's back: it would hand the client a live connection on
    /// which the client has read nothing, and the client's next intent would be composed from a projection the wall no
    /// longer has.
    test('after a drop, the read is first, the watch is second, and nothing was queued', () async {
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(deviceKey),
      );
      await carrier.connect();
      final first = await relay.next;

      final client = Client();
      await client.connect(carrier);
      final reading = client.read();
      await first.frame();
      await first.send(relayAnswer(reads[0]));
      await reading;
      final watching = carrier.watch();
      await first.frame();
      await first.send(grant());
      await watching;
      expect(
        carrier.isWatching,
        isTrue,
        reason: 'the first connection is watching the wall',
      );

      // **The relay goes away**, mid-stream, with a watch open.
      first.hangUp();
      await until(() => !carrier.isConnected, 'the carrier to notice the drop');
      expect(
        carrier.isWatching,
        isFalse,
        reason: 'a drop left the watch open: a watch belongs to a connection (#221)',
      );
      expect(
        carrier.hasRead,
        isFalse,
        reason: 'a drop left the read standing: a connection that comes back knows nothing (decision 1)',
      );

      // The client's next intent finds nothing to write, and is **not** kept for later.
      await expectLater(
        client.act('pane.focus', {'pane': 2}),
        throwsA(isA<Lost>()),
        reason: 'a client that cannot write must say so, not queue the change for the next connection',
      );

      // **The app dials again.** A carrier is a connection: the same object may be connected once more, and the new
      // connection starts knowing nothing.
      await carrier.connect();
      final second = await relay.next;
      final resumed = Client();
      await resumed.connect(carrier);

      // **An intent and a watch, before a read on this connection, are both refused without a byte going out.**
      await expectLater(
        carrier.send('{"v":$protocol,"intent":{"verb":"pane.focus","pane":2}}'),
        throwsA(isA<NotRead>()),
        reason:
            'the carrier wrote an intent on a connection it has not read on: read, then act is one rule, and a drop '
            'is not a queue',
      );
      await expectLater(
        carrier.watch(),
        throwsA(isA<NotRead>()),
        reason:
            'the carrier asked for a watch on a connection it has not read on: a watch is not resumed, it is asked '
            'after a read has landed',
      );
      expect(
        await second.quietFor(const Duration(milliseconds: 150)),
        isTrue,
        reason: 'a refused intent or watch reached the wire: nothing may be sent before the read on a new connection',
      );

      // **The read is the first line**, and only then the ask.
      final again = resumed.read();
      expect(
        await second.frame(),
        {'v': protocol, 'device': deviceKey, 'read': 'wall'},
        reason:
            'the first line after a reconnection is the read and only the read',
      );
      await second.send(relayAnswer(reads[1]));
      expect(
        (await again).focused,
        2,
        reason:
            'the wall changed while the phone was away, and the read says so',
      );

      final rewatching = carrier.watch();
      expect(
        await second.frame(),
        {'v': protocol, 'device': deviceKey, 'watch': 'wall'},
        reason: 'the watch is asked again — after the read that landed on this connection, never before it',
      );
      await second.send(grant());
      await rewatching;

      expect(
        await second.quietFor(const Duration(milliseconds: 150)),
        isTrue,
        reason:
            'the connection carried a line nobody asked for: an intent from before the drop was replayed, and a drop '
            'is not a queue',
      );

      await carrier.close();
      await relay.stop();
    });

    /// **A dial that does not come up is retried with backoff**, and the numbers are the wall's own: it is one relay
    /// down for both ends of the wire (`BACKOFF_START` 200 ms, `BACKOFF_MAX` 5 s, in `src/control/relay.rs`).
    test('a relay that is not up yet is dialled again, with backoff', () async {
      // A port nobody is listening on: bind one, learn it, and let it go.
      final probe = await ServerSocket.bind(InternetAddress.loopbackIPv4, 0);
      final port = probe.port;
      await probe.close();

      final carrier = RelayCarrier(
        relay: Uri.parse('tcp://127.0.0.1:$port'),
        keys: MemoryKeyStore(deviceKey),
        backoffStart: const Duration(milliseconds: 20),
        backoffMax: const Duration(milliseconds: 80),
        dialWithin: const Duration(milliseconds: 250),
      );
      final dialling = carrier.connect();
      // The relay comes up while the phone is backing off.
      final relay = await FakeRelay.start(port: port);
      await dialling;
      expect(
        carrier.isConnected,
        isTrue,
        reason: 'the carrier gave up on a relay that came up during a backoff',
      );
      final wire = await relay.next;
      final reading = carrier.receive();
      await carrier.send('{"v":$protocol,"read":"wall"}');
      expect(
        (await wire.frame())['read'],
        'wall',
        reason: 'the dial wrote something: the read is the client\'s line',
      );
      await wire.send(relayAnswer(reads[0]));
      await reading;

      await carrier.close();
      await relay.stop();
    });
  });

  group('a refusal', () {
    /// **A refusal is mapped from its `kind`, over a socket, and the projection the intent was composed from stands**
    /// (#198).
    test('the wall\'s no reaches the client with its kind, and the connection stays usable', () async {
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(deviceKey),
      );
      await carrier.connect();
      final client = Client();
      await client.connect(carrier);
      final wire = await relay.next;

      final reading = client.read();
      await wire.frame();
      await wire.send(relayAnswer(reads[0]));
      await reading;

      final acting = client.act('pane.close', {'pane': 99});
      await wire.frame();
      await wire.send(relayAnswer(ruleRefusal));
      await expectLater(
        acting,
        throwsA(
          isA<Refusal>()
              .having((refusal) => refusal.kind, 'kind', RefusalKind.rule)
              .having((refusal) => refusal.next, 'next', Next.reread),
        ),
        reason: 'a refusal\'s action comes from its kind, never from its sentence (`INV-07`, #198)',
      );
      expect(
        client.wall,
        isNotNull,
        reason: 'a refusal moved nothing, so the projection it was composed from stands',
      );

      // **The connection is still usable**: a rule is not a broken wire.
      final again = client.read();
      await wire.frame();
      await wire.send(relayAnswer(reads[1]));
      expect(
        (await again).focused,
        2,
        reason: 'the read after a rule is answered on the same connection',
      );

      await carrier.close();
      await relay.stop();
    });

    /// **A version the wall does not speak is a stop, and the carrier does not retry it** (`ADR-0008` decision 5).
    ///
    /// The falsifier is a carrier that redials or re-asks after `kind: version` — a phone arguing with an update.
    test('a version refusal stops the carrier: no dial, no ask, no frame', () async {
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(deviceKey),
      );
      await carrier.connect();
      final client = Client();
      await client.connect(carrier);
      final wire = await relay.next;

      final reading = client.read();
      await wire.frame();
      await wire.send(relayAnswer(versionRefusal));
      await expectLater(
        reading,
        throwsA(
          isA<Refusal>().having(
            (refusal) => refusal.kind,
            'kind',
            RefusalKind.version,
          ),
        ),
        reason: 'the wall refused this build\'s version by name',
      );
      expect(
        client.stop,
        isNotNull,
        reason:
            'the client stopped, and the update screen is drawn from the stop',
      );

      // **The carrier stopped too**, and says so without touching the socket.
      await expectLater(
        carrier.watch(),
        throwsA(
          isA<Refusal>().having(
            (refusal) => refusal.kind,
            'kind',
            RefusalKind.version,
          ),
        ),
        reason: 'the carrier offered a watch to a wall that has already said it cannot speak to this build',
      );
      await expectLater(
        carrier.send('{"v":$protocol,"read":"wall"}'),
        throwsA(
          isA<Refusal>().having(
            (refusal) => refusal.kind,
            'kind',
            RefusalKind.version,
          ),
        ),
        reason: 'the carrier wrote a line after a version refusal: a stop is not a retry',
      );
      expect(
        await wire.quietFor(const Duration(milliseconds: 150)),
        isTrue,
        reason: 'a line reached the relay after `kind: version`, which is the ticket\'s falsifier',
      );

      // And a dial is refused the same way — a carrier that reconnected would be a session arguing with its update.
      await carrier.close();
      await expectLater(
        carrier.connect(),
        throwsA(
          isA<Refusal>().having(
            (refusal) => refusal.kind,
            'kind',
            RefusalKind.version,
          ),
        ),
        reason: 'the carrier redialled after a version refusal',
      );
      expect(
        await relay.quietFor(const Duration(milliseconds: 150)),
        isTrue,
        reason: 'the relay saw a second connection from a phone the wall has stopped',
      );

      await relay.stop();
    });
  });

  group('a revoked phone', () {
    /// **What a revoked phone can observe: silence** (#256).
    ///
    /// `pair.forget` deletes the row, and `src/control/relay.rs` answers the next request `that device is not paired`,
    /// `kind: rule`, **with no `to`** — the wall has no row, so it has no name to address, and the frame stops at the
    /// relay. `docs/control.md` says `to` is how the relay routes an answer back to the device that asked: a frame
    /// without one names no device, so a relay that routes by `to` has nothing to route it to. The phone's read is
    /// written and never answered.
    ///
    /// This test makes the fake relay do exactly what `RelayCarrier::refuse` does — write the refusal, with no `to` —
    /// and proves the phone cannot hear it: the read does not land, and what it gets is [Unanswered] (which the client
    /// reports as [Lost]), not a refusal. **Nothing on this wire says "not paired" to the phone**, so no screen can say
    /// it either; `mobile-remote.md` decision 1's "not connected" is what a read that did not land draws.
    test(
      'a refused key is silence — no "to", no answer, a read that did not land',
      () async {
        final relay = await FakeRelay.start();
        final carrier = RelayCarrier(
          relay: relay.url,
          keys: MemoryKeyStore(deviceKey),
          answerWithin: const Duration(milliseconds: 200),
        );
        await carrier.connect();
        final wire = await relay.next;

        final receiving = carrier.receive();
        await carrier.send('{"v":$protocol,"read":"wall"}');
        expect(
          await wire.frame(),
          {'v': protocol, 'device': deviceKey, 'read': 'wall'},
          reason: 'the phone asks as it always does: a revoked key is a stranger\'s only because no row holds it',
        );

        // **The wall's own answer, to the relay**, exactly as `RelayCarrier::refuse` writes it: no `to`, because the wall
        // has no name for a key it does not know. `docs/control.md` says `to` is how the relay routes an answer back, so a
        // relay that routes on `to` has nothing to route: the line stops here, on the relay's side of the wire.
        final notPaired = <String, Object?>{
          'v': protocol,
          'ok': false,
          'why': 'that device is not paired',
          'kind': 'rule',
        };
        expect(
          notPaired.containsKey('to'),
          isFalse,
          reason: 'a device the wall cannot name is not addressed, and the relay routes on the name',
        );
        // The fake relay **is** the phone's peer, so writing this line would be routing it; the relay that routed on `to`
        // writes nothing, which is the silence the rest of this test is about.

        // **The phone's read never lands.** The deadline is the only thing that ends it, and the sentence is
        // `Unanswered` — not a refusal, because nothing the phone can read said no.
        await expectLater(
          receiving,
          throwsA(isA<Unanswered>()),
          reason:
              'a revoked phone observed something other than silence: the wall\'s refusal has no `to`, so it cannot '
              'reach the phone, and a carrier that invented a refusal would be a second authority',
        );
        expect(
          carrier.isConnected,
          isFalse,
          reason: 'a request that expired left the connection up: a late answer could not be told from the next one\'s',
        );
        expect(
          await wire.quietFor(const Duration(milliseconds: 50)),
          isTrue,
          reason: 'the phone wrote something after the read expired: a read that did not land is not retried by a carrier',
        );

        await carrier.close();
        await relay.stop();
      },
    );

    /// **If the relay did route the unaddressed refusal back anyway, the phone sees a `rule` and not a stop.**
    ///
    /// A relay is somebody else's process: it may choose to answer on the connection the request arrived on, though the
    /// pinned contract does not require it. Then the phone reads the wall's words and category — `rule`, whose action is
    /// a read — and it is **not** a version: the build is fine, the key is not. What the screen says about a `rule`
    /// refusal is `docs/mobile-ui.md`'s, and "your key was revoked" is not a string this ticket invents.
    test('if the refusal is routed back, it is a rule and the session is not stopped', () async {
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(deviceKey),
      );
      await carrier.connect();
      final client = Client();
      await client.connect(carrier);
      final wire = await relay.next;

      final reading = client.read();
      await wire.frame();
      await wire.send({
        'v': protocol,
        'to': 'phone',
        'ok': false,
        'why': 'that device is not paired',
        'kind': 'rule',
      });
      await expectLater(
        reading,
        throwsA(
          isA<Refusal>()
              .having(
                (refusal) => refusal.why,
                'why',
                'that device is not paired',
              )
              .having((refusal) => refusal.kind, 'kind', RefusalKind.rule)
              .having((refusal) => refusal.next, 'next', Next.reread),
        ),
        reason: 'the wall\'s words are carried verbatim, and the kind is what the client acts on (#198)',
      );
      expect(
        client.stop,
        isNull,
        reason: 'a revoked key is not a version this build cannot speak: nothing is stopped',
      );
      expect(
        client.wall,
        isNull,
        reason: 'the read did not land, so the client holds nothing',
      );

      await carrier.close();
      await relay.stop();
    });

    /// **A live watch is not cut off by a revocation**: the next *request* meets the silence, not the stream.
    ///
    /// The wall's publish path writes to the device it is watching and reads no `devices.toml` (`RelayCarrier::write_line`
    /// and `publish`); the file is read **per request** (`docs/control.md`). So a phone that was already watching keeps
    /// receiving publishes until its connection ends — which is a fact a screen drawn for a revoked phone has to live
    /// with, and the reason the sentence this ticket writes is about silence rather than about a cut stream.
    test('a watch open before the revocation keeps publishing until the connection ends', () async {
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(deviceKey),
      );
      await carrier.connect();
      final client = Client();
      await client.connect(carrier);
      final wire = await relay.next;

      final reading = client.read();
      await wire.frame();
      await wire.send(relayAnswer(reads[0]));
      await reading;
      final published = <Projection>[];
      final listening = carrier.published.listen(published.add);
      final watching = carrier.watch();
      await wire.frame();
      await wire.send(grant());
      await watching;

      // `pair.forget phone` on the wall: the row goes. The publishes do not, because nothing on the publish path reads
      // the file (src/control/relay.rs).
      await wire.send(publishOf(reads[1]));
      await until(
        () => published.isNotEmpty,
        'the publish after the revocation',
      );
      expect(
        [for (final projection in published) projection.watch],
        [Watch.wall],
        reason: 'the revocation cut a live watch: the wall\'s publish path reads no devices.toml, so it cannot',
      );

      // **The sentence a revoked phone gets**, from the same wall: the next *request* is a stranger's, and its answer has
      // no `to` (`RelayCarrier::refuse`), so the phone's read is the one the test above proves — silent. What this test
      // adds is that the stream went on until then, and the connection is what ends it, not the revocation.
      await wire.send(publishOf(reads[0]));
      await until(
        () => published.length == 2,
        'the second publish after the revocation',
      );
      expect(
        published.length,
        2,
        reason: 'a second publish after the revocation did not arrive: the wall publishes until the connection ends',
      );

      await listening.cancel();
      await carrier.close();
      await relay.stop();
    });
  });

  group('the key', () {
    /// **The one secret in this ticket never becomes a string that is printed** (`INV-08`).
    ///
    /// The falsifier is the ordinary Dart mistake: `ArgumentError.value(key, 'key')`, a `toString` that lists the
    /// carrier's fields, or a refusal that quotes the key it did not recognise. The test formats every error path and
    /// every object's `toString`, and asserts the key's characters appear in none of them.
    test('the device key is in "device" and in no message, exception or toString', () async {
      final stored = <(String?, Object)>[
        (deviceKey, const NoDeviceKey()),
        (deviceKey, const NotADeviceKey()),
        (deviceKey, const NotARelay('there is no port')),
        (deviceKey, const Unanswered()),
        (deviceKey.toUpperCase(), const NotADeviceKey()),
        (deviceKey.substring(1), const NotADeviceKey()),
      ];
      for (final (key, error) in stored) {
        final carrier = RelayCarrier(
          relay: Uri.parse('tcp://127.0.0.1:1'),
          keys: MemoryKeyStore(key),
        );
        final texts = <String>[
          '$carrier',
          '$error',
          carrier.published.toString(),
          '${const NotADeviceKey()}',
        ];
        for (final text in texts) {
          for (final secret in [deviceKey, deviceKey.toUpperCase()]) {
            expect(
              text.contains(secret),
              isFalse,
              reason:
                  'INV-08: the device key reached a printed string ($text). A key that can be interpolated into a '
                  'message will be, and the one copy of it is the phone\'s',
            );
          }
        }
        await carrier.close();
      }

      // **And the frames carry it exactly once, in `device`, lowercase.** A key in another case is a different string,
      // and the wall hashes the string it is given (src/config/devices.rs, `Key::from_hex`).
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(deviceKey),
      );
      await carrier.connect();
      final wire = await relay.next;
      final reading = carrier.receive();
      await carrier.send('{"v":$protocol,"read":"wall"}');
      final frame = await wire.frame();
      expect(
        frame['device'],
        deviceKey,
        reason: 'the key is the store\'s own string, lowercase hex',
      );
      expect(
        jsonEncode(frame).split(deviceKey).length - 1,
        1,
        reason: 'the key appears in more than one field of a request: the only field it may be in is "device"',
      );
      await wire.send(relayAnswer(reads[0]));
      await reading;
      await carrier.close();
      await relay.stop();
    });

    test('a phone that has not paired dials nothing, and says so by name', () async {
      final relay = await FakeRelay.start();
      final carrier = RelayCarrier(
        relay: relay.url,
        keys: MemoryKeyStore(null),
      );
      await expectLater(
        carrier.connect(),
        throwsA(isA<NoDeviceKey>()),
        reason: 'a phone with no key dialled a relay: pairing is #260, and a frame with no key is a stranger\'s',
      );
      expect(
        await relay.quietFor(const Duration(milliseconds: 100)),
        isTrue,
        reason:
            'a connection was made by a phone with nothing to prove who it is',
      );
      await carrier.close();
      await relay.stop();
    });

    test('a key in any case but lowercase hex is not a key', () async {
      for (final notAKey in [
        deviceKey.toUpperCase(),
        deviceKey.substring(1),
        '',
        '${deviceKey}0',
      ]) {
        final carrier = RelayCarrier(
          relay: Uri.parse('tcp://127.0.0.1:1'),
          keys: MemoryKeyStore(notAKey),
        );
        await expectLater(
          carrier.connect(),
          throwsA(isA<NotADeviceKey>()),
          reason:
              'a store that held ${notAKey.length} characters of the wrong shape was dialled with: a key is 64 '
              'lowercase hex characters, and the wall hashes the string it is given',
        );
        await carrier.close();
      }
    });

    test(
      'a relay is tcp:// or tls://, and a wss:// relay is a ticket of its own',
      () {
        final store = MemoryKeyStore(deviceKey);
        expect(
          () => RelayCarrier(
            relay: Uri.parse('wss://relay.example.test/'),
            keys: store,
          ),
          throwsA(isA<NotARelay>()),
          reason:
              'a wss:// relay needs a websocket carrier, which is not this ticket: the phone refuses the URL rather '
              'than dialling something it cannot speak (docs/control.md, "what this build dials")',
        );
        expect(
          () => RelayCarrier(relay: Uri.parse('tcp://127.0.0.1'), keys: store),
          throwsA(isA<NotARelay>()),
        );
        expect(
          () => RelayCarrier(relay: Uri.parse('tcp://:7030'), keys: store),
          throwsA(isA<NotARelay>()),
        );
        // `tls://` is what this build dials with `SecureSocket.connect`. It is parsed here and **not proved end to end**:
        // a TLS relay needs a certificate, and a certificate a test made up is a fixture nobody recorded.
        expect(
          () => RelayCarrier(
            relay: Uri.parse('tls://relay.example.test:7030'),
            keys: store,
          ),
          returnsNormally,
          reason: 'tls:// is one of the two schemes this build dials (docs/control.md)',
        );
      },
    );
  });
}

// -----------------------------------------------------------------------------------------------------------------
// The recording, read by path. The answers are the wall's; the only field added to one is `to`, which the relay adds.
// -----------------------------------------------------------------------------------------------------------------

/// One recorded exchange: the request a client sent, and the wall's answer **verbatim**.
final class Exchange {
  const Exchange(this.send, this.expect);

  final Map<String, Object?> send;
  final Map<String, Object?> expect;
}

List<Exchange> readCorpus() {
  final file = File(corpusPath);
  if (!file.existsSync()) {
    throw StateError(
      'no corpus at $corpusPath: the answers in this suite are the wall\'s own recording '
      '(cargo run -- --record), and a projection written here would be a wall nobody has',
    );
  }
  final exchanges = <Exchange>[];
  for (final line in const LineSplitter().convert(file.readAsStringSync())) {
    if (line.trim().isEmpty) {
      continue;
    }
    final decoded = jsonDecode(line);
    if (decoded is! Map<String, Object?>) {
      continue;
    }
    final send = decoded['send'];
    final expect = decoded['expect'];
    if (send is Map<String, Object?> && expect is Map<String, Object?>) {
      exchanges.add(Exchange(send, expect));
    }
  }
  return exchanges;
}

/// Whether a recorded send is exactly a read: the version and `"read": "wall"`, and nothing else.
bool isRead(Map<String, Object?> send) =>
    send.length == 2 && send['v'] == protocol && send['read'] == 'wall';

/// The body of a recorded read's answer, which is what a publish carries.
Map<String, Object?> wallOf(Map<String, Object?> readAnswer) {
  final wall = readAnswer['wall'];
  if (wall is Map<String, Object?>) {
    return wall;
  }
  throw StateError(
    'a recorded read answer has no "wall": fixtures/control/session.jsonl',
  );
}

Map<String, Object?> firstWhere(
  List<Exchange> corpus,
  bool Function(Exchange) wanted,
) {
  for (final exchange in corpus) {
    if (wanted(exchange)) {
      return exchange.expect;
    }
  }
  throw StateError(
    'the recording holds no exchange this suite needs: fixtures/control/session.jsonl',
  );
}

/// **An answer as the relay writes it**: the socket's answer with `to` on it, and nothing else added.
///
/// `src/control/relay.rs`'s `write_line` is the same code: `value[frame::TO] = json!(to)` over the line `wire::reply`
/// wrote, and its test asserts the stripped frame is that line byte for byte.
Map<String, Object?> relayAnswer(
  Map<String, Object?> answer, {
  String to = 'phone',
}) {
  final frame = <String, Object?>{'v': answer['v'], 'to': to};
  answer.forEach((key, value) {
    if (key != 'v') {
      frame[key] = value;
    }
  });
  return frame;
}

/// **The grant** (#221): one answer to the ask, with the stream's name on it and no projection — the client has just
/// read one, and the wall's baseline is the `seq` it held when it granted (`src/control/server.rs`'s `grant`).
Map<String, Object?> grant({String to = 'phone'}) => {
  'v': protocol,
  'to': to,
  'ok': true,
  'watch': Watch.wall.word,
};

/// **A publish**: a recorded read's body, with the watch it belongs to on it. A publish is a read's answer with `watch`
/// on it, so a body this test made up would be a projection nobody sent.
Map<String, Object?> publishOf(
  Map<String, Object?> readAnswer, {
  String to = 'phone',
}) => {
  'v': protocol,
  'to': to,
  'ok': true,
  'watch': Watch.wall.word,
  'wall': wallOf(readAnswer),
};

// -----------------------------------------------------------------------------------------------------------------
// The fake relay: a ServerSocket on loopback speaking the documented frames.
// -----------------------------------------------------------------------------------------------------------------

/// **A relay on loopback** (#256), with no clock a test does not control and no frame it did not read.
///
/// It is a `ServerSocket`, because the phone on the other end of a real network is a real socket: `tcp://` and the
/// socket's own framing are what this ticket's carrier is. Each accepted connection is a [RelayEnd]; the test reads the
/// phone's lines and writes the relay's, one at a time, so every claim is about a frame rather than about a race.
final class FakeRelay {
  FakeRelay._(this._server);

  final ServerSocket _server;
  final List<RelayEnd> _accepted = <RelayEnd>[];
  final List<RelayEnd> _unread = <RelayEnd>[];
  Completer<RelayEnd>? _waiting;

  /// A relay listening on a port the machine gives it, or on [port] when a test needs the address first.
  static Future<FakeRelay> start({int port = 0}) async {
    final server = await ServerSocket.bind(InternetAddress.loopbackIPv4, port);
    final relay = FakeRelay._(server);
    server.listen((socket) {
      final end = RelayEnd(socket);
      relay._accepted.add(end);
      final waiting = relay._waiting;
      if (waiting != null) {
        relay._waiting = null;
        waiting.complete(end);
      } else {
        relay._unread.add(end);
      }
    });
    return relay;
  }

  /// The address the phone dials.
  Uri get url => Uri.parse('tcp://127.0.0.1:${_server.port}');

  /// **The next connection the phone makes.** Not a stream a test can fall behind: one connection at a time.
  Future<RelayEnd> get next async {
    if (_unread.isNotEmpty) {
      return _unread.removeAt(0);
    }
    final waiting = Completer<RelayEnd>();
    _waiting = waiting;
    return waiting.future;
  }

  /// Whether **nothing** connected within [within]: the falsifier of a dial after a stop.
  Future<bool> quietFor(Duration within) async {
    final before = _accepted.length;
    await Future<void>.delayed(within);
    if (_accepted.length == before) {
      return true;
    }
    // A connection that did arrive is closed, so a test looking for silence does not leave a socket open.
    for (final end in _accepted) {
      end.hangUp();
    }
    return false;
  }

  Future<void> stop() async {
    for (final end in _accepted) {
      end.hangUp();
    }
    await _server.close();
  }
}

/// **The relay's end of one connection**: one JSON object per line, in both directions.
final class RelayEnd {
  RelayEnd(this._socket) {
    _socket
        .cast<List<int>>()
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(_onLine, onDone: () => _closed = true);
  }

  final Socket _socket;
  final List<Object?> _incoming = <Object?>[];
  Completer<void>? _arrived;
  bool _closed = false;

  void _onLine(String line) {
    if (line.trim().isEmpty) {
      return;
    }
    _incoming.add(jsonDecode(line));
    final arrived = _arrived;
    _arrived = null;
    arrived?.complete();
  }

  /// Write bytes as they are: a frame split across packets is still one line.
  void writeBytes(List<int> bytes) => _socket.add(bytes);

  /// The next frame the phone wrote, or a failure naming what did not arrive.
  Future<Map<String, Object?>> frame({
    Duration within = const Duration(seconds: 5),
  }) async {
    final next = await _next(within);
    if (next is Map<String, Object?>) {
      return next;
    }
    fail(
      'the phone wrote something that is not a JSON object: ${jsonEncode(next)}',
    );
  }

  /// **Write one frame** from the relay.
  Future<void> send(Map<String, Object?> frame) async {
    _socket.add(utf8.encode('${jsonEncode(frame)}\n'));
    await _socket.flush();
  }

  /// Whether **nothing** arrived within [within]: the tap that catches a carrier sending what it should not.
  Future<bool> quietFor(Duration within) async {
    if (_incoming.isNotEmpty) {
      return false;
    }
    if (!_closed) {
      final arrived = Completer<void>();
      _arrived = arrived;
      await Future.any<void>([arrived.future, Future<void>.delayed(within)]);
    }
    return _incoming.isEmpty;
  }

  Future<Object?> _next(Duration within) async {
    if (_incoming.isEmpty && !_closed) {
      final arrived = Completer<void>();
      _arrived = arrived;
      await Future.any<void>([arrived.future, Future<void>.delayed(within)]);
    }
    if (_incoming.isEmpty) {
      fail('the phone wrote nothing within ${within.inMilliseconds} ms');
    }
    return _incoming.removeAt(0);
  }

  /// The relay goes away — a restart, a redeploy, a wire that was cut.
  void hangUp() => _socket.destroy();
}

/// **A key store in memory**, which is what the ticket asks a test to use: the app's is the Keychain or the Keystore,
/// and neither is this package's (#260).
final class MemoryKeyStore implements KeyStore {
  MemoryKeyStore(this.maybeKey);

  /// The key, or `null` for a phone that has not paired.
  final String? maybeKey;

  @override
  Future<String?> key() async => maybeKey;
}

/// **Let the socket deliver what it has.** A real delay and not `pumpEventQueue`: a zero-duration timer turns microtasks,
/// and a socket's line is an IO event that is not promised by that. Fifty milliseconds is an aeon on loopback and is not
/// a clock the test's claims depend on — every wait that *must* happen is [until].
Future<void> settle([
  Duration within = const Duration(milliseconds: 50),
]) async {
  await Future<void>.delayed(within);
}

/// Wait for a condition a socket makes: an end of file arrives on the reader's own turn, and a test that assumed it had
/// already is a test about a scheduler.
Future<void> until(bool Function() done, String what) async {
  final deadline = DateTime.now().add(const Duration(seconds: 5));
  while (!done()) {
    if (DateTime.now().isAfter(deadline)) {
      fail('waited five seconds for $what, and it did not happen');
    }
    await Future<void>.delayed(const Duration(milliseconds: 2));
  }
}

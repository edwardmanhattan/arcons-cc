/// **The relay's routing, over real loopback sockets** (#268).
///
/// Every assertion here is a field on a wire, which is the class of bug the fakes could not catch: a forwarded frame
/// that is not byte-identical, an answer with `to` that reaches nobody, the `pair.claim` answer that has **no** `to`
/// and must go back to the connection that asked, and a log line that would print a key.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:dev_relay/relay.dart';
import 'package:test/test.dart';

/// One end of a connection, with the lines it has received so far.
final class Peer {
  Peer._(this._socket, this._lines);

  static Future<Peer> connect(int port) async {
    final socket = await Socket.connect('127.0.0.1', port);
    final lines = StreamIterator<String>(
      socket.cast<List<int>>().transform(const Utf8Decoder()).transform(const LineSplitter()),
    );
    return Peer._(socket, lines);
  }

  final Socket _socket;
  final StreamIterator<String> _lines;

  void send(Map<String, Object?> frame) => _socket.add(utf8.encode('${jsonEncode(frame)}\n'));

  void sendRaw(String line) => _socket.add(utf8.encode('$line\n'));

  /// The next line, decoded — with a timeout, because a dropped frame is exactly what these tests look for.
  Future<Map<String, Object?>> next() async {
    final arrived = await _lines.moveNext().timeout(const Duration(seconds: 2));
    if (!arrived) {
      throw StateError('the connection closed before a line arrived');
    }
    return jsonDecode(_lines.current) as Map<String, Object?>;
  }

  Future<void> close() async {
    await _socket.close();
    _socket.destroy();
  }
}

/// A relay on an ephemeral port, plus every line it said (for the redaction test).
final class Harness {
  final List<String> said = <String>[];
  late final DevRelay relay = DevRelay(report: said.add);

  Future<void> start() async {
    await relay.start(port: 0);
  }

  Future<Peer> wall() async {
    final peer = await Peer.connect(relay.port);
    peer.send(<String, Object?>{'v': 1, 'hello': <String, Object?>{'credential': <String, Object?>{'refresh_token': 'x'}}});
    return peer;
  }

  Future<Peer> phone() => Peer.connect(relay.port);

  Future<void> close() => relay.close();
}

void main() {
  late Harness harness;

  setUp(() async {
    harness = Harness();
    await harness.start();
  });

  tearDown(() => harness.close());

  test('the wall says hello and is welcomed', () async {
    final wall = await harness.wall();
    expect(await wall.next(), <String, Object?>{'v': 1, 'welcome': true});
    await wall.close();
  });

  test('a phone\'s frame reaches the wall byte for byte', () async {
    final wall = await harness.wall();
    await wall.next();
    final phone = await harness.phone();

    // The exact bytes the phone app writes (`clients/phone/lib/relay.dart`): one object, one line.
    const line = '{"v":1,"device":"9a2db2e2","intent":{"verb":"pane.focus","pane":2}}';
    phone.sendRaw(line);

    final decoded = await wall.next();
    expect(jsonEncode(decoded), line, reason: 'a forwarded frame must be the frame: one field renamed is a bug');
    await phone.close();
    await wall.close();
  });

  test('an answer with "to" reaches the phone that asked, and publishes keep arriving', () async {
    final wall = await harness.wall();
    await wall.next();
    final phone = await harness.phone();
    phone.send(<String, Object?>{'v': 1, 'device': '9a2d', 'read': 'wall'});
    await wall.next();

    wall.send(<String, Object?>{'v': 1, 'to': 'phone', 'ok': true, 'wall': <String, Object?>{'seq': 1}});
    expect(await phone.next(), containsPair('to', 'phone'));

    // The name was learnt from that answer, so a publish with nobody asking still finds the device.
    wall.send(<String, Object?>{
      'v': 1,
      'to': 'phone',
      'ok': true,
      'watch': 'wall',
      'wall': <String, Object?>{'seq': 2},
    });
    final published = await phone.next();
    expect(published, containsPair('watch', 'wall'));
    expect(published['wall'], containsPair('seq', 2), reason: 'the publish body is the read\'s body, untouched');

    await phone.close();
    await wall.close();
  });

  test('an answer with NO "to" goes back to the connection that asked — the pairing frame', () async {
    // This is the one piece of routing that cannot be looked up: `pair.claim` arrives from a phone with no key, and its
    // answer carries no `to` because the wall has no name for a device it has not paired (#262).
    final wall = await harness.wall();
    await wall.next();
    final phone = await harness.phone();
    phone.send(<String, Object?>{
      'v': 1,
      'intent': <String, Object?>{'verb': 'pair.claim', 'code': '481902', 'name': 'phone'},
    });
    expect(await wall.next(), containsPair('v', 1));

    wall.send(<String, Object?>{
      'v': 1,
      'ok': true,
      'paired': <String, Object?>{'name': 'phone', 'key': 'ab' * 32},
    });

    final answer = await phone.next();
    expect(answer, containsPair('ok', true));
    expect(answer.containsKey('to'), isFalse, reason: 'the relay does not invent an address the wall did not give');
    await phone.close();
    await wall.close();
  });

  test('a phone frame with no wall is dropped, and said out loud', () async {
    final phone = await harness.phone();
    phone.send(<String, Object?>{'v': 1, 'device': '9a2d', 'read': 'wall'});
    await Future<void>.delayed(const Duration(milliseconds: 50));
    expect(harness.said.join('\n'), contains(Words.unroutable));
    await phone.close();
  });

  test('an answer with no "to" and nobody asking is dropped', () async {
    final wall = await harness.wall();
    await wall.next();
    wall.send(<String, Object?>{'v': 1, 'ok': true, 'changed': true});
    await Future<void>.delayed(const Duration(milliseconds: 50));
    expect(harness.said.join('\n'), contains(Words.unroutable));
    await wall.close();
  });

  test('the log redacts the device key and the pairing key, and nothing else', () async {
    final wall = await harness.wall();
    await wall.next();
    final phone = await harness.phone();
    phone.send(<String, Object?>{
      'v': 1,
      'device': '9a2db2e2deadbeef' * 4,
      'intent': <String, Object?>{'verb': 'pane.focus', 'pane': 2},
    });
    await wall.next();
    wall.send(<String, Object?>{
      'v': 1,
      'ok': true,
      'paired': <String, Object?>{'name': 'phone', 'key': 'ab' * 32},
    });

    final log = harness.said.join('\n');
    expect(log, isNot(contains('9a2db2e2deadbeef')), reason: 'the device key must never be logged (INV-08)');
    expect(log, isNot(contains('abababab')), reason: 'the pairing key must never be logged (INV-08)');
    expect(log, contains('"device":"…"'), reason: 'redacted by field, so the rest of the frame is still readable');
    await phone.close();
    await wall.close();
  });
}

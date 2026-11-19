/// **A development relay** — `docs/control.md`'s relay frames, and nothing else (#268).
///
/// The wall dials out and a phone dials in; this process carries their lines between them. It is the smallest thing
/// that makes the phone's path real: every other component in this epic has only ever met a fake, and the bugs that
/// hides are all fields on a wire.
///
/// **It is not the contract.** No authentication, no persistence, no TLS, no rate limiting (`docs/control.md`,
/// *The relay*: the relay's own process is somebody else's). What it does implement, exactly:
///
/// - the wall says `{"v": 1, "hello": {…}}` first and gets `{"v": 1, "welcome": true}`;
/// - a phone's frame is forwarded to the wall **byte for byte**;
/// - the wall's answer with `"to": "<name>"` goes to the connection that name was learnt on;
/// - the wall's answer with **no `to`** goes back to the connection the request arrived on — which is the only address a
///   device with no name has, and is exactly `pair.claim` (#262);
/// - a frame it cannot route is dropped and logged, never guessed at and never broadcast.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

/// The relay's own words, kept in one place so the log and the tests read the same.
abstract final class Words {
  static const String hello = 'hello';
  static const String welcome = 'welcome';
  static const String to = 'to';
  static const String device = 'device';
  static const String key = 'key';

  /// A frame the relay could not route. Said out loud rather than swallowed: a dev tool's silence is the thing a
  /// person is trying to debug.
  static const String unroutable = 'nothing to route this to';
}

/// What the relay says about a frame, for a `--verbose` run and for the tests.
typedef Report = void Function(String line);

/// The relay itself: one listening socket, one wall, as many phones as connect.
final class DevRelay {
  DevRelay({this.report, this.redact = true});

  /// Where a line goes when `--verbose` is on. `null` is silence.
  final Report? report;

  /// **Whether the log redacts the two secrets a frame can carry** (`INV-08`): the device key, and the pairing key in
  /// `paired`. A dev tool that prints a key is how a key ends up in a paste.
  final bool redact;

  ServerSocket? _server;
  final List<Socket> _walls = <Socket>[];
  final Map<String, Socket> _byName = <String, Socket>{};
  final Map<Socket, Socket> _askedFrom = <Socket, Socket>{};
  final List<Socket> _phones = <Socket>[];

  /// The port the relay is listening on, once [start] has returned.
  int get port => _server?.port ?? 0;

  /// **Start listening.** `127.0.0.1` by default: a laptop's relay is for a phone on the same desk, and `0.0.0.0` is a
  /// deliberate `--host` for a phone in the same room.
  Future<void> start({String host = '127.0.0.1', int port = 7000}) async {
    _server = await ServerSocket.bind(host, port);
    _server!.listen(_accept);
  }

  Future<void> close() async {
    for (final socket in [..._walls, ..._phones]) {
      socket.destroy();
    }
    await _server?.close();
    _server = null;
  }

  void _accept(Socket socket) {
    var buffer = '';
    socket
        .cast<List<int>>()
        .transform(const Utf8Decoder(allowMalformed: true))
        .listen(
      (chunk) {
        buffer += chunk;
        while (true) {
          final newline = buffer.indexOf('\n');
          if (newline < 0) {
            break;
          }
          final line = buffer.substring(0, newline);
          buffer = buffer.substring(newline + 1);
          if (line.trim().isNotEmpty) {
            _from(socket, line);
          }
        }
      },
      onDone: () => _gone(socket),
      onError: (_) => _gone(socket),
      cancelOnError: true,
    );
  }

  void _from(Socket socket, String line) {
    final Object? frame;
    try {
      frame = jsonDecode(line);
    } on FormatException {
      _say('dropped $socket: not JSON (${_safe(line)})');
      return;
    }
    if (frame is! Map<String, Object?>) {
      _say('dropped $socket: not a JSON object');
      return;
    }

    // **The wall is whoever said `hello`.** There is no other way to be one, and nothing else about the wall is
    // inspected: the relay routes, it does not judge.
    if (frame.containsKey(Words.hello)) {
      if (!_walls.contains(socket)) {
        _walls.add(socket);
        socket.add(utf8.encode('${jsonEncode(<String, Object?>{'v': 1, Words.welcome: true})}\n'));
        _say('wall connected on port $port');
      }
      return;
    }

    if (_walls.contains(socket)) {
      _toPhone(socket, frame, line);
      return;
    }

    _toWall(socket, frame, line);
  }

  /// A phone's frame: to the wall, **byte for byte**, and remember which phone asked so a `to`-less answer can find
  /// its way back.
  void _toWall(Socket phone, Map<String, Object?> frame, String line) {
    if (!_phones.contains(phone)) {
      _phones.add(phone);
    }
    if (_walls.isEmpty) {
      _say('${Words.unroutable}: no wall is connected, dropped a phone frame');
      return;
    }
    for (final wall in _walls) {
      _askedFrom[wall] = phone;
      wall.add(utf8.encode('$line\n'));
    }
    _say('phone → wall: ${_safe(line)}');
  }

  /// The wall's answer or publish: by `to` when it names one, and otherwise back to whoever asked.
  void _toPhone(Socket wall, Map<String, Object?> frame, String line) {
    final named = frame[Words.to];
    Socket? target;
    if (named is String) {
      // **The name is learnt from the wall's own answers** (`docs/control.md`: `to` is how the relay routes a frame
      // back), so the relay never has to hash a key or read `devices.toml`.
      target = _askedFrom[wall];
      if (target != null) {
        _byName[named] = target;
      }
      target = _byName[named] ?? target;
    } else {
      target = _askedFrom[wall];
    }

    if (target == null) {
      _say('${Words.unroutable}: ${_safe(line)}');
      return;
    }
    target.add(utf8.encode('$line\n'));
    _say('wall → ${named ?? 'the asker'}: ${_safe(line)}');
  }

  void _gone(Socket socket) {
    final wasWall = _walls.remove(socket);
    _phones.remove(socket);
    _askedFrom.remove(socket);
    _byName.removeWhere((_, phone) => phone == socket);
    _say(wasWall ? 'wall disconnected' : 'phone disconnected');
  }

  /// **The log line, with the two secrets taken out.** Not a filter over prose: `device` and `paired.key` are the only
  /// two fields that can carry a key, and both are replaced by name.
  String _safe(String line) {
    if (!redact) {
      return line;
    }
    return line
        .replaceAllMapped(
          RegExp('"${Words.device}"\\s*:\\s*"[^"]*"'),
          (_) => '"${Words.device}":"…"',
        )
        .replaceAllMapped(
          RegExp('"${Words.key}"\\s*:\\s*"[^"]*"'),
          (_) => '"${Words.key}":"…"',
        );
  }

  void _say(String message) => report?.call(message);
}

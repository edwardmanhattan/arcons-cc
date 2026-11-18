/// **The phone's carrier** (#256): a socket to the relay, in the frames `docs/control.md` pins.
///
/// `docs/mobile-remote.md` decision 2 is the shape of this file: **the wall does not listen.** The wall dials the
/// deployment's relay, and the phone talks to the relay — so a phone's `Carrier` (`lib/client.dart`) is a socket, and
/// what travels on it is the wall's own request and answer, **with one field on it**: the phone's device key in
/// `device`, and the device name an answer is for in `to` (`src/control/relay.rs`, *The relay's frames*).
///
/// ```text
/// a request   {"v": 1, "device": "<64 lowercase hex>", "read": "wall"}
///             {"v": 1, "device": "<64 lowercase hex>", "intent": {…}}
///             {"v": 1, "device": "<64 lowercase hex>", "watch": "wall"}
/// an answer   {"v": 1, "to": "phone", "ok": true, "wall": {…}}          the read's answer: no `watch`
/// a grant     {"v": 1, "to": "phone", "ok": true, "watch": "wall"}       one answer, like any request
/// a publish   {"v": 1, "to": "phone", "ok": true, "watch": "wall", "wall": {…}}   the read's body, with its watch
/// ```
///
/// Four rules, each of them a test in `test/relay_test.dart`, driven by a **fake relay on loopback** — because there
/// is no fixture for a watch: the unix socket refuses one by name and `fixtures/README.md` refuses a timed exchange
/// (a coalesced publish is a race, and a race is not a fixture). The projections and answers the fake relay hands back
/// are the wall's **own recording**, `fixtures/control/session.jsonl`, read by path; the stream's three lines are the
/// ones `src/control/wire.rs` and `docs/control.md` spell, and nothing else in that test is invented.
///
/// 1. **One JSON object per line, both directions**, over `tcp://host:port` (`Socket.connect`) or `tls://host:port`
///    (`SecureSocket.connect`). A request the client writes is framed here: the version stays where the client put it,
///    `device` is added, and nothing else is added — a key anywhere but `device` is a stranger, and a key in any case
///    but lowercase hex is not the key `devices.toml` hashed.
/// 2. **The key arrives from outside** ([KeyStore]): this package has no Keychain, no Keystore, no platform channel, and
///    no pairing (#260 owns both the bootstrap and the key's home). The one secret in this file is never interpolated
///    into a message, an exception or a `toString` (`INV-08`), and a test holds it to that.
/// 3. **A connection that comes back reads first and resumes nothing.** A drop takes the socket, the read that landed
///    on it and the watch that belonged to it; a reconnect is a dial ([connect]) and the app's own read, never a
///    buffer. The carrier refuses to write an intent on a connection it has not seen a read land on ([NotRead]), and
///    refuses to ask for a watch on it either — so "a watch is not resumed" is enforced here, not only in [Client].
/// 4. **Nothing waits forever.** A request with no answer inside [answerWithin] is a read that did not land: the
///    connection goes with it (a late answer could not be told from the next one's) and the caller gets [Unanswered].
///    That deadline is the only thing a **revoked** phone ever observes — see [Unanswered].
library;

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'client.dart' show Carrier, Lost, NoConnection, NotRead, Refusal;
import 'wire.dart';

/// **Where the phone keeps its device key** (#256) — the one thing this package needs from the app.
///
/// A phone is paired out of band (`pair.claim`, `docs/control.md`: *a phone pairs*), and the key it is handed is 32
/// bytes as **64 lowercase hex characters**. Its home — the Keychain, the Keystore, a file — is **not this ticket's**:
/// ADR-0008 decision 4 says written by hand, and that needs platform code and a phone in a hand (#260 records the
/// bootstrap that is still to be decided). This package asks for the key and never invents one, never stores one and
/// never writes one down.
///
/// `null` is a phone that has not paired: the carrier says so by name ([NoDeviceKey]) rather than dialling with a
/// stranger's frame. An implementation that returns anything but 64 lowercase hex characters is [NotADeviceKey].
abstract interface class KeyStore {
  /// The device key this phone paired with, 64 lowercase hex characters, or `null` when it has none.
  ///
  /// A carrier reads it on every dial, so a phone that paired while the app was running is a phone that connects —
  /// and a test can hand a different key to a second connection without a restart.
  Future<String?> key();
}

/// **This phone has no device key**: it has never paired (#260), and a relay frame without one is a stranger's.
final class NoDeviceKey implements Exception {
  const NoDeviceKey();

  @override
  String toString() =>
      'this phone has no device key: it has not paired with a wall (#260)';
}

/// **The key store handed back something that is not a device key** — 64 lowercase hex characters and nothing else.
///
/// The **value** is deliberately not in this sentence, or in any other this package writes (`INV-08`): a message that
/// quotes the thing it is complaining about is how a secret reaches a log.
final class NotADeviceKey implements Exception {
  const NotADeviceKey();

  @override
  String toString() =>
      'the key store holds no device key: a key is 64 lowercase hex characters';
}

/// **Not a relay this build dials**: `tcp://host:port` or `tls://host:port`.
///
/// The reason names the part that is wrong and never the address: a host is `INV-08`'s business too, and a URL is a
/// measurement of somebody's deployment rather than a fact about this phone.
final class NotARelay implements Exception {
  const NotARelay(this.why);

  final String why;

  @override
  String toString() => 'not a relay this build dials: $why';
}

/// **A request the relay never answered** — what a phone whose key was revoked can actually observe (#256).
///
/// The wall refuses a frame from a key no row of `devices.toml` holds as `that device is not paired`, `kind: rule`,
/// **with no `to`** (`src/control/relay.rs`: `RelayCarrier::refuse`, and the test
/// `a_key_no_row_in_devices_holds_is_refused_by_name`): the wall has no row, so it has no name to address, and the
/// frame goes to the relay and no further. `docs/control.md` says `to` is *how the relay
/// routes an answer back to the device that asked* — a frame without one names no device, so a relay that routes by
/// `to` has nothing to route it to.
///
/// What that leaves the phone is **silence**: its read is written, and no line ever comes back. This exception is
/// **not** a refusal — the wall said something, to the relay, and the phone cannot hear it — and there is no field on
/// the wire that would let the phone tell "revoked" from "the relay dropped it", "the relay is not answering" or "a
/// wall that is busy". The screen's answer to a read that did not land is `mobile-remote.md` decision 1's **"not
/// connected"**, and choosing words for a revoked key is not this ticket's to invent.
///
/// **A live watch is not cut off by a revocation**: the wall's publish path writes to the device it is watching and
/// reads no `devices.toml` (`RelayCarrier::write_line`), and the file is read **per request**. So a phone that was
/// already watching keeps receiving publishes until its connection ends, and the revocation takes effect at its next
/// request — which is the one that meets this silence.
final class Unanswered implements Exception {
  const Unanswered();

  @override
  String toString() => 'the relay did not answer';
}

/// **Whether a request is out** — what the next line from the relay is the answer to.
///
/// One request at a time, because the relay's answer carries a device name and no request id: the only thing that says
/// which question a line answers is the order the wall answered them in.
enum _Awaiting {
  /// A read: its answer is the projection the connection's knowledge comes from.
  read,

  /// A change: its answer is `changed` or a refusal, and it moves nothing.
  intent,

  /// The carrier's own watch ask: its answer is a grant, or a refusal to grant one.
  watch,
}

/// **A phone's carrier: a socket to the relay** (#256), and the watch the unix socket refuses.
///
/// The three methods of [Carrier] are the client's ([Client]): write a line, take the next answer, learn the connection
/// is gone. Everything else here is the carrier's own — the dial, the key, the watch, the publishes — because a client
/// that knew about sockets would be a client per transport.
///
/// **A carrier is a connection.** A drop surfaces as end of file ([Carrier.receive]'s `null`) and a write that cannot
/// happen ([NoConnection]); it is never papered over by a redial, because a redial would hand [Client] a live connection
/// on which it has read nothing — the one thing `mobile-remote.md` decision 1 forbids. The app dials again ([connect]),
/// hands the carrier to a client, and reads: **the read is the client's**, and the carrier's only part in "read first"
/// is refusing to write anything else before one has landed.
final class RelayCarrier implements Carrier {
  /// A carrier for [relay], which must be `tcp://host:port` or `tls://host:port`.
  ///
  /// [answerWithin] is how long a request may go unanswered before the connection is treated as not answering for this
  /// phone. Ten seconds is twice the wall's own patience with itself (`ANSWER_WITHIN`, five seconds, in
  /// `src/control/server.rs`), so a wall that is slow is not a wall that vanished. [backoffStart] and [backoffMax] are
  /// the dial's, and they are the wall's own numbers (`BACKOFF_START` 200 ms, `BACKOFF_MAX` 5 s, in
  /// `src/control/relay.rs`) so a phone and a wall do not disagree about a relay that is down.
  RelayCarrier({
    required Uri relay,
    required this._keys,
    this._answerWithin = const Duration(seconds: 10),
    this._backoffStart = const Duration(milliseconds: 200),
    this._backoffMax = const Duration(seconds: 5),
    this._dialWithin = const Duration(seconds: 5),
  }) : _relay = relay {
    _check(relay);
  }

  // A caller writes `keys:`, `answerWithin:`, … — Dart 3.14's private named parameters drop the underscore from the
  // name outside the library and keep the field private here.
  final Uri _relay;
  final KeyStore _keys;
  final Duration _answerWithin;
  final Duration _backoffStart;
  final Duration _backoffMax;
  final Duration _dialWithin;

  /// The connection, when there is one. **A drop takes the read and the watch with it** — nothing here could outlive
  /// the socket it came from, because that is how a projection from the connection before gets drawn as this one's.
  Socket? _socket;
  StreamSubscription<String>? _lines;

  /// The device key, exactly as the store handed it over. **Never printed** (`INV-08`): it is written into `device` on
  /// a frame and into nothing else — not a message, not an exception, not a `toString`.
  String? _key;

  /// Whether a read has landed on the connection this carrier holds now. The first line after a dial is a read, and
  /// nothing else is written until it lands.
  bool _read = false;

  /// Whether a watch is open on it. Cleared by a drop, never restored by one: a watch belongs to a connection.
  bool _watching = false;

  /// Whether [close] was called while a dial was in flight, so the dial gives up instead of reconnecting a session
  /// somebody ended.
  bool _closed = false;

  /// The dial in flight, so two callers want one connection and not two.
  Future<void>? _dialling;

  /// The refusal this carrier may not argue with: a `version` the wall does not speak ends the session, and no dial,
  /// ask or frame follows it (`ADR-0008` decision 5, `wire.dart`'s [RefusalKind.version]).
  Refusal? _stopped;

  /// What the next line from the relay answers, if a request is out.
  _Awaiting? _awaiting;

  /// The deadline on that request. Armed when a frame goes out, cancelled by its answer.
  Timer? _deadline;

  /// Answers that arrived before [receive] asked for them, in the order the relay wrote them.
  final List<String> _answers = <String>[];

  /// The call waiting for the next answer, if one is.
  Completer<String?>? _waiting;

  /// A request that expired before its answer was asked for: the reason the next [receive] gives.
  Object? _failure;

  /// The watch ask that is out, if one is.
  Completer<Granted>? _grant;

  /// The publishes of the open watch. Created on demand and closed by [close]; a broadcast stream, because a publish
  /// is not an answer and a client that is not listening for one must not lose its next answer to it.
  StreamController<Projection>? _published;

  /// The relay this carrier dials, as an operator wrote it. **Not a secret** — it is where the relay is.
  Uri get relay => _relay;

  /// Whether there is a socket under this carrier right now.
  bool get isConnected => _socket != null;

  /// Whether a read has landed on this connection. What "act" and "watch" wait for.
  bool get hasRead => _read;

  /// Whether a watch is open on this connection.
  bool get isWatching => _watching;

  /// **The wall's publishes of the open watch** (#221): whole projections, each carrying [Projection.watch].
  ///
  /// A publish is **not** an answer and never appears in [receive]: it has no request of its own, and a client with a
  /// read in flight must not be handed one as its answer (that is the ticket's falsifier). A `seq` the listener did not
  /// expect is a `read: "wall"` — `docs/control.md` says so, and this stream does not reconstruct a difference. A
  /// publish this build cannot read is an **error on this stream**, never a line in [receive]: a stream's line has no
  /// request behind it, so it may not become one.
  Stream<Projection> get published =>
      (_published ??= StreamController<Projection>.broadcast()).stream;

  /// **Dial, and keep dialling**, with backoff from [backoffStart] to [backoffMax] until the relay answers or [close]
  /// ends the attempt. Resolves when the socket is up; it does not send anything — the client's read is the first line.
  ///
  /// A dial that fails is not a refused credential: a relay that is down is worth retrying, and a phone with no key is
  /// not (that throws at once, because no number of attempts makes a key). A stopped carrier ([Refusal]) never dials.
  Future<void> connect() {
    final stop = _stopped;
    if (stop != null) {
      return Future<void>.error(stop);
    }
    if (_socket != null) {
      return Future<void>.value();
    }
    return _dialling ??= _dial().whenComplete(() => _dialling = null);
  }

  /// **Let go of the connection, and of the dial in flight.** The carrier may be [connect]ed again — an app that
  /// reconnects does exactly that — but nothing the last connection knew survives: not the read, not the watch.
  @override
  Future<void> close() async {
    _closed = true;
    if (_socket != null) {
      _dropped();
    }
    _failure = null;
    _answers.clear();
    final grant = _grant;
    _grant = null;
    if (grant != null && !grant.isCompleted) {
      grant.completeError(const NoConnection());
    }
    final waiting = _waiting;
    _waiting = null;
    if (waiting != null && !waiting.isCompleted) {
      waiting.complete(null);
    }
    final published = _published;
    _published = null;
    if (published != null && !published.isClosed) {
      await published.close();
    }
  }

  /// **Write one request**, in the relay's frame: the client's line with `device` on it and nothing else added.
  ///
  /// Refused **without touching the socket** when this connection has not had a read land on it ([NotRead]): a drop is
  /// not a queue, and an intent composed against a wall nobody has read is a change made wrongly
  /// (`mobile-remote.md` decision 1). A `watch` line is refused here by name because a watch is the carrier's ask
  /// ([watch]) and the client's version of it would be a second vocabulary.
  @override
  Future<void> send(String line) async {
    final stop = _stopped;
    if (stop != null) {
      throw stop;
    }
    if (_socket == null) {
      throw const NoConnection();
    }
    final Object? decoded;
    try {
      decoded = jsonDecode(line);
    } on FormatException {
      throw const WireException('a request is not JSON');
    }
    if (decoded is! Map<String, Object?>) {
      throw const WireException('a request is not a JSON object');
    }
    if (decoded.containsKey('device')) {
      throw const WireException(
        'a request carries no "device": the key is the carrier\'s field, not the client\'s',
      );
    }
    if (decoded.containsKey('watch')) {
      throw const WireException(
        'a watch is the carrier\'s ask (RelayCarrier.watch), not a client\'s line',
      );
    }
    final _Awaiting kind;
    if (decoded.containsKey('read')) {
      kind = _Awaiting.read;
    } else if (decoded['intent'] is Map<String, Object?>) {
      kind = _Awaiting.intent;
    } else {
      throw const WireException(
        'a request carries "read" or "intent", and this line carries neither',
      );
    }
    if (kind != _Awaiting.read && !_read) {
      throw const NotRead();
    }
    final frame = <String, Object?>{
      'v': decoded['v'] ?? protocol,
      'device': _key!,
    };
    for (final entry in decoded.entries) {
      if (entry.key == 'v') {
        continue;
      }
      frame[entry.key] = entry.value;
    }
    await _write(frame, kind);
  }

  /// **The next answer the relay wrote**, or `null` when the connection ended.
  ///
  /// A publish never appears here: it belongs to [published]. A request the relay never answered throws [Unanswered]
  /// once [answerWithin] has passed — the one thing a revoked phone observes.
  @override
  Future<String?> receive() async {
    if (_answers.isNotEmpty) {
      return _answers.removeAt(0);
    }
    final failure = _failure;
    if (failure != null) {
      _failure = null;
      throw failure;
    }
    if (_socket == null) {
      return null;
    }
    final waiting = Completer<String?>();
    _waiting = waiting;
    return waiting.future;
  }

  /// **Ask the wall to publish its projection when it moves** (#221) — one ask, one grant, and then [published].
  ///
  /// Refused **without touching the socket** when no read has landed on this connection ([NotRead]). That is
  /// `mobile-remote.md` decision 1 applied to the stream: a watch is a connection's, so a phone that has not read on
  /// this one has nothing to watch from, and the wall's grant would be a baseline the phone could not interpret.
  ///
  /// A second ask on one connection is a new baseline and not a second stream (`docs/control.md`), so asking again is
  /// a grant and not an error. A refusal is the wall's ([Refusal]) — a `version` stops the carrier, and every other
  /// kind leaves the connection and the read where they were.
  Future<Granted> watch() async {
    final stop = _stopped;
    if (stop != null) {
      throw stop;
    }
    if (_socket == null) {
      throw const NoConnection();
    }
    if (!_read) {
      throw const NotRead();
    }
    if (_grant != null) {
      throw StateError(
        'a watch ask is already out: a carrier has one request in flight',
      );
    }
    final granted = Completer<Granted>();
    _grant = granted;
    try {
      await _write(<String, Object?>{
        'v': protocol,
        'device': _key!,
        'watch': Watch.wall.word,
      }, _Awaiting.watch);
    } on Object {
      // The write failed and the connection went with it: the grant's own failure is the sentence for it, and it has
      // already been completed if there was anything to complete.
      if (!granted.isCompleted) {
        _grant = null;
        throw const NoConnection();
      }
    }
    return granted.future;
  }

  // -------------------------------------------------------------------------------------------------------------
  // The dial.
  // -------------------------------------------------------------------------------------------------------------

  Future<void> _dial() async {
    _closed = false;
    var wait = _backoffStart;
    while (true) {
      if (_closed) {
        throw const NoConnection();
      }
      // **The key is read on every dial** (a phone may have paired since the last one), and a store that holds
      // something that is not a key ends the attempt rather than dialling as a stranger.
      final key = await _keys.key();
      if (key == null) {
        throw const NoDeviceKey();
      }
      if (!_deviceKey.hasMatch(key)) {
        throw const NotADeviceKey();
      }
      try {
        final Socket socket = switch (_relay.scheme) {
          'tcp' => await Socket.connect(
            _relay.host,
            _relay.port,
            timeout: _dialWithin,
          ),
          'tls' => await SecureSocket.connect(
            _relay.host,
            _relay.port,
            timeout: _dialWithin,
          ),
          _ => throw const NotARelay('the scheme is neither "tcp" nor "tls"'),
        };
        _attach(socket, key);
        return;
      } on Object catch (error) {
        // Only a wire that did not come up is worth another try: a programming error is not a relay that is down.
        if (error is! IOException && error is! TimeoutException) {
          rethrow;
        }
        if (_closed) {
          throw const NoConnection();
        }
        await Future<void>.delayed(wait);
        wait = wait * 2 > _backoffMax ? _backoffMax : wait * 2;
      }
    }
  }

  /// Take the socket: a new connection knows nothing — no read, no watch, no answers from the last one.
  void _attach(Socket socket, String key) {
    _socket = socket;
    _key = key;
    _read = false;
    _watching = false;
    _awaiting = null;
    _answers.clear();
    _failure = null;
    _deadline?.cancel();
    _deadline = null;
    _lines = socket
        .cast<List<int>>()
        .transform(utf8.decoder)
        .transform(const LineSplitter())
        .listen(_onLine, onError: (Object _) => _dropped(), onDone: _dropped);
  }

  /// **The connection is gone**: the socket, the read, the watch, the request that was out, and every answer the last
  /// connection queued. A caller waiting for a line gets end of file; a caller waiting for a grant gets [Lost].
  void _dropped() {
    final socket = _socket;
    if (socket == null) {
      return;
    }
    _socket = null;
    final lines = _lines;
    _lines = null;
    lines?.cancel();
    socket.destroy();
    _read = false;
    _watching = false;
    _awaiting = null;
    _deadline?.cancel();
    _deadline = null;
    _answers.clear();
    final grant = _grant;
    _grant = null;
    if (grant != null && !grant.isCompleted) {
      grant.completeError(const Lost());
    }
    final waiting = _waiting;
    _waiting = null;
    if (waiting != null && !waiting.isCompleted) {
      waiting.complete(null);
    }
  }

  // -------------------------------------------------------------------------------------------------------------
  // The frames.
  // -------------------------------------------------------------------------------------------------------------

  /// **One line from the relay**: a publish, a grant, or an answer — never more than one, and never a guess.
  ///
  /// A publish is routed by its shape (`watch` **and** `wall`), because it is the only line with no request behind it.
  /// Everything else is what the request that is out is waiting for.
  void _onLine(String raw) {
    final line = raw.trim();
    if (line.isEmpty) {
      return;
    }
    final Object? decoded;
    try {
      decoded = jsonDecode(line);
    } on FormatException {
      // Not a frame at all. It is still the answer to what is out — the client's decoder is what says so, and it says
      // it by name — but it cannot be a publish.
      _answer(line);
      return;
    }
    if (decoded is Map<String, Object?> &&
        decoded['ok'] == true &&
        decoded.containsKey('wall') &&
        decoded.containsKey('watch')) {
      _publish(line);
      return;
    }
    if (_awaiting == _Awaiting.watch) {
      _granted(line);
      return;
    }
    if (decoded is Map<String, Object?> &&
        decoded['ok'] == true &&
        decoded.containsKey('watch') &&
        !decoded.containsKey('wall')) {
      // **A grant with no ask behind it** is an answer to a request nobody made. Dropped rather than queued: an answer
      // waiting for no client would be handed to the next read as if it were its own, and a connection that came back
      // cannot be watching anything (`docs/mobile-remote.md` decision 1).
      return;
    }
    _answer(line);
  }

  /// The answer to the request that was out: the connection's knowledge moves with it, and the line is handed on.
  void _answer(String line) {
    final awaiting = _take();
    final answer = _decode(line);
    if (answer is Refused && answer.kind == RefusalKind.version) {
      _stopped = Refusal(answer.why, answer.kind);
    }
    if (awaiting == _Awaiting.read &&
        answer is Projection &&
        answer.watch == null) {
      _read = true;
    }
    final waiting = _waiting;
    if (waiting != null && !waiting.isCompleted) {
      _waiting = null;
      waiting.complete(line);
    } else {
      _answers.add(line);
    }
  }

  /// The answer to the carrier's own ask: a grant, or the wall's refusal to grant one.
  void _granted(String line) {
    _take();
    final granted = _grant;
    _grant = null;
    if (granted == null) {
      // An unsolicited grant is a connection that thinks it is watching something: a connection that came back cannot
      // be, so there is nobody to tell and nothing to resume.
      return;
    }
    final Answer answer;
    try {
      answer = decodeAnswer(line);
    } on WireException catch (error) {
      granted.completeError(error);
      return;
    }
    switch (answer) {
      case Granted():
        _watching = true;
        granted.complete(answer);
      case Refused(:final why, :final kind):
        final refusal = Refusal(why, kind);
        if (kind == RefusalKind.version) {
          _stopped = refusal;
        }
        granted.completeError(refusal);
      case Changed():
      case Projection():
        granted.completeError(
          const WireException(
            'the relay answered a watch with something that is not a grant',
          ),
        );
    }
  }

  /// A publish: the projection under the watch it belongs to. **Never** an answer to a request.
  void _publish(String line) {
    final stream = _published ??= StreamController<Projection>.broadcast();
    final Answer answer;
    try {
      answer = decodeAnswer(line);
    } on WireException catch (error) {
      stream.addError(error);
      return;
    }
    if (answer is Projection && answer.watch != null) {
      stream.add(answer);
      return;
    }
    stream.addError(
      const WireException('a publish is a projection with its watch on it'),
    );
  }

  /// Clear the request that was out, and its deadline with it: the next line is the next request's answer.
  _Awaiting? _take() {
    final awaiting = _awaiting;
    _awaiting = null;
    _deadline?.cancel();
    _deadline = null;
    return awaiting;
  }

  /// **A request the relay did not answer inside [answerWithin]**: the connection goes with it.
  ///
  /// A late answer could not be told from the next request's, and an answer to nothing is worse than a socket that has
  /// to be dialled again. This is the only thing a **revoked** phone ever observes ([Unanswered]).
  void _expire() {
    _deadline = null;
    _awaiting = null;
    const error = Unanswered();
    final granted = _grant;
    _grant = null;
    if (granted != null && !granted.isCompleted) {
      granted.completeError(error);
    }
    final waiting = _waiting;
    _waiting = null;
    if (waiting != null && !waiting.isCompleted) {
      waiting.completeError(error);
    } else {
      _failure = error;
    }
    _dropped();
  }

  /// Write one framed line, and arm its deadline. The socket is the only thing this touches.
  Future<void> _write(Map<String, Object?> frame, _Awaiting kind) async {
    final socket = _socket;
    if (socket == null) {
      throw const NoConnection();
    }
    _awaiting = kind;
    _deadline?.cancel();
    _deadline = Timer(_answerWithin, _expire);
    try {
      socket.add(utf8.encode('${jsonEncode(frame)}\n'));
      await socket.flush();
    } on Object {
      _dropped();
      throw const NoConnection();
    }
  }

  /// The answer on the line, or `null` when it is a shape this client cannot read — the client's own decoder is what
  /// names it, and this carrier only needs to know whether a read landed and whether the session is stopped.
  Answer? _decode(String line) {
    try {
      return decodeAnswer(line);
    } on WireException {
      return null;
    }
  }

  /// **This build dials two schemes and no others**, and a URI with no address is not one of them (§ *The relay's
  /// frames*: a `wss://` relay needs a websocket carrier, which is a ticket of its own rather than a guess).
  static void _check(Uri relay) {
    switch (relay.scheme) {
      case 'tcp' || 'tls':
        break;
      default:
        throw const NotARelay('the scheme is neither "tcp" nor "tls"');
    }
    if (relay.host.isEmpty) {
      throw const NotARelay('there is no host');
    }
    if (relay.port == 0) {
      throw const NotARelay('there is no port');
    }
  }
}

/// **A device key is 64 lowercase hex characters** — 32 bytes, as `devices.toml`'s `Key::hex` writes them and
/// `Key::from_hex` reads them (`src/config/devices.rs`). A key in another case is a different string, and the wall hashes
/// the string it is given: an uppercase copy of the key is a stranger's frame.
final RegExp _deviceKey = RegExp(r'^[0-9a-f]{64}$');

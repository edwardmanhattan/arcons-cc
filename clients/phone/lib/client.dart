/// **A client's rules, in the phone's own language** (#250).
///
/// `docs/mobile-remote.md` decision 1 is the whole of this file: **the projection is the phone's only truth.** The
/// wall's own client — `src/control/client.rs` (#197) — holds the same four rules over the wall's session loop, and
/// `test/client_test.dart` here holds them over a fake carrier. Two clients, two languages, one set of claims: that is
/// what makes "a client is a client" a test rather than a paragraph.
///
/// The rules, in the note's order:
///
/// 1. **Read, then act.** The first line a session sends is `{"v":1,"read":"wall"}`, and nothing is sent until that read
///    has landed: [Client.act] throws [NotRead] while the projection is still out.
/// 2. **A connection that comes back knows nothing.** [Client.connect] and [Client.disconnect] both leave the client
///    knowing nothing, and there is no queue in this file for an intent to wait in. A watch is not resumed either: a
///    watch is the **carrier's** (`lib/relay.dart`, #256), asked only after a read has landed on the connection —
///    `RelayCarrier.watch` refuses to ask before one has — and a connection that comes back is not watching anything.
/// 3. **A wall changed elsewhere is read, never corrected.** [Client.read] replaces what the client held, wholesale.
///    There is no second writer here: an `ok` answer does not patch the projection, and neither does a refusal.
/// 4. **A version the wall does not speak is a stop.** A refusal whose kind is [RefusalKind.version] sets [Client.stop],
///    and every call after it throws that same refusal without a line being written. Never a retry, never a
///    negotiation (`ADR-0008` decision 5).
///
/// The fifth is the one that made a client's behaviour decidable: **a refusal is mapped from its `kind`, never from its
/// sentence** (#198). [Refusal.next] is [Next.of]'s answer, the same one the screen reads, so there is one mapping in
/// this package and not two.
///
/// **What it deliberately is not.** Not a screen: the four states `docs/mobile-ui.md` draws are derived from
/// [Client.wall], [Client.isConnected] and [Client.stop], and what to draw is the screen's business. Not a renderer: it
/// never reads the fleet, and it has no scene — a client that could draw the water would have the picture (`ADR-0005`
/// decision 7).
library;

import 'dart:convert';

import 'wire.dart';

/// **A carrier** — a line out, a line in, and a way to know the connection ended (#195, one language over).
///
/// The wall's carrier is a trait with six methods because a socket has a deadline and an origin. A phone's wire is a
/// relay's frame, and what a client needs from it is three things: write a line, take the next one, and learn that the
/// connection is gone. Small enough that a fake is twenty lines — `test/client_test.dart` has one — and that is the
/// only way these rules are provable without a socket, a network or a clock.
///
/// **End of file is `null`**, never an empty string and never a blank line, exactly as the wall's `read_line` returns
/// `Ok(None)` at end of file: a wall that hung up and a wall that wrote nothing are different things, and a carrier
/// that conflated them would hand the client a projection made of nothing.
abstract interface class Carrier {
  /// Write one line. A carrier that cannot is a connection that has gone, and the client reports it as one.
  Future<void> send(String line);

  /// The next line the wall wrote, or `null` when there is no connection any more.
  Future<String?> receive();

  /// Hang the connection up. The client calls this when it lets go of a carrier.
  Future<void> close();
}

/// **Why a client's request did not land**: the wall's no, or this client's own rule.
///
/// A refusal was *sent and answered*; the others are a client that never got that far, and saying which is the
/// difference between "the wall said no" and "there is no wall" — a distinction the screen draws, because the four
/// states in `docs/mobile-ui.md` turn on exactly it.
sealed class Denied implements Exception {
  const Denied();

  /// **What this client may do about it**, when the wall named a category.
  ///
  /// `null` is this client's own rule: no wall said anything to decide for the phone, so the answer is to connect or to
  /// read. A refusal's action comes from [Next.of], the same switch the screen reads — one mapping, not two.
  Next? get next => null;
}

/// **The wall said no** (#198): its words in [why], its category in [kind], and the category is what decides.
final class Refusal extends Denied {
  const Refusal(this.why, this.kind, {this.detail});

  /// The wall's own words, verbatim. A screen prints them; a client does not act on them.
  final String why;

  /// The category, which is what a client does act on.
  final RefusalKind kind;

  /// The wall's longer words about the same refusal, when it sent any (#264): drawn verbatim, and `null` on a wall
  /// that says only [why].
  final String? detail;

  @override
  Next get next => Next.of(kind);

  @override
  String toString() => 'the wall refused: $why (${kind.word})';
}

/// **There is no connection**: this client is not talking to a wall, yet or any more.
final class NoConnection extends Denied {
  const NoConnection();
}

/// **Nothing has been read on this connection.** Read, then act.
final class NotRead extends Denied {
  const NotRead();
}

/// The carrier failed, or the wall hung up, while the request was out — the connection went with it, and so did the
/// projection it had read.
final class Lost extends Denied {
  const Lost();
}

/// The wall answered a line this protocol does not have, in this client's words. Not the wall's no: a refusal is
/// [Refusal], and this is a shape neither side defined.
final class Unreadable extends Denied {
  const Unreadable(this.why);

  final String why;

  @override
  String toString() => 'unreadable: $why';
}

/// **What this client knows.** One value, so "a projection from the connection before" is not a state the type can
/// express: every way a connection can end leaves [_Nothing].
sealed class _Known {
  const _Known();
}

/// Nothing has been read on this connection. A client here may read and may not act.
final class _Nothing extends _Known {
  const _Nothing();
}

/// The projection it last read — **the only thing it knows about the wall**.
final class _Read extends _Known {
  const _Read(this.wall);

  final Wall wall;
}

/// A refusal a client may not argue with: the wall does not speak this client's version (#198's `version`).
final class _Stopped extends _Known {
  const _Stopped(this.refusal);

  final Refusal refusal;
}

/// **A client of the wall**: a carrier, the one protocol it speaks, and the projection it last read.
///
/// ```text
/// connect ─► read ─► act ─► read          one connection
///    ▲                                    a read is always before an act
///    └── new connection, known = nothing  a connection that came back knows nothing
/// ```
final class Client {
  /// A client that speaks this build's protocol ([protocol]).
  Client() : _version = protocol;

  /// **A client built against another protocol** — what an app from before the wall's update is. It sends its own
  /// version, and the wall refuses rather than negotiating (#198).
  Client.speaking(this._version);

  /// The protocol this build speaks. One number: a client does not negotiate (`docs/control.md`).
  final int _version;

  /// The connection, when there is one. **A drop takes the projection with it** — there is no field here that could
  /// outlive its wire, because that is how a stale projection happens.
  Carrier? _carrier;

  /// What it knows, which is only ever what it has just read.
  _Known _known = const _Nothing();

  /// The version this client speaks, and the one it writes on every line it sends.
  int get version => _version;

  /// Whether this client has a connection. It may still know nothing: a read has not landed on it.
  bool get isConnected => _carrier != null;

  /// The projection this client last read — the only thing it knows about the wall, or `null`.
  Wall? get wall => switch (_known) {
        _Read(:final wall) => wall,
        _Nothing() || _Stopped() => null,
      };

  /// The pane the wall had the keys on, **from the projection this client holds** — never from a memory of an earlier
  /// read.
  int? get focused => wall?.focused;

  /// **The refusal that ended this session** (#198's `version`), or `null`. What the update screen is drawn from
  /// (`docs/mobile-ui.md`).
  Refusal? get stop => switch (_known) {
        _Stopped(:final refusal) => refusal,
        _Nothing() || _Read() => null,
      };

  /// **Connect, and know nothing.** A new connection is not a new memory: whatever the last one read is gone, and
  /// nothing is sent on this one until the client asks for the wall.
  Future<void> connect(Carrier carrier) async {
    final previous = _carrier;
    _carrier = carrier;
    _known = const _Nothing();
    if (previous != null) {
      await _letGo(previous);
    }
  }

  /// Let go: the connection and the projection both. **Nothing is queued here**, because an intent is a change, and a
  /// change composed against a wall nobody is reading is a change made wrongly.
  Future<void> disconnect() async {
    final carrier = _carrier;
    _carrier = null;
    _known = const _Nothing();
    if (carrier != null) {
      await _letGo(carrier);
    }
  }

  /// **Read the wall** (#181): ask for the projection, keep what came back, and hand it to the caller.
  ///
  /// A read is the only way this client learns anything. A refusal stops it when the kind is `version`, and a line it
  /// cannot read leaves it knowing nothing — so the next act is refused until it has read again.
  Future<Wall> read() async {
    final stopped = stop;
    if (stopped != null) {
      throw stopped;
    }
    final carrier = _carrier;
    if (carrier == null) {
      _known = const _Nothing();
      throw const NoConnection();
    }
    final String line;
    try {
      line = await _exchange(carrier, _readLine);
    } on Lost {
      await disconnect();
      rethrow;
    }
    final Answer answer;
    try {
      answer = decodeAnswer(line);
    } on WireException catch (error) {
      // A read answered with a line this build cannot read is a wall it cannot say anything about. Forget what it
      // held: a projection it cannot vouch for is worse than none.
      _known = const _Nothing();
      throw Unreadable(error.why);
    }
    switch (answer) {
      case Projection(:final wall):
        _known = _Read(wall);
        return wall;
      case Refused(:final why, :final kind, :final detail):
        throw _refused(Refusal(why, kind, detail: detail));
      case Changed():
        _known = const _Nothing();
        throw const Unreadable(
          'a read was answered with "changed", and a projection is the only answer to one',
        );
      // A grant is the answer to the carrier's ask and never to a read. Handing it here would be a client that took
      // the existence of a stream for the arrangement it asked for.
      case Granted():
        _known = const _Nothing();
        throw const Unreadable(
          'a read was answered with a watch grant, and a projection is the only answer to one',
        );
      // **A pairing is the answer to `pair.claim`** (#262), the carrier's own ask and never a read's: a line that
      // carried one here is a wall answering a question nobody asked.
      case Paired():
        _known = const _Nothing();
        throw const Unreadable(
          'a read was answered with a pairing, and a projection is the only answer to one',
        );
    }
  }

  /// **Act on the wall** — a settled change, never a gesture (`docs/control.md`: the wall sees one intent on release).
  ///
  /// Refused **without touching the carrier** when this client has not read on this connection: that is the client's
  /// first rule, and the reason a drop cannot become a queue. [verb] is the wall's own vocabulary (`pane.focus`,
  /// `pane.drop`, …) and [fields] are that verb's fields; a verb the wall does not know is refused with `input`, which
  /// is this app's bug, so a free-form verb here is a line the wall checks rather than a hole.
  Future<Changed> act(String verb, [Map<String, Object?> fields = const {}]) async {
    switch (_known) {
      case _Stopped(:final refusal):
        throw refusal;
      case _Nothing():
        throw _carrier == null ? const NoConnection() : const NotRead();
      case _Read():
        break;
    }
    final carrier = _carrier;
    if (carrier == null) {
      throw const NoConnection();
    }
    final String line;
    try {
      line = await _exchange(carrier, _intentLine(verb, fields));
    } on Lost {
      await disconnect();
      rethrow;
    }
    final Answer answer;
    try {
      answer = decodeAnswer(line);
    } on WireException catch (error) {
      // An unreadable answer cannot say whether the change landed. Forget the projection: read again.
      _known = const _Nothing();
      throw Unreadable(error.why);
    }
    switch (answer) {
      case Changed():
        return answer;
      case Refused(:final why, :final kind, :final detail):
        // **A refusal moved nothing**, so the projection the intent was composed from is still the wall's: the client
        // keeps it, and the next read decides what really happened.
        throw _refused(Refusal(why, kind, detail: detail));
      case Projection():
        _known = const _Nothing();
        throw const Unreadable(
          'an intent was answered with a projection, and a read is the only request that asks for one',
        );
      case Granted():
        _known = const _Nothing();
        throw const Unreadable(
          'an intent was answered with a watch grant, and a change is the only answer to one',
        );
      // **The one intent whose answer is more than a flag** (#194, #262): `pair.claim`'s key. It is the carrier's ask
      // (`RelayCarrier.claim`), so an `act` that met one is a client that sent a claim by the wrong door.
      case Paired():
        _known = const _Nothing();
        throw const Unreadable(
          'an intent was answered with a pairing, and a change is the only answer to one',
        );
    }
  }

  /// **A refusal, and what it does to the client.** A `version` refusal is a stop (#198): the wall will never answer
  /// this build, so sending again is a client arguing with an update it needs. Every other kind leaves the connection
  /// usable — the wall is still there and the next line may be a fresh request (a `rule` or an `input` refusal is not
  /// an error state) — and leaves the projection alone, because a refusal moved nothing.
  Refusal _refused(Refusal refusal) {
    if (refusal.kind == RefusalKind.version) {
      _known = _Stopped(refusal);
    }
    return refusal;
  }

  /// The line a read sends: `src/control/wire.rs`'s `request_line_at` for `Request::Wall`, byte for byte. It is the
  /// first line a session sends, and the recording's first line is the same object.
  String get _readLine => jsonEncode({'v': _version, 'read': 'wall'});

  /// The line an intent sends, in the same encoder's shape: the version, and the intent inside it with its verb.
  String _intentLine(String verb, Map<String, Object?> fields) =>
      jsonEncode({'v': _version, 'intent': {'verb': verb, ...fields}});

  /// Close a carrier and note nothing about how it went: the connection is gone either way, and a close that failed
  /// has nothing left to fail at.
  Future<void> _letGo(Carrier carrier) async {
    try {
      await carrier.close();
    } on Object {
      // A carrier that cannot be closed is a carrier that is already gone.
    }
  }
}

/// **One request out and one answer line back** — the only place a client touches its carrier, so "nothing is sent
/// before a read" is one function and not a habit.
///
/// End of file is a wall that hung up — a lost connection, never a blank answer.
Future<String> _exchange(Carrier carrier, String request) async {
  try {
    await carrier.send(request);
  } on Object {
    throw const Lost();
  }
  final String? line;
  try {
    line = await carrier.receive();
  } on Object {
    throw const Lost();
  }
  if (line == null) {
    throw const Lost();
  }
  return line;
}

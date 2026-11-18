/// **The app's shell** — the relay, the key, the read and the watch (`docs/mobile-ui.md`; #264).
///
/// The screen (#255), the drag (#257), the carrier (#256) and the claim (#262) each exist. This file is the join:
///
/// ```text
/// main ── ARCONS_RELAY ──▶ PhoneShell ── no relay ──▶ overview: not connected · no relay is configured
///                              │
///                              ├── no device key ──▶ pairing: six digits, a name, one Pair
///                              │                         └── a claim ──▶ the key store, once
///                              └── a key ──▶ connect ──▶ read ──▶ watch ──▶ the wall's publishes
/// ```
///
/// **The corpus is not on this path.** The recording (`fixtures/control/session.jsonl`) is the tests' fixture: this app
/// reads the wall over a relay or it has nothing, and a phone that replayed a file would draw a wall nobody has.
///
/// **The four states are the controller's** (`controller.dart`): this shell only decides which of the two screens is on
/// the glass, and puts the wire's events where the controller can derive them from. The clock is passed in, so no test
/// depends on the wall clock, and no secret is printed: not the relay's address, not the six digits, not the key
/// (`INV-08`).
library;

import 'dart:async';
import 'dart:developer' as developer;

import 'package:arcons_phone/client.dart' show Carrier;
import 'package:arcons_phone/relay.dart' show KeyStore, RelayCarrier;
import 'package:arcons_phone/wire.dart' show Granted, Paired, Projection;
import 'package:material_ui/material_ui.dart';

import 'controller.dart';
import 'overview.dart';
import 'pairing.dart';

/// **The relay, as the shell needs it**: the client's [Carrier], the dial, the watch, and the one claim a phone with no
/// key may send.
///
/// The carrier package says what a watch is the carrier's ask (#256) and what a claim is the one frame a stranger may
/// send (#262); the shell needs both and a `Client` needs neither, so this is the seam the app is built on. `main`
/// wraps a [RelayCarrier]; a test hands over a fake with the same four verbs and no socket.
abstract interface class PhoneRelay implements Carrier {
  /// Dial, and keep dialling while the relay is down (`RelayCarrier.connect`).
  Future<void> connect();

  /// Ask for the wall's publishes, **after a read has landed** on this connection.
  Future<Granted> watch();

  /// The wall's publishes of the open watch.
  Stream<Projection> get published;

  /// Claim the code on the wall: the phone's first key, and the only frame with no `device` on it.
  Future<Paired> claim({required String code, required String name});
}

/// **The relay this build dials**: `RelayCarrier`, wrapped so the app never names a socket outside this file.
final class Relay implements PhoneRelay {
  Relay(this._carrier);

  final RelayCarrier _carrier;

  @override
  Future<void> connect() => _carrier.connect();

  @override
  Future<Granted> watch() => _carrier.watch();

  @override
  Stream<Projection> get published => _carrier.published;

  @override
  Future<Paired> claim({required String code, required String name}) =>
      _carrier.claim(code: code, name: name);

  @override
  Future<void> send(String line) => _carrier.send(line);

  @override
  Future<String?> receive() => _carrier.receive();

  @override
  Future<void> close() => _carrier.close();
}

/// A carrier built from the deployment's relay URL. `main` passes this; a test passes its own.
PhoneRelay dialRelay(Uri relay, KeyStore keys) => Relay(
      RelayCarrier(relay: relay, keys: keys),
    );

/// **The deployment's relay, parsed, or `null` when none is configured.**
///
/// Empty, unparseable, or a scheme this build cannot dial is the same state: **no relay is configured**, and the
/// overview says so without naming the value (`INV-08`). A URL is a measurement of somebody's deployment, not a fact
/// about this phone, so a malformed one is not printed back at the operator either.
Uri? parseRelay(String configured) {
  if (configured.isEmpty) {
    return null;
  }
  final Uri relay;
  try {
    relay = Uri.parse(configured);
  } on FormatException {
    return null;
  }
  if (relay.host.isEmpty || relay.port == 0) {
    return null;
  }
  return switch (relay.scheme) {
    'tcp' || 'tls' => relay,
    _ => null,
  };
}

/// **A key store with no platform under it** — what this app holds until #265 lands the Keychain and the Keystore.
///
/// ADR-0008 decision 4 says the platform's store and **not a file**, so this is deliberately not one: a memory store is
/// a phone that has to pair again after a restart, which is a smaller lie than a key written somewhere this repository
/// can read. `INV-08`: the key can never be formatted into a string by this object.
final class MemoryKeyStore implements KeyStore {
  MemoryKeyStore([this._key]);

  String? _key;

  @override
  Future<String?> key() async => _key;

  @override
  Future<void> remember(String key) async {
    _key = key;
  }

  @override
  String toString() => 'a key store (its key is not printed)';
}

/// **The app.** The pairing screen before a key, the overview after it, and the relay under both.
final class PhoneShell extends StatefulWidget {
  const PhoneShell({
    super.key,
    required this.relay,
    required this.keys,
    this.openRelay,
    this.clock,
    this.log,
    this.onUpdate,
  });

  /// The relay to dial, or `null` when no relay is configured. **Never printed** (`INV-08`).
  final Uri? relay;

  /// Where the device key lives. Read once on start to decide whether this phone is paired.
  final KeyStore keys;

  /// How a carrier is built; `null` is [dialRelay]. A test hands over a fake.
  final PhoneRelay Function(Uri relay, KeyStore keys)? openRelay;

  /// The clock the `not connected` age is measured with. A test passes a fixed one: no test depends on the wall clock.
  final DateTime Function()? clock;

  /// Where an `input` refusal and a publish this build cannot read are reported. One line, and no field (`INV-08`).
  final void Function(String message)? log;

  /// The update screen's one button, which is the shell's: this package has no store page.
  final VoidCallback? onUpdate;

  @override
  State<PhoneShell> createState() => _PhoneShellState();
}

final class _PhoneShellState extends State<PhoneShell> {
  late final OverviewController _controller;

  PhoneRelay? _phone;
  StreamSubscription<Projection>? _publishes;

  /// The phone has no key yet: the pairing screen is the only screen before the overview.
  bool _pairing = false;

  @override
  void initState() {
    super.initState();
    _controller = OverviewController(
      clock: widget.clock ?? DateTime.now,
      log: widget.log,
    );
    unawaited(_start());
  }

  @override
  void dispose() {
    unawaited(_publishes?.cancel() ?? Future<void>.value());
    unawaited(_phone?.close() ?? Future<void>.value());
    _controller.dispose();
    super.dispose();
  }

  /// **Read the key, then decide.** A phone with a key has a wall to read; a phone without one has a code to type; a
  /// phone with no relay has nowhere to dial, which is a state and not a crash (#264).
  Future<void> _start() async {
    final relay = widget.relay;
    if (relay == null) {
      _controller.offline(why: noRelayConfigured);
      return;
    }
    String? key;
    try {
      key = await widget.keys.key();
    } on Object {
      // A store that cannot answer is a phone that has to pair: nothing here invents a key, and `pair.claim` is how a
      // key is made.
      key = null;
    }
    if (!mounted) {
      return;
    }
    if (key == null) {
      setState(() {
        _pairing = true;
      });
      return;
    }
    await _open(relay);
  }

  /// **Dial, connect, read** — the client's first rule, made the app's (`mobile-remote.md` decision 1).
  Future<void> _open(Uri relay) async {
    final phone = _phone ??= (widget.openRelay ?? dialRelay)(relay, widget.keys);
    await _session(phone);
  }

  /// A connection's life: dial, read, and ask for the watch **only after the read has landed**. A watch is a
  /// connection's, so nothing here resumes one: a new connection gets a new read and a new ask.
  Future<void> _session(PhoneRelay phone) async {
    try {
      await phone.connect();
    } on Object {
      // The carrier names what was wrong with the key or the relay; the screen's line says what the phone knows, and it
      // is never the address (`INV-08`).
      if (mounted) {
        _controller.offline();
      }
      return;
    }
    if (!mounted) {
      return;
    }
    await _controller.connect(phone);
    await _watch(phone);
  }

  /// **The watch, after the read.** The carrier refuses the ask on a connection that has not seen a read land, so this
  /// is also how the app says "a watch is not resumed": the only place it is asked is a session whose read landed.
  Future<void> _watch(PhoneRelay phone) async {
    if (!mounted || _controller.state != PhoneState.live) {
      return;
    }
    try {
      await phone.watch();
    } on Object {
      // A wall that will not grant one leaves the read where it is: the screen is live, and that is still true.
      return;
    }
    if (!mounted) {
      return;
    }
    await _publishes?.cancel();
    _publishes = phone.published.listen(
      (projection) => unawaited(_controller.published(projection)),
      onError: (Object _) =>
          _log('phone: a publish this build cannot read was not drawn'),
    );
  }

  /// One line, and **never a field**: no address, no code, no key (`INV-08`).
  void _log(String message) =>
      (widget.log ??
          (message) => developer.log(message, name: 'arcons-phone'))(message);

  /// **The claim landed.** The carrier that claimed holds the key, so the shell adopts it: the same connection reads
  /// first and then watches, and the phone's identity is the key the store now holds.
  void _adopt(PhoneRelay phone) {
    _phone = phone;
    setState(() {
      _pairing = false;
    });
    unawaited(_session(phone));
  }

  @override
  Widget build(BuildContext context) {
    final relay = widget.relay;
    if (_pairing && relay != null) {
      return PairingScreen(
        relay: relay,
        keys: widget.keys,
        openRelay: widget.openRelay ?? dialRelay,
        onPaired: _adopt,
      );
    }
    return OverviewScreen(controller: _controller, onUpdate: widget.onUpdate);
  }
}

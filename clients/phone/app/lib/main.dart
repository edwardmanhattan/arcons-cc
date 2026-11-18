/// **The phone's app** — the screen, the relay, and the pairing screen before it (#264).
///
/// This file is only the shell's outside: the deployment's relay URL, the key store, and `runApp`. Everything the app
/// does is in `shell.dart`, `controller.dart`, `overview.dart` and `pairing.dart`; the recording in
/// `fixtures/control/session.jsonl` is the **tests'** fixture and is not read here.
///
/// **The relay is deployment configuration and never a secret**: it is the address the wall dials and the phone talks
/// to, so `main` reads it from `--dart-define=ARCONS_RELAY=tcp://host:port` (or `tls://`) and never prints it.
/// `INV-08`'s spirit: a phone that put a deployment's host in a screenshot would be leaking the network it is for, and a
/// URL in the tree is how one quietly becomes committed configuration.
///
/// **No relay configured is a state, not a crash**: `parseRelay` answers `null` for an empty, malformed or
/// undialable value, and the overview draws `not connected` with one sentence that names the field and not the value
/// (`shell.dart`, `controller.dart`).
library;

import 'dart:developer' as developer;

import 'package:arcons_phone/relay.dart' show KeyStore;
import 'package:material_ui/material_ui.dart';

import 'shell.dart';

/// The variable the deployment sets: `flutter run --dart-define=ARCONS_RELAY=tcp://HOST:PORT`.
const String relayVariable = 'ARCONS_RELAY';

void main() {
  WidgetsFlutterBinding.ensureInitialized();
  runApp(
    PhoneApp(
      relay: parseRelay(const String.fromEnvironment(relayVariable)),
      // **The platform's store is #265's.** Until it lands this is memory: a phone that pairs again after a restart is
      // a smaller lie than a key in a file this repository can read (ADR-0008 decision 4).
      keys: MemoryKeyStore(),
    ),
  );
}

/// The app: one screen, whichever the shell chose, and the update screen's one button.
final class PhoneApp extends StatelessWidget {
  const PhoneApp({
    super.key,
    required this.relay,
    required this.keys,
    this.openRelay,
    this.clock,
    this.log,
    this.onUpdate,
  });

  /// The relay this build dials, or `null` for a build with none configured. **Never printed** (`INV-08`).
  final Uri? relay;

  /// Where the device key lives. A phone with no key pairs first.
  final KeyStore keys;

  /// How a carrier is built; `null` is the relay's socket. A test hands over a fake.
  final PhoneRelay Function(Uri relay, KeyStore keys)? openRelay;

  /// The clock the `not connected` age is measured with; a test passes a fixed one.
  final DateTime Function()? clock;

  /// One log line for an `input` refusal or a publish this build cannot read — no field, no host, no token.
  final void Function(String message)? log;

  /// The update screen's button. This package has no store page, so the default writes one line and sends nothing —
  /// which is the rule for a version refusal anyway: **a client that retries a version it cannot speak loops**
  /// (`docs/control.md`).
  final VoidCallback? onUpdate;

  @override
  Widget build(BuildContext context) {
    return MaterialApp(
      debugShowCheckedModeBanner: false,
      home: Scaffold(
        body: PhoneShell(
          relay: relay,
          keys: keys,
          openRelay: openRelay,
          clock: clock,
          log: log,
          onUpdate: onUpdate ?? _noStorePage,
        ),
      ),
    );
  }
}

/// The update button's default: one line, no retry, and no address in it.
void _noStorePage() => developer.log(
      'this build cannot speak the wall\'s protocol: this package has no store page to open',
      name: 'arcons-phone',
    );

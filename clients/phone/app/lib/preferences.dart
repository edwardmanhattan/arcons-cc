/// **The relay the operator typed or scanned**, remembered between runs (#271).
///
/// A relay URL is **deployment configuration, not a secret**: it is where the wall's relay is, and the phone shows it in
/// a field on the pairing screen. So it does not live in the [KeyStore] — the KeyStore is the device key's home (#265,
/// ADR-0008 decision 4) — and it is not a `Secret`. It lives in the platform's ordinary preferences, which are exactly
/// the place for a setting that may be read and rewritten.
///
/// The seam exists so a widget test never reaches the platform: [MemoryRelayPreferences] is the test's and
/// [SharedRelayPreferences] is `main`'s. The real class is the only one that imports `shared_preferences`.
library;

import 'package:flutter/services.dart'
    show MissingPluginException, PlatformException;
import 'package:shared_preferences/shared_preferences.dart';

/// The key one relay URL is stored under. Versioned with the app's own shape, not the wall's protocol.
const String relayPreferenceKey = 'cc.arcons.phone/relay';

/// **Where the relay field's value is kept**: one string, read at start and written when it changes.
abstract interface class RelayPreferences {
  /// The remembered relay, or `null` when this phone has not kept one.
  Future<String?> relay();

  /// Remember [relay] for the next run. **Not a secret**, so the whole URL is stored; it is still never logged.
  Future<void> rememberRelay(String relay);
}

/// **A relay remembered in the platform's preferences** — the real implementation. `main` builds it; no test does.
final class SharedRelayPreferences implements RelayPreferences {
  SharedRelayPreferences({SharedPreferencesAsync? preferences})
    : _preferences = preferences ?? SharedPreferencesAsync();

  final SharedPreferencesAsync _preferences;

  /// The remembered URL, or `null`. A platform that cannot answer is the same state as none remembered: the compiled
  /// default is still there, and a phone that cannot read a setting is not a phone that crashes on start.
  @override
  Future<String?> relay() async {
    try {
      return await _preferences.getString(relayPreferenceKey);
    } on PlatformException {
      return null;
    } on MissingPluginException {
      return null;
    }
  }

  /// Keep the URL. A platform that cannot write leaves the field working for this run; it is a setting, not a secret,
  /// and losing it is recoverable.
  @override
  Future<void> rememberRelay(String relay) async {
    try {
      await _preferences.setString(relayPreferenceKey, relay);
    } on PlatformException {
      // A setting that could not be stored is not a failure the operator has to fix at this screen.
    } on MissingPluginException {
      // No platform under this build: the same fact, and the field still holds the value for this run.
    }
  }

  @override
  String toString() => 'the relay preference (its value is not printed)';
}

/// **A relay remembered in a test's own memory** — and the default for a shell that was handed no store.
///
/// It is deliberately not a file: a widget test must not write into the runner's preferences, and `main` is the one
/// place that names the platform's implementation.
final class MemoryRelayPreferences implements RelayPreferences {
  MemoryRelayPreferences([this._relay]);

  String? _relay;

  @override
  Future<String?> relay() async => _relay;

  @override
  Future<void> rememberRelay(String relay) async {
    _relay = relay;
  }

  @override
  String toString() =>
      'an in-memory relay preference (its value is not printed)';
}

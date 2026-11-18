/// **The device key's home** (#265): `KeyStore` over one `MethodChannel`, and nothing else.
///
/// ADR-0008 decision 4 is the whole of this file: `pair.claim` hands a phone a key once, and the platform's store is the
/// only place it can live — the Keychain on iOS, an AES key in the AndroidKeyStore wrapping the secret on Android, both
/// written by hand and neither through a package. The platform half is
/// `android/app/src/main/kotlin/cc/arcons/phone/PlatformKeyStore.kt` and `ios/Runner/AppDelegate.swift`: **the only code
/// in this repository that no test here can run**, which is why the surface is three methods and two strings.
///
/// ```text
/// key.read      → String?   the key as stored, or null: this phone has not paired
/// key.remember  <key>       the one write `pair.claim` makes, and the only time a key crosses the channel
/// key.forget    → null      the key is gone: an uninstall, or `Forget this wall`
/// ```
///
/// **One constant per side, no other traffic.** The channel's name and the three method names are written here and
/// again in the Kotlin and the Swift, because a name shared through a generator is a build step this ticket does not
/// add. The strings are compared by the checklist in the PR (`pair`, kill, reopen) — a typo on either side is a read
/// that answers [KeyStoreUnavailable] and a phone that offers to pair again, never a silent `null`.
///
/// **Nothing here prints a key** (`INV-08`): the store holds no field a key is assigned to, its `toString` is a
/// sentence, and a platform failure becomes a [KeyStoreUnavailable] that carries our code and never the platform's
/// message.
library;

import 'package:arcons_phone/relay.dart' show KeyStore;
import 'package:flutter/services.dart'
    show MethodChannel, MissingPluginException, PlatformException;

/// **The channel**: one name, and the same string in `PlatformKeyStore.kt` and `AppDelegate.swift`.
const String keyStoreChannel = 'cc.arcons.phone/key-store';

/// **Read the key**, answering the stored key or `null`.
const String keyReadMethod = 'key.read';

/// **Write the key the wall handed over**, once (`RelayCarrier.claim`).
const String keyRememberMethod = 'key.remember';

/// **Delete the key**, and the wrapping key with it.
const String keyForgetMethod = 'key.forget';

/// **The name of the one argument `key.remember` carries**: the key itself, and nothing beside it.
const String keyArgument = 'key';

/// **A device key is 32 bytes as 64 lowercase hex characters**, the shape `pair.claim` hands back (`wire.dart`'s
/// `Paired`, and `src/control/relay.rs`'s generator). Anything else the platform hands back is not a key.
final RegExp _deviceKey = RegExp(r'^[0-9a-f]{64}$');

/// **The platform's store did not answer** — the one failure this app's half of the channel can have.
///
/// A refusal and not a crash: a Keychain that will not unlock, a Keystore whose key was invalidated, a platform where
/// the channel was never registered. The shell reads this as *this phone has no key to read* and offers pairing, which
/// is the honest state and the recovery — a key that cannot be read cannot be dialled with.
///
/// [code] is **ours** (`cc.arcons.phone/key-store`), never the platform's message: a message is the one field of a
/// platform failure this repository does not write, and a sentence that quotes the thing it is complaining about is how
/// a secret reaches a log (`INV-08`).
final class KeyStoreUnavailable implements Exception {
  const KeyStoreUnavailable(this.code);

  /// The channel that did not answer: a constant, and never the key, the item or the address of a store.
  final String code;

  @override
  String toString() =>
      'the platform key store did not answer ($code): this phone has no key to read, so it pairs (#265)';
}

/// **`KeyStore` over the platform**: the Keychain on iOS, the AndroidKeyStore on Android (#265).
///
/// The channel is **injectable** so a test drives the whole contract through
/// `setMockMethodCallHandler` — the real store and the fake platform, with nothing in between that only a phone could
/// exercise. `main` builds it with the default channel; no test needs a phone to hold this class to its promises.
final class PlatformKeyStore implements KeyStore {
  /// A caller writes `channel:` and the field stays private: Dart 3.14's private named parameters drop the underscore
  /// from the name outside the library (`relay.dart` leans on the same feature).
  PlatformKeyStore({this._channel = const MethodChannel(keyStoreChannel)});

  final MethodChannel _channel;

  /// **The key the platform holds**, or `null` for a phone that has not paired.
  ///
  /// A value the platform hands back that is not 64 lowercase hex characters is **not a key**: the store answers
  /// `null`, which is a phone that pairs again, rather than a stranger's frame the carrier would have to refuse
  /// (`relay.dart`'s `NotADeviceKey`). The branch is named because it can happen — a restored backup, a half-written
  /// item — and pairing is the recovery, not a crash on the first frame.
  @override
  Future<String?> key() async {
    final Object? stored = await _ask(keyReadMethod);
    if (stored == null) {
      return null;
    }
    if (stored is! String || !_deviceKey.hasMatch(stored)) {
      return null;
    }
    return stored;
  }

  /// **Keep the key** the wall handed over (`RelayCarrier.claim`, and nothing else).
  ///
  /// A write that fails is a [KeyStoreUnavailable]; the claim does not invent a second home for a key it could not
  /// store, and the caller is `RelayCarrier.claim`, whose own rule is one write in one place.
  @override
  Future<void> remember(String key) async {
    await _ask(
      keyRememberMethod,
      arguments: <String, Object?>{keyArgument: key},
    );
  }

  /// **Delete the key** — and, on Android, the AES key that wraps it, so nothing is left to unwrap.
  ///
  /// Not part of [KeyStore]: the interface is the carrier's two verbs, and forgetting is the app's own (an uninstall
  /// clears the app's data, a `Forget this wall` row would call this, and the checklist in #265's PR drives it by
  /// reinstalling).
  Future<void> forget() async {
    await _ask(keyForgetMethod);
  }

  /// One call on the one channel. **A platform failure is a refusal, not a crash** (`INV-07`'s spirit: a named branch
  /// with an action, never a swallowed error) — and the platform's message is not carried into the refusal, so no word
  /// the platform chose can become a log line on this side (`INV-08`).
  Future<Object?> _ask(String method, {Map<String, Object?>? arguments}) async {
    try {
      return await _channel.invokeMethod<Object?>(method, arguments);
    } on PlatformException {
      throw const KeyStoreUnavailable(keyStoreChannel);
    } on MissingPluginException {
      // No platform under this build: a widget test with no fake, or a desktop run of a phone's app. The same refusal,
      // because it is the same fact — there is no key store to answer.
      throw const KeyStoreUnavailable(keyStoreChannel);
    }
  }

  /// **The key is not in this sentence** (`INV-08`): the store has no field a key is assigned to, and this is the only
  /// string it writes.
  @override
  String toString() => 'the platform key store (its key is not printed)';
}

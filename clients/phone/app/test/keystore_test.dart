/// **The device key's platform home, held to its contract** (#265) — the Dart half, with the platform injected.
///
/// ADR-0008 decision 4 is the subject: `pair.claim` hands a phone a key once, and the Keychain or the Keystore is the
/// only place it may live. The Kotlin and the Swift that do the storing are the one part of this repository no test
/// here can run (there is no Java, no Android SDK and no macOS on the machine this was written on), so this file does
/// the two things a laptop can: **it drives the whole seam through `setMockMethodCallHandler`**, which is the actual
/// channel and the actual store with a fake platform under it, and **it holds the two sides' strings to each other**,
/// because a channel name spelled two ways is a read that answers `null` for ever.
///
/// What is proved, one test each:
///
/// 1. `read` answers what was remembered, and `null` before anything was;
/// 2. `forget` makes `read` answer `null` — the key is gone, and nothing is left that a second read could return;
/// 3. one channel, three methods, **no other traffic**: the calls the platform saw are exactly the six this file made;
/// 4. a channel that refuses is a [KeyStoreUnavailable] — a refusal, not a crash — for all three methods;
/// 5. a value the platform holds that is not 64 lowercase hex characters is not a key: it reads as a phone that has not
///    paired, and the app offers pairing instead of dialling as a stranger;
/// 6. the app reads its key **through this store** (the shell is wired to the channel, not to memory), and a channel
///    that refuses at start draws the pairing screen;
/// 7. **`INV-08`**: the key is in no `toString`, no log line and no widget's text — asserted over the whole tree, and
///    over the sentences this file's own refusals write;
/// 8. the channel's name and its three methods are the **same strings in the Kotlin and the Swift** that answer them.
library;

import 'dart:io';

import 'package:arcons_phone/relay.dart' show KeyStore;
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';

import 'package:arcons_phone_app/keystore.dart';
import 'package:arcons_phone_app/main.dart' show PhoneApp;
import 'package:arcons_phone_app/overview.dart' show canvasKey;

import 'harness.dart';

/// A relay URL the fake never dials: the reader is [FakeRelay], and no socket is opened.
final Uri appRelay = Uri.parse('tcp://relay.invalid:7000');

/// **The platform, as a laptop holds it**: the stored item, the calls it was asked for, and the failure it can answer
/// with instead.
///
/// This is the *other* side of the channel, not a `KeyStore`: the point is that [PlatformKeyStore] is exercised through
/// `MethodChannel.invokeMethod` with the standard codec, so the seam is the one a phone has and not a seam invented for
/// the test. `stored` is the item (the Keychain's, the Keystore's), and it is the only place a value lives.
final class FakePlatform {
  /// The item the platform holds: a key's text, or `null` for a phone that has not paired.
  String? stored;

  /// Every method the channel was asked for, in order — what "one channel, three methods, no other traffic" is about.
  final List<String> called = <String>[];

  /// The failure the platform answers with, when a test wants one. `null` is a platform that answers.
  PlatformException? failure;

  Future<Object?> handle(MethodCall call) async {
    called.add(call.method);
    final refused = failure;
    if (refused != null) {
      throw refused;
    }
    switch (call.method) {
      case keyReadMethod:
        return stored;
      case keyRememberMethod:
        // The one write: the key, and nothing beside it.
        stored = (call.arguments as Map<Object?, Object?>)[keyArgument] as String?;
        return null;
      case keyForgetMethod:
        stored = null;
        return null;
      default:
        // A method this channel does not name is a loud failure, never a `null` that reads as "not paired".
        throw MissingPluginException();
    }
  }
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  // The channel `PlatformKeyStore` builds by default, by its name: the mock is keyed by the name, so the store under
  // test is the one `main.dart` builds and not a variant.
  const MethodChannel channel = MethodChannel(keyStoreChannel, StandardMethodCodec());

  late FakePlatform platform;

  setUp(() {
    platform = FakePlatform();
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, platform.handle);
  });

  tearDown(() {
    TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger
        .setMockMethodCallHandler(channel, null);
  });

  group('the store', () {
    test('read answers what was remembered, and null before anything was', () async {
      final store = PlatformKeyStore(channel: channel);
      expect(
        store,
        isA<KeyStore>(),
        reason: 'the platform store does not implement the interface the carrier asks (#256): the app would have to '
            'know a second vocabulary for a key',
      );
      expect(
        await store.key(),
        isNull,
        reason: 'a store with nothing in it answered something other than null: `null` is a phone that has not paired, '
            'and it is what makes the pairing screen the first screen',
      );

      await store.remember(testDeviceKey);
      expect(
        await store.key(),
        testDeviceKey,
        reason: 'the key did not survive a round trip through the channel: `pair.claim` writes it once and every dial '
            'reads it back, so a read that does not answer is a phone that re-pairs on every launch (#265)',
      );
    });

    test('forget makes read null', () async {
      final store = PlatformKeyStore(channel: channel);
      await store.remember(testDeviceKey);
      expect(await store.key(), testDeviceKey);

      await store.forget();
      expect(
        await store.key(),
        isNull,
        reason: 'the key survived `forget`: ADR-0008 decision 4 says the platform store is the only place a key lives, '
            'so a forget that leaves it readable leaves it reachable — and `pair.forget` on the wall would be a wall '
            'that forgot a phone which still holds the key (#265)',
      );
    });

    test('one channel, three methods, and no other traffic', () async {
      final store = PlatformKeyStore(channel: channel);
      await store.key();
      await store.remember(testDeviceKey);
      await store.key();
      await store.forget();
      await store.key();

      expect(
        platform.called,
        [
          keyReadMethod,
          keyRememberMethod,
          keyReadMethod,
          keyForgetMethod,
          keyReadMethod,
        ],
        reason: 'the platform was asked for something this channel does not name, or the names are not the three '
            'constants: every crossing is code no test in this repository can run, which is why #265 allows exactly '
            '`key.read`, `key.remember` and `key.forget`',
      );
      expect(
        {keyStoreChannel, keyReadMethod, keyRememberMethod, keyForgetMethod}.length,
        4,
        reason: 'the channel and its three methods are not four distinct constants',
      );
    });

    test('a channel that refuses is a refusal, and not a crash', () async {
      platform.failure = PlatformException(
        code: 'key-store',
        message: 'the keychain item could not be read (OSStatus -25300)',
      );
      final store = PlatformKeyStore(channel: channel);

      await expectLater(
        store.key(),
        throwsA(isA<KeyStoreUnavailable>()),
        reason: 'a platform failure escaped as something the app cannot name: `INV-07` one layer over — a failure has a '
            'branch and an action, and the action here is "this phone has no key to read"',
      );
      await expectLater(
        store.remember(testDeviceKey),
        throwsA(isA<KeyStoreUnavailable>()),
        reason: 'a write the platform refused escaped as something the caller cannot name: the claim must be able to say '
            'the key was not stored rather than believe it was',
      );
      await expectLater(
        store.forget(),
        throwsA(isA<KeyStoreUnavailable>()),
        reason: 'a failed forget escaped as something the caller cannot name',
      );
    });

    test('what the platform holds but is not a key is not a key', () async {
      platform.stored = 'not a device key';
      expect(
        await PlatformKeyStore(channel: channel).key(),
        isNull,
        reason: 'a value the platform holds that is not 64 lowercase hex characters was handed on as a key: a restored '
            'backup or a half-written item then becomes a stranger\'s frame the carrier has to refuse, where pairing is '
            'the recovery (`relay.dart`\'s `NotADeviceKey`)',
      );

      // The shape is the wall's, both ways: 64 characters, and lowercase hex.
      for (final wrong in [testDeviceKey.toUpperCase(), '${testDeviceKey}0', testDeviceKey.substring(1)]) {
        platform.stored = wrong;
        expect(
          await PlatformKeyStore(channel: channel).key(),
          isNull,
          reason: 'a key of the wrong shape was accepted: `pair.claim` hands back 32 bytes as 64 lowercase hex '
              'characters (`src/control/relay.rs`), and `devices.toml` hashes exactly that',
        );
      }
    });
  });

  group('the app', () {
    final corpus = readCorpus();
    final first = projectionLine(corpus);
    final now = DateTime.utc(2026, 11, 6, 12);

    testWidgets('reads its key through the channel, and draws the wall the key dials', (tester) async {
      platform.stored = testDeviceKey;
      final fake = FakeRelay([first]);
      final lines = <String>[];
      await pumpApp(
        tester,
        PhoneApp(
          relay: appRelay,
          keys: PlatformKeyStore(channel: channel),
          clock: () => now,
          log: lines.add,
          openRelay: (relay, keys) {
            fake.keys = keys;
            return fake;
          },
        ),
      );
      await tester.pumpAndSettle();

      expect(
        platform.called,
        contains(keyReadMethod),
        reason: 'the app started without reading the platform store: the shell is still wired to a store of its own, so '
            'the key a phone really holds is never asked for (#265)',
      );
      expect(
        find.text('Pair with a wall'),
        findsNothing,
        reason: 'a phone whose platform store holds a key was shown the pairing screen: it would pair a second time and '
            'write a second row on the wall',
      );
      expect(
        find.byKey(canvasKey),
        findsOneWidget,
        reason: 'the app read a key through the channel and drew nothing: the store answered and the shell did not use '
            'the answer',
      );

      // **INV-08**, over every string the screen would draw and every line the shell logged.
      for (final text in screenText(tester)) {
        expect(
          text.contains(testDeviceKey),
          isFalse,
          reason: 'INV-08: the device key reached a widget\'s text. The key lives in the platform\'s store, in `device` '
              'on the wire, and in nothing a screenshot or a screen reader can read',
        );
      }
      for (final line in lines) {
        expect(
          line.contains(testDeviceKey),
          isFalse,
          reason: 'INV-08: the device key reached a log line. A secret that can be printed is a secret that will be',
        );
      }
    });

    testWidgets('a channel that refuses at start offers pairing, and does not crash', (tester) async {
      platform.failure = PlatformException(code: 'key-store', message: 'the keychain is locked');
      final fake = FakeRelay(const []);
      final lines = <String>[];
      await pumpApp(
        tester,
        PhoneApp(
          relay: appRelay,
          keys: PlatformKeyStore(channel: channel),
          clock: () => now,
          log: lines.add,
          openRelay: (relay, keys) {
            fake.keys = keys;
            return fake;
          },
        ),
      );
      await tester.pumpAndSettle();

      expect(
        find.text('Pair with a wall'),
        findsOneWidget,
        reason: 'a store that could not answer did not lead to pairing: a key that cannot be read cannot be dialled '
            'with, and `pair.claim` is how a key is made (#265)',
      );
      expect(
        fake.sent,
        isEmpty,
        reason: 'the app sent a frame with no key behind it: a failed read is not a transformation of the key, and a '
            'stranger\'s read would be refused on the wall',
      );
      expect(
        lines,
        isNotEmpty,
        reason: 'a platform store that refused was swallowed without a line: the operator gets a pairing screen for no '
            'stated reason (`INV-07`: a failure is a named branch with an action, not a silence)',
      );
      for (final line in lines) {
        expect(
          line.contains(testDeviceKey),
          isFalse,
          reason: 'INV-08: the refusal\'s line carried the key',
        );
      }
    });

    test('`main` builds the platform store, and nothing that pretends to be one', () {
      final source = File('lib/main.dart').readAsStringSync();
      expect(
        source.contains('PlatformKeyStore()'),
        isTrue,
        reason: '`main` does not build the platform key store: the app would run on a store that forgets at every '
            'restart, which is ADR-0008 decision 4\'s falsifier seen from the other side (#265)',
      );
      expect(
        source.contains('MemoryKeyStore'),
        isFalse,
        reason: '`main` still names a memory key store: #265 replaced it, and a phone that pairs again after every '
            'restart is the state this ticket ends',
      );
    });
  });

  group('the key is not a string this app writes', () {
    test('no `toString` carries it', () {
      // The sentences this half of the channel can produce, and the object a caller might print.
      for (final sentence in [
        PlatformKeyStore(channel: channel).toString(),
        const KeyStoreUnavailable(keyStoreChannel).toString(),
      ]) {
        expect(
          sentence.contains(testDeviceKey),
          isFalse,
          reason: 'INV-08: a `toString` carried the device key. A key that can be interpolated into a string will be, '
              'which is why the store holds no field a key is assigned to',
        );
      }
    });

    test('the channel and its three names are the Kotlin\'s and the Swift\'s own strings', () {
      // The one drift this repository cannot compile away: one constant per side, and a typo on either side is a read
      // that answers `null` for ever. Read as text, because that is all a Linux runner can do with a `.kt` and a
      // `.swift` — the compilers are CI's `phone-android` and `phone-ios` jobs.
      for (final file in [
        'android/app/src/main/kotlin/cc/arcons/phone/PlatformKeyStore.kt',
        'ios/Runner/AppDelegate.swift',
      ]) {
        final source = File(file);
        expect(
          source.existsSync(),
          isTrue,
          reason: '$file is gone: the platform half of #265 is not in the tree, so the channel answers nothing on a '
              'phone',
        );
        final text = source.readAsStringSync();
        for (final named in [keyStoreChannel, keyReadMethod, keyRememberMethod, keyForgetMethod]) {
          expect(
            text.contains('"$named"'),
            isTrue,
            reason: '$file does not name "$named": the Dart side and the platform side disagree about the channel or a '
                'method, which is a read that answers `null` (or never answers) with nothing failing loudly (#265)',
          );
        }
      }
    });
  });
}

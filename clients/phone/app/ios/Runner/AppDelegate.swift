import Flutter
import Security
import UIKit

/// **The phone's iOS shell — and the device key's home** (#265, ADR-0008 decision 4).
///
/// The Keychain half of `app/lib/keystore.dart`, written by hand against `SecItemAdd` / `SecItemCopyMatching` /
/// `SecItemDelete` rather than through a package: sixty lines an auditor can read, which is what decision 4 asks for.
/// One channel, three methods, and **no other traffic** between Dart and iOS.
///
/// **Two facts about the Keychain decide this file.** The first is that an item survives the app being deleted — the
/// Keychain belongs to the device, not to the bundle — so a reinstall would find yesterday's key and the pairing screen
/// would never be offered, which #265 rejects by name (*a key that survives an uninstall — it must not*). The app's
/// `UserDefaults` **is** deleted with the app, so the absence of one flag is the only honest signal a first run has, and
/// a first run deletes the item: see [DeviceKeyStore.forgetOnFirstRun]. The second is that
/// `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` is the only accessibility class that is both *readable by a phone
/// in a cradle after a reboot* and *never carried to another device by a restore* — a paired device is a device, and a
/// backup is not one.
///
/// **Not proved by any command in this repository**: no machine here has macOS or Xcode, so the Swift has no local
/// proof at all. CI's `phone-ios` job is what says this file compiles, and #265's checklist in the PR is what says it
/// works. That is said out loud rather than implied.
@main
@objc class AppDelegate: FlutterAppDelegate, FlutterImplicitEngineDelegate {
  override func application(
    _ application: UIApplication,
    didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]?
  ) -> Bool {
    // **Before anything can read a key**: a first run clears what an uninstall left behind (see `DeviceKeyStore`).
    DeviceKeyStore.forgetOnFirstRun()
    return super.application(application, didFinishLaunchingWithOptions: launchOptions)
  }

  func didInitializeImplicitFlutterEngine(_ engineBridge: FlutterImplicitEngineBridge) {
    GeneratedPluginRegistrant.register(with: engineBridge.pluginRegistry)

    // The engine's own messenger, through the application registrar: this channel is the app's, not a plugin's.
    let channel = FlutterMethodChannel(
      name: KeyChannel.name,
      binaryMessenger: engineBridge.applicationRegistrar.messenger()
    )
    channel.setMethodCallHandler { call, result in
      switch call.method {
      case KeyChannel.read:
        DeviceKeyStore.read(result)
      case KeyChannel.remember:
        guard
          let arguments = call.arguments as? [String: Any],
          let key = arguments[KeyChannel.keyArgument] as? String
        else {
          result(
            FlutterError(
              code: KeyChannel.failureCode,
              message: "key.remember was called with no key",
              details: nil
            )
          )
          return
        }
        DeviceKeyStore.remember(key, result)
      case KeyChannel.forget:
        DeviceKeyStore.forget(result)
      default:
        // **Not one of this channel's three methods.** The platform's own "no such method" answer, so a Dart call this
        // side does not implement fails loudly rather than answering a `nil` that would read as "not paired".
        result(FlutterMethodNotImplemented)
      }
    }
  }
}

/// **The one channel, and its three methods** — the same strings as `app/lib/keystore.dart` and
/// `android/…/PlatformKeyStore.kt`. They are written on each side because a name shared through a generator is a build
/// step #265 does not add; a typo is a read that answers `key-store` and a phone that offers pairing, never a silent
/// `nil`.
private enum KeyChannel {
  static let name = "cc.arcons.phone/key-store"
  static let read = "key.read"
  static let remember = "key.remember"
  static let forget = "key.forget"

  /// The one argument `key.remember` carries, and the code every failure replies with (the Dart side turns it into
  /// `KeyStoreUnavailable`).
  static let keyArgument = "key"
  static let failureCode = "key-store"
}

/// **The Keychain item, and the first-run flag that keeps it from outliving the app** (#265).
///
/// One generic password, named by a service and an account, in the device's Keychain. Nothing is written to
/// `UserDefaults` but the flag: **a key in `UserDefaults` or a file would be ADR-0008's falsifier**, and the flag is a
/// boolean that says whether this installation has run before.
private enum DeviceKeyStore {
  /// The item's service and account: the pair that names it, and the only two strings that find it again.
  static let service = "cc.arcons.phone.device-key"
  static let account = "device-key"

  /// **Whether this installation has run before** — in `UserDefaults`, which is deleted with the app, which is the
  /// point.
  static let launchedBefore = "cc.arcons.phone.launched-before"

  /// The item's query: a generic password, this item, and nothing else.
  private static func query() -> [String: Any] {
    return [
      kSecClass as String: kSecClassGenericPassword,
      kSecAttrService as String: service,
      kSecAttrAccount as String: account,
    ]
  }

  /// **A first run deletes what an uninstall left behind.**
  ///
  /// `SecItemDelete` on a device that never held the item answers `errSecItemNotFound`, which is success here: a phone
  /// with no key has nothing to forget. The flag is set in the same breath, so the deletion happens once per
  /// installation and an upgrade — which keeps `UserDefaults` — keeps its key.
  static func forgetOnFirstRun() {
    let defaults = UserDefaults.standard
    if defaults.bool(forKey: launchedBefore) {
      return
    }
    delete()
    defaults.set(true, forKey: launchedBefore)
  }

  /// The key as stored, or `nil` for a phone that has not paired.
  static func read(_ result: @escaping FlutterResult) {
    var query = self.query()
    query[kSecReturnData as String] = true
    query[kSecMatchLimit as String] = kSecMatchLimitOne
    var item: CFTypeRef?
    let status = SecItemCopyMatching(query as CFDictionary, &item)
    switch status {
    case errSecSuccess:
      guard let data = item as? Data, let key = String(data: data, encoding: .utf8) else {
        // An item this store did not write, or one whose bytes are not text: not a key, and a phone that pairs again.
        result(
          FlutterError(
            code: KeyChannel.failureCode,
            message: "the keychain item is not a device key",
            details: nil
          )
        )
        return
      }
      result(key)
    case errSecItemNotFound:
      // The ordinary answer for a phone that has not paired.
      result(nil)
    default:
      // Every other `OSStatus` is the Keychain refusing: locked, or an entitlement it does not have. The status is a
      // number and never an item's bytes.
      result(failure(status, "the keychain item could not be read"))
    }
  }

  /// Write the key the wall handed over, once (`RelayCarrier.claim`).
  static func remember(_ key: String, _ result: @escaping FlutterResult) {
    guard let data = key.data(using: .utf8) else {
      result(
        FlutterError(
          code: KeyChannel.failureCode,
          message: "the key is not UTF-8",
          details: nil
        )
      )
      return
    }
    // **`kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly`**: readable after the first unlock following a reboot, so a
    // phone in a cradle reads its key without a hand on it — and `ThisDeviceOnly` so a restore to another phone does
    // not carry the pairing with it.
    let attributes: [String: Any] = [
      kSecValueData as String: data,
      kSecAttrAccessible as String: kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly,
    ]
    let addStatus = SecItemAdd(query().merging(attributes) { _, new in new } as CFDictionary, nil)
    switch addStatus {
    case errSecSuccess:
      result(nil)
    case errSecDuplicateItem:
      // A phone that pairs again replaces the item it holds: one key, one home. The accessibility attribute is set at
      // add and cannot be changed by an update, which is why the item is replaced rather than left odd.
      let updateStatus = SecItemUpdate(
        query() as CFDictionary,
        [kSecValueData as String: data] as CFDictionary
      )
      if updateStatus == errSecSuccess {
        result(nil)
      } else {
        result(failure(updateStatus, "the keychain item could not be written"))
      }
    default:
      result(failure(addStatus, "the keychain item could not be added"))
    }
  }

  /// Delete the item — the shell's `forget`, and the first run's.
  static func forget(_ result: @escaping FlutterResult) {
    let status = delete()
    if status == errSecSuccess {
      result(nil)
    } else {
      result(failure(status, "the keychain item could not be deleted"))
    }
  }

  /// `errSecItemNotFound` is success: a phone with no key has nothing to forget.
  @discardableResult
  private static func delete() -> OSStatus {
    let status = SecItemDelete(query() as CFDictionary)
    return status == errSecItemNotFound ? errSecSuccess : status
  }

  /// A Keychain status as the reply a Dart caller reads. **The status is an `OSStatus`** — a number, never an item's
  /// bytes and never the key (`INV-08`).
  private static func failure(_ status: OSStatus, _ what: String) -> FlutterError {
    return FlutterError(
      code: KeyChannel.failureCode,
      message: "\(what) (OSStatus \(status))",
      details: nil
    )
  }
}

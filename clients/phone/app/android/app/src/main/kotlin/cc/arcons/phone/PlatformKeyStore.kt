package cc.arcons.phone

import android.content.Context
import android.content.SharedPreferences
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import android.util.Base64
import android.util.Log
import io.flutter.plugin.common.MethodCall
import io.flutter.plugin.common.MethodChannel
import java.io.IOException
import java.security.GeneralSecurityException
import java.security.KeyStore
import javax.crypto.Cipher
import javax.crypto.KeyGenerator
import javax.crypto.SecretKey
import javax.crypto.spec.GCMParameterSpec

/**
 * **The device key's home on Android** (#265, ADR-0008 decision 4) — the Kotlin half of `app/lib/keystore.dart`.
 *
 * Two pieces, and neither is a third-party package:
 *
 * - **the wrapping key**: a random AES-256 key generated **inside the AndroidKeyStore**, which means its bytes never
 *   leave the keystore: a `Cipher` is handed the key and returns ciphertext. It is not exportable, not backed up and not
 *   in this app's data directory. `setUserAuthenticationRequired(false)` is deliberate: a phone in a cradle must read
 *   its key after a reboot without a fingerprint prompt. The item is therefore only as strong as the device's
 *   encryption, which is what `kSecAttrAccessibleAfterFirstUnlockThisDeviceOnly` is on the other platform.
 * - **the wrapped secret**: the device key, encrypted with that AES key in GCM mode, written as two Base64 strings in
 *   **this app's own** `SharedPreferences` (private to the app, `MODE_PRIVATE`). The blob alone is useless — it cannot
 *   be unwrapped without a key that never leaves the keystore — and a `SharedPreferences` file is *not* where the key
 *   lives, which is the distinction ADR-0008's falsifier (*a key in shared preferences*) turns on.
 *
 * **A write is a `commit()` and not an `apply()`**: this is the one secret the app writes, once, and a key that was
 * still in memory when the phone was killed is a phone that pairs again for no reason. Sixty-four bytes of disk I/O at
 * the moment of pairing is the price.
 *
 * **Nothing here logs a key, an IV or plaintext** (`INV-08`): the log lines name the operation and the failure, and the
 * error replies carry a sentence and our channel's name. The one value that crosses this class is the argument of
 * `key.remember`.
 *
 * **A blob this store cannot unwrap is forgotten**, not guessed at: a restore to another phone, a lock-screen change
 * that invalidated the keystore entry, or a half-written file each mean the same thing — this phone has no key — and
 * the answer is `null` so the app offers pairing. The branch is named in [read], with the exception family that means
 * it, rather than a catch-all.
 *
 * **Not proved by any command in this repository**: no test here has Java, an Android SDK or a device. CI's
 * `phone-android` job is what says this file compiles, and #265's checklist in the PR is what says it works.
 */
internal class PlatformKeyStore(private val context: Context) {

    /**
     * The channel's three methods, and its name — **the same strings as `app/lib/keystore.dart`**, written on both
     * sides because a name shared through a generator would be a build step this ticket does not add. A typo here is a
     * read that answers `KeyStoreUnavailable` and a phone that pairs again, never a silent `null`.
     */
    fun handle(call: MethodCall, result: MethodChannel.Result) {
        // **The whole dispatch is guarded** by the two exception families this store's operations can raise — a keystore
        // that will not load (`IOException`), a key it will not hand over (`GeneralSecurityException`). A failure that
        // reaches a Flutter message handler uncaught can take the platform thread with it, and a crash is not a state a
        // phone can draw. The reply carries our code and a sentence, and nothing from Dart's side reads it as a key.
        try {
            dispatch(call, result)
        } catch (unavailable: GeneralSecurityException) {
            Log.w(TAG, "the AndroidKeyStore did not answer: $unavailable")
            result.error(ERROR_CODE, "the AndroidKeyStore did not answer", null)
        } catch (unreadable: IOException) {
            Log.w(TAG, "the AndroidKeyStore could not be loaded: $unreadable")
            result.error(ERROR_CODE, "the AndroidKeyStore could not be loaded", null)
        }
    }

    private fun dispatch(call: MethodCall, result: MethodChannel.Result) {
        when (call.method) {
            METHOD_READ -> result.success(read())
            METHOD_REMEMBER -> {
                val key = call.argument<String>(ARGUMENT_KEY)
                if (key == null) {
                    result.error(ERROR_CODE, "key.remember was called with no key", null)
                    return
                }
                remember(key)
                result.success(null)
            }
            METHOD_FORGET -> {
                forget()
                result.success(null)
            }
            // **Not one of this channel's three methods.** The platform's own "no such method" answer, so a Dart call
            // this side does not implement fails loudly instead of answering a `null` that would read as "not paired".
            else -> result.notImplemented()
        }
    }

    /**
     * The key as stored, or `null` for a phone that has not paired — and `null` for a blob this store cannot unwrap,
     * which is the same fact from the app's side: there is no key to dial with, so pairing is the recovery.
     */
    private fun read(): String? {
        val preferences = preferences()
        val iv = preferences.getString(PREFERENCE_IV, null)
        val wrapped = preferences.getString(PREFERENCE_WRAPPED, null)
        if (iv == null || wrapped == null) {
            return null
        }
        val unwrapping = wrappingKey(create = false)
        if (unwrapping == null) {
            // The AES key that wraps it is gone: a restore to a device whose keystore never held it, or a lock-screen
            // change that invalidated the entry. Nothing can unwrap the blob, so it is deleted rather than kept.
            Log.w(TAG, "the wrapping key is gone; the stored device key is forgotten and the phone pairs again")
            clearWrapped(preferences)
            return null
        }
        return try {
            val cipher = Cipher.getInstance(TRANSFORMATION)
            cipher.init(
                Cipher.DECRYPT_MODE,
                unwrapping,
                GCMParameterSpec(TAG_BITS, decode(iv)),
            )
            String(cipher.doFinal(decode(wrapped)), Charsets.UTF_8)
        } catch (unreadable: GeneralSecurityException) {
            // GCM's tag failed, or the parameters are not this cipher's: these bytes are not a key this store wrote.
            Log.w(TAG, "the stored device key could not be unwrapped; it is forgotten and the phone pairs again")
            clearWrapped(preferences)
            null
        } catch (notBase64: IllegalArgumentException) {
            // `Base64.decode` on a value that is not Base64: the same answer, and the same reason.
            Log.w(TAG, "the stored device key is not base64; it is forgotten and the phone pairs again")
            clearWrapped(preferences)
            null
        }
    }

    /** Wrap [key] with the keystore's AES key and write the blob — the one write `pair.claim` makes. */
    private fun remember(key: String) {
        val wrapping = wrappingKey(create = true)
            ?: throw GeneralSecurityException("the AndroidKeyStore generated no AES key")
        val cipher = Cipher.getInstance(TRANSFORMATION)
        cipher.init(Cipher.ENCRYPT_MODE, wrapping)
        val wrapped = cipher.doFinal(key.toByteArray(Charsets.UTF_8))
        // The IV is the cipher's, and it is stored beside the blob: an IV is not a secret, and the blob cannot be read
        // without it. Both are written in one `commit()`, so there is no state in which one exists and the other does.
        val written = preferences()
            .edit()
            .putString(PREFERENCE_IV, Base64.encodeToString(cipher.iv, Base64.NO_WRAP))
            .putString(PREFERENCE_WRAPPED, Base64.encodeToString(wrapped, Base64.NO_WRAP))
            .commit()
        if (!written) {
            throw GeneralSecurityException("the wrapped device key could not be written")
        }
    }

    /** Delete the blob **and** the AES key that wraps it, so nothing is left that could unwrap anything. */
    private fun forget() {
        clearWrapped(preferences())
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE)
        keyStore.load(null)
        keyStore.deleteEntry(ALIAS)
    }

    /**
     * **The AES key that wraps the secret**, generated in the AndroidKeyStore on first use.
     *
     * `create = false` never generates: a read must not invent a wrapping key for a blob it cannot have written, or the
     * store would answer `null` for ever without ever saying so.
     */
    private fun wrappingKey(create: Boolean): SecretKey? {
        val keyStore = KeyStore.getInstance(ANDROID_KEYSTORE)
        keyStore.load(null)
        val existing = keyStore.getKey(ALIAS, null) as? SecretKey
        if (existing != null) {
            return existing
        }
        if (!create) {
            return null
        }
        val generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, ANDROID_KEYSTORE)
        generator.init(
            KeyGenParameterSpec.Builder(
                ALIAS,
                KeyProperties.PURPOSE_ENCRYPT or KeyProperties.PURPOSE_DECRYPT,
            )
                .setBlockModes(KeyProperties.BLOCK_MODE_GCM)
                .setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE)
                // The IV is the cipher's, never ours: a repeat would be the one way to misuse GCM.
                .setRandomizedEncryptionRequired(true)
                // **No fingerprint prompt**: a phone in a cradle reads its key after a reboot, which is the whole point
                // of the key — the wall is unmanned. The device's own encryption is the protection, as it is on iOS.
                .setUserAuthenticationRequired(false)
                .build(),
        )
        return generator.generateKey()
    }

    private fun preferences() = context.getSharedPreferences(PREFERENCES, Context.MODE_PRIVATE)

    /** Both halves of the blob, together: a file that keeps one and not the other is not a key either. */
    private fun clearWrapped(preferences: SharedPreferences) {
        preferences.edit().remove(PREFERENCE_IV).remove(PREFERENCE_WRAPPED).commit()
    }

    private fun decode(value: String): ByteArray = Base64.decode(value, Base64.NO_WRAP)

    internal companion object {
        /**
         * **The channel**, and the one constant `MainActivity` reads — the same string as `app/lib/keystore.dart` and
         * `ios/Runner/AppDelegate.swift`.
         */
        const val CHANNEL = "cc.arcons.phone/key-store"

        /** The three methods, and the argument `key.remember` carries: named as the Dart side names them. */
        private const val METHOD_READ = "key.read"
        private const val METHOD_REMEMBER = "key.remember"
        private const val METHOD_FORGET = "key.forget"
        private const val ARGUMENT_KEY = "key"

        /** The reply's code when the platform will not answer: the Dart side turns it into `KeyStoreUnavailable`. */
        private const val ERROR_CODE = "key-store"

        /** This app's own private preferences, and the two halves of the wrapped secret inside them. */
        private const val PREFERENCES = "cc.arcons.phone.keys"
        private const val PREFERENCE_IV = "device-key-iv"
        private const val PREFERENCE_WRAPPED = "device-key-wrapped"

        /** The keystore, the alias of the wrapping key, and the transformation that uses it. */
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        private const val ALIAS = "cc.arcons.phone.device-key"
        private const val TRANSFORMATION = "AES/GCM/NoPadding"

        /** GCM's tag: 128 bits, the size `Cipher` is specified to produce. */
        private const val TAG_BITS = 128

        private const val TAG = "arcons-phone-keystore"
    }
}

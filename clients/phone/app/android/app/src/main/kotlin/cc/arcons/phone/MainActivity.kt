package cc.arcons.phone

import io.flutter.embedding.android.FlutterActivity
import io.flutter.embedding.engine.FlutterEngine
import io.flutter.plugin.common.MethodChannel

/**
 * **The app's one activity**, and the one place the platform channel is opened (#265).
 *
 * `MethodChannel` is the whole of the traffic between Dart and Android: `key.read`, `key.remember` and `key.forget`,
 * answered by [PlatformKeyStore]. Nothing else in this app crosses the channel — no second channel, no events, no
 * method name that is not one of those three — because every crossing is code no test in this repository can run.
 */
class MainActivity : FlutterActivity() {
    override fun configureFlutterEngine(flutterEngine: FlutterEngine) {
        super.configureFlutterEngine(flutterEngine)
        val keyStore = PlatformKeyStore(this)
        MethodChannel(flutterEngine.dartExecutor.binaryMessenger, PlatformKeyStore.CHANNEL)
            .setMethodCallHandler { call, result -> keyStore.handle(call, result) }
    }
}

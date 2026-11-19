/// **The camera seam** (#271): one method, the plugin behind it, and a fake the widget tests use.
///
/// Camera plus decoding is the one thing in this app that is not written by hand. ADR-0008 decision 4's "by hand" is
/// about the **device key and the platform's stores**; a QR decoder is a codec with a camera attached, and the
/// maintained package for it is `mobile_scanner` (BSD-3-Clause, `THIRD-PARTY-NOTICES.md`). The plugin is named here and
/// **nowhere else**: a widget test hands [PairingScreen] a fake [Scanner] and no test ever constructs a camera.
///
/// The interface returns the raw payload rather than the parsed pairing, because the payload is what a camera reads and
/// `arcons_phone/pairing.dart` is the one place that decides whether it is a pairing. A refused camera is
/// [CameraUnavailable], a named answer with a sentence; backing out of the camera is `null` and is not a refusal.
library;

import 'package:mobile_scanner/mobile_scanner.dart';
import 'package:material_ui/material_ui.dart';

import 'theme.dart';

/// **Open a camera and read one payload**, or answer `null` when the operator backs out.
///
/// Throws [CameraUnavailable] when the camera cannot be used: the screen then keeps the typed path and says so in
/// words. The context is the route's, because a camera preview is a screen and not a service.
abstract interface class Scanner {
  Future<String?> scan(BuildContext context);
}

/// **Why a camera did not open.** Two reasons, both a sentence the pairing screen can draw.
enum CameraRefusal {
  /// The operator (or the platform's policy) denied the camera. Typing still works, and the sentence says so.
  permissionDenied,

  /// There is no camera, or the platform could not start it. The same recovery.
  unavailable;

  /// Words for the pairing screen — never the plugin's own message, which is not ours (`INV-08`'s spirit).
  String get sentence => switch (this) {
    CameraRefusal.permissionDenied =>
      'The camera is off. Type the six digits instead.',
    CameraRefusal.unavailable =>
      'The camera is not available. Type the six digits instead.',
  };
}

/// **The camera did not open**, by name. The pairing screen catches this and leaves the typed path working.
final class CameraUnavailable implements Exception {
  const CameraUnavailable(this.refusal);

  final CameraRefusal refusal;

  @override
  String toString() => 'the camera did not open: ${refusal.name}';
}

/// **A build with no camera plugin**: what a test's shell uses when it was handed no [Scanner]. Tapping Scan says the
/// camera is unavailable in words rather than opening anything.
final class NoCamera implements Scanner {
  const NoCamera();

  @override
  Future<String?> scan(BuildContext context) async {
    throw const CameraUnavailable(CameraRefusal.unavailable);
  }
}

/// **The real camera**: `mobile_scanner` behind one route. `main` constructs this; no test does.
final class CameraScanner implements Scanner {
  const CameraScanner();

  @override
  Future<String?> scan(BuildContext context) async {
    final result = await Navigator.of(context).push<_ScanResult>(
      MaterialPageRoute<_ScanResult>(
        fullscreenDialog: true,
        builder: (context) => const _CameraRoute(),
      ),
    );
    return switch (result) {
      null || _ScanCancelled() => null,
      _ScanPayload(:final payload) => payload,
      _ScanRefused(:final refusal) => throw CameraUnavailable(refusal),
    };
  }
}

/// What the camera route hands back. Sealed, so the three answers are a compile-visible set rather than a `null` with a
/// meaning nobody wrote down.
sealed class _ScanResult {
  const _ScanResult();
}

final class _ScanPayload extends _ScanResult {
  const _ScanPayload(this.payload);

  final String payload;
}

final class _ScanCancelled extends _ScanResult {
  const _ScanCancelled();
}

final class _ScanRefused extends _ScanResult {
  const _ScanRefused(this.refusal);

  final CameraRefusal refusal;
}

/// The camera's screen: a preview, one hint, and a way out. It pops with the first payload the decoder reads.
final class _CameraRoute extends StatefulWidget {
  const _CameraRoute();

  @override
  State<_CameraRoute> createState() => _CameraRouteState();
}

final class _CameraRouteState extends State<_CameraRoute> {
  /// A route pops once, even when several frames or an error arrive before it is gone.
  bool _finished = false;

  void _finish(_ScanResult result) {
    if (_finished || !mounted) {
      return;
    }
    _finished = true;
    Navigator.of(context).pop(result);
  }

  /// The plugin's error code, mapped to the screen's two reasons, **one arm per code and no default**: a plugin bump
  /// that adds a code is a compile error here rather than a camera failure silently called *unavailable*
  /// (`INV-07`'s spirit). **No plugin message is carried** (`INV-08`).
  CameraRefusal _refusalFor(
    MobileScannerException error,
  ) => switch (error.errorCode) {
    MobileScannerErrorCode.permissionDenied => CameraRefusal.permissionDenied,
    MobileScannerErrorCode.unsupported => CameraRefusal.unavailable,
    MobileScannerErrorCode.genericError => CameraRefusal.unavailable,
    MobileScannerErrorCode.controllerAlreadyInitialized =>
      CameraRefusal.unavailable,
    MobileScannerErrorCode.controllerDisposed => CameraRefusal.unavailable,
    MobileScannerErrorCode.controllerUninitialized => CameraRefusal.unavailable,
    MobileScannerErrorCode.controllerInitializing => CameraRefusal.unavailable,
    MobileScannerErrorCode.controllerNotAttached => CameraRefusal.unavailable,
  };

  @override
  Widget build(BuildContext context) {
    return Scaffold(
      backgroundColor: PhoneTokens.canvas,
      body: SafeArea(
        child: Stack(
          children: [
            Positioned.fill(
              child: MobileScanner(
                onDetect: (capture) {
                  for (final barcode in capture.barcodes) {
                    final payload = barcode.rawValue;
                    if (payload != null && payload.isNotEmpty) {
                      _finish(_ScanPayload(payload));
                      return;
                    }
                  }
                },
                // The error widget is built inside `MobileScanner`'s build, so the pop is a frame later.
                errorBuilder: (context, error) {
                  WidgetsBinding.instance.addPostFrameCallback((_) {
                    _finish(_ScanRefused(_refusalFor(error)));
                  });
                  return const ColoredBox(color: PhoneTokens.canvas);
                },
              ),
            ),
            Positioned(
              left: PhoneTokens.gap * 2,
              right: PhoneTokens.gap * 2,
              bottom: PhoneTokens.gap * 2,
              child: const Text(
                'Point the camera at the wall\'s pairing QR.',
                style: TextStyle(
                  color: PhoneTokens.title,
                  fontSize: PhoneTokens.screenSentenceSize,
                ),
              ),
            ),
            Positioned(
              top: PhoneTokens.gap,
              right: PhoneTokens.gap,
              child: Semantics(
                button: true,
                label: 'Close the camera',
                excludeSemantics: true,
                child: GestureDetector(
                  key: scannerCloseKey,
                  onTap: () => _finish(const _ScanCancelled()),
                  behavior: HitTestBehavior.opaque,
                  child: Container(
                    height: PhoneTokens.buttonHeight,
                    padding: PhoneTokens.chipPadding,
                    decoration: const BoxDecoration(
                      color: PhoneTokens.cardFill,
                      borderRadius: BorderRadius.all(PhoneTokens.corner),
                    ),
                    child: const Row(
                      mainAxisSize: MainAxisSize.min,
                      children: [
                        Icon(
                          Icons.close,
                          color: PhoneTokens.title,
                          size: PhoneTokens.glyphSize,
                        ),
                        SizedBox(width: PhoneTokens.gap),
                        Text(
                          'Close',
                          style: TextStyle(
                            color: PhoneTokens.title,
                            fontSize: PhoneTokens.buttonTextSize,
                          ),
                        ),
                      ],
                    ),
                  ),
                ),
              ),
            ),
          ],
        ),
      ),
    );
  }
}

/// The close control's name, so a test (or a future screen) can find it without a glyph.
const Key scannerCloseKey = ValueKey('scanner-close');

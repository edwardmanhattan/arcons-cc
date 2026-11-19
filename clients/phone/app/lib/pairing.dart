/// **The pairing screen** — six digits, a relay, a name, a scan, and one button (`docs/mobile-ui.md`, *pairing*; #194,
/// #262, #264, #271).
///
/// It is **the only screen before the overview**: a phone with no device key has nothing to read, and the code is a
/// picture on the wall in the room rather than a message, so the phone does not ask for one — it is typed here, or read
/// from the wall's QR with a camera.
///
/// ```text
/// Pair with a wall
/// [ 4 ][ 8 ][ 1 ]  [ 9 ][ 0 ][ 2 ]       the six digits the wall draws
/// [ Scan the wall's QR ]                  the camera, when there is one
/// Relay            [ tcp://host:port ]    prefilled from --dart-define, or remembered
/// Name this phone  [ operations phone ]
/// Pair
/// ```
///
/// **Which relay is used**: a scan wins over the field, and the field wins over the compiled default. A scan fills both
/// the code and the relay; typing six digits still works and uses the field; the field starts with the remembered value
/// when there is one and `--dart-define=ARCONS_RELAY` otherwise. The relay is a URL and not a secret, so it is kept in
/// preferences rather than the KeyStore (#271).
///
/// Three refusals from the wall, three **steps**, and the steps are the note's:
///
/// | the wall's words | the phone says | what it does |
/// | --- | --- | --- |
/// | `no pairing is open` | the code expired | the code field is emptied and focused: the wall has to be asked again |
/// | `that code is not the code` | that code is not the wall's | the code stays and its digits are selected |
/// | `that device is already paired` | this name is taken | the name field keeps the focus |
///
/// A **burnt** code is `no pairing is open` (#262: the wall gives up after five wrong ones and answers with the same
/// name), so it takes the first row's step and the phone invents no sentence for it. The three are matched by the
/// wall's **words**, because `docs/mobile-ui.md` says a wrong code and an expired code are refused by name — a client
/// that could not tell them apart would retry the wrong one forever. Any other refusal is drawn **verbatim** with no
/// step of the phone's own: a step nobody wrote is a rule invented at the screen.
///
/// **A camera that will not open is not a dead end.** A denied permission or a phone with no camera leaves the typed
/// path working and says so in words. A payload the parser refuses fills **nothing** and names the refusal. The
/// scanner is the `Scanner` seam (`scanner.dart`), so no test here constructs a camera.
///
/// **Nothing secret is printed.** The six digits live in one [TextEditingController] and are cleared when they are
/// spent; the key `pair.claim` hands back goes to the [KeyStore] inside the carrier and is never a widget's text, a log
/// line or a `toString` (`INV-08`). The relay field's value is shown because it is the setting being edited, and it is
/// never interpolated into a log either.
library;

import 'dart:async';

import 'package:arcons_phone/client.dart' show Denied, Refusal;
import 'package:arcons_phone/pairing.dart'
    show
        Pairing,
        PairingRefused,
        PairingRefusal,
        parseDialableRelay,
        parsePairingUri;
import 'package:arcons_phone/relay.dart' show KeyStore;
import 'package:flutter/services.dart'
    show FilteringTextInputFormatter, LengthLimitingTextInputFormatter;
import 'package:material_ui/material_ui.dart';

import 'preferences.dart' show RelayPreferences;
import 'scanner.dart' show CameraUnavailable, Scanner;
import 'shell.dart' show PhoneRelay;
import 'theme.dart';

/// The wall's three pairing refusals, **by their words** (`src/control.rs`'s `REFUSALS`, `docs/control.md`).
const String noPairingOpen = 'no pairing is open';
const String notTheCode = 'that code is not the code';
const String alreadyPaired = 'that device is already paired';

/// The phone's sentence for each of them, exactly as `docs/mobile-ui.md`'s pairing table gives it. The burnt code has
/// no sentence of its own: it is [noPairingOpen], and the note says so rather than letting a fifth phrase appear.
const String codeExpiredSentence = 'the code expired';
const String wrongCodeSentence = "that code is not the wall's";
const String nameTakenSentence = 'this name is taken';

/// What the name field says before the operator types: a device name, not a secret, and the wall's row will hold it.
const String defaultDeviceName = 'operations phone';

/// The six boxes, the relay and name fields, the scan affordance and the one button, by name, so a test presses the
/// control it means.
const Key pairCodeKey = ValueKey('pair-code');
const Key pairRelayKey = ValueKey('pair-relay');
const Key pairNameKey = ValueKey('pair-name');
const Key pairScanKey = ValueKey('pair-scan');
const Key pairButtonKey = ValueKey('pair-button');
const Key pairSentenceKey = ValueKey('pair-sentence');

/// One of the six drawn boxes, by its place — the picture's own names, so a test can say *six*. The field over them is
/// [pairCodeKey]: the boxes are drawn, the field is typed into.
Key pairDigitKey(int index) => ValueKey('pair-digit-$index');

/// **Pair with a wall.** One screen, and it hands the paired relay back to the shell ([onPaired]) so the shell can read
/// and watch over the connection the claim was made on.
final class PairingScreen extends StatefulWidget {
  const PairingScreen({
    super.key,
    required this.relay,
    required this.keys,
    required this.scanner,
    required this.preferences,
    required this.openRelay,
    required this.onPaired,
  });

  /// The relay this build starts with: the remembered one or the compiled default, or `null` when neither exists.
  ///
  /// **Not printed** (`INV-08`), and not necessarily the relay used: the operator may edit the field or scan a wall.
  final Uri? relay;

  /// The store the carrier writes the key to, once (`RelayCarrier.claim`). The screen never sees the key.
  final KeyStore keys;

  /// How a camera is opened. `main` passes the plugin; a test passes a fake and no camera is constructed.
  final Scanner scanner;

  /// Where the relay field's value is remembered. A URL is a setting, not a secret, so it is not in [keys].
  final RelayPreferences preferences;

  /// How a carrier is built. `main` passes the relay's socket; a test passes a fake.
  final PhoneRelay Function(Uri relay, KeyStore keys) openRelay;

  /// The claim landed: this relay holds the key, and the shell takes it from here.
  final void Function(PhoneRelay relay) onPaired;

  @override
  State<PairingScreen> createState() => _PairingScreenState();
}

final class _PairingScreenState extends State<PairingScreen> {
  final TextEditingController _code = TextEditingController();
  final TextEditingController _relayField = TextEditingController();
  final TextEditingController _name = TextEditingController(
    text: defaultDeviceName,
  );
  final FocusNode _codeFocus = FocusNode();
  final FocusNode _relayFocus = FocusNode();
  final FocusNode _nameFocus = FocusNode();

  /// The carrier this screen claims over, and the relay it was built for. Built on the first press, so a phone that
  /// never pairs never dials; rebuilt if the operator changes the relay before a claim lands.
  PhoneRelay? _carrier;
  Uri? _carrierRelay;

  /// A claim is out: one request at a time, and a second press is not a second claim.
  bool _pairing = false;

  /// A camera is open: the scan button is not a second camera.
  bool _scanning = false;

  /// The phone's sentence under the fields, or `null` before a refusal.
  String? _sentence;

  /// The relay was handed to the shell, which owns it now; this screen must not close it.
  bool _handedOver = false;

  /// The last **dialable** value written to preferences, so a selection change on the same text is not a second write.
  String? _rememberedRelay;

  @override
  void initState() {
    super.initState();
    _relayField.text = widget.relay?.toString() ?? '';
    _relayField.addListener(_rememberValidRelay);
  }

  @override
  void dispose() {
    _relayField.removeListener(_rememberValidRelay);
    _code.dispose();
    _relayField.dispose();
    _name.dispose();
    _codeFocus.dispose();
    _relayFocus.dispose();
    _nameFocus.dispose();
    if (!_handedOver) {
      // A carrier the claim never used, or one a refusal left open: letting go is the screen's, because the shell
      // never received it.
      unawaited(_carrier?.close() ?? Future<void>.value());
    }
    super.dispose();
  }

  /// Keep a **dialable** relay in preferences as it is typed or scanned. A half-typed URL is not remembered: on the
  /// next run that would be a field pre-filled with something this build cannot dial.
  void _rememberValidRelay() {
    final relay = _relayField.text;
    if (relay == _rememberedRelay || parseDialableRelay(relay) == null) {
      return;
    }
    _rememberedRelay = relay;
    unawaited(widget.preferences.rememberRelay(relay));
  }

  bool get _busy => _pairing || _scanning;

  bool get _ready =>
      !_busy &&
      _code.text.length == 6 &&
      _name.text.trim().isNotEmpty &&
      parseDialableRelay(_relayField.text) != null;

  /// **Read one QR, and fill what it names.** A scan wins over the field: it writes both the code and the relay. A
  /// refused payload writes **nothing**, and a refused camera leaves the typed path exactly as it was.
  Future<void> _scan() async {
    if (_busy) {
      return;
    }
    setState(() {
      _scanning = true;
      _sentence = null;
    });
    try {
      final payload = await widget.scanner.scan(context);
      if (!mounted || payload == null) {
        return;
      }
      final result = parsePairingUri(payload);
      switch (result) {
        case Pairing(:final relay, :final code):
          _dropCarrier();
          // Setting the field runs `_rememberValidRelay`: the scanned relay is remembered with the code it came with.
          _relayField.text = relay.toString();
          _code.text = code;
          if (mounted) {
            setState(() {
              _sentence = null;
            });
          }
        case PairingRefused(:final refusal):
          if (mounted) {
            setState(() {
              _sentence = _payloadSentence(refusal, result);
            });
          }
      }
    } on CameraUnavailable catch (camera) {
      if (mounted) {
        setState(() {
          _sentence = camera.refusal.sentence;
        });
      }
    } finally {
      if (mounted) {
        setState(() {
          _scanning = false;
        });
      }
    }
  }

  /// The refusal's **name** and the phone's words about it. Neither is the payload: a QR that named a relay has not
  /// earned a place in a sentence, and the six digits are the code (`INV-08`).
  String _payloadSentence(PairingRefusal refusal, PairingRefused result) =>
      'Not a pairing QR (${result.name}): ${refusal.sentence}.';

  Future<void> _pair() async {
    if (!_ready) {
      return;
    }
    final code = _code.text;
    final name = _name.text.trim();
    final relayUri = parseDialableRelay(_relayField.text);
    if (relayUri == null) {
      return;
    }
    final relay = _carrierFor(relayUri);
    setState(() {
      _pairing = true;
      _sentence = null;
    });
    try {
      await relay.claim(code: code, name: name);
      if (!mounted) {
        return;
      }
      // **The key is in the store and nowhere on this screen** (`INV-08`): `claim` writes it once, and the shell
      // draws the overview from here.
      _handedOver = true;
      widget.onPaired(relay);
    } on Refusal catch (refusal) {
      if (!mounted) {
        return;
      }
      setState(() {
        _sentence = _sentenceFor(refusal);
      });
      _stepFor(refusal);
    } on Denied {
      if (!mounted) {
        return;
      }
      // No socket, or a claim the wall never answered: the note's busy sentence, because the change may not have
      // happened and the retry is another press.
      setState(() {
        _sentence = 'The wall is not answering.';
      });
    } finally {
      if (mounted) {
        setState(() {
          _pairing = false;
        });
      }
    }
  }

  /// The carrier for the relay in the field, closing one built for a different address: a claim goes to the address
  /// the operator can see, never to the one the field held a moment ago.
  PhoneRelay _carrierFor(Uri relay) {
    if (_carrierRelay != relay) {
      _dropCarrier();
      _carrierRelay = relay;
    }
    return _carrier ??= widget.openRelay(relay, widget.keys);
  }

  void _dropCarrier() {
    unawaited(_carrier?.close() ?? Future<void>.value());
    _carrier = null;
    _carrierRelay = null;
  }

  /// The phone's sentence for a refusal, or **the wall's words verbatim** for one this screen has no row for.
  String _sentenceFor(Refusal refusal) => switch (refusal.why) {
    noPairingOpen => codeExpiredSentence,
    notTheCode => wrongCodeSentence,
    alreadyPaired => nameTakenSentence,
    _ => refusal.why,
  };

  /// **The refusal's step**, exactly as `docs/mobile-ui.md` gives it. The three rows are a closed set of the wall's
  /// words; anything else gets **no step of this screen's own**.
  void _stepFor(Refusal refusal) {
    switch (refusal.why) {
      case noPairingOpen:
        // Nothing began, the two minutes ran out, the code was spent, or five wrong guesses burnt it: the field is
        // emptied and the wall has to be asked again (`pair.begin`). One step, and no sentence for a burnt code.
        _code.clear();
        _codeFocus.requestFocus();
      case notTheCode:
        // The code stays, and its digits are selected: the operator corrects one digit, not six.
        _code.selection = TextSelection(
          baseOffset: 0,
          extentOffset: _code.text.length,
        );
        _codeFocus.requestFocus();
      case alreadyPaired:
        // A second phone is a second name: the code was right, and the name is what has to change.
        _nameFocus.requestFocus();
      default:
        // The wall's words are the sentence, and the screen does nothing it was not told to do.
        break;
    }
  }

  @override
  Widget build(BuildContext context) {
    final sentence = _sentence;
    return ColoredBox(
      color: PhoneTokens.canvas,
      child: SafeArea(
        child: Padding(
          padding: PhoneTokens.screen,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              const SizedBox(height: PhoneTokens.gap * 2),
              const Text(
                'Pair with a wall',
                style: TextStyle(
                  color: PhoneTokens.title,
                  fontSize: PhoneTokens.screenTitleSize,
                ),
              ),
              const SizedBox(height: PhoneTokens.gap * 2),
              _CodeField(
                controller: _code,
                focus: _codeFocus,
                onChanged: (_) => setState(() {}),
              ),
              const SizedBox(height: PhoneTokens.gap),
              _ScanButton(enabled: !_busy, onTap: _scan),
              const SizedBox(height: PhoneTokens.gap * 2),
              Row(
                children: [
                  const Text(
                    'Relay',
                    style: TextStyle(
                      color: PhoneTokens.kind,
                      fontSize: PhoneTokens.screenLabelSize,
                    ),
                  ),
                  const SizedBox(width: PhoneTokens.gap),
                  Expanded(
                    child: SizedBox(
                      height: PhoneTokens.fieldHeight,
                      child: TextField(
                        key: pairRelayKey,
                        controller: _relayField,
                        focusNode: _relayFocus,
                        keyboardType: TextInputType.url,
                        textInputAction: TextInputAction.next,
                        style: const TextStyle(
                          color: PhoneTokens.title,
                          fontSize: PhoneTokens.buttonTextSize,
                        ),
                        decoration: const InputDecoration(
                          filled: true,
                          fillColor: PhoneTokens.cardFill,
                          contentPadding: PhoneTokens.fieldPadding,
                          border: OutlineInputBorder(
                            borderRadius: BorderRadius.all(PhoneTokens.corner),
                            borderSide: BorderSide(color: PhoneTokens.cardEdge),
                          ),
                          enabledBorder: OutlineInputBorder(
                            borderRadius: BorderRadius.all(PhoneTokens.corner),
                            borderSide: BorderSide(color: PhoneTokens.cardEdge),
                          ),
                          focusedBorder: OutlineInputBorder(
                            borderRadius: BorderRadius.all(PhoneTokens.corner),
                            borderSide: BorderSide(
                              color: PhoneTokens.ring,
                              width: PhoneTokens.ringWidth,
                            ),
                          ),
                        ),
                        onChanged: (_) => setState(() {}),
                        onSubmitted: (_) => _nameFocus.requestFocus(),
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: PhoneTokens.gap),
              Row(
                children: [
                  const Text(
                    'Name this phone',
                    style: TextStyle(
                      color: PhoneTokens.kind,
                      fontSize: PhoneTokens.screenLabelSize,
                    ),
                  ),
                  const SizedBox(width: PhoneTokens.gap),
                  Expanded(
                    child: SizedBox(
                      height: PhoneTokens.fieldHeight,
                      child: TextField(
                        key: pairNameKey,
                        controller: _name,
                        focusNode: _nameFocus,
                        textInputAction: TextInputAction.done,
                        style: const TextStyle(
                          color: PhoneTokens.title,
                          fontSize: PhoneTokens.buttonTextSize,
                        ),
                        decoration: const InputDecoration(
                          filled: true,
                          fillColor: PhoneTokens.cardFill,
                          contentPadding: PhoneTokens.fieldPadding,
                          border: OutlineInputBorder(
                            borderRadius: BorderRadius.all(PhoneTokens.corner),
                            borderSide: BorderSide(color: PhoneTokens.cardEdge),
                          ),
                          enabledBorder: OutlineInputBorder(
                            borderRadius: BorderRadius.all(PhoneTokens.corner),
                            borderSide: BorderSide(color: PhoneTokens.cardEdge),
                          ),
                          focusedBorder: OutlineInputBorder(
                            borderRadius: BorderRadius.all(PhoneTokens.corner),
                            borderSide: BorderSide(
                              color: PhoneTokens.ring,
                              width: PhoneTokens.ringWidth,
                            ),
                          ),
                        ),
                        onChanged: (_) => setState(() {}),
                        onSubmitted: (_) => _pair(),
                      ),
                    ),
                  ),
                ],
              ),
              const SizedBox(height: PhoneTokens.gap),
              if (sentence case final String sentence)
                Text(
                  sentence,
                  key: pairSentenceKey,
                  style: const TextStyle(
                    color: PhoneTokens.warning,
                    fontSize: PhoneTokens.screenSentenceSize,
                  ),
                ),
              const SizedBox(height: PhoneTokens.gap * 2),
              _PairButton(enabled: _ready, onTap: _pair),
            ],
          ),
        ),
      ),
    );
  }
}

/// **The scan affordance**: one press, a camera, and a word beside the glyph (#111).
final class _ScanButton extends StatelessWidget {
  const _ScanButton({required this.enabled, required this.onTap});

  final bool enabled;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      enabled: enabled,
      label: "Scan the wall's QR",
      excludeSemantics: true,
      child: GestureDetector(
        key: pairScanKey,
        onTap: enabled ? onTap : null,
        behavior: HitTestBehavior.opaque,
        child: Container(
          height: PhoneTokens.fieldHeight,
          alignment: Alignment.center,
          decoration: const BoxDecoration(
            color: PhoneTokens.cardFill,
            borderRadius: BorderRadius.all(PhoneTokens.corner),
            border: Border.fromBorderSide(
              BorderSide(color: PhoneTokens.cardEdge),
            ),
          ),
          child: Row(
            mainAxisSize: MainAxisSize.min,
            children: [
              Icon(
                Icons.qr_code_scanner,
                color: enabled ? PhoneTokens.ring : PhoneTokens.kind,
                size: PhoneTokens.glyphSize,
              ),
              const SizedBox(width: PhoneTokens.gap),
              Text(
                "Scan the wall's QR",
                style: TextStyle(
                  color: enabled ? PhoneTokens.title : PhoneTokens.kind,
                  fontSize: PhoneTokens.buttonTextSize,
                ),
              ),
            ],
          ),
        ),
      ),
    );
  }
}

/// **The six digit boxes**, and the one field that holds the code.
///
/// The boxes are drawn from the field's own value, so the picture and the value are one thing. The field is over the
/// row with transparent text: it is a real field — a real keyboard, a real selection, which is what makes "select the
/// digits" a step this screen can take — and the glyphs are the boxes'. A code is one value, not six widgets, and a
/// refusal's step says *the code field* in the singular.
final class _CodeField extends StatelessWidget {
  const _CodeField({
    required this.controller,
    required this.focus,
    required this.onChanged,
  });

  final TextEditingController controller;
  final FocusNode focus;

  /// A digit was typed or removed: the parent redraws the button's state with it, which is what makes `Pair` a press
  /// only when the code is six digits.
  final ValueChanged<String> onChanged;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: PhoneTokens.codeBoxHeight,
      child: Stack(
        children: [
          Positioned.fill(
            child: ValueListenableBuilder<TextEditingValue>(
              valueListenable: controller,
              builder: (context, value, child) {
                final digits = value.text;
                return Row(
                  children: [
                    for (var index = 0; index < 6; index++) ...[
                      if (index == 3)
                        const SizedBox(width: PhoneTokens.codeBoxGroupGap)
                      else if (index > 0)
                        const SizedBox(width: PhoneTokens.codeBoxGap),
                      _DigitBox(
                        key: pairDigitKey(index),
                        digit: index < digits.length ? digits[index] : '',
                      ),
                    ],
                  ],
                );
              },
            ),
          ),
          Positioned.fill(
            child: Semantics(
              label: 'the six digits on the wall',
              textField: true,
              child: TextField(
                key: pairCodeKey,
                controller: controller,
                focusNode: focus,
                keyboardType: TextInputType.number,
                textInputAction: TextInputAction.done,
                inputFormatters: [
                  FilteringTextInputFormatter.digitsOnly,
                  LengthLimitingTextInputFormatter(6),
                ],
                // **The boxes draw the digits; the field is the control.** Its own text is transparent so a digit is
                // drawn once, and the caret is not a second cursor in the middle of a box.
                showCursor: false,
                cursorColor: PhoneTokens.transparent,
                style: const TextStyle(
                  color: PhoneTokens.transparent,
                  fontSize: PhoneTokens.codeDigitSize,
                ),
                textAlign: TextAlign.center,
                decoration: const InputDecoration.collapsed(hintText: ''),
                onChanged: onChanged,
                onSubmitted: (_) {},
              ),
            ),
          ),
        ],
      ),
    );
  }
}

/// One drawn digit box. Empty is an empty box, which is what `connecting` is to a diagram: nothing is not a value.
final class _DigitBox extends StatelessWidget {
  const _DigitBox({super.key, required this.digit});

  final String digit;

  @override
  Widget build(BuildContext context) {
    return Container(
      width: PhoneTokens.codeBoxWidth,
      height: PhoneTokens.codeBoxHeight,
      alignment: Alignment.center,
      decoration: const BoxDecoration(
        color: PhoneTokens.cardFill,
        borderRadius: BorderRadius.all(PhoneTokens.corner),
        border: Border.fromBorderSide(BorderSide(color: PhoneTokens.cardEdge)),
      ),
      child: Text(
        digit,
        style: const TextStyle(
          color: PhoneTokens.title,
          fontSize: PhoneTokens.codeDigitSize,
        ),
      ),
    );
  }
}

/// The one button. A press sends the claim; there is no second button and no retry of its own.
final class _PairButton extends StatelessWidget {
  const _PairButton({required this.enabled, required this.onTap});

  final bool enabled;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      enabled: enabled,
      label: 'Pair',
      excludeSemantics: true,
      child: GestureDetector(
        key: pairButtonKey,
        onTap: enabled ? onTap : null,
        behavior: HitTestBehavior.opaque,
        child: Container(
          height: PhoneTokens.buttonHeight,
          alignment: Alignment.center,
          decoration: const BoxDecoration(
            color: PhoneTokens.chipActive,
            borderRadius: BorderRadius.all(PhoneTokens.corner),
          ),
          child: Text(
            'Pair',
            style: TextStyle(
              color: enabled ? PhoneTokens.title : PhoneTokens.kind,
              fontSize: PhoneTokens.buttonTextSize,
            ),
          ),
        ),
      ),
    );
  }
}

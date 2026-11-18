/// **The pairing screen** — six digits, a name, and one button (`docs/mobile-ui.md`, *pairing*; #194, #262, #264).
///
/// It is **the only screen before the overview**: a phone with no device key has nothing to read, and the code is a
/// picture on the wall in the room rather than a message, so the phone does not ask for one — it is typed here.
///
/// Three refusals, three **steps**, and the steps are the note's:
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
/// **Nothing secret is printed.** The six digits live in one [TextEditingController] and are cleared when they are
/// spent; the key `pair.claim` hands back goes to the [KeyStore] inside the carrier and is never a widget's text, a log
/// line or a `toString` (`INV-08`).
library;

import 'package:arcons_phone/client.dart' show Denied, Refusal;
import 'package:arcons_phone/relay.dart' show KeyStore;
import 'package:flutter/services.dart'
    show FilteringTextInputFormatter, LengthLimitingTextInputFormatter;
import 'package:material_ui/material_ui.dart';

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

/// The six boxes, the name field and the one button, by name, so a test presses the control it means.
const Key pairCodeKey = ValueKey('pair-code');
const Key pairNameKey = ValueKey('pair-name');
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
    required this.openRelay,
    required this.onPaired,
  });

  /// The relay this build dials — where the claim goes. **Not printed** (`INV-08`).
  final Uri relay;

  /// The store the carrier writes the key to, once (`RelayCarrier.claim`). The screen never sees the key.
  final KeyStore keys;

  /// How a carrier is built. `main` passes the relay's socket; a test passes a fake.
  final PhoneRelay Function(Uri relay, KeyStore keys) openRelay;

  /// The claim landed: this relay holds the key, and the shell takes it from here.
  final void Function(PhoneRelay relay) onPaired;

  @override
  State<PairingScreen> createState() => _PairingScreenState();
}

final class _PairingScreenState extends State<PairingScreen> {
  final TextEditingController _code = TextEditingController();
  final TextEditingController _name = TextEditingController(text: defaultDeviceName);
  final FocusNode _codeFocus = FocusNode();
  final FocusNode _nameFocus = FocusNode();

  /// The carrier this screen claims over. Built on the first press, so a phone that never pairs never dials.
  PhoneRelay? _relay;

  /// A claim is out: one request at a time, and a second press is not a second claim.
  bool _pairing = false;

  /// The phone's sentence under the fields, or `null` before a refusal.
  String? _sentence;

  /// The relay was handed to the shell, which owns it now; this screen must not close it.
  bool _handedOver = false;

  @override
  void dispose() {
    _code.dispose();
    _name.dispose();
    _codeFocus.dispose();
    _nameFocus.dispose();
    if (!_handedOver) {
      // A carrier the claim never used, or one a refusal left open: letting go is the screen's, because the shell
      // never received it.
      _relay?.close();
    }
    super.dispose();
  }

  bool get _ready =>
      !_pairing && _code.text.length == 6 && _name.text.trim().isNotEmpty;

  Future<void> _pair() async {
    if (!_ready) {
      return;
    }
    final code = _code.text;
    final name = _name.text.trim();
    final relay = _relay ??= widget.openRelay(widget.relay, widget.keys);
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
              const SizedBox(height: PhoneTokens.gap * 2),
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
                style: const TextStyle(color: PhoneTokens.transparent, fontSize: PhoneTokens.codeDigitSize),
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

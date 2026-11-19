/// **The wall's pairing QR, as a value** (#271): `arcons://pair?relay=…&code=…`.
///
/// The wall draws this payload beside its six digits while a code is open (#270), and a phone reads it with a camera.
/// The payload is not a wire frame and it is not JSON: it is one URI, and this file is the **whole** of the phone's
/// understanding of it. The parser lives in the package with no UI toolkit so it is proved by `dart test`, beside the
/// other wire parsers, rather than by a widget test that needs a camera.
///
/// ```text
/// arcons://pair?relay=<percent-encoded relay URL>&code=481902
/// ```
///
/// There are **six ways** a payload is refused, and each has a name. The list is closed: [PairingRefusal] has no
/// `other` member and [parsePairingUri] has no catch-all branch (`INV-07`), so a seventh reason is a compile-visible
/// decision here rather than a silent `null` on the screen.
///
/// | the payload | the refusal |
/// | --- | --- |
/// | not an `arcons://` URI | [PairingRefusal.notAnArconsUri] |
/// | `pair` is not the host | [PairingRefusal.pairIsNotTheHost] |
/// | no `relay`, or an empty one | [PairingRefusal.noRelay] |
/// | no `code`, or an empty one | [PairingRefusal.noCode] |
/// | a code that is not exactly six digits | [PairingRefusal.codeIsNotSixDigits] |
/// | a relay this build cannot dial | [PairingRefusal.relayCannotBeDialled] |
///
/// **The payload is never printed.** Its `relay` names somebody's deployment, and the six digits are the code
/// (`INV-08`): [Pairing.toString] says what the value is and not what it holds, and a refusal's sentence names the
/// reason rather than quoting the payload that failed.
library;

/// **What a pairing QR parsed to**: a pairing, or a refusal with a name.
sealed class PairingResult {
  const PairingResult();
}

/// **A pairing a phone can make**: the relay to dial and the six digits to send.
final class Pairing extends PairingResult {
  const Pairing({required this.relay, required this.code});

  /// The relay the wall named: `tcp://host:port` or `tls://host:port`, and nothing else.
  final Uri relay;

  /// The six digits, exactly as the wall drew them. Not a secret that may be printed (`INV-08`).
  final String code;

  /// **Nothing in a pairing is printed** (`INV-08`): the relay is a deployment's address and the code is the code.
  @override
  String toString() => 'a pairing: a relay and six digits (neither is printed)';
}

/// **A pairing QR this build refuses**, with one of the six names and no seventh.
final class PairingRefused extends PairingResult {
  const PairingRefused(this.refusal);

  final PairingRefusal refusal;

  /// The refusal's name, for a test or a sentence. It is a word about the payload's shape, never the payload.
  String get name => refusal.name;

  @override
  String toString() => 'a pairing QR refused: ${refusal.name}';
}

/// **The six reasons a payload is not a pairing.** Each is a name the screen can say and a test can assert.
enum PairingRefusal {
  /// The scheme is not `arcons`, including a string that is not a URI at all.
  notAnArconsUri,

  /// The scheme is right and the host is not `pair`.
  pairIsNotTheHost,

  /// There is no `relay` query value, or it is empty.
  noRelay,

  /// There is no `code` query value, or it is empty.
  noCode,

  /// The `code` is not exactly six digits. Letters, five digits and seven digits are all this one reason.
  codeIsNotSixDigits,

  /// The `relay` is not a relay this build dials: not `tcp://` or `tls://`, or without a host, or without a port.
  relayCannotBeDialled;

  /// The words the pairing screen may say about this refusal. **No payload is quoted** (`INV-08`).
  String get sentence => switch (this) {
    PairingRefusal.notAnArconsUri => 'that is not a pairing QR',
    PairingRefusal.pairIsNotTheHost => 'that QR is not for pairing',
    PairingRefusal.noRelay => 'the QR names no relay',
    PairingRefusal.noCode => 'the QR carries no code',
    PairingRefusal.codeIsNotSixDigits => 'the QR does not carry six digits',
    PairingRefusal.relayCannotBeDialled =>
      'the QR names a relay this build cannot dial',
  };
}

/// **Parse one payload a camera read** (#271). The only entry point a screen needs.
///
/// Percent-encoding is the platform standard's, not a private spelling: `Uri.parse` decodes `relay`'s `%3A` and `%2F`
/// back to `:` and `/`, which is what `src/control/pair.rs` writes (#270). The relay's own string is then held to
/// [parseDialableRelay], the same rule the app's compiled relay goes through.
PairingResult parsePairingUri(String payload) {
  final Uri uri;
  try {
    uri = Uri.parse(payload);
  } on FormatException {
    return const PairingRefused(PairingRefusal.notAnArconsUri);
  }
  if (uri.scheme != 'arcons') {
    return const PairingRefused(PairingRefusal.notAnArconsUri);
  }
  if (uri.host != 'pair') {
    return const PairingRefused(PairingRefusal.pairIsNotTheHost);
  }

  final relayValue = uri.queryParameters['relay'];
  if (relayValue == null || relayValue.isEmpty) {
    return const PairingRefused(PairingRefusal.noRelay);
  }
  final code = uri.queryParameters['code'];
  if (code == null || code.isEmpty) {
    return const PairingRefused(PairingRefusal.noCode);
  }
  if (!RegExp(r'^[0-9]{6}$').hasMatch(code)) {
    return const PairingRefused(PairingRefusal.codeIsNotSixDigits);
  }

  final relay = parseDialableRelay(relayValue);
  if (relay == null) {
    return const PairingRefused(PairingRefusal.relayCannotBeDialled);
  }
  return Pairing(relay: relay, code: code);
}

/// **The relay this build dials**: `tcp://host:port` or `tls://host:port`, and `null` for anything else.
///
/// The rule was `shell.dart`'s `parseRelay` (#264); it lives here now so the compiled default, the relay field and a
/// scanned payload cannot disagree about what a relay is. Empty, unparseable, a scheme this build cannot dial, no host
/// or no port are all the same answer: **not a relay**, and never a printed value (`INV-08`).
Uri? parseDialableRelay(String configured) {
  if (configured.isEmpty) {
    return null;
  }
  final Uri relay;
  try {
    relay = Uri.parse(configured);
  } on FormatException {
    return null;
  }
  if (relay.host.isEmpty || relay.port == 0) {
    return null;
  }
  if (relay.scheme == 'tcp' || relay.scheme == 'tls') {
    return relay;
  }
  return null;
}

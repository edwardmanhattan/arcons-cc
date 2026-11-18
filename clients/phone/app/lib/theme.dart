/// **The phone's own tokens** — the colours and sizes this screen names, in one file.
///
/// The wall's rule, one program over: nothing outside this file names a colour or a size (`AGENTS.md`,
/// `src/ui/tokens.rs`). They are the phone's and not the wall's, because a phone in a dark room and a wall on a
/// projector are two different surfaces — and copying the wall's numbers would make one of them wrong.
library;

import 'package:flutter/painting.dart';

abstract final class PhoneTokens {
  /// The screen behind everything: the wall's panel, one shade darker than a card.
  static const Color canvas = Color(0xFF0B1016);

  /// A card's fill, and its edge when it is not the one with the keys.
  static const Color cardFill = Color(0xFF121A23);
  static const Color cardEdge = Color(0xFF22303D);

  /// The ring: the pane that has the keys. It is drawn *and* spoken, never colour alone (#111).
  static const Color ring = Color(0xFF4BB3E6);

  /// Text: a title, a kind set quieter, and the state line honest about what the phone knows.
  static const Color title = Color(0xFFE6EDF3);
  static const Color kind = Color(0xFF8A9BA8);
  static const Color state = Color(0xFF6E7F8C);
  static const Color warning = Color(0xFFE6A23C);

  /// A chip: the active one carries the ring colour, an empty one is still a place (#141).
  static const Color chipActive = Color(0xFF1B2733);

  /// One card's corner, its padding, and the ring's width.
  static const Radius corner = Radius.circular(10);
  static const double padding = 10;
  static const double ringWidth = 2;

  /// The bands: a chip row, a card's title, the kind under it, and the state line.
  static const double chipHeight = 28;
  static const double stateHeight = 26;
  static const double gap = 8;

  static const EdgeInsets screen = EdgeInsets.symmetric(
    horizontal: 12,
    vertical: 8,
  );
  static const EdgeInsets cardPadding = EdgeInsets.all(padding);
}

/// **The two intents the canvas sends, as the wall spells them** (#257).
///
/// `pane.drop` and `pane.resize` are `docs/control.md`'s, and this file is the only place the phone writes their fields
/// — which is the whole of the claim the hand and the list have to keep: **one release, one request line**, whichever
/// of the two produced it. A second encoder somewhere else is how a voice user and a hand user land in different
/// places (`docs/mobile-ui.md`, *accessibility*).
///
/// The values are the wall's and the words are the phone's: [Edge] is `left`, `right`, `top`, `bottom`, `centre` on the
/// wire, and the list's rows say *left half of "Feed"*.
library;

import 'package:arcons_phone/cells.dart';

/// The five slots a `pane.drop` names, **in the wall's order of edges** (`src/control.rs`'s `EDGES`):
/// `left`, `right`, `top`, `bottom`, `centre`.
///
/// An enum rather than a `String`, because the wire's five values are a closed set and `INV-07` is the same rule one
/// language over: a word this build does not know is an error at the wall (`input`), so the phone does not invent one.
enum Edge {
  left('left'),
  right('right'),
  top('top'),
  bottom('bottom'),
  centre('centre');

  const Edge(this.word);

  /// The word on the wire and in `docs/control.md`.
  final String word;

  /// The edge a `Side` names — `Edge::of_side`. Side has four values, so this switch has four arms and no default.
  static Edge of(Side side) => switch (side) {
        Side.left => Edge.left,
        Side.right => Edge.right,
        Side.top => Edge.top,
        Side.bottom => Edge.bottom,
      };
}

/// **One `pane.drop`**: `pane` lands in `onto`'s [edge], and `centre` is a swap — one verb, because a swap is a slot
/// like any other (`docs/control.md`).
///
/// The hand builds one of these from `dropFor`'s answer ([DropIntent.of]); the **Move to…** list builds one from the row
/// that was pressed. Both then take the same field writer ([fields]) to the same door, so the two cannot disagree.
final class DropIntent {
  const DropIntent({required this.pane, required this.onto, required this.edge});

  /// The verb, exactly as `docs/control.md` spells it.
  static const String verb = 'pane.drop';

  /// The pane that moves.
  final int pane;

  /// The pane whose slot it lands in.
  final int onto;

  final Edge edge;

  /// **The hand's drop, as a request.** `dropFor` answers with a [Retile] for an edge and a [Swap] for the middle, and
  /// a swap's `b` is the dragged pane — the pair the wall's own `Edge::drop_on` builds.
  factory DropIntent.of(int dragged, Drop drop) => switch (drop) {
        Retile(:final target, :final side) =>
          DropIntent(pane: dragged, onto: target, edge: Edge.of(side)),
        Swap(:final a) => DropIntent(pane: dragged, onto: a, edge: Edge.centre),
      };

  /// **The drop this row is** — `Edge::drop_on`, so the list's preview is the hand's landing and the wall's `apply`.
  Drop get drop => switch (edge) {
        Edge.left => Retile(onto, Side.left),
        Edge.right => Retile(onto, Side.right),
        Edge.top => Retile(onto, Side.top),
        Edge.bottom => Retile(onto, Side.bottom),
        // The same pair `dropFor` builds when the pointer is over the middle of the target: the dropped pane is `b`.
        Edge.centre => Swap(onto, pane),
      };

  /// The verb's fields, in the wire's spelling.
  Map<String, Object?> get fields => {'pane': pane, 'onto': onto, 'edge': edge.word};

  @override
  bool operator ==(Object other) =>
      other is DropIntent && other.pane == pane && other.onto == onto && other.edge == edge;

  @override
  int get hashCode => Object.hash(pane, onto, edge);

  @override
  String toString() => 'DropIntent($pane onto $onto, ${edge.word})';
}

/// **One `pane.resize`**: the share the first side of the border between [pane] and [other] takes.
///
/// The pair names the border in either order (`docs/control.md`), because an id names one pane and a pane has a border
/// on each of its sides. [ratio] is always a share the wall accepts: it comes from [Numbers.shareAlong] or
/// [Numbers.clampShare], never from a widget's arithmetic.
final class ResizeIntent {
  const ResizeIntent({required this.pane, required this.other, required this.ratio});

  /// The verb, exactly as `docs/control.md` spells it.
  static const String verb = 'pane.resize';

  final int pane;
  final int other;
  final double ratio;

  /// The verb's fields, in the wire's spelling.
  Map<String, Object?> get fields => {'pane': pane, 'other': other, 'ratio': ratio};

  @override
  bool operator ==(Object other) =>
      other is ResizeIntent && other.pane == pane && other.other == this.other && other.ratio == ratio;

  @override
  int get hashCode => Object.hash(pane, other, ratio);

  @override
  String toString() => 'ResizeIntent($pane | $other at $ratio)';
}

/// **The tiling, laid into a rectangle** (#249): `view::Layout::cells`, ported and proved.
///
/// `docs/mobile-ui.md`'s canvas draws the wall's arrangement into **the phone's own rectangle**. A `ratio` is `a`'s
/// share of its cell, so the same tree is the same shape at 360 pt and in a 1280 pt panel and nothing is scaled from
/// the wall's pixels. The arithmetic is `Layout::cells` and `view.rs`'s rect walk, and it is already a contract: #220
/// recorded nine cases in `fixtures/control/tiling/` precisely so a client in another language could be held to it.
/// `test/cells_test.dart` is that holding: every recorded case, every recorded rectangle, every recorded release.
///
/// **One walk, and everything that asks where a pane is asks it here.** A leaf's cell is what a finger is
/// hit-tested against ([paneAt], [dropFor]) and the drawn card is that cell pulled in by half the gap ([rects]) — so
/// the picture and the gesture cannot disagree about which pane was pressed. The wall keeps this discipline for the
/// same reason: `Layout::rects` is a second name for `Layout::cells`, and `view::panes_on` is the one place the gap
/// becomes a look.
///
/// **The numbers are the wall's, cited rather than guessed.** [Numbers.wall] is `src/ui/tokens.rs`'s `MIN_PANE_PT`,
/// `PANE_EDGE_BAND` and `GAP` and `src/config/layout.rs`'s `RATIO_RANGE`, and a test holds it to the numbers every
/// recorded case carries — so a token that moves is a red test here, which is the moment to ask whether a client in
/// another language is still right. This file also cannot *derive* them: a Dart package does not read a Rust crate's
/// constants, so the citation is the proof and the recording is the check.
///
/// **What is not here.** The canvas, the gesture, the menu (#225's screen), and a camera: this is arithmetic, and it
/// is the reason the screen can be written in a week rather than a month. The gap drawn between two cards is the
/// phone's own look (`docs/mobile-ui.md`, *the cells are not the cards*); [rects] is the wall's, at the wall's gap,
/// because that is the rectangle the recording holds.
library;

import 'dart:typed_data';

import 'wire.dart';

/// **The wall's rectangles are `f32`** (`view::Rect`), and so are this module's: a value here is a number the wall
/// could have produced.
///
/// It is not pedantry. `side-by-side-dragged.json` records a 58% split of 1280 pt as `742.39996`, because `0.58` is
/// not a binary fraction and the wall multiplies it in `f32`. The same product in a `double` is `742.4`, which draws
/// the same picture and is a different number — and the recording is a contract of numbers, so a phone that computed
/// in `double` would be right about the shape and wrong about the file. Every value that leaves this file is rounded
/// once, by [CellRect] or here, and the test compares two lists with `==` instead of a tolerance nobody can see the
/// width of. A sum or a difference of two float32 values is exact in a `double`, so one rounding here is the one
/// rounding the wall's `f32` does there.
double _f32(double value) {
  _scratch[0] = value;
  return _scratch[0];
}

/// One float32, reused: assigning a `double` to a `Float32List` rounds it to the nearest float32, which is the whole
/// implementation of [_f32]. One list rather than one allocation per coordinate.
final Float32List _scratch = Float32List(1);

/// The larger of `value` and zero — `f32::max(0.0)` as `Layout::cells`'s viewport, `split_rects`'s second side and
/// `panes_on`'s half-gap use it, NaN included (which reads as zero, as it does there).
double _atLeastZero(double value) => value > 0.0 ? value : 0.0;

/// The larger of `value` and one — `view::inset_rect`'s floor, so no drawn card has an area of nothing.
double _atLeastOne(double value) => value > 1.0 ? value : 1.0;

/// **A rectangle in the caller's points** — the wall's `view::Rect`, rounded to the float32 the wall's is (see
/// [_f32]).
///
/// Not named `Rect` on purpose: the canvas is Flutter (`ADR-0008`), `dart:ui` already has a `Rect`, and a name that
/// every import has to `hide` is a name fighting the toolkit.
final class CellRect {
  /// A rectangle at (`x`, `y`) of `w` by `h`. Each number is rounded to float32 once, here.
  CellRect(double x, double y, double w, double h)
      : x = _f32(x),
        y = _f32(y),
        w = _f32(w),
        h = _f32(h);

  final double x;
  final double y;
  final double w;
  final double h;

  /// The right edge, `x + w` in the wall's arithmetic.
  double get right => _f32(x + w);

  /// The bottom edge, `y + h` in the wall's arithmetic.
  double get bottom => _f32(y + h);

  /// Whether a point is inside — **the edges belong to the rectangle they open**, which is `Rect::contains` and is
  /// what makes the cells a partition a press lands in exactly once.
  bool contains(double atX, double atY) =>
      atX >= x && atX < right && atY >= y && atY < bottom;

  @override
  bool operator ==(Object other) =>
      other is CellRect && other.x == x && other.y == y && other.w == w && other.h == h;

  @override
  int get hashCode => Object.hash(x, y, w, h);

  @override
  String toString() => 'CellRect($x, $y, $w, $h)';
}

/// **One pane's rectangle**: the pane's id and where the tiling put it.
///
/// The id is a pane id and not an index into `panes` (#218): a phone holds a projection across a close, and after a
/// `pane.close` an index would name a different pane — the right arrangement of the wrong panes.
final class Cell {
  Cell(this.pane, this.rect);

  final int pane;
  final CellRect rect;

  @override
  bool operator ==(Object other) =>
      other is Cell && other.pane == pane && other.rect == rect;

  @override
  int get hashCode => Object.hash(pane, rect);

  @override
  String toString() => 'Cell(pane: $pane, $rect)';
}

/// **The numbers a client must not guess** — `fixtures/control/tiling/`'s `numbers`, and `tiling::Numbers` on the
/// wall.
///
/// The recording writes them into every case because none of them is on the wire: a client that guessed the band
/// would send `centre` where the wall's own hand sends `left`. They are `f32` there and are rounded here, so
/// [Numbers.wall] equals a `Numbers` read from the file exactly.
final class Numbers {
  Numbers({
    required double minPane,
    required double edgeBand,
    required double gap,
    required double ratioMin,
    required double ratioMax,
  })  : minPane = _f32(minPane),
        edgeBand = _f32(edgeBand),
        gap = _f32(gap),
        ratioMin = _f32(ratioMin),
        ratioMax = _f32(ratioMax);

  /// **The wall's numbers**, each citing the line it comes from. The recording is the check: a token that moved is a
  /// red test in `cells_test.dart` rather than a phone drawing a share the wall refuses.
  static final Numbers wall = Numbers(
    // src/ui/tokens.rs: "the smallest a pane may be dragged to".
    minPane: 160.0,
    // src/ui/tokens.rs: `PANE_EDGE_BAND`, the share of an edge that reads as "drop against this edge".
    edgeBand: 0.3,
    // src/ui/tokens.rs: `GAP`, the space between two drawn panes.
    gap: 8.0,
    // src/config/layout.rs: `RATIO_RANGE`, the share a split may take.
    ratioMin: 0.05,
    ratioMax: 0.95,
  );

  /// The smallest a pane may be **in the rectangle the tiling is laid into** — `tokens::MIN_PANE_PT`, whose own
  /// comment says "in points".
  ///
  /// A note on the record: a phone's canvas is not the wall's panel, so this is 160 points *of the rectangle the
  /// caller passes* — which is the wall's rule applied to the wall's own viewport in every recorded case, and a
  /// decision the canvas ticket takes for a 360 pt phone (`docs/mobile-ui.md`, *the gutter*: the phone draws the
  /// share it read and lets the wall clamp). Nothing here decides that; [cells] simply applies the number it is
  /// given, as `Layout::cells` applies the token.
  final double minPane;

  /// The share of a pane's span, from an edge, that reads as "drop against this edge" — `tokens::PANE_EDGE_BAND`.
  final double edgeBand;

  /// The space between two drawn panes: [rects] pulls every cell in by half of it — `tokens::GAP`.
  final double gap;

  /// The share a split may take — `config::layout::RATIO_RANGE`.
  final double ratioMin;
  final double ratioMax;

  /// **The share the wall would hold** for [share]: `RATIO_RANGE`, rounded to the float32 the wall stores.
  ///
  /// The wall clamps in two places and this is both — `Border::ratio_for` on the way in and `Layout::set_ratio` →
  /// `set_ratio_on` on the way to the tree (`src/ui/view.rs`) — so a phone that clamps here cannot preview, or send,
  /// a share the wall refuses. A share that is not a number is not a share: this throws rather than inventing one,
  /// and [previewResize] is where the wall's own answer to that case lives.
  double clampShare(double share) {
    if (!share.isFinite) {
      throw ArgumentError.value(share, 'share', 'not a share: RATIO_RANGE is a finite range (src/config/layout.rs)');
    }
    return _f32(share.clamp(ratioMin, ratioMax));
  }

  /// **A gutter, as the hand asks for it**: the pointer at [at], as its share of a split that starts at [start] and
  /// is [span] long — `Border::ratio_for`, clamped, so a preview drawn from this can never name a refused share.
  ///
  /// A split with no span has no share to read, and the wall's answer there is **0.5** — the ratio a fresh split is
  /// made with (`replace_leaf`) — so a drag the phone cannot place along a split previews the share the wall draws
  /// for a split nobody has dragged. The same answer for a point or a span that is not a number: the recording is
  /// finite points, and a phone's own layout is finite spans, so that is a bug in the caller and not a share the
  /// operator asked for.
  double shareAlong({required double at, required double start, required double span}) {
    if (!at.isFinite || !start.isFinite || !span.isFinite || span <= 0.0) {
      return _f32(0.5);
    }
    return clampShare((at - start) / span);
  }

  @override
  bool operator ==(Object other) =>
      other is Numbers &&
      other.minPane == minPane &&
      other.edgeBand == edgeBand &&
      other.gap == gap &&
      other.ratioMin == ratioMin &&
      other.ratioMax == ratioMax;

  @override
  int get hashCode => Object.hash(minPane, edgeBand, gap, ratioMin, ratioMax);

  @override
  String toString() => 'Numbers(minPane: $minPane, edgeBand: $edgeBand, gap: $gap, '
      'ratioMin: $ratioMin, ratioMax: $ratioMax)';
}

/// **The tiling's cells** — `Layout::cells`, one per pane and covering [into] exactly.
///
/// The tree walk is `walk_cells`: `a` then `b`, at every split, which is the order the recording lists its cells in and
/// the order a reader hears the panes in. Each split's share becomes points in `split_length`: the ratio clamped to
/// `0…1`, times the cell's length along the split's axis, then clamped so neither side falls below
/// [Numbers.minPane] — unless **both** would, in which case the split halves rather than starving one. That last
/// branch is `window-too-small.json`: 300 pt cannot hold two 160 pt panes, and a client that applied the minimum
/// anyway would draw a pane the wall never draws.
///
/// [numbers] defaults to [Numbers.wall]; a test passes the recording's own numbers, so that a case's rectangles are
/// about the walk and [Numbers.wall] is about the constants.
List<Cell> cells(Tiling tiling, CellRect into, {Numbers? numbers}) {
  final wall = numbers ?? Numbers.wall;
  final out = <Cell>[];
  void walk(Tiling node, CellRect rect) {
    switch (node) {
      case TilingLeaf(:final pane):
        out.add(Cell(pane, rect));
      case TilingSplit(:final a, :final b, :final ratio, :final split):
        final (first, second) = _split(rect, ratio, split, wall);
        walk(a, first);
        walk(b, second);
    }
  }

  // `Layout::cells` starts at the panel's origin and does not read a negative size: `(viewport.w as f32).max(0.0)`.
  walk(tiling, CellRect(into.x, into.y, _atLeastZero(into.w), _atLeastZero(into.h)));
  return out;
}

/// **The rectangles the operator sees** — `view::panes_on`: the cells, pulled in by half the gap on each side, so two
/// neighbours are a gap apart and the panel's edge stays where it was.
///
/// One walk, in [cells]: the drawn card is the cell it was hit-tested in, and a phone that computed the two any other
/// way would draw a card in one rectangle and press it in another. Named `rects` because that is the recording's
/// field; the wall's `Layout::rects` is a second name for `cells`, so this module uses the recording's words.
List<Cell> rects(Tiling tiling, CellRect into, {Numbers? numbers}) {
  final wall = numbers ?? Numbers.wall;
  final half = _atLeastZero(wall.gap / 2.0);
  final laid = cells(tiling, into, numbers: wall);
  if (half == 0.0) {
    return laid;
  }
  return [
    for (final cell in laid) Cell(cell.pane, _inset(cell.rect, half)),
  ];
}

/// A rectangle pulled in by `by` on every side — `view::inset_rect`, floored at one point so a card always has a
/// face.
CellRect _inset(CellRect rect, double by) => CellRect(
      rect.x + by,
      rect.y + by,
      _atLeastOne(rect.w - by * 2.0),
      _atLeastOne(rect.h - by * 2.0),
    );

/// The two rectangles one split makes of a cell — `view::split_rects`, **the only place a ratio becomes points**.
(CellRect, CellRect) _split(CellRect rect, double ratio, Split split, Numbers numbers) {
  switch (split) {
    case Split.side:
      final aWidth = _splitLength(rect.w, ratio, numbers);
      return (
        CellRect(rect.x, rect.y, aWidth, rect.h),
        CellRect(rect.x + aWidth, rect.y, _atLeastZero(rect.w - aWidth), rect.h),
      );
    case Split.stack:
      final aHeight = _splitLength(rect.h, ratio, numbers);
      return (
        CellRect(rect.x, rect.y, rect.w, aHeight),
        CellRect(rect.x, rect.y + aHeight, rect.w, _atLeastZero(rect.h - aHeight)),
      );
  }
}

/// How much of `length` the `a` side takes — `view::split_length`, the clamp that keeps every pane drawable.
///
/// The ratio is rounded to float32 **before** it is multiplied, because that is the number the wall stores
/// (`SavedNode::ratio` is an `f32`): a phone that multiplied `0.58` as a `double` would compute `742.4` where the
/// recording holds `742.39996`.
double _splitLength(double length, double ratio, Numbers numbers) {
  if (length <= 0.0) {
    return 0.0;
  }
  final min = numbers.minPane;
  final wanted = _f32(_f32(ratio).clamp(0.0, 1.0) * length);
  if (length <= min * 2.0) {
    // Too small for two panes: halve it rather than give one an empty rectangle.
    return _f32(length / 2.0);
  }
  return _f32(wanted.clamp(min, length - min));
}

/// The pane under a point, if any — `Layout::pane_at`, **against the cells** and never against the drawn cards.
///
/// A press in the gutter between two cards still belongs to a pane's cell: that is the wall's rule, and the reason
/// [cells] and [rects] are different rectangles. The point is the caller's (a finger, or a probe from the recording)
/// and is not rounded; what it is tested against is the wall's.
Cell? paneAt(List<Cell> cells, {required double x, required double y}) {
  for (final cell in cells) {
    if (cell.rect.contains(x, y)) {
      return cell;
    }
  }
  return null;
}

/// Which edge of a pane a drop names — the wall's `Side`. Declared in the order `drop_for` tests the edges (left,
/// right, top, bottom), because a tie between two is broken by the first that counts, so the order is part of the
/// answer.
enum Side { left, right, top, bottom }

/// **What a release at a point would do** — `view::Drop`: a retile against an edge, or a swap.
sealed class Drop {
  const Drop();
}

/// The two panes trade places; the pane under the pointer is [a] and the one in the air is [b] (`centre`).
final class Swap extends Drop {
  const Swap(this.a, this.b);

  final int a;
  final int b;

  @override
  bool operator ==(Object other) => other is Swap && other.a == a && other.b == b;

  @override
  int get hashCode => Object.hash('swap', a, b);

  @override
  String toString() => 'Swap($a, $b)';
}

/// [dragged] takes the [side] of [target] — the pane **keeps its id**, which is what a re-tile applies.
final class Retile extends Drop {
  const Retile(this.target, this.side);

  final int target;
  final Side side;

  @override
  bool operator ==(Object other) =>
      other is Retile && other.target == target && other.side == side;

  @override
  int get hashCode => Object.hash('retile', target, side);

  @override
  String toString() => 'Retile($target, ${side.name})';
}

/// **What a pane released at a point would do** — `Layout::drop_for`, and the one place [Numbers.edgeBand] decides
/// something.
///
/// The band's rule, stated once (#220): a point belongs to the pane under it; for each of that pane's four edges,
/// `|point − edge| / span` is how deep into the band it is; an edge only counts when that share is **below**
/// [Numbers.edgeBand]; the nearest edge that counts wins; and **no edge in band is a swap**. The band is a share of
/// the *pane's* span and not of the canvas — the recording's probes are fractions of a pane's own rectangle, never of
/// the window, because a band measured against the window is the bug it exists to catch (`src/control/tiling.rs`).
///
/// `null` when nothing is under the point, or when the point is over the dragged pane itself: a release where it
/// started is a no-op and not a swap with itself, exactly as on the wall.
///
/// The [cells] are the caller's — the ones the picture was drawn from — so a preview and the card under the finger
/// cannot be two answers to "which pane is that" (#225, *the landing is drawn before release, and it is computed the
/// wall's way*).
Drop? dropFor(
  List<Cell> cells, {
  required int dragged,
  required double x,
  required double y,
  Numbers? numbers,
}) {
  final wall = numbers ?? Numbers.wall;
  final target = paneAt(cells, x: x, y: y);
  if (target == null || target.pane == dragged) {
    return null;
  }
  final rect = target.rect;
  final edges = [
    (Side.left, x - rect.x, rect.w),
    (Side.right, rect.right - x, rect.w),
    (Side.top, y - rect.y, rect.h),
    (Side.bottom, rect.bottom - y, rect.h),
  ];
  (Side, double)? nearest;
  for (final (side, distance, span) in edges) {
    if (span <= 0.0 || distance / span >= wall.edgeBand) {
      continue;
    }
    if (nearest == null || distance < nearest.$2) {
      nearest = (side, distance);
    }
  }
  if (nearest == null) {
    return Swap(target.pane, dragged);
  }
  return Retile(target.pane, nearest.$1);
}

/// **The tiling a gutter drag previews** — `Layout::set_ratio` → `set_ratio_on`: the split that separates [pane] from
/// [other], at [share], clamped by [Numbers.clampShare].
///
/// The pair names the split, **in either order**: `pane.resize` takes `pane` and `other` unordered and looks the border
/// up both ways (`src/control.rs`, "the pair names the border"), and `set_ratio_on` sees it the one way a `Border`
/// carries it. A phone that took only one order would draw a preview for a release it cannot land — so both, here, and
/// the **outermost** split whose two sides hold the pair, which is the wall's walk. A pair this tiling does not
/// separate is answered with the tiling unchanged, which is `set_ratio`'s `false`.
///
/// **A share the wall cannot read is not written**: [share] that is not a number keeps the split's own ratio, which is
/// `set_ratio_on`'s answer for a non-finite ratio. So no preview this module builds can carry a ratio outside
/// `RATIO_RANGE`, and a release built from one cannot name a share the wall refuses.
Tiling previewResize(
  Tiling tiling, {
  required int pane,
  required int other,
  required double share,
  Numbers? numbers,
}) {
  final wall = numbers ?? Numbers.wall;
  switch (tiling) {
    case TilingLeaf():
      return tiling;
    case TilingSplit(:final a, :final b, :final ratio, :final split):
      final separates = (_names(a, pane) && _names(b, other)) ||
          (_names(a, other) && _names(b, pane));
      if (separates) {
        final next = share.isFinite ? wall.clampShare(share) : ratio;
        return TilingSplit(a, b, next, split);
      }
      return TilingSplit(
        previewResize(a, pane: pane, other: other, share: share, numbers: wall),
        previewResize(b, pane: pane, other: other, share: share, numbers: wall),
        ratio,
        split,
      );
  }
}

/// Whether a subtree holds [pane] — `contains_leaf`, which is how the wall names the two sides of a border.
bool _names(Tiling node, int pane) => switch (node) {
      TilingLeaf(pane: final id) => id == pane,
      TilingSplit(:final a, :final b) => _names(a, pane) || _names(b, pane),
    };

/// **The tiling is arithmetic before it is a picture** (#249), and the arithmetic is the wall's.
///
/// `fixtures/control/tiling/` is nine cases #220 recorded from the wall itself — a viewport, the four numbers a client
/// must not guess, the pane ids, the arrangement, the cells, the drawn rectangles and the release probes — so a client
/// in another language can be held to the same rectangles in the same order. This suite is that holding, and it reads
/// the files **by path** (`clients/phone/` → the repository's root), never regenerates them and never keeps a copy: a
/// fixture written from this package's own types would prove only that the phone agrees with itself.
///
/// Five claims, each with a failure that names it:
///
/// 1. **The phone's numbers are the wall's** — [Numbers.wall] against `numbers` in every case, so a token that moved
///    is a red test here rather than a phone drawing a share the wall refuses.
/// 2. **Every case lays out the recorded cells**, in the recorded order — `Layout::cells`, the rectangles a finger is
///    hit-tested against.
/// 3. **Every case draws the recorded rects** — `view::panes_on` at the wall's gap, the rectangles the operator sees.
/// 4. **Every recorded release lands where the wall says** — `Layout::drop_for`, which is what `PANE_EDGE_BAND` is
///    for: a client that guessed 20% sends `centre` where the wall's own hand sends `left`.
/// 5. **A preview cannot name a share the wall refuses** — `RATIO_RANGE`, at the two places the wall clamps (the
///    pointer and the tree).
///
/// The arithmetic is float32 because the wall's is, so the comparisons here are `==` and not a tolerance: the reason
/// is [CellRect]'s, and `side-by-side-dragged.json` is the case that notices a `double` (`742.39996`, not `742.4`).
library;

import 'dart:convert';
import 'dart:io';

import 'package:arcons_phone/cells.dart';
import 'package:arcons_phone/wire.dart';
import 'package:test/test.dart';

/// The recording, by path from this package: `clients/phone/` → the repository's root. `cargo run -- --record`
/// writes it (`src/control/tiling.rs`), and nothing here writes it back.
const String tilingDirectory = '../../fixtures/control/tiling';

/// One recorded case: a wall, as the file spells it.
final class RecordedCase {
  const RecordedCase({
    required this.name,
    required this.into,
    required this.numbers,
    required this.panes,
    required this.tiling,
    required this.cells,
    required this.rects,
    required this.drops,
  });

  /// The file's name without `.json` — a case's only name.
  final String name;

  /// The rectangle the tiling lands in, in the *recording's* points. The phone's canvas is the phone's; these are the
  /// wall's, because they are the rectangle the recorded numbers answer for.
  final CellRect into;
  final Numbers numbers;
  final List<int> panes;

  /// The arrangement, in the wire's spelling (leaves are pane ids), translated from the file's once, in
  /// [readTiling].
  final Tiling tiling;
  final List<Cell> cells;
  final List<Cell> rects;
  final List<Probe> drops;
}

/// A released pane, at a point, and what the wall said it would do — `tiling::Probe`.
final class Probe {
  const Probe(this.dragged, this.x, this.y, this.drop);

  final int dragged;
  final double x;
  final double y;

  /// `null` when the release does nothing: over the dragged pane's own cell, or outside every pane.
  final Drop? drop;
}

void main() {
  final cases = readCases();

  group('the recording', () {
    test('is the wall\'s, and every case in it is read', () {
      expect(
        cases.length,
        greaterThanOrEqualTo(9),
        reason:
            'fixtures/control/tiling/ holds ${cases.length} cases: #220 recorded nine, and a recording that shrank '
            'would let every test below pass while proving less',
      );
      expect(
        {for (final one in cases) one.name}.length,
        cases.length,
        reason: 'two cases answer to one name: a failure would not say which wall it is about',
      );
      expect(
        {for (final one in cases) ...one.panes}.length,
        greaterThanOrEqualTo(4),
        reason: 'the recording names ${cases.length} cases and no pane beyond the first few: a corpus of one-pane '
            'walls would not exercise the tree at all',
      );
      // Non-vacuity for the two numbers that are easy to leave untested: a case where MIN_PANE_PT lifts a share the
      // clamp would otherwise grant (`narrowest-share`), and a case where the rectangle is too small to hold two panes
      // (`window-too-small`), so `split_length`'s halving branch is exercised and not just present.
      final lifted = cases.where((one) {
        return one.tiling.leaves.any(
          (pane) => one.cells.any((c) => c.pane == pane && c.rect.w == one.numbers.minPane),
        );
      });
      expect(
        lifted,
        isNotEmpty,
        reason: 'no recorded case has a pane the minimum lifted: MIN_PANE_PT (src/ui/tokens.rs) would be untested',
      );
      final halved = cases.where((one) => one.into.w < one.numbers.minPane * 2 || one.into.h < one.numbers.minPane * 2);
      expect(
        halved,
        isNotEmpty,
        reason: 'no recorded case is too small for two panes: split_length\'s halving branch (src/ui/view.rs) would '
            'never run, and a client that always applied the minimum would draw a pane the wall never draws',
      );
    });

    test('the wall\'s numbers are the phone\'s, and the recording says so', () {
      for (final one in cases) {
        expect(
          one.numbers,
          Numbers.wall,
          reason:
              '${one.name}.json records ${one.numbers}, and the phone holds ${Numbers.wall}: the numbers are the '
              'wall\'s (src/ui/tokens.rs: MIN_PANE_PT, PANE_EDGE_BAND, GAP; src/config/layout.rs: RATIO_RANGE), so the '
              'phone\'s copy is stale — re-record and ask what else moved before trusting a client in another '
              'language (#220)',
        );
      }
    });
  });

  group('the cells', () {
    test('every case lays out the recorded cells, in the recorded order', () {
      var checked = 0;
      for (final one in cases) {
        // The file's own numbers, not the phone's copy: this test is about the walk, and the constants are held to the
        // same files a test above.
        final laid = cells(one.tiling, one.into, numbers: one.numbers);
        checked += laid.length;
        expect(
          laid,
          one.cells,
          reason:
              '${one.name}.json records these cells and the phone laid out $laid: `Layout::cells` is the '
              'rectangle a finger is hit-tested against, so a client that differs draws a card it cannot press '
              '(#220)',
        );
        expect(
          laid.map((cell) => cell.pane),
          one.tiling.leaves,
          reason: '${one.name}.json: the cells are in the tree\'s own reading order, a before b at every split',
        );
      }
      expect(
        checked,
        greaterThanOrEqualTo(20),
        reason: 'only $checked cells were checked: the nine cases hold more than that, so the walk was not exercised',
      );
    });

    test('every case draws the recorded rects, in the recorded order', () {
      for (final one in cases) {
        expect(
          rects(one.tiling, one.into, numbers: one.numbers),
          one.rects,
          reason:
              '${one.name}.json records these rectangles as `view::panes_on` at the wall\'s gap: the drawn card is '
              'the cell it is hit-tested in, pulled in by half the gap — GAP is the wall\'s (src/ui/tokens.rs), never '
              'the phone\'s guess',
        );
      }
    });

    test('a share is a share: the same tree in another rectangle is the same shape', () {
      final three = caseNamed(cases, 'three');
      // A rectangle no case records, big enough that the minimum is inert: what is left is the ratio.
      final canvas = CellRect(0.0, 0.0, 1000.0, 500.0);
      final laid = cells(three.tiling, canvas, numbers: three.numbers);
      expect(laid, hasLength(3));
      expect(
        laid.first.rect.w / canvas.w,
        closeTo(0.58, 1e-6),
        reason: 'a ratio is a share of its cell, not a number of points: 0.58 of 1000 pt is 580 pt, and a phone that '
            'scaled the wall\'s 1280 pt cells would draw 742.4 there (#220, docs/mobile-ui.md)',
      );
      expect(laid.first.rect.h / canvas.h, closeTo(1.0, 1e-6));
      expect(laid[1].rect.w / canvas.w, closeTo(0.42, 1e-6));
      expect(laid[1].rect.h / canvas.h, closeTo(0.5, 1e-6));
      final area = laid.fold<double>(0, (sum, cell) => sum + cell.rect.w * cell.rect.h);
      expect(
        area,
        closeTo(canvas.w * canvas.h, 0.5),
        reason: 'the cells cover the rectangle exactly — no gap between panes and no overlap, at any size',
      );
    });
  });

  group('the drop', () {
    test('every recorded release lands where the wall says', () {
      var swaps = 0;
      var retiles = 0;
      var nothing = 0;
      final sides = <Side>{};
      for (final one in cases) {
        // The phone's own cells, from its own walk: the release is hit-tested against the picture it drew.
        final laid = cells(one.tiling, one.into, numbers: one.numbers);
        expect(one.drops, isNotEmpty, reason: '${one.name}.json records no release: the band would be untested');
        for (final probe in one.drops) {
          final landed = dropFor(
            laid,
            dragged: probe.dragged,
            x: probe.x,
            y: probe.y,
            numbers: one.numbers,
          );
          expect(
            landed,
            probe.drop,
            reason:
                '${one.name}.json: a release of pane ${probe.dragged} at (${probe.x}, ${probe.y}) means ${probe.drop} '
                'to the wall and $landed to the phone. PANE_EDGE_BAND (src/ui/tokens.rs, in the recording) decides '
                'an edge from a swap, and it is the pane\'s own span: a band measured against the canvas, or guessed, '
                'is the phone and the wall disagreeing about what the operator did (#220)',
          );
          switch (landed) {
            case null:
              nothing++;
            case Swap():
              swaps++;
            case Retile(:final side):
              retiles++;
              sides.add(side);
          }
        }
      }
      expect(swaps, greaterThan(0), reason: 'no recorded release is a swap: the middle of a pane would be untested');
      expect(retiles, greaterThan(0), reason: 'no recorded release is an edge: the band would be untested');
      expect(
        nothing,
        greaterThan(0),
        reason: 'no recorded release does nothing: "released where it started" is a no-op on the wall, and a client '
            'that swapped a pane with itself would send an intent the wall refuses',
      );
      expect(
        sides,
        Side.values.toSet(),
        reason: 'the recording pins only $sides: all four edges are a share of their own span, and a client that read '
            'the band off the wrong axis gets side-by-side right and stacked wrong',
      );
    });

    test('the band is the share the caller gives, not one the phone keeps', () {
      final side = caseNamed(cases, 'side-by-side');
      final laid = cells(side.tiling, side.into, numbers: side.numbers);
      final target = laid.first.rect;
      // 35% into the pane, halfway down: a step outside the wall's band and well inside a wider one.
      final x = target.x + target.w * 0.35;
      final y = target.y + target.h / 2.0;
      expect(
        dropFor(laid, dragged: 2, x: x, y: y, numbers: side.numbers),
        Swap(1, 2),
        reason: 'at PANE_EDGE_BAND 0.3 a point 35% into a pane is past the band, so the release is a swap: 0.35 is the '
            'step outside the band that the recording probes',
      );
      expect(
        dropFor(laid, dragged: 2, x: x, y: y, numbers: numbersWith(edgeBand: 0.5)),
        Retile(1, Side.left),
        reason:
            'the band is the number the caller gives — tokens::PANE_EDGE_BAND, in the recording — and not one the '
            'phone keeps: at 0.5 the same point is against the pane\'s left edge, and a client with the wall\'s 0.3 '
            'baked in would send `centre` for an edge the wall draws (#220)',
      );
    });
  });

  group('a preview of a resize', () {
    test('the share a gutter asks for is always inside RATIO_RANGE', () {
      final wall = Numbers.wall;
      // A pointer before the split, on it, past it, and a long way past it: the clamp is what stops a drag naming a
      // share with no pane in it.
      for (final at in [-1e9, -50.0, 0.0, 4.9999, 5.0, 50.0, 95.0, 95.0001, 100.0, 150.0, 1e9]) {
        final share = wall.shareAlong(at: at, start: 0.0, span: 100.0);
        expect(
          share,
          allOf(greaterThanOrEqualTo(wall.ratioMin), lessThanOrEqualTo(wall.ratioMax)),
          reason: 'a pointer at $at of a 100 pt split asks for ${at / 100}: RATIO_RANGE is the wall\'s clamp '
              '(src/config/layout.rs), and a preview drawn outside it is a release the wall refuses',
        );
      }
      expect(
        wall.shareAlong(at: 50.0, start: 0.0, span: 100.0),
        closeTo(0.5, 1e-7),
        reason: 'a pointer in the middle of its split is half: the function is a share, not a clamp that always bites',
      );
      // A split with no span has no share to read; the wall's answer there is the ratio a fresh split is made with.
      expect(wall.shareAlong(at: 30.0, start: 0.0, span: 0.0), 0.5);
      expect(wall.shareAlong(at: 30.0, start: 0.0, span: -5.0), 0.5);
      expect(wall.shareAlong(at: double.nan, start: 0.0, span: 100.0), 0.5);
    });

    test('the range a share is held to is the range the caller gives', () {
      final narrow = numbersWith(ratioMin: 0.2, ratioMax: 0.8);
      expect(narrow.clampShare(0.0), narrow.ratioMin);
      expect(narrow.clampShare(1.0), narrow.ratioMax);
      expect(narrow.clampShare(0.5), 0.5);
      expect(
        narrow.shareAlong(at: -100.0, start: 0.0, span: 100.0),
        narrow.ratioMin,
        reason: 'RATIO_RANGE is src/config/layout.rs\'s range, read from the numbers given — a range baked into the '
            'phone would keep clamping a caller whose wall moved',
      );
    });

    test('no preview can carry a share the wall refuses', () {
      final three = caseNamed(cases, 'three');
      final before = three.tiling as TilingSplit;
      for (final asked in [-7.0, 0.0, 0.049, 0.05, 0.5, 0.95, 0.951, 1.0, 42.0]) {
        final preview = previewResize(three.tiling, pane: 1, other: 2, share: asked) as TilingSplit;
        expect(
          preview.ratio,
          allOf(greaterThanOrEqualTo(Numbers.wall.ratioMin), lessThanOrEqualTo(Numbers.wall.ratioMax)),
          reason: 'a preview asked for a share of $asked kept ${preview.ratio}: `pane.resize` is clamped to '
              'RATIO_RANGE on the wall (src/config/layout.rs), so a preview outside it is a picture of a release '
              'that cannot land',
        );
        // And the tiling it previews still tiles: the clamp moves the line, it does not empty a cell.
        final area = cells(preview, three.into, numbers: three.numbers)
            .fold<double>(0, (sum, cell) => sum + cell.rect.w * cell.rect.h);
        expect(area, closeTo(three.into.w * three.into.h, 0.5));
      }
      // A share the wall cannot read is not written: `set_ratio_on` keeps the split's own ratio, so the preview is
      // the tiling the operator already has — the same cells, not a second tree with a NaN in it.
      for (final unreadable in [double.nan, double.infinity]) {
        final kept = previewResize(three.tiling, pane: 1, other: 2, share: unreadable) as TilingSplit;
        expect(kept.ratio, before.ratio);
        expect(
          cells(kept, three.into, numbers: three.numbers),
          cells(before, three.into, numbers: three.numbers),
        );
      }
    });

    test('the pair names the split, and the outermost one that separates them', () {
      final three = caseNamed(cases, 'three');
      final before = three.tiling as TilingSplit;
      final held = Numbers.wall.clampShare(0.4);

      // Panes 1 and 2 are separated by the outer split: 2 is inside the inner one, and the border a hand grabs is
      // the one that runs between the two panes it holds.
      final outer = previewResize(three.tiling, pane: 1, other: 2, share: 0.4) as TilingSplit;
      expect(outer.ratio, held);
      expect((outer.b as TilingSplit).ratio, (before.b as TilingSplit).ratio);

      // Panes 2 and 3 are the inner split's two sides: the outer ratio is untouched.
      final inner = previewResize(three.tiling, pane: 2, other: 3, share: 0.4) as TilingSplit;
      expect(inner.ratio, before.ratio);
      expect((inner.b as TilingSplit).ratio, held);

      // A pair this tiling does not separate moves nothing — `set_ratio`'s `false`, and the same cells afterwards.
      expect(
        cells(
          previewResize(three.tiling, pane: 1, other: 9, share: 0.4),
          three.into,
          numbers: three.numbers,
        ),
        cells(three.tiling, three.into, numbers: three.numbers),
        reason: 'a preview of a border this tiling does not hold must move no line: the release would be refused as a '
            'rule, and a picture that moved anyway is a card the operator watched go somewhere it cannot land',
      );

      // The pair is unordered, as `pane.resize` takes it (`src/control.rs`, "the pair names the border"): the same two
      // panes the other way round are the same preview, or the phone would draw a release the wall cannot land.
      expect(
        cells(
          previewResize(three.tiling, pane: 2, other: 1, share: 0.4),
          three.into,
          numbers: three.numbers,
        ),
        cells(outer, three.into, numbers: three.numbers),
        reason: 'a gutter separates two panes, and which one a hand names first is not part of what it asked for',
      );
    });
  });

  group('one walk', () {
    test('a press in the gutter belongs to a cell and not to a card', () {
      final side = caseNamed(cases, 'side-by-side');
      final laid = cells(side.tiling, side.into, numbers: side.numbers);
      final drawn = rects(side.tiling, side.into, numbers: side.numbers);
      final gutter = (drawn.first.rect.right + drawn[1].rect.x) / 2;
      final middle = gutter - 1;
      for (final at in [middle, gutter]) {
        expect(
          drawn.any((cell) => cell.rect.contains(at, side.into.h / 2)),
          isFalse,
          reason: 'the drawn cards leave a gap at ${drawn.first.rect.right}…${drawn[1].rect.x}: a press there is in '
              'the gutter, which is the phone\'s own look',
        );
        expect(
          paneAt(laid, x: at, y: side.into.h / 2),
          isNotNull,
          reason:
              'a point at $at is in the gutter and still belongs to a pane\'s cell: the wall hit-tests the tiling and '
              'not the drawn rectangle (docs/mobile-ui.md, the cells are not the cards), so the picture and the '
              'gesture agree about which pane was pressed',
        );
      }
    });

    test('two neighbouring cards are the gap the caller gives apart', () {
      final side = caseNamed(cases, 'side-by-side');
      final wide = numbersWith(gap: 16.0);
      final drawn = rects(side.tiling, side.into, numbers: wide);
      expect(
        drawn[1].rect.x - drawn[0].rect.right,
        wide.gap,
        reason: 'the gutter between two cards is GAP (src/ui/tokens.rs), taken out of both cells — the number the '
            'caller gives, never one the phone keeps: a half-gap baked in here draws a gap the recording does not '
            'have, and the recording is what a client is held to (#220)',
      );
    });

    test('an empty tiling is no pane at all, not a pane covering everything', () {
      expect(paneAt(const [], x: 0.0, y: 0.0), isNull);
      expect(dropFor(const [], dragged: 1, x: 0.0, y: 0.0), isNull);
    });
  });

  group('a preview of a drop', () {
    test('a re-tile closes the lifted pane\'s slot and gives it half of the target', () {
      final three = caseNamed(cases, 'three');
      final before = three.tiling;
      final target = cellOf(before, three.into, 1, three.numbers);
      for (final side in Side.values) {
        final preview = previewDrop(
          before,
          dragged: 3,
          drop: Retile(1, side),
        );
        expect(
          preview,
          isNotNull,
          reason: 'a re-tile of pane 3 onto pane 1\'s ${side.name} has no preview: `Layout::apply` answers this drop, '
              'so a client that draws nothing here is a client whose release lands somewhere it never showed (#84)',
        );
        final laid = cells(preview!, three.into, numbers: three.numbers);
        expect(
          laid.map((cell) => cell.pane).toSet(),
          {1, 2, 3},
          reason: 'the preview of a re-tile changed which panes exist: a re-tile **keeps the id** — a camera, a thread '
              'and a frame live on it, and a preview that minted a new pane would draw a title over nothing',
        );
        final slot = laid.firstWhere((cell) => cell.pane == 3).rect;
        final horizontal = side == Side.left || side == Side.right;
        expect(
          slot.w,
          horizontal ? target.w / 2.0 : target.w,
          reason: 'the ${side.name} preview gave pane 3 a cell ${slot.w} wide: `apply` splits the target at one half on '
              'the named side (docs/mobile-ui.md, *the preview\'s shape is Layout::apply\'s*)',
        );
        expect(
          slot.h,
          horizontal ? target.h : target.h / 2.0,
          reason: 'the ${side.name} preview gave pane 3 a cell ${slot.h} tall, and the split\'s axis is the side\'s',
        );
        // And it lands inside the cell the target had, on the half it was dropped against.
        final onTheSide = switch (side) {
          Side.left => slot.x == target.x,
          Side.right => slot.right == target.right,
          Side.top => slot.y == target.y,
          Side.bottom => slot.bottom == target.bottom,
        };
        expect(
          onTheSide,
          isTrue,
          reason: 'pane 3 previewed at $slot, which is not the ${side.name} half of pane 1\'s cell $target: a drop '
              'against an edge lands against that edge',
        );
        // **A preview is a tiling like any other**: it covers the canvas exactly, as the cells the recording holds do.
        final area = laid.fold<double>(0, (sum, cell) => sum + cell.rect.w * cell.rect.h);
        expect(
          area,
          closeTo(three.into.w * three.into.h, 0.5),
          reason: 'the preview\'s cells cover $area of ${three.into.w * three.into.h}: a picture before release that does '
              'not tile is a picture the wall never draws',
        );
      }
    });

    test('a swap trades the two leaves and every ratio stays where it was', () {
      final three = caseNamed(cases, 'three');
      final before = three.tiling;
      final preview = previewDrop(before, dragged: 3, drop: const Swap(1, 3));
      expect(preview, isNotNull, reason: 'a swap of two panes the tiling holds has no preview');
      expect(
        preview!.leaves,
        before.leaves.map((pane) => pane == 1 ? 3 : (pane == 3 ? 1 : pane)).toList(),
        reason: 'a swap is the two leaves trading places and nothing else: ${preview.leaves} is not that trade',
      );
      expect(
        ratiosOf(preview),
        ratiosOf(before),
        reason: 'the swap moved a ratio: `swap_leaves` touches leaves only, and every split above them stays where it '
            'was (docs/mobile-ui.md)',
      );
      final laid = cells(preview, three.into, numbers: three.numbers);
      expect(
        laid.firstWhere((cell) => cell.pane == 3).rect,
        cellOf(before, three.into, 1, three.numbers),
        reason: 'the swapped pane did not take the cell of the pane it swapped with',
      );
    });

    test('a drop the wall refuses has no preview', () {
      final three = caseNamed(cases, 'three');
      final single = caseNamed(cases, 'one-pane');
      expect(
        previewDrop(three.tiling, dragged: 1, drop: const Swap(1, 1)),
        isNull,
        reason: 'a swap with itself previewed a tiling: the wall refuses it by name (`the pane cannot be swapped with '
            'itself`), and a card drawn where a refused release cannot land is a picture of a wall that will not exist',
      );
      expect(
        previewDrop(three.tiling, dragged: 1, drop: const Retile(9, Side.left)),
        isNull,
        reason: 'a re-tile onto a pane the tiling does not hold previewed something: `no such pane` is a rule',
      );
      expect(
        previewDrop(three.tiling, dragged: 9, drop: const Retile(1, Side.left)),
        isNull,
        reason: 'a re-tile of a pane the tiling does not hold previewed something: the wall has no pane to close out',
      );
      expect(
        previewDrop(single.tiling, dragged: 1, drop: const Retile(1, Side.left)),
        isNull,
        reason: 'a one-pane tiling previewed a re-tile: `the tiling refused the pane`, because a workspace holding one '
            'pane cannot give half of it away',
      );
    });
  });

  group('the gutters', () {
    const grip = 24.0;

    test('every split is a gutter, and its line is where two of the cells meet', () {
      var checked = 0;
      for (final one in cases) {
        final list = gutters(one.tiling, one.into, numbers: one.numbers);
        expect(
          list.length,
          splitsIn(one.tiling),
          reason: '${one.name}.json holds ${splitsIn(one.tiling)} splits and the phone found ${list.length} gutters: '
              '`Layout::borders` walks the tree once, and a gutter with no line is a handle nobody can grab',
        );
        final laid = cells(one.tiling, one.into, numbers: one.numbers);
        for (final gutter in list) {
          checked++;
          final vertical = gutter.split == Split.side;
          // The cells that meet the line **and lie in the split's own rectangle**: two cells elsewhere on the canvas
          // can share the coordinate without a border running between them.
          final touching = laid.where(
            (cell) =>
                (vertical
                    ? cell.rect.right == gutter.at || cell.rect.x == gutter.at
                    : cell.rect.bottom == gutter.at || cell.rect.y == gutter.at) &&
                (vertical
                    ? cell.rect.y >= gutter.rect.y &&
                          cell.rect.bottom <= gutter.rect.bottom
                    : cell.rect.x >= gutter.rect.x &&
                          cell.rect.right <= gutter.rect.right),
          );
          expect(
            touching.length,
            greaterThanOrEqualTo(2),
            reason: '${one.name}.json: the gutter ${gutter.at} has ${touching.length} cells against it — a border is '
                'the line two panes meet on, and a line with one cell on it is not between two panes',
          );
          for (final cell in touching) {
            final inside = vertical
                ? cell.rect.y >= gutter.rect.y && cell.rect.bottom <= gutter.rect.bottom
                : cell.rect.x >= gutter.rect.x && cell.rect.right <= gutter.rect.right;
            expect(
              inside,
              isTrue,
              reason: '${one.name}.json: cell ${cell.rect} meets the gutter ${gutter.at} outside the split\'s own '
                  'rectangle ${gutter.rect}: a border is as long as its split and not as long as the canvas',
            );
          }
          expect(
            gutter.at,
            inInclusiveRange(vertical ? gutter.rect.x : gutter.rect.y,
                vertical ? gutter.rect.right : gutter.rect.bottom),
            reason: '${one.name}.json: the gutter is drawn at ${gutter.at}, outside the split it belongs to',
          );
        }
      }
      expect(
        checked,
        greaterThanOrEqualTo(10),
        reason: 'only $checked gutters were checked across nine recorded tilings: the walk was not exercised',
      );
    });

    test('a gutter names the pair `pane.resize` takes, and the pair moves that line', () {
      for (final one in cases) {
        for (final gutter in gutters(one.tiling, one.into, numbers: one.numbers)) {
          final moved = previewResize(
            one.tiling,
            pane: gutter.a,
            other: gutter.b,
            share: 0.42,
            numbers: one.numbers,
          );
          final after = gutters(moved, one.into, numbers: one.numbers);
          final same = after.where((other) => other.rect == gutter.rect);
          expect(
            same,
            isNotEmpty,
            reason: '${one.name}.json: resizing (${gutter.a}, ${gutter.b}) removed the split it names: the pair is kept '
                'on each side of a border so that a resize can name it at all',
          );
          final line = same.first;
          final expected = Numbers.wall.clampShare(0.42);
          if (gutter.span >= one.numbers.minPane * 2.0) {
            // A split too small for two panes halves whatever share it is given (`split_length`), which is
            // `window-too-small.json`: the stored ratio moves and the drawn line cannot, on the wall either.
            expect(
              (line.at - line.start) / line.span,
              closeTo(expected, 1e-5),
              reason: '${one.name}.json: resizing (${gutter.a}, ${gutter.b}) to $expected left the line at ${line.at} of '
                  '${line.start}…${line.start + line.span}: the pair names a border, and a border is the split its two '
                  'sides share — the outermost one that separates them',
            );
          }
        }
      }
    });

    test('a share read off a gutter is always inside RATIO_RANGE', () {
      final narrow = caseNamed(cases, 'narrowest-share');
      final gutter = gutters(narrow.tiling, narrow.into, numbers: narrow.numbers).first;
      for (final at in [-1e9, -50.0, gutter.at, 1e9]) {
        final point = gutter.split == Split.side
            ? (x: at, y: narrow.into.h / 2.0)
            : (x: narrow.into.w / 2.0, y: at);
        final share = gutter.shareAt(point, narrow.numbers);
        expect(
          share,
          allOf(
            greaterThanOrEqualTo(narrow.numbers.ratioMin),
            lessThanOrEqualTo(narrow.numbers.ratioMax),
          ),
          reason: 'a pointer at $at of the split asked for $share: RATIO_RANGE is the wall\'s clamp '
              '(src/config/layout.rs), and the preview the phone draws at the end of a gutter is the share it sends',
        );
      }
    });

    test('a press is on the gutter within the grip, and off it outside', () {
      final three = caseNamed(cases, 'three');
      final list = gutters(three.tiling, three.into, numbers: three.numbers);
      final root = list.first;
      expect(root.split, Split.side, reason: 'three.json\'s first split is side by side: a different case would make '
          'this test about a horizontal line');
      final across = root.at;
      expect(
        gutterAt(list, x: across, y: 720.0, grip: grip),
        same(root),
        reason: 'a press on the line is not on the border it draws',
      );
      expect(
        gutterAt(list, x: across - grip / 2.0 + 0.1, y: 720.0, grip: grip),
        same(root),
        reason: 'a press at the edge of the grip is not on the line, and a grip narrower than the drawn gap is a handle '
            'the eye can see and the hand cannot find',
      );
      expect(
        gutterAt(list, x: across - grip, y: 720.0, grip: grip),
        isNull,
        reason: 'a press a grip away from the line was taken as a drag of it',
      );
      // The line stops at its own split: a nested split's gutter is not as tall as the canvas.
      final inner = list.firstWhere((gutter) => gutter.split == Split.stack);
      expect(
        gutterAt(list, x: 1200.0, y: inner.at, grip: grip),
        same(inner),
        reason: 'a press on a horizontal gutter inside one column is not on it: a border is as long as its split',
      );
      expect(
        gutterAt(list, x: 100.0, y: inner.at, grip: grip),
        isNull,
        reason: 'the horizontal gutter between panes 2 and 3 was grabbed at x = 100, which is outside the split it '
            'belongs to: the pane there has no border at that y',
      );
    });

    test('where two lines cross, the outer split is the one a press takes', () {
      final three = caseNamed(cases, 'three');
      final list = gutters(three.tiling, three.into, numbers: three.numbers);
      final outer = list.first;
      expect(
        gutterAt(list, x: outer.at, y: 360.0, grip: grip),
        same(outer),
        reason: 'a press where the outer line crosses the inner one took the inner line: `border_at` takes the first '
            'border of a root-first walk, so the outer split is the one the operator grabs',
      );
      // And a tiling of one pane has no gutter, which is not a gutter at the whole canvas.
      final single = caseNamed(cases, 'one-pane');
      expect(gutters(single.tiling, single.into, numbers: single.numbers), isEmpty);
    });
  });
}


/// The recorded rectangle of one pane, from the wall's own walk — `Layout::cells`, through [cells].
CellRect cellOf(Tiling tiling, CellRect into, int pane, Numbers numbers) =>
    cells(tiling, into, numbers: numbers).firstWhere((cell) => cell.pane == pane).rect;

/// Every split's ratio in reading order — what a swap must leave alone and what a resize moves one of.
List<double> ratiosOf(Tiling tiling) => switch (tiling) {
  TilingLeaf() => const [],
  TilingSplit(:final a, :final b, :final ratio) => [
    ratio,
    ...ratiosOf(a),
    ...ratiosOf(b),
  ],
};

/// How many splits a tiling holds — one gutter each, in `Layout::borders`' walk.
int splitsIn(Tiling tiling) => ratiosOf(tiling).length;

/// The case with a given name. A missing one is a red test with the names that are there, because the recording is the
/// wall's and a case that vanished is a claim this suite no longer covers.
RecordedCase caseNamed(List<RecordedCase> cases, String name) {
  for (final one in cases) {
    if (one.name == name) {
      return one;
    }
  }
  fail('the recording holds ${cases.map((one) => one.name).toList()}, and no "$name"');
}

List<RecordedCase> readCases() {
  final directory = Directory(tilingDirectory);
  if (!directory.existsSync()) {
    fail(
      'the recording is not at $tilingDirectory: the phone is held to fixtures/control/tiling/, which `cargo run -- '
      '--record` writes (src/control/tiling.rs). A missing corpus is not a test that passes.',
    );
  }
  final files = directory
      .listSync()
      .whereType<File>()
      .where((file) => file.path.endsWith('.json'))
      .toList()
    ..sort((left, right) => left.path.compareTo(right.path));
  return [for (final file in files) readCase(file)];
}

RecordedCase readCase(File file) {
  final name = file.uri.pathSegments.last.replaceAll('.json', '');
  final json = asObject(jsonDecode(file.readAsStringSync()), name);
  final viewport = asObject(json['viewport'], '$name\'s viewport');
  final numbers = asObject(json['numbers'], '$name\'s numbers');
  final panes = [for (final pane in asArray(json['panes'], '$name\'s panes')) pane as int];
  return RecordedCase(
    name: name,
    // The recording writes the panel's size, and the wall lays its cells from the panel's origin.
    into: CellRect(0.0, 0.0, asDouble(viewport['w']), asDouble(viewport['h'])),
    numbers: Numbers(
      minPane: asDouble(numbers['min_pane_pt']),
      edgeBand: asDouble(numbers['edge_band']),
      gap: asDouble(numbers['gap']),
      ratioMin: asDouble(numbers['ratio_min']),
      ratioMax: asDouble(numbers['ratio_max']),
    ),
    panes: panes,
    tiling: readTiling(json['tiling'], panes),
    cells: readCells(json['cells']),
    rects: readCells(json['rects']),
    drops: [
      for (final probe in asArray(json['drops'], '$name\'s drops')) readProbe(probe),
    ],
  );
}

/// The recording's `tiling`, in the wire's spelling.
///
/// The file writes `layout.toml`'s shape (`SavedNode`, `src/config/layout.rs`), where **a leaf is an index into
/// `panes`**; the projection writes a leaf as a **pane id** (`wire.dart`, #218). That translation is the test's, once,
/// here — the file is the wall's and is not edited, and pretending it already spoke the wire would leave the
/// index-to-id rule untested on both sides of the boundary.
Tiling readTiling(Object? value, List<int> panes) {
  final node = asObject(value, 'a tiling');
  final split = node['split'];
  if (split == null) {
    final index = node['pane'] as int;
    if (index < 0 || index >= panes.length) {
      fail('a tiling leaf names pane index $index, and the case holds ${panes.length} panes');
    }
    return TilingLeaf(panes[index]);
  }
  return TilingSplit(
    readTiling(node['a'], panes),
    readTiling(node['b'], panes),
    asDouble(node['ratio']),
    Split.ofWord(split),
  );
}

List<Cell> readCells(Object? value) => [
      for (final cell in asArray(value, 'a cell list'))
        Cell(
          asObject(cell, 'a cell')['pane'] as int,
          readRect(asObject(cell, 'a cell')['rect']),
        ),
    ];

CellRect readRect(Object? value) {
  final rect = asObject(value, 'a rectangle');
  return CellRect(
    asDouble(rect['x']),
    asDouble(rect['y']),
    asDouble(rect['w']),
    asDouble(rect['h']),
  );
}

Probe readProbe(Object? value) {
  final probe = asObject(value, 'a release probe');
  final at = asArray(probe['at'], 'a release probe\'s "at"');
  return Probe(
    probe['dragged'] as int,
    asDouble(at[0]),
    asDouble(at[1]),
    readDrop(probe['drop']),
  );
}

Drop? readDrop(Object? value) {
  if (value == null) {
    return null;
  }
  final drop = asObject(value, 'a drop');
  return switch (drop['kind']) {
    'swap' => Swap(drop['a'] as int, drop['b'] as int),
    'retile' => Retile(drop['target'] as int, readSide(drop['side'])),
    final kind => fail(
        'the recording names a drop of kind ${jsonEncode(kind)}, which this client does not know: a new kind is a '
        'vocabulary the phone must type, not a default (#219, INV-07 one language over)',
      ),
  };
}

/// The four edge words `src/control/tiling.rs` records — the same words `pane.drop` takes (#219), so a client reads one
/// vocabulary for both.
Side readSide(Object? word) => switch (word) {
      'left' => Side.left,
      'right' => Side.right,
      'top' => Side.top,
      'bottom' => Side.bottom,
      final other => fail(
          'the recording names the edge ${jsonEncode(other)}, and an edge of a pane is left, right, top or bottom '
          '(src/control/tiling.rs::Edge)',
        ),
    };

Map<String, Object?> asObject(Object? value, String what) {
  if (value is Map<String, Object?>) {
    return value;
  }
  fail('$what is not a JSON object, and the recording is a contract of shapes');
}

List<Object?> asArray(Object? value, String what) {
  if (value is List<Object?>) {
    return value;
  }
  fail('$what is not a JSON array, and the recording is a contract of shapes');
}

double asDouble(Object? value) {
  if (value is num) {
    return value.toDouble();
  }
  fail('${jsonEncode(value)} is not a number: the recording writes point values, never strings');
}

/// The wall's numbers with one or more replaced — for the tests that hold a number to **the caller's** and not to a
/// constant the phone keeps. The fields a test does not name are [Numbers.wall]'s, which the test above holds to every
/// case in the recording.
Numbers numbersWith({
  double? minPane,
  double? edgeBand,
  double? gap,
  double? ratioMin,
  double? ratioMax,
}) =>
    Numbers(
      minPane: minPane ?? Numbers.wall.minPane,
      edgeBand: edgeBand ?? Numbers.wall.edgeBand,
      gap: gap ?? Numbers.wall.gap,
      ratioMin: ratioMin ?? Numbers.wall.ratioMin,
      ratioMax: ratioMax ?? Numbers.wall.ratioMax,
    );

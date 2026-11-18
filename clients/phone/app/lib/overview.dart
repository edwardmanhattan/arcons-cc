/// **The overview** — the wall's arrangement as a diagram you can arrange (`docs/mobile-ui.md`, #255, #257).
///
/// Three bands, and each is a value the phone *read* rather than something it remembers: the chips from
/// `active` and `workspaces[].name`, the canvas from `workspaces[].tiling` and `panes[]`, the state line from the last
/// read's outcome and a refusal's `kind`. Nothing here asks the wall for a window size or a camera, and nothing is
/// scaled from the wall's pixels — a `ratio` is a share, so the same tree is the same shape on a phone and on a panel
/// (#220).
///
/// **What this file is not allowed to know.** Where a card lands, whether a drag is over an edge or the middle, which
/// share a gutter asks for, and what a landing looks like: those are `cells.dart`'s (`dropFor`, `previewDrop`,
/// `gutters`, `previewResize`), because the wall's own hand uses the same functions and a widget that worked them out
/// again would be a second layout arguing with the first. What *is* here is the drawing and the gestures: a long press
/// lifts, a move previews, a release asks ([OverviewController.release]).
library;

import 'package:arcons_phone/cells.dart';
// `Split` is the tiling's, and the toolkit exports one of its own (a curve): the wire's names are asked for by name.
import 'package:arcons_phone/wire.dart' show Pane, Split, Tiling, Wall, Workspace;
import 'package:flutter/semantics.dart' show CustomSemanticsAction;
// A curve the toolkit calls `Split` is not the tiling's: the wire's name wins in this file.
import 'package:material_ui/material_ui.dart' hide Split;

import 'controller.dart';
import 'intents.dart';
import 'theme.dart';

/// The canvas's own key, so a test can find the rectangle the fingers use: a golden compares pixels and only a gesture
/// proves that a drag reached a handler (`fixtures/README.md`, `scripts/`).
const Key canvasKey = ValueKey('phone-canvas');

/// One card, where the wall's own arithmetic put it, and the sentence a reader hears for it.
final class LaidCard {
  const LaidCard({
    required this.pane,
    required this.rect,
    required this.spoken,
  });

  final Pane pane;
  final CellRect rect;

  /// **Produced by the walk that drew it** (`mobile-ui.md`): the same traversal that gives the rects gives the words,
  /// so a pane the canvas draws is a pane a reader hears.
  final String spoken;
}

/// **The walk** — the arrangement laid into the phone's own rectangle, in reading order.
///
/// One function gives the drawing and the words, and a widget that drew from a second traversal would be a screen whose
/// picture and whose speech could disagree. [tiling] is the arrangement to draw — the projection's, or the one a drag is
/// previewing — and it comes from `cells.dart`, never from a widget.
List<LaidCard> layOut(Wall wall, CellRect canvas, {Tiling? tiling}) {
  final workspace = activeWorkspace(wall);
  final drawn = tiling ?? workspace?.tiling;
  final panes = workspace?.panes;
  if (drawn == null || panes == null) {
    return const [];
  }
  final byId = {for (final pane in panes) pane.id: pane};
  final laid = <LaidCard>[];
  for (final cell in rects(drawn, canvas)) {
    final pane = byId[cell.pane];
    if (pane == null) {
      // A leaf names a pane its workspace does not hold; the corpus test refuses that from the wire (#248), so a card
      // with nothing behind it never reaches the screen.
      continue;
    }
    laid.add(LaidCard(pane: pane, rect: cell.rect, spoken: spokenOf(pane)));
  }
  return laid;
}

/// The workspace the wall is showing, or `null` when `active` names none — a projection from a wall with no workspace
/// is a wall with nothing to draw, not an error to raise on a phone.
Workspace? activeWorkspace(Wall wall) {
  if (wall.active < 0 || wall.active >= wall.workspaces.length) {
    return null;
  }
  return wall.workspaces[wall.active];
}

/// **What a card says out loud.** Its kind and its title, because a description that says `map` twice is one the
/// operator cannot act on; and whether it is the pane with the keys.
String spokenOf(Pane pane) {
  final title = pane.title.isEmpty ? '' : ' "${pane.title}"';
  final focus = pane.focused ? ', focused' : '';
  return '${pane.kind}$title$focus';
}

/// **What the canvas says out loud**, from the same walk: the workspace, how many panes, and each of them in the order
/// the cards are read.
String spokenCanvas(Wall wall, List<LaidCard> cards) {
  final workspace = activeWorkspace(wall);
  if (workspace == null) {
    return 'no workspaces';
  }
  if (cards.isEmpty) {
    return '${workspace.name}, no panes yet';
  }
  final count = cards.length == 1 ? 'one pane' : '${cards.length} panes';
  return '${workspace.name}, $count: ${cards.map((card) => card.spoken).join('; ')}';
}

/// **The phone's words for a slot**, and the list's rows and the drag's preview both use them: `left half of "Feed"`,
/// `swap with "Feed"`. The **values** stay the wall's (`left`, `right`, `top`, `bottom`, `centre`); only the words are
/// the phone's (`mobile-ui.md`, *Move to…*).
String placeWords(Edge edge, String title) => switch (edge) {
      Edge.left => 'left half of "$title"',
      Edge.right => 'right half of "$title"',
      Edge.top => 'top half of "$title"',
      Edge.bottom => 'bottom half of "$title"',
      Edge.centre => 'swap with "$title"',
    };

/// **The places a pane can go**: every other pane in the workspace, and the five slots of each, **in the wall's own
/// order of edges**. The list's rows and the drag's band offer the same slots — this is the list's half of that claim,
/// and `placeWords` is the words it draws them with.
List<DropIntent> placesFor(Workspace workspace, int pane) => [
      for (final target in workspace.panes)
        if (target.id != pane)
          for (final edge in Edge.values)
            DropIntent(pane: pane, onto: target.id, edge: edge),
    ];

/// **The screen.** It reads the controller and sends nothing of its own: every intent here is one of the controller's
/// and every rectangle is `cells.dart`'s.
final class OverviewScreen extends StatelessWidget {
  const OverviewScreen({super.key, required this.controller, this.onUpdate});

  final OverviewController controller;

  /// The update screen's one button. `mobile-ui.md` gives it the store page; this package has no store, so the shell
  /// that owns `main` decides what opening one is, and a test passes nothing and finds the button still drawn.
  final VoidCallback? onUpdate;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, child) => ColoredBox(
        color: PhoneTokens.canvas,
        child: SafeArea(
          child: Padding(
            padding: PhoneTokens.screen,
            child: controller.state == PhoneState.updateRequired
                ? _UpdateScreen(
                    words: controller.stop?.why ?? '',
                    onUpdate: onUpdate,
                  )
                : Column(
                    crossAxisAlignment: CrossAxisAlignment.stretch,
                    children: [
                      // **`connecting` is not `not connected`**: a phone that has read nothing has no shape to draw,
                      // so the chips are absent and the canvas is empty.
                      if (controller.state != PhoneState.connecting) ...[
                        _Chips(controller: controller),
                        const SizedBox(height: PhoneTokens.gap),
                      ],
                      Expanded(
                        child: controller.state == PhoneState.connecting
                            ? const SizedBox.shrink()
                            : LayoutBuilder(
                                builder: (context, constraints) {
                                  final canvas = CellRect(
                                    0,
                                    0,
                                    constraints.maxWidth,
                                    constraints.maxHeight,
                                  );
                                  final places = controller.placesPane;
                                  if (places != null) {
                                    return _Places(
                                      controller: controller,
                                      pane: places,
                                    );
                                  }
                                  return _Canvas(
                                    controller: controller,
                                    canvas: canvas,
                                  );
                                },
                              ),
                      ),
                      const SizedBox(height: PhoneTokens.gap),
                      _StateLine(controller: controller),
                    ],
                  ),
          ),
        ),
      ),
    );
  }
}

/// The workspaces, as chips: a press is a switch, and an emptied one is still a place with its name (#141).
final class _Chips extends StatelessWidget {
  const _Chips({required this.controller});

  final OverviewController controller;

  @override
  Widget build(BuildContext context) {
    final wall = controller.wall;
    if (wall == null) {
      return const SizedBox(height: PhoneTokens.chipHeight);
    }
    return SizedBox(
      height: PhoneTokens.chipHeight,
      // A row rather than a lazy list: a wall holds a handful of workspaces, and a chip that is not built is a chip a
      // screen reader cannot reach and a test cannot find.
      child: SingleChildScrollView(
        scrollDirection: Axis.horizontal,
        child: Row(
          children: [
            for (var index = 0; index < wall.workspaces.length; index++) ...[
              if (index > 0) const SizedBox(width: PhoneTokens.gap),
              _chip(index),
            ],
          ],
        ),
      ),
    );
  }

  Widget _chip(int index) {
    final wall = controller.wall!;
    final workspace = wall.workspaces[index];
    final active = index == wall.active;
    return Semantics(
      button: true,
      selected: active,
      excludeSemantics: true,
      label:
          '${workspace.name}, workspace ${index + 1}${workspace.empty ? ', empty' : ''}',
      child: Container(
        padding: const EdgeInsets.symmetric(horizontal: 12),
        alignment: Alignment.center,
        decoration: BoxDecoration(
          color: active ? PhoneTokens.chipActive : Colors.transparent,
          borderRadius: const BorderRadius.all(Radius.circular(14)),
          border: Border.all(
            color: active ? PhoneTokens.ring : PhoneTokens.cardEdge,
            width: active ? PhoneTokens.ringWidth : 1,
          ),
        ),
        child: Text(
          workspace.name,
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: TextStyle(
            color: active ? PhoneTokens.title : PhoneTokens.kind,
            fontSize: 13,
          ),
        ),
      ),
    );
  }
}

/// The arrangement, drawn where the wall's arithmetic put it, with the hand's gestures over it.
final class _Canvas extends StatefulWidget {
  const _Canvas({required this.controller, required this.canvas});

  final OverviewController controller;
  final CellRect canvas;

  @override
  State<_Canvas> createState() => _CanvasState();
}

final class _CanvasState extends State<_Canvas> {
  /// Where the pointer went **down**, which is the only point a gutter grip can be taken at: a `PanGestureRecognizer`
  /// reports its start after the slop, by which time the finger is somewhere else.
  Offset? _down;

  @override
  Widget build(BuildContext context) {
    final controller = widget.controller;
    final canvas = widget.canvas;
    final wall = controller.wall;
    final tiling = controller.drawn;
    final cards = wall == null || tiling == null
        ? const <LaidCard>[]
        : layOut(wall, canvas, tiling: tiling);
    if (wall == null || tiling == null || cards.isEmpty) {
      final workspace = wall == null ? null : activeWorkspace(wall);
      return Center(
        child: Text(
          // The wall's own words for an emptied workspace, so the phone and the wall say the same thing (#141).
          workspace != null && workspace.empty ? 'No panes yet.' : 'Nothing to show',
          style: const TextStyle(color: PhoneTokens.kind, fontSize: 14),
        ),
      );
    }

    final liftedId = controller.liftedPane;
    final lifted = _find(cards, liftedId);
    final held = controller.gutter;
    final live = controller.state == PhoneState.live;

    // **The diagram the phone could not re-read is drawn dimmed** (`mobile-ui.md`, *not connected*): it is the
    // arrangement it last saw, and the dimming is half of what keeps it from passing for this moment's.
    final dimmed = controller.state == PhoneState.notConnected;
    return GestureDetector(
      key: canvasKey,
      behavior: HitTestBehavior.opaque,
      // The hold is the phone's: `dropFor` is asked again on every move, and nothing is on the wire until the release.
      onLongPressStart: (details) => controller.lift(
        x: details.localPosition.dx,
        y: details.localPosition.dy,
        canvas: canvas,
      ),
      onLongPressMoveUpdate: (details) => controller.hold(
        x: details.localPosition.dx,
        y: details.localPosition.dy,
        canvas: canvas,
      ),
      onLongPressEnd: (details) => controller.release(
        x: details.localPosition.dx,
        y: details.localPosition.dy,
        canvas: canvas,
      ),
      onLongPressCancel: controller.cancel,
      // The other settled gesture: a gutter is a hold on a line, not on a card.
      // The grab is the point the finger went **down**, not the point at which the drag was recognised: a pan reports
      // itself after `kTouchSlop` of movement, and a gutter grabbed there would be a gutter a finger has already left.
      onPanDown: (details) => _down = details.localPosition,
      onPanStart: (details) {
        final down = _down ?? details.localPosition;
        // The grip is taken where the finger went down, and the share is the finger's **now**: a drag recognised after
        // the slop has already moved, and a preview that waited for the next event would lag the hand by one.
        if (controller.grabGutter(x: down.dx, y: down.dy, canvas: canvas)) {
          controller.dragGutter(
            x: details.localPosition.dx,
            y: details.localPosition.dy,
          );
        }
      },
      onPanUpdate: (details) => controller.dragGutter(
        x: details.localPosition.dx,
        y: details.localPosition.dy,
      ),
      onPanEnd: (details) {
        _down = null;
        controller.releaseGutter();
      },
      onPanCancel: () {
        _down = null;
        controller.cancelGutter();
      },
      child: Opacity(
        opacity: dimmed ? PhoneTokens.dimmed : 1.0,
        child: Semantics(
          label: spokenCanvas(wall, cards),
          container: true,
          explicitChildNodes: true,
          child: Stack(
            children: [
              for (final card in cards)
                if (card.pane.id != liftedId)
                  Positioned(
                    left: card.rect.x,
                    top: card.rect.y,
                    width: card.rect.w,
                    height: card.rect.h,
                    child: _Card(
                      card: card,
                      onFocus: live ? () => controller.focusPane(card.pane.id) : null,
                      onPlaces: live ? () => controller.openPlaces(card.pane.id) : null,
                    ),
                  )
                else if (controller.slotShaded)
                  Positioned(
                    left: card.rect.x,
                    top: card.rect.y,
                    width: card.rect.w,
                    height: card.rect.h,
                    // **The slot is shaded, not copied**: a copy hanging from the hand covers the one thing the
                    // operator is looking at (`mobile-ui.md`, *the preview is the slot, not the finger*).
                    child: const _Slot(),
                  ),
              // The gutters, on top of the cards: a line two panes meet on, with a grip at its middle.
              if (live && held == null)
                for (final gutter in gutters(tiling, canvas))
                  _GutterBar(gutter: gutter),
              if (held != null) _GutterBar(gutter: held.gutter, held: true),
              // The card in the air, last so it is above everything, and untouchable so the canvas keeps its gestures.
              if (lifted != null && controller.lifted != null)
                Positioned(
                  left: controller.lifted!.rect.x,
                  top: controller.lifted!.rect.y,
                  width: controller.lifted!.rect.w,
                  height: controller.lifted!.rect.h,
                  child: IgnorePointer(
                    child: _Card(card: lifted, lifted: true),
                  ),
                ),
            ],
          ),
        ),
      ),
    );
  }

  static LaidCard? _find(List<LaidCard> cards, int? pane) {
    if (pane == null) {
      return null;
    }
    for (final card in cards) {
      if (card.pane.id == pane) {
        return card;
      }
    }
    return null;
  }
}

/// One card: the glyph, the title, the kind in words, and the ring that says this one has the keys.
final class _Card extends StatelessWidget {
  const _Card({
    required this.card,
    this.lifted = false,
    this.onFocus,
    this.onPlaces,
  });

  final LaidCard card;

  /// A card the hand has lifted: the same face, a brighter edge, and a shadow, so it reads as *above* the canvas.
  final bool lifted;

  /// A press on a card sends `pane.focus`, and the ring moves when the next projection says it moved.
  final VoidCallback? onFocus;

  /// The card's **Move to…**, which is the drag's accessible half: a drag is not a reachable control, so the same
  /// intent has a path made of words (`mobile-ui.md`, *accessibility*).
  final VoidCallback? onPlaces;

  @override
  Widget build(BuildContext context) {
    final pane = card.pane;
    final ring = lifted
        ? PhoneTokens.liftedEdge
        : (pane.focused ? PhoneTokens.ring : PhoneTokens.cardEdge);
    return Semantics(
      button: true,
      selected: pane.focused,
      excludeSemantics: true,
      label: card.spoken,
      onTap: onFocus,
      customSemanticsActions: onPlaces == null
          ? const {}
          : {const CustomSemanticsAction(label: placesAction): onPlaces!},
      child: GestureDetector(
        onTap: onFocus,
        child: DecoratedBox(
          decoration: BoxDecoration(
            color: PhoneTokens.cardFill,
            borderRadius: const BorderRadius.all(PhoneTokens.corner),
            border: Border.all(
              color: ring,
              width: lifted || pane.focused ? PhoneTokens.ringWidth : 1,
            ),
            boxShadow: lifted
                ? const [
                    BoxShadow(
                      color: PhoneTokens.shadow,
                      blurRadius: 16,
                      offset: Offset(0, 6),
                    ),
                  ]
                : null,
          ),
          child: Padding(
            padding: PhoneTokens.cardPadding,
            child: Column(
              crossAxisAlignment: CrossAxisAlignment.start,
              children: [
                Icon(_glyphOf(pane.kind), size: 18, color: PhoneTokens.kind),
                const Spacer(),
                Text(
                  pane.title,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(color: PhoneTokens.title, fontSize: 14),
                ),
                Text(
                  pane.kind,
                  maxLines: 1,
                  overflow: TextOverflow.ellipsis,
                  style: const TextStyle(color: PhoneTokens.kind, fontSize: 12),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// The name of the card's semantic action, and the header of the list it opens. One string, because a screen reader that
/// offers one thing and a list that says another is two controls.
const String placesAction = 'Move to…';

/// The slot a lifted card would land in, shaded and named — *this* is where it goes.
final class _Slot extends StatelessWidget {
  const _Slot();

  @override
  Widget build(BuildContext context) {
    return DecoratedBox(
      decoration: BoxDecoration(
        color: PhoneTokens.slotFill,
        borderRadius: const BorderRadius.all(PhoneTokens.corner),
        border: Border.all(color: PhoneTokens.slotEdge, width: PhoneTokens.ringWidth),
      ),
    );
  }
}

/// A gutter: a grip on the line two panes meet on. **The bar is the phone's own look** — the wall has no such control,
/// and how wide a gutter is drawn and grabbed is a design choice (`mobile-ui.md`, *the gutter*).
final class _GutterBar extends StatelessWidget {
  const _GutterBar({required this.gutter, this.held = false});

  final Gutter gutter;
  final bool held;

  @override
  Widget build(BuildContext context) {
    final vertical = gutter.split == Split.side;
    // The bar sits on the middle of its line, and computed from the split's own rectangle — the border's extent, which
    // is what `cells.dart` gives, not the canvas's.
    final middle = vertical
        ? (gutter.rect.y + gutter.rect.bottom) / 2.0
        : (gutter.rect.x + gutter.rect.right) / 2.0;
    return Positioned(
      left: vertical
          ? gutter.at - PhoneTokens.gutterBar.width / 2.0
          : middle - PhoneTokens.gutterBar.width / 2.0,
      top: vertical
          ? middle - PhoneTokens.gutterBar.height / 2.0
          : gutter.at - PhoneTokens.gutterBar.height / 2.0,
      width: vertical
          ? PhoneTokens.gutterBar.width
          : PhoneTokens.gutterBar.height,
      height: vertical
          ? PhoneTokens.gutterBar.height
          : PhoneTokens.gutterBar.width,
      child: DecoratedBox(
        decoration: BoxDecoration(
          color: held ? PhoneTokens.gutterHeld : PhoneTokens.gutterInk,
          borderRadius: const BorderRadius.all(PhoneTokens.corner),
        ),
      ),
    );
  }
}

/// **Move to…** — a pane's slots, as words (`mobile-ui.md`, *accessibility*).
///
/// Five rows for every other pane on the canvas, in the wall's own order of edges, and a cancel. **The list is a pane's
/// slots and not the canvas's regions**: `pane.drop` names a slot *of a pane*, so "top right" of the canvas is not a
/// name this vocabulary has, and a list that offered it would be offering two intents with one press.
final class _Places extends StatelessWidget {
  const _Places({required this.controller, required this.pane});

  final OverviewController controller;
  final int pane;

  @override
  Widget build(BuildContext context) {
    final wall = controller.wall;
    final workspace = wall == null ? null : activeWorkspace(wall);
    if (wall == null || workspace == null) {
      return const SizedBox.shrink();
    }
    final named = [
      for (final candidate in workspace.panes)
        if (candidate.id == pane) candidate,
    ];
    if (named.isEmpty) {
      return const SizedBox.shrink();
    }
    final moved = named.first;
    final others = [
      for (final candidate in workspace.panes)
        if (candidate.id != pane) candidate,
    ];
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          // The note's own header for the list (`docs/mobile-ui.md`, *accessibility*: `Move "North"…`), while the
          // card's action is named after the control: **Move to…**.
          'Move "${moved.title}"…',
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: const TextStyle(color: PhoneTokens.title, fontSize: 14),
        ),
        Expanded(
          child: ListView(
            children: [
              for (final row in placesFor(workspace, moved.id))
                _PlaceRow(
                  words: placeWords(row.edge, titleOf(others, row.onto)),
                  onTap: () => controller.dropAt(row),
                ),
              _PlaceRow(words: 'cancel', onTap: controller.closePlaces),
            ],
          ),
        ),
      ],
    );
  }
}

/// The title a target's slot is named after: a row says `left half of "Feed"`, and a pane the projection does not hold
/// is named by its id rather than by nothing — the wall gives every pane a title (#83), and a list row with no name
/// would be two rows nobody can tell apart.
String titleOf(List<Pane> panes, int id) {
  for (final pane in panes) {
    if (pane.id == id) {
      return pane.title;
    }
  }
  return 'pane $id';
}

/// One row of the list: a press, its words, and nothing else — the intent is the controller's.
final class _PlaceRow extends StatelessWidget {
  const _PlaceRow({required this.words, required this.onTap});

  final String words;
  final VoidCallback onTap;

  @override
  Widget build(BuildContext context) {
    return Semantics(
      button: true,
      label: words,
      excludeSemantics: true,
      child: GestureDetector(
        onTap: onTap,
        behavior: HitTestBehavior.opaque,
        child: Container(
          height: PhoneTokens.placeHeight,
          alignment: Alignment.centerLeft,
          padding: PhoneTokens.placePadding,
          child: Text(
            words,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(color: PhoneTokens.title, fontSize: 14),
          ),
        ),
      ),
    );
  }
}

/// The interrupt: the wall does not speak this app's protocol, so the screen is one page, the wall's words and one
/// button that is not a retry (`docs/mobile-ui.md`, *the four states*).
final class _UpdateScreen extends StatelessWidget {
  const _UpdateScreen({required this.words, this.onUpdate});

  /// The wall's words, **verbatim**: the rules are the wall's and a paraphrase is a second copy of one.
  final String words;
  final VoidCallback? onUpdate;

  @override
  Widget build(BuildContext context) {
    return Column(
      mainAxisAlignment: MainAxisAlignment.center,
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          words,
          textAlign: TextAlign.center,
          style: const TextStyle(color: PhoneTokens.title, fontSize: 18),
        ),
        const SizedBox(height: PhoneTokens.gap),
        const Text(
          'This app is older than this wall. Update it to control the wall.',
          textAlign: TextAlign.center,
          style: TextStyle(color: PhoneTokens.kind, fontSize: 14),
        ),
        const SizedBox(height: PhoneTokens.gap * 2),
        Semantics(
          button: true,
          label: 'Update',
          excludeSemantics: true,
          child: GestureDetector(
            onTap: onUpdate,
            behavior: HitTestBehavior.opaque,
            child: Container(
              height: PhoneTokens.placeHeight,
              alignment: Alignment.center,
              decoration: const BoxDecoration(
                color: PhoneTokens.chipActive,
                borderRadius: BorderRadius.all(PhoneTokens.corner),
              ),
              child: const Text(
                'Update',
                style: TextStyle(color: PhoneTokens.title, fontSize: 15),
              ),
            ),
          ),
        ),
      ],
    );
  }
}

/// The kind as a glyph from the framework's own catalogue (`mobile-ui.md`'s rule: the glyph is never the only word —
/// the kind is drawn beside it).
IconData _glyphOf(String kind) => switch (kind) {
      'map' => Icons.map_outlined,
      'newsroom' => Icons.article_outlined,
      'tree' => Icons.account_tree_outlined,
      // A kind this build does not know still draws a card, and its word is still what the wall sent: the phone shows
      // the arrangement it was given rather than hiding a pane it cannot name.
      _ => Icons.crop_square,
    };

/// The state line: what the phone actually knows, and nothing dressed up as more.
///
/// `null` is **silence**, and on a live connection silence is the phone saying *this is the wall* (`mobile-ui.md`).
final class _StateLine extends StatelessWidget {
  const _StateLine({required this.controller});

  final OverviewController controller;

  @override
  Widget build(BuildContext context) {
    final sentence = controller.sentence;
    return SizedBox(
      height: PhoneTokens.stateHeight,
      child: Align(
        alignment: Alignment.centerLeft,
        child: Text(
          sentence ?? '',
          style: TextStyle(color: _colour, fontSize: 12),
        ),
      ),
    );
  }

  Color get _colour => switch (controller.state) {
        PhoneState.live || PhoneState.connecting => PhoneTokens.state,
        PhoneState.notConnected || PhoneState.updateRequired => PhoneTokens.warning,
      };
}

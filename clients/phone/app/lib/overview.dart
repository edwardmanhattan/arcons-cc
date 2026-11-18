/// **The overview** — the wall's arrangement as a diagram you can press (`docs/mobile-ui.md`, #255).
///
/// Three bands, and each is a value the phone *read* rather than something it remembers: the chips from
/// `active` and `workspaces[].name`, the canvas from `workspaces[].tiling` and `panes[]`, the state line from the last
/// read's outcome. Nothing here asks the wall for a window size or a camera, and nothing is scaled from the wall's
/// pixels — a `ratio` is a share, so the same tree is the same shape on a phone and on a panel (#220).
library;

import 'package:arcons_phone/cells.dart';
import 'package:arcons_phone/wire.dart';
import 'package:material_ui/material_ui.dart';

import 'theme.dart';

/// **What the phone knows about the wall it is showing**, and never more than that (`mobile-ui.md`'s four states).
enum PhoneState {
  /// The last read landed and the connection is up.
  live,

  /// A connection is being made, and nothing has been read yet — a stale diagram labelled current is the failure these
  /// four states exist to forbid.
  connecting,

  /// Nothing is being shown because nothing was read.
  notConnected;

  String get words => switch (this) {
    PhoneState.live => 'live',
    PhoneState.connecting => 'connecting',
    PhoneState.notConnected => 'not connected',
  };

  Color get colour => switch (this) {
    PhoneState.live => PhoneTokens.state,
    PhoneState.connecting => PhoneTokens.state,
    PhoneState.notConnected => PhoneTokens.warning,
  };
}

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
/// picture and whose speech could disagree.
List<LaidCard> layOut(Wall wall, Size size) {
  if (wall.active < 0 || wall.active >= wall.workspaces.length) {
    return const [];
  }
  final workspace = wall.workspaces[wall.active];
  final tiling = workspace.tiling;
  if (tiling == null) {
    return const [];
  }

  final byId = {for (final pane in workspace.panes) pane.id: pane};
  final laid = <LaidCard>[];
  for (final cell in rects(tiling, CellRect(0, 0, size.width, size.height))) {
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
  if (wall.active < 0 || wall.active >= wall.workspaces.length) {
    return 'no workspaces';
  }
  final workspace = wall.workspaces[wall.active];
  if (cards.isEmpty) {
    return '${workspace.name}, no panes yet';
  }
  final count = cards.length == 1 ? 'one pane' : '${cards.length} panes';
  return '${workspace.name}, $count: ${cards.map((card) => card.spoken).join('; ')}';
}

/// **The screen.**
final class OverviewScreen extends StatelessWidget {
  const OverviewScreen({super.key, required this.wall, required this.state});

  final Wall wall;
  final PhoneState state;

  @override
  Widget build(BuildContext context) {
    return ColoredBox(
      color: PhoneTokens.canvas,
      child: SafeArea(
        child: Padding(
          padding: PhoneTokens.screen,
          child: Column(
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              _Chips(wall: wall),
              const SizedBox(height: PhoneTokens.gap),
              Expanded(
                child: LayoutBuilder(
                  builder: (context, constraints) => _Canvas(
                    wall: wall,
                    size: Size(constraints.maxWidth, constraints.maxHeight),
                  ),
                ),
              ),
              const SizedBox(height: PhoneTokens.gap),
              _StateLine(state: state, wall: wall),
            ],
          ),
        ),
      ),
    );
  }
}

/// The workspaces, as chips: a press is a switch, and an emptied one is still a place with its name (#141).
final class _Chips extends StatelessWidget {
  const _Chips({required this.wall});

  final Wall wall;

  @override
  Widget build(BuildContext context) {
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

/// The arrangement, drawn where the wall's arithmetic put it.
final class _Canvas extends StatelessWidget {
  const _Canvas({required this.wall, required this.size});

  final Wall wall;
  final Size size;

  @override
  Widget build(BuildContext context) {
    final cards = layOut(wall, size);
    if (cards.isEmpty) {
      final empty =
          wall.active >= 0 &&
          wall.active < wall.workspaces.length &&
          wall.workspaces[wall.active].empty;
      return Center(
        child: Text(
          // The wall's own words for an emptied workspace, so the phone and the wall say the same thing (#141).
          empty ? 'No panes yet.' : 'Nothing to show',
          style: const TextStyle(color: PhoneTokens.kind, fontSize: 14),
        ),
      );
    }

    return Semantics(
      label: spokenCanvas(wall, cards),
      container: true,
      explicitChildNodes: true,
      child: Stack(
        children: [
          for (final card in cards)
            Positioned(
              left: card.rect.x,
              top: card.rect.y,
              width: card.rect.w,
              height: card.rect.h,
              child: _Card(card: card),
            ),
        ],
      ),
    );
  }
}

/// One card: the glyph, the title, the kind in words, and the ring that says this one has the keys.
final class _Card extends StatelessWidget {
  const _Card({required this.card});

  final LaidCard card;

  @override
  Widget build(BuildContext context) {
    final pane = card.pane;
    final ring = pane.focused ? PhoneTokens.ring : PhoneTokens.cardEdge;
    return Semantics(
      button: true,
      selected: pane.focused,
      excludeSemantics: true,
      label: card.spoken,
      child: Container(
        decoration: BoxDecoration(
          color: PhoneTokens.cardFill,
          borderRadius: const BorderRadius.all(PhoneTokens.corner),
          border: Border.all(
            color: ring,
            width: pane.focused ? PhoneTokens.ringWidth : 1,
          ),
        ),
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
final class _StateLine extends StatelessWidget {
  const _StateLine({required this.state, required this.wall});

  final PhoneState state;
  final Wall wall;

  @override
  Widget build(BuildContext context) {
    return SizedBox(
      height: PhoneTokens.stateHeight,
      child: Align(
        alignment: Alignment.centerLeft,
        child: Text(
          // The projection's own version counter is shown only where it is true to show it: a diagram that says
          // `live` without a read is the one thing these states forbid.
          state == PhoneState.live
              ? '${state.words} · wall seq ${wall.seq}'
              : state.words,
          style: TextStyle(color: state.colour, fontSize: 12),
        ),
      ),
    );
  }
}

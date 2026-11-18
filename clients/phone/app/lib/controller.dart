/// **The drag, and the four states, over `Client`** (#257).
///
/// The screen `#255` drew is a picture of a projection; this is the hand on it. `docs/mobile-ui.md` is the
/// specification and this file is the part of it a widget cannot hold:
///
/// | it owns | because |
/// | --- | --- |
/// | the four states | they are *derived* — from a read landing, a read failing, and a refusal whose kind is `version` — and never felt (`mobile-ui.md`, *the four states*) |
/// | the wall | it is what the last read said, and **the only thing that moves a card** (`mobile-remote.md` decision 1) |
/// | the in-flight intent | one release is one intent, and while it is out the canvas takes no second lift |
/// | the lift, the landing and the gutter | computed by `cells.dart` — `dropFor`, `previewDrop`, `gutters`, `previewResize` — and never by arithmetic here |
///
/// **Nothing is optimistic.** A card is drawn where the *preview* puts it only while the hand is holding it or while an
/// intent is out, and the preview is `Layout::apply`'s answer ([previewDrop]); an `ok` answer moves nothing, because an
/// `ok` is not a projection. The next read is what moves a card (`ADR-0006` decision 4).
///
/// **A refusal is not an error state.** `rule` and `input` return the card to the cell the last projection gave it and
/// re-read; `busy` keeps the pending card and retries the **read**, never the change; `version` ends the session. The
/// action comes from [Denied.next] — the same [Next.of] switch `Client` maps a refusal with — so there is one mapping in
/// this package and not two (`docs/mobile-ui.md`, *refusals → what the phone does*).
library;

import 'dart:async';
import 'dart:developer' as developer;

import 'package:arcons_phone/cells.dart';
import 'package:arcons_phone/client.dart';
import 'package:arcons_phone/wire.dart';
import 'package:flutter/foundation.dart';

import 'intents.dart';
import 'theme.dart';

/// **What the phone knows about its connection**, and exactly four things (`mobile-ui.md`, *the four states*).
///
/// `connecting` is not `not connected`: a phone that has never heard from the wall is not showing a stale arrangement,
/// it is showing none.
enum PhoneState {
  /// A connection is being made, and no read has landed on it yet: the chips are absent and the canvas is empty.
  connecting,

  /// The last read landed. Silence is the phone saying *this is the wall*.
  live,

  /// A read did not land: the diagram it last read, dimmed, and the age of that read.
  notConnected,

  /// A refusal whose kind is `version`: one screen, the wall's words, and **nothing more sent in this session**.
  updateRequired,
}

/// **A card the hand has lifted** (`mobile-ui.md`, *the drag*): which pane is in the air, and where it is drawn.
final class Lift {
  const Lift({required this.pane, required this.rect});

  final int pane;

  /// The rectangle the card is drawn in while it is in the air — **centred on the finger**, so that "the card under
  /// the middle of the lifted card is the target" and "the slot under the pointer" are the same point. The wall asks
  /// `drop_for` of a pointer; a phone whose card hung off its finger would be a second answer to where the hand is.
  final CellRect rect;

  @override
  String toString() => 'Lift($pane at ${rect.x}, ${rect.y})';
}

/// **A gutter being dragged**: the border the hand has hold of, and the share the pointer asks for.
final class GutterDrag {
  const GutterDrag({required this.gutter, required this.share});

  final Gutter gutter;

  /// A share the wall accepts — `Numbers.shareAlong` clamps it to `RATIO_RANGE`, so a preview drawn from this can
  /// never name a share `pane.resize` refuses.
  final double share;

  @override
  String toString() => 'GutterDrag($gutter, $share)';
}

/// **The phone's own sentence for a refusal it caused** — an `input` is this app's bug, and the operator is not asked to
/// fix `unknown verb` (`mobile-ui.md`, the refusal table).
const String ownBugSentence = 'This app asked the wall for something it does not understand.';

/// **The phone's own sentence while the wall has not answered** — a `busy` refusal, which is not a refusal at all: the
/// change may have landed.
const String busySentence = 'The wall is not answering.';

/// **What the screen does, and the one place it does it.**
final class OverviewController extends ChangeNotifier {
  OverviewController({
    Client? client,
    DateTime Function()? clock,
    void Function(String message)? log,
    this.retryDelay = const Duration(seconds: 2),
  })  : _client = client ?? Client(),
        _clock = clock ?? DateTime.now,
        _log = log ?? _toTheLog;

  final Client _client;
  final DateTime Function() _clock;

  /// One line for an `input` refusal, and nothing else here logs (`INV-08`: no secret, no host, no token).
  final void Function(String message) _log;

  /// How long a `busy` waits before the read it retries. A test passes `Duration.zero`: no clock it does not control.
  final Duration retryDelay;

  PhoneState _state = PhoneState.connecting;
  Wall? _wall;
  DateTime? _readAt;
  bool _moving = false;
  bool _waiting = false;
  String? _words;
  Tiling? _preview;
  Lift? _lift;
  Drop? _landing;
  GutterDrag? _gutter;
  int? _placesPane;

  /// The four states, derived.
  PhoneState get state => _state;

  /// **The projection the last read gave** — the phone's only truth about the arrangement.
  Wall? get wall => _wall;

  /// The refusal that ended this session, when one did: the update screen's words (`docs/mobile-ui.md`).
  Refusal? get stop => _client.stop;

  /// The workspace the canvas draws, or `null` when there is nothing to draw.
  Workspace? get workspace {
    final wall = _wall;
    if (wall == null || wall.active < 0 || wall.active >= wall.workspaces.length) {
      return null;
    }
    return wall.workspaces[wall.active];
  }

  /// The arrangement **the last projection gave**. Never a preview: this is what a refusal returns a card to.
  Tiling? get projected => workspace?.tiling;

  /// **The arrangement the canvas draws.** The preview while the hand holds a card over a slot, or a gutter is being
  /// dragged, or an intent is out (`moving…`) — and otherwise the projection's own tiling. This is the only getter a
  /// canvas asks, so "nothing is optimistic" is one expression rather than a habit.
  Tiling? get drawn => _preview ?? projected;

  /// Whether an intent is out and its projection has not landed: `mobile-ui.md`'s `moving…`.
  bool get moving => _moving;

  /// The card in the air, if the hand is holding one.
  Lift? get lifted => _lift;

  /// The slot the lifted card would land in, if any: `dropFor`'s answer at the pointer.
  Drop? get landing => _landing;

  /// The gutter the hand has hold of, if any.
  GutterDrag? get gutter => _gutter;

  /// The pane whose card the canvas does not draw, because the hand has it.
  int? get liftedPane => _lift?.pane;

  /// Whether the lifted pane's slot is shaded — the landing is drawn before release, and the slot is shaded rather
  /// than a copy of the card, so the operator can see the place and not just the thing (`mobile-ui.md`).
  bool get slotShaded => _lift != null && _landing != null;

  /// The pane whose **Move to…** places are listed, or `null`.
  int? get placesPane => _placesPane;

  /// When the last read landed, or `null` if none has on this connection.
  DateTime? get readAt => _readAt;

  /// **What the state line says**, or `null` for silence — which is the phone saying *this is the wall*
  /// (`mobile-ui.md`, the four states).
  String? get sentence => switch (_state) {
        PhoneState.connecting => 'connecting…',
        PhoneState.notConnected => _notConnectedWords,
        PhoneState.live => _waiting
            ? busySentence
            : (_moving ? 'moving…' : _words),
        // The update screen carries the wall's words itself; a line under it would be a second sentence.
        PhoneState.updateRequired => null,
      };

  String get _notConnectedWords {
    final readAt = _readAt;
    if (readAt == null) {
      return 'not connected';
    }
    return 'not connected · last read ${_ageWords(_clock().difference(readAt))} ago';
  }

  /// An age a person can act on, which is what `not connected` reports and a sequence is not (`mobile-ui.md`).
  static String _ageWords(Duration age) {
    if (age.inSeconds < 60) {
      return '${age.inSeconds} s';
    }
    if (age.inMinutes < 60) {
      return '${age.inMinutes} min';
    }
    return '${age.inHours} h';
  }

  /// **Connect, then read** — the first line a session sends is the read (`mobile-remote.md` decision 1). A new
  /// connection knows nothing: whatever the last one read is gone.
  Future<void> connect(Carrier carrier) async {
    _state = PhoneState.connecting;
    _wall = null;
    _readAt = null;
    _words = null;
    _preview = null;
    _moving = false;
    _waiting = false;
    _forgetTheGesture();
    notifyListeners();
    await _client.connect(carrier);
    await read();
  }

  /// **Read the wall**, and let the read decide the state. Nothing is ever replayed: a read is how the phone learns
  /// what happened, and the only retry this file knows.
  Future<void> read() async {
    if (_state == PhoneState.updateRequired) {
      return;
    }
    try {
      final wall = await _client.read();
      _wall = wall;
      _state = PhoneState.live;
      _readAt = _clock();
      _preview = null;
      _moving = false;
      _waiting = false;
    } on Refusal catch (refusal) {
      // The only refusal a read can provoke in this build is a version, and it ends the session (#198).
      _state = refusal.next == Next.update ? PhoneState.updateRequired : PhoneState.notConnected;
      _words = refusal.why;
      _preview = null;
      _moving = false;
      _waiting = false;
    } on Denied {
      // No socket, no read on it, or a line this build cannot read: the wall is not there, and the projection it had
      // is gone with the connection.
      _state = PhoneState.notConnected;
      _preview = null;
      _moving = false;
      _waiting = false;
    }
    _forgetTheGesture();
    notifyListeners();
  }

  /// **A card is pressed**: `pane.focus`, and the ring moves when the next projection says it moved.
  Future<void> focusPane(int pane) => _send(
        'pane.focus',
        {'pane': pane},
        preview: null,
      );

  /// **A long press lifts a card.** The hold is the phone's; nothing is on the wire yet, and while an intent is out the
  /// canvas takes no second lift — two drops in flight are two answers to "where is that pane".
  ///
  /// `false` means no card came up: the point is over no pane, the phone has not read on this connection, or a card is
  /// already in the air or moving.
  bool lift({required double x, required double y, required CellRect canvas}) {
    if (_state != PhoneState.live || _moving || _lift != null) {
      return false;
    }
    final tiling = projected;
    if (tiling == null) {
      return false;
    }
    final cell = paneAt(cells(tiling, canvas), x: x, y: y);
    if (cell == null) {
      return false;
    }
    final size = _drawnRect(tiling, canvas, cell.pane) ?? cell.rect;
    _lift = Lift(pane: cell.pane, rect: _centred(size, x, y));
    _landing = null;
    _preview = null;
    _words = null;
    notifyListeners();
    return true;
  }

  /// **The card follows the finger and the landing is drawn before release**, computed the wall's way: `dropFor` on
  /// the cells the last projection gave, so the picture of the landing is the landing.
  void hold({required double x, required double y, required CellRect canvas}) {
    final lift = _lift;
    final tiling = projected;
    if (lift == null || tiling == null) {
      return;
    }
    final size = _drawnRect(tiling, canvas, lift.pane) ?? lift.rect;
    _lift = Lift(pane: lift.pane, rect: _centred(size, x, y));
    final landing = dropFor(cells(tiling, canvas), dragged: lift.pane, x: x, y: y);
    final preview = landing == null ? null : previewDrop(tiling, dragged: lift.pane, drop: landing);
    // A landing with no preview is not a landing: `previewDrop` answers `null` where `Layout::apply` would refuse, and a
    // release the wall refuses is not one the phone draws.
    _landing = preview == null ? null : landing;
    _preview = preview;
    notifyListeners();
  }

  /// **A release sends one intent** — or, over the lifted card's own cell, nothing at all: `dropFor` answers no drop
  /// there, which is what makes "released where it started" a no-op on the wall too (`mobile-ui.md`, *cancel is a
  /// place, not a button*).
  Future<void> release({required double x, required double y, required CellRect canvas}) async {
    final lift = _lift;
    if (lift == null) {
      return;
    }
    hold(x: x, y: y, canvas: canvas);
    final landing = _landing;
    final preview = _preview;
    if (landing == null || preview == null) {
      cancel();
      return;
    }
    _lift = null;
    _landing = null;
    await _send(
      DropIntent.verb,
      DropIntent.of(lift.pane, landing).fields,
      preview: preview,
    );
  }

  /// The system's back gesture, and letting go where nothing is: the card settles back, **nothing is sent**, and the
  /// screen says nothing, because nothing happened.
  void cancel() {
    if (_lift == null) {
      return;
    }
    _forgetTheGesture();
    notifyListeners();
  }

  /// **A row of `Move to…`** — the same `pane.drop` the hand sends, through the same door ([_send]). The list is a
  /// pane's slots and not the canvas's regions (`mobile-ui.md`, *accessibility*).
  Future<void> dropAt(DropIntent intent) async {
    _placesPane = null;
    final tiling = projected;
    if (tiling == null) {
      notifyListeners();
      return;
    }
    await _send(
      DropIntent.verb,
      intent.fields,
      preview: previewDrop(tiling, dragged: intent.pane, drop: intent.drop),
    );
  }

  /// **A gutter's grip is taken** — `gutterAt` on the walk `cells.dart` gives, or nothing.
  bool grabGutter({required double x, required double y, required CellRect canvas}) {
    if (_state != PhoneState.live || _moving || _lift != null || _gutter != null) {
      return false;
    }
    final tiling = projected;
    if (tiling == null) {
      return false;
    }
    final found = gutterAt(gutters(tiling, canvas), x: x, y: y, grip: PhoneTokens.gutterGrip);
    if (found == null) {
      return false;
    }
    _gutter = GutterDrag(
      gutter: found,
      share: found.shareAt((x: x, y: y), Numbers.wall),
    );
    _words = null;
    notifyListeners();
    return true;
  }

  /// **The share follows the pointer, inside `RATIO_RANGE`** — the preview is `previewResize`, so the picture is the
  /// wall's `set_ratio` and never a number a widget computed.
  void dragGutter({required double x, required double y}) {
    final drag = _gutter;
    final tiling = projected;
    if (drag == null || tiling == null) {
      return;
    }
    final share = drag.gutter.shareAt((x: x, y: y), Numbers.wall);
    _gutter = GutterDrag(gutter: drag.gutter, share: share);
    _preview = previewResize(
      tiling,
      pane: drag.gutter.a,
      other: drag.gutter.b,
      share: share,
    );
    notifyListeners();
  }

  /// **A gutter release sends one `pane.resize`** — the pair the border separates and the share the preview showed.
  Future<void> releaseGutter() async {
    final drag = _gutter;
    final preview = _preview;
    if (drag == null) {
      return;
    }
    _gutter = null;
    await _send(
      ResizeIntent.verb,
      ResizeIntent(pane: drag.gutter.a, other: drag.gutter.b, ratio: drag.share).fields,
      preview: preview,
    );
  }

  /// Let go of a gutter without asking for anything: the tiling goes back to the projection's.
  void cancelGutter() {
    if (_gutter == null) {
      return;
    }
    _gutter = null;
    _preview = null;
    notifyListeners();
  }

  /// **Move to…** — the list of places for [pane], which is the drag's accessible half.
  void openPlaces(int pane) {
    if (_state != PhoneState.live || _moving) {
      return;
    }
    _placesPane = pane;
    _words = null;
    notifyListeners();
  }

  void closePlaces() {
    if (_placesPane == null) {
      return;
    }
    _placesPane = null;
    notifyListeners();
  }

  /// **One intent, once.** The only place this controller puts a change on the wire, so "one release is one intent",
  /// "nothing is sent before the read" and "no retry of the change" are one function rather than three habits.
  ///
  /// [preview] is what the canvas draws until a projection lands: the card in the slot the release asked for, under
  /// `moving…`.
  Future<void> _send(
    String verb,
    Map<String, Object?> fields, {
    required Tiling? preview,
  }) async {
    if (_moving || _state == PhoneState.updateRequired || _state != PhoneState.live) {
      return;
    }
    _moving = true;
    _waiting = false;
    _preview = preview;
    _lift = null;
    _landing = null;
    _gutter = null;
    _placesPane = null;
    _words = null;
    notifyListeners();
    try {
      await _client.act(verb, fields);
    } on Refusal catch (refusal) {
      await _refused(refusal);
      return;
    } on Denied {
      // The wall is not there: no socket, no read on this one, or an answer this build cannot read.
      _state = PhoneState.notConnected;
      _preview = null;
      _moving = false;
      notifyListeners();
      return;
    }
    // **An `ok` is not a projection.** The card moves when the read lands, and not before.
    await read();
  }

  /// `docs/mobile-ui.md`'s refusal table, one arm per kind and no default: a fifth kind is a compile error here rather
  /// than a screen with no answer (`INV-07`).
  Future<void> _refused(Refusal refusal) async {
    // What the canvas was drawing while the intent was out: `busy` keeps it, every other kind gives it back.
    final pending = _preview;
    _lift = null;
    _landing = null;
    _gutter = null;
    switch (refusal.next) {
      case Next.reread:
        // The premise moved. The card is back in the cell the last projection gave it, the wall's words are on the
        // line, and the phone re-reads — it never sends the same intent again.
        _moving = false;
        _preview = null;
        _words = refusal.why;
        notifyListeners();
        await read();
      case Next.ownBug:
        // This is the phone's bug, and an operator is not asked to fix `unknown verb`. One line, with no secret and no
        // host (`INV-08`), then a read.
        _moving = false;
        _preview = null;
        _words = ownBugSentence;
        _log('phone: the wall refused the intent this app sent — ${refusal.why} (${refusal.kind.word})');
        notifyListeners();
        await read();
      case Next.update:
        // Nothing more is sent in this session: `Client` has stopped, and the screen is one page with one button.
        _moving = false;
        _preview = null;
        _state = PhoneState.updateRequired;
        _words = refusal.why;
        notifyListeners();
      case Next.retryRead:
        // **`busy` is not a refusal**: the change may have landed. Keep the pending card *and* the fact that the phone
        // is still asking (`moving`), say that the wall has not answered, wait, and retry the read.
        _preview = pending;
        _waiting = true;
        notifyListeners();
        await Future<void>.delayed(retryDelay);
        if (_state == PhoneState.updateRequired) {
          return;
        }
        await read();
    }
  }

  /// Nothing is held and nothing is previewed: the canvas goes back to the projection's own arrangement.
  void _forgetTheGesture() {
    _lift = null;
    _landing = null;
    _gutter = null;
    _placesPane = null;
  }
}

/// A card centred on a point, at the size the projection draws it — the lifted card's own rectangle.
CellRect _centred(CellRect size, double x, double y) =>
    CellRect(x - size.w / 2.0, y - size.h / 2.0, size.w, size.h);

/// The rectangle a pane's card is drawn in, from the walls' own walk (`view::panes_on`), or `null` when the arrangement
/// does not hold it.
CellRect? _drawnRect(Tiling tiling, CellRect canvas, int pane) {
  for (final cell in rects(tiling, canvas)) {
    if (cell.pane == pane) {
      return cell.rect;
    }
  }
  return null;
}

/// The one line an `input` refusal writes. It names the wall's words and the category, and **never a field**: a token, a
/// host or a device key in a log line is `INV-08` broken at rest.
void _toTheLog(String message) => developer.log(message, name: 'arcons-phone');

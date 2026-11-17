/// **The wire, seen from the phone's side** (#217, ADR-0008).
///
/// The wall's encoder is `src/control/wire.rs` and the reference is `docs/control.md`; neither is duplicated here.
/// What this file is held to is the recording: `fixtures/control/session.jsonl`, replayed line by line by
/// `test/corpus_test.dart`. A field named here that the wall does not send is a red test, and a shape the corpus grows
/// is a red test until it is typed — which is the only way a client in another language stays honest.
///
/// **Three answers, and one that must not exist.** A request's success is `{"ok": true, "changed": …}`, a read's is a
/// projection under `"wall"`, and a no is `{"ok": false, "why": …, "kind": …}` — one of those, never two. The other two
/// shapes `docs/control.md` defines — a granted watch and a published projection — belong to a carrier that carries a
/// watch, and the recording holds neither yet (the unix socket refuses one by name). They arrive with the relay's
/// fixtures, and this file will refuse to parse them until that ticket types them, which is deliberate.
library;

import 'dart:convert';

/// The protocol a client speaks. Compared, never negotiated: the wall refuses another version by name, so a client
/// that guessed would be a wall two clients disagree about (`docs/control.md`).
///
/// The number itself is `PROTOCOL` in `src/control.rs`, and `cargo test --test doc_claims` holds ADR-0008 to it.
const int protocol = 1;

/// A line this client cannot read. Never caught to be ignored: every one of them is either the wall saying something
/// this build does not know (update it) or this build sending something the wall never defined (a bug in the phone).
final class WireException implements Exception {
  const WireException(this.why);

  final String why;

  @override
  String toString() => 'wire: $why';
}

/// **What the wall answered.** One of the three, and the phone acts on the type, never on prose.
sealed class Answer {
  const Answer();
}

/// **The wall changed** — `{"ok": true, "changed": true | false}`.
///
/// `changed: false` is not a failure: the client asked for a state the wall was already in (`docs/control.md`).
final class Changed extends Answer {
  const Changed(this.changed);

  final bool changed;
}

/// **A projection** — the body of `{"v": 1, "read": "wall"}`, and the phone's only truth about the arrangement
/// (`mobile-remote.md` decision 1: it reads, it does not remember).
final class Projection extends Answer {
  const Projection(this.wall);

  final Wall wall;
}

/// **The wall said no** — its words in [why], its category in [kind], and the category is what the screen acts on.
final class Refused extends Answer {
  const Refused(this.why, this.kind);

  final String why;
  final RefusalKind kind;
}

/// The four categories a refusal carries (`RefusalKind` in `src/control.rs`, #198).
enum RefusalKind {
  /// Understood and refused. The premise moved; sending it again changes nothing.
  rule('rule'),

  /// This client speaks a version the wall does not. Send nothing more: update the app.
  version('version'),

  /// The request was malformed. This is the phone's own bug.
  input('input'),

  /// The wall did not answer now. The change may have landed — retry the *read*, never the change.
  busy('busy');

  const RefusalKind(this.word);

  /// The word on the wire and in `docs/control.md`.
  final String word;

  /// The kind a refusal's word names.
  ///
  /// A word this client does not know is a [WireException] and never a default: `INV-07` says a new refusal must be a
  /// visible branch, and a client that guessed a category would act on the wrong one.
  static RefusalKind ofWord(Object? word) => switch (word) {
        'rule' => RefusalKind.rule,
        'version' => RefusalKind.version,
        'input' => RefusalKind.input,
        'busy' => RefusalKind.busy,
        _ => throw WireException('refusal kind ${jsonEncode(word)} is not one this client knows'),
      };
}

/// **What the phone does next**, from `docs/mobile-ui.md`'s refusal table — and the switch below has no default arm,
/// so a fifth kind is a compile error here rather than a screen with no answer.
enum Next {
  /// `rule`: return the card to the cell the last projection gave it, then re-read. Never the same intent again.
  reread,

  /// `input`: the phone asked for something the wall does not understand, which is this app's bug — re-read, restore
  /// the cell, one log line, and an operator is not asked to fix it.
  ownBug,

  /// `version`: the update screen, and nothing more in this session.
  update,

  /// `busy`: keep the diagram and the pending card, wait, and retry the **read** first.
  retryRead;

  /// The action `docs/mobile-ui.md` gives this kind of refusal.
  static Next of(RefusalKind kind) => switch (kind) {
        RefusalKind.rule => Next.reread,
        RefusalKind.input => Next.ownBug,
        RefusalKind.version => Next.update,
        RefusalKind.busy => Next.retryRead,
      };
}

/// **The arrangement**, as the wall says it is: `active`, the pane with the keys, the maximised one, and the `seq`
/// that says how old this is (#181, #218, #221).
final class Wall {
  const Wall({
    required this.active,
    required this.focused,
    required this.maximised,
    required this.seq,
    required this.workspaces,
  });

  final int active;
  final int? focused;
  final int? maximised;

  /// The projection's version counter: the phone compares it rather than assuming an order (#221).
  final int seq;
  final List<Workspace> workspaces;

  factory Wall.from(Object? value) {
    final object = _asMap(value, 'the wall');
    return Wall(
      active: _asInt(object['active'], '"active"'),
      focused: _asIntOrNull(object['focused'], '"focused"'),
      maximised: _asIntOrNull(object['maximised'], '"maximised"'),
      seq: _asInt(object['seq'], '"seq"'),
      workspaces: [
        for (final workspace in _asList(object['workspaces'], '"workspaces"'))
          Workspace.from(workspace),
      ],
    );
  }
}

/// One workspace: a place, with its name, its panes and its arrangement.
final class Workspace {
  const Workspace({
    required this.name,
    required this.empty,
    required this.focused,
    required this.panes,
    required this.tiling,
  });

  final String name;

  /// An emptied workspace keeps its chip and its name (#141) — it is a place, not a gap.
  final bool empty;
  final int? focused;
  final List<Pane> panes;

  /// `null` exactly when [empty] is true: an empty workspace has no arrangement to draw.
  final Tiling? tiling;

  factory Workspace.from(Object? value) {
    final object = _asMap(value, 'a workspace');
    final empty = _asBool(object['empty'], '"empty"');
    final tiling = object['tiling'];
    if (empty != (tiling == null)) {
      throw WireException(
        'a workspace is empty: $empty with a tiling: ${tiling != null} — '
        'an empty workspace has no arrangement and a drawn one is not empty',
      );
    }
    return Workspace(
      name: _asString(object['name'], '"name"'),
      empty: empty,
      focused: _asIntOrNull(object['focused'], '"focused"'),
      panes: [for (final pane in _asList(object['panes'], '"panes"')) Pane.from(pane)],
      tiling: tiling == null ? null : Tiling.from(tiling),
    );
  }
}

/// One pane: what the card says, and the id every leaf of the tiling names.
final class Pane {
  const Pane({
    required this.id,
    required this.kind,
    required this.title,
    required this.focused,
    required this.selected,
    required this.camera,
  });

  final int id;

  /// `map`, `newsroom` or `tree` — the three kinds the wall draws.
  final String kind;
  final String title;
  final bool focused;

  /// The hull a pane's card is about, by id: naming it is not drawing it (`mobile-ui.md`).
  final int? selected;

  /// A map's camera. Present for a map pane, `null` otherwise.
  final Camera? camera;

  factory Pane.from(Object? value) {
    final object = _asMap(value, 'a pane');
    return Pane(
      id: _asInt(object['id'], '"id"'),
      kind: _asString(object['kind'], '"kind"'),
      title: _asString(object['title'], '"title"'),
      focused: _asBool(object['focused'], '"focused"'),
      selected: _asIntOrNull(object['selected'], '"selected"'),
      camera: object['camera'] == null ? null : Camera.from(object['camera']),
    );
  }
}

/// A map pane's camera: a value a map client sends, not a fact about an arrangement.
final class Camera {
  const Camera({required this.lat, required this.lon, required this.zoom});

  final double lat;
  final double lon;
  final double zoom;

  factory Camera.from(Object? value) {
    final object = _asMap(value, 'a camera');
    return Camera(
      lat: _asDouble(object['lat'], '"lat"'),
      lon: _asDouble(object['lon'], '"lon"'),
      zoom: _asDouble(object['zoom'], '"zoom"'),
    );
  }
}

/// **The arrangement, as a tree of shares** (#218, #220).
///
/// A leaf is a **pane id**, not an index into `panes`: a phone holds a projection across a close, and after a
/// `pane.close` an index would name a different pane — the right arrangement of the wrong panes.
sealed class Tiling {
  const Tiling();

  factory Tiling.from(Object? value) {
    final object = _asMap(value, 'a tiling');
    final split = object['split'];
    if (split == null) {
      return TilingLeaf(_asInt(object['pane'], 'a tiling leaf\'s "pane"'));
    }
    return TilingSplit(
      Tiling.from(object['a']),
      Tiling.from(object['b']),
      _asDouble(object['ratio'], '"ratio"'),
      Split.ofWord(split),
    );
  }

  /// Every pane id this arrangement names, in the order a reader hears them: `a` before `b`, at every split.
  List<int> get leaves;
}

/// One pane's cell, at some depth of the tree.
final class TilingLeaf extends Tiling {
  const TilingLeaf(this.pane);

  final int pane;

  @override
  List<int> get leaves => [pane];
}

/// Two cells and a share: [ratio] is `a`'s share of this cell, so the shape is the same on a 360 pt phone and in a
/// 1280 pt panel (#220).
final class TilingSplit extends Tiling {
  const TilingSplit(this.a, this.b, this.ratio, this.split);

  final Tiling a;
  final Tiling b;
  final double ratio;
  final Split split;

  @override
  List<int> get leaves => [...a.leaves, ...b.leaves];
}

/// The two ways a cell divides: `side` is left/right, `stack` is top/bottom.
enum Split {
  side('side'),
  stack('stack');

  const Split(this.word);

  final String word;

  static Split ofWord(Object? word) => switch (word) {
        'side' => Split.side,
        'stack' => Split.stack,
        _ => throw WireException('split ${jsonEncode(word)} is neither "side" nor "stack"'),
      };
}

/// **Read one answer line** — the phone's side of `wire::request`'s discipline: one shape, no defaults.
///
/// Throws [WireException] for a line this client cannot act on, and never returns a guess.
Answer decodeAnswer(String line) {
  final Object? value;
  try {
    value = jsonDecode(line);
  } on FormatException {
    throw const WireException('not JSON');
  }
  final object = _asMap(value, 'an answer');

  final version = object['v'];
  if (version != protocol) {
    throw WireException(
      'an answer at version ${jsonEncode(version)}, and this client speaks $protocol — '
      'this is a wall to update against, not a line to guess at',
    );
  }

  final ok = _asBool(object['ok'], '"ok"');
  if (ok) {
    if (object.containsKey('changed')) {
      return Changed(_asBool(object['changed'], '"changed"'));
    }
    if (object.containsKey('wall')) {
      return Projection(Wall.from(object['wall']));
    }
    throw const WireException(
      'an "ok" answer that carries neither "changed" nor "wall": the protocol has no third shape for it yet',
    );
  }

  return Refused(
    _asString(object['why'], '"why"'),
    RefusalKind.ofWord(object['kind']),
  );
}

Map<String, Object?> _asMap(Object? value, String what) {
  if (value is Map<String, Object?>) {
    return value;
  }
  throw WireException('$what is not a JSON object');
}

List<Object?> _asList(Object? value, String what) {
  if (value is List<Object?>) {
    return value;
  }
  throw WireException('$what is not a JSON array');
}

int _asInt(Object? value, String what) {
  if (value is int) {
    return value;
  }
  throw WireException('$what is not an integer');
}

int? _asIntOrNull(Object? value, String what) => value == null ? null : _asInt(value, what);

double _asDouble(Object? value, String what) {
  if (value is num) {
    return value.toDouble();
  }
  throw WireException('$what is not a number');
}

String _asString(Object? value, String what) {
  if (value is String) {
    return value;
  }
  throw WireException('$what is not a string');
}

bool _asBool(Object? value, String what) {
  if (value is bool) {
    return value;
  }
  throw WireException('$what is not a boolean');
}

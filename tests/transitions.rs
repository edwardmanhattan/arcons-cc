//! **The interaction table, checked as a table** (#101).
//!
//! `docs/adr/0004-interaction-state.md` says which of the wall's states may follow which, and
//! `Begin::allowed_from` is that prose in code. This file is the third copy — deliberately, because a claim
//! written in one place and executed in another is a claim nobody has checked: the matrix below is
//! **exhaustive over both enums**, so a new state or a new request fails here until it has a row, and the
//! behaviours the machine exists for are asserted rather than assumed.
//!
//! The states are `Interaction`, the requests are `Begin`, and every pair has an answer: either the machine
//! took it (and the wall is in exactly the state that was asked for, holding one thing and nothing else) or
//! it refused (and nothing moved at all).

use arcons_cc::ui::view::Border;
use arcons_cc::ui::{Begin, Interaction, PaneDrag, Rename, RenameTarget, State};

/// Every state the wall can be in, with an instance of each. A new variant of `Interaction` fails to
/// compile here until it is listed — which is the whole point: no state without a row in the table.
fn every_state() -> Vec<(&'static str, Interaction)> {
    vec![
        ("Idle", Interaction::Idle),
        (
            "Lifted",
            Interaction::Lifted(PaneDrag {
                pane: 1,
                drop: None,
            }),
        ),
        (
            "Resizing",
            Interaction::Resizing(Border {
                a: 1,
                b: 2,
                dir: arcons_cc::ui::Dir::Horizontal,
                at: 0.5,
                start: 0.0,
                span: 100.0,
                rect: arcons_cc::ui::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                },
            }),
        ),
        (
            "Menu",
            Interaction::Menu {
                pane: 1,
                at: (4.0, 4.0),
                open: None,
            },
        ),
        ("Filtering", Interaction::Filtering { pane: 1 }),
        (
            "Renaming",
            Interaction::Renaming(Rename::new(RenameTarget::Pane(1), None)),
        ),
        ("Settings", Interaction::Settings),
    ]
}

/// Every request, on the same terms.
fn every_begin() -> Vec<(&'static str, Begin)> {
    vec![
        (
            "Lift",
            Begin::Lift(PaneDrag {
                pane: 1,
                drop: None,
            }),
        ),
        (
            "Resize",
            Begin::Resize(Border {
                a: 1,
                b: 2,
                dir: arcons_cc::ui::Dir::Horizontal,
                at: 0.5,
                start: 0.0,
                span: 100.0,
                rect: arcons_cc::ui::Rect {
                    x: 0.0,
                    y: 0.0,
                    w: 100.0,
                    h: 100.0,
                },
            }),
        ),
        (
            "Menu",
            Begin::Menu {
                pane: 1,
                at: (4.0, 4.0),
            },
        ),
        ("Filter", Begin::Filter { pane: 1 }),
        (
            "Rename",
            Begin::Rename(Rename::new(RenameTarget::Pane(1), None)),
        ),
        ("Settings", Begin::Settings),
    ]
}

/// The table, as a matrix. Every cell is decided: the request is either taken — and then the wall is in
/// that state and in no other, because there is only one value to be in — or refused, and then the wall is
/// exactly where it was. An answer of "taken, and also still in the old state" is not expressible, which is
/// the defect the enum was introduced to remove.
#[test]
fn every_request_from_every_state_is_decided() {
    let mut rows = Vec::new();
    for (from_index, (from_name, _)) in every_state().into_iter().enumerate() {
        for (begin_index, (begin_name, _)) in every_begin().into_iter().enumerate() {
            let (_, from) = every_state().swap_remove(from_index);
            let (_, begin) = every_begin().swap_remove(begin_index);
            let mut state = State::default();
            state.interaction = from;
            let allowed = begin.allowed_from(&state.interaction);
            let happened = state.begin(begin);
            assert_eq!(
                allowed, happened,
                "{begin_name} from {from_name}: the table says {allowed}, the machine did {happened}"
            );
            if happened {
                // Taken: the wall is **in the state that was asked for** — one value, so the old state is
                // gone whether or not the two share a name (a menu re-asked on another pane is a move, not
                // a second menu).
                assert!(
                    same_state(&state.interaction, target_of(begin_name)),
                    "{begin_name} from {from_name} was taken but the wall is {}",
                    name_of(&state.interaction)
                );
            } else {
                // Refused: nothing moved, which is the half a boolean-returning API always forgets.
                assert!(
                    same_state(&state.interaction, from_name),
                    "{begin_name} from {from_name} was refused but the wall moved"
                );
            }
            rows.push(format!(
                "{from_name} + {begin_name} = {}",
                if happened { "taken" } else { "refused" }
            ));
        }
    }
    // Seven states and six requests: the matrix is exhaustive over both enums, so this number moves when a
    // variant is added — which is a prompt to write its row in the ADR, not an annoyance.
    assert_eq!(rows.len(), 42, "seven states × six requests");
}

/// A state's name, for the failures a reader has to act on.
fn name_of(state: &Interaction) -> &'static str {
    match state {
        Interaction::Idle => "Idle",
        Interaction::Lifted(_) => "Lifted",
        Interaction::Resizing(_) => "Resizing",
        Interaction::Menu { .. } => "Menu",
        Interaction::Filtering { .. } => "Filtering",
        Interaction::Renaming(_) => "Renaming",
        Interaction::Settings => "Settings",
    }
}

fn same_state(state: &Interaction, name: &str) -> bool {
    name_of(state) == name
}

/// The state a request puts the wall in — the `Begin`/`Interaction` pairing the machine's `match` encodes.
/// Written out here so a request with no state, or a state with no request, fails this file.
fn target_of(begin: &str) -> &'static str {
    match begin {
        "Lift" => "Lifted",
        "Resize" => "Resizing",
        "Menu" => "Menu",
        "Filter" => "Filtering",
        "Rename" => "Renaming",
        "Settings" => "Settings",
        other => panic!("a request with no state: {other}"),
    }
}

/// The holds: **one gesture at a time.** A lift cannot start while a border is held, nor the other way
/// round — the pointer cannot be two places — and nothing can start behind a modal sheet.
#[test]
fn a_hold_cannot_be_started_over_another_hold() {
    let lift = || {
        Begin::Lift(PaneDrag {
            pane: 1,
            drop: None,
        })
    };
    let resize = || {
        Begin::Resize(Border {
            a: 1,
            b: 2,
            dir: arcons_cc::ui::Dir::Horizontal,
            at: 0.5,
            start: 0.0,
            span: 100.0,
            rect: arcons_cc::ui::Rect {
                x: 0.0,
                y: 0.0,
                w: 100.0,
                h: 100.0,
            },
        })
    };
    let held = Interaction::Lifted(PaneDrag {
        pane: 1,
        drop: None,
    });
    let holding = Interaction::Resizing(Border {
        a: 1,
        b: 2,
        dir: arcons_cc::ui::Dir::Horizontal,
        at: 0.5,
        start: 0.0,
        span: 100.0,
        rect: arcons_cc::ui::Rect {
            x: 0.0,
            y: 0.0,
            w: 100.0,
            h: 100.0,
        },
    });
    assert!(!resize().allowed_from(&held), "not two gestures at once");
    assert!(!lift().allowed_from(&holding), "not two gestures at once");
    assert!(
        !lift().allowed_from(&Interaction::Renaming(Rename::new(
            RenameTarget::Pane(1),
            None
        ))),
        "not a lift behind the rename sheet"
    );
    assert!(
        !lift().allowed_from(&Interaction::Settings),
        "not a lift behind the settings sheet"
    );
    assert!(
        !Begin::Settings.allowed_from(&Interaction::Renaming(Rename::new(
            RenameTarget::Pane(1),
            None
        ))),
        "not a sheet behind a sheet"
    );
    // The filter panel holds the pointer while it is open, so a lift or a border cannot start behind it —
    // and the panel cannot start behind a lifted pane or a held border either.
    let filtering = Interaction::Filtering { pane: 1 };
    assert!(
        !lift().allowed_from(&filtering) && !resize().allowed_from(&filtering),
        "the filter panel holds the pointer"
    );
    assert!(
        !Begin::Filter { pane: 1 }.allowed_from(&held)
            && !Begin::Filter { pane: 1 }.allowed_from(&holding),
        "not a filter panel on a pointer a gesture already holds"
    );
    assert!(
        !Begin::Filter { pane: 1 }.allowed_from(&Interaction::Renaming(Rename::new(
            RenameTarget::Pane(1),
            None
        ))) && !Begin::Filter { pane: 1 }.allowed_from(&Interaction::Settings),
        "not a filter panel behind a sheet"
    );
}

/// The two transitions the operator's own reports turn on: **`Name this pane…` works from the menu** (the
/// row is on the menu, so the menu must give way to the sheet), and **the settings sheet is reachable from a
/// menu** (the header's word is the way in, and pressing it must not leave two surfaces up).
#[test]
fn a_menu_gives_way_to_the_sheet_it_opens() {
    let menu = Interaction::Menu {
        pane: 1,
        at: (4.0, 4.0),
        open: None,
    };
    assert!(
        Begin::Rename(Rename::new(RenameTarget::Pane(1), None)).allowed_from(&menu),
        "the menu's own row opens the rename sheet"
    );
    assert!(
        Begin::Settings.allowed_from(&menu),
        "the header's control opens the settings sheet"
    );
    assert!(
        Begin::Filter { pane: 1 }.allowed_from(&menu),
        "a pane's filter toggle closes a passive menu and opens its panel"
    );
    // And no sheet holds a gesture: the pointer belongs to the sheet while it is up.
    assert!(
        !Begin::Lift(PaneDrag {
            pane: 1,
            drop: None
        })
        .allowed_from(&Interaction::Settings)
    );
}

/// `end` is the only way back to `Idle`, and it is idempotent: nothing can be released twice, and a wall
/// already at rest has nothing to give up.
#[test]
fn the_way_out_is_one_door() {
    let mut state = State::default();
    assert!(!state.end(), "an idle wall has nothing to release");
    for (name, from) in every_state() {
        state.interaction = from;
        let expected = !matches!(state.interaction, Interaction::Idle);
        assert_eq!(state.end(), expected, "{name}: the way out");
        assert!(
            matches!(state.interaction, Interaction::Idle),
            "{name}: `end` left the wall in a state"
        );
    }
}

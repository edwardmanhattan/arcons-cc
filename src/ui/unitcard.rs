//! The card's drawing: a small floating panel beside its marker.
//!
//! Everything about *what* it says and *where* it goes is in [`crate::units::card`], which is pure and
//! tested. This file turns that into elements, and follows the rule the rest of the UI keeps: every
//! colour and size comes from [`crate::tokens`].
//!
//! The card is a display with exactly **one** control (#240): `Follow` / `Stop following`, because locking a
//! pane's camera onto a hull was a verb only a remote could send — a reading nobody at the glass could act on,
//! in the one place the hull is already named.

use std::time::Instant;

use rune::{El, col, el, row, spacer, text};

use crate::tokens;
use crate::units::card::{Card, Placement, WIDTH};
use crate::verb::Intent;
use crate::verb::ViewId;

/// **The card's one control** (#240, #169): lock this pane's camera onto the hull the card is about.
pub const FOLLOW: &str = "Follow";
/// And the same control while the pane is locked on: `object.follow` with `unit: null` is the release, which is why
/// letting go of a hull is a verb and not a gesture only a pointer can make.
pub const STOP_FOLLOWING: &str = "Stop following";

/// **Whether the card offers its control, and to whom** (#240): the pane the press is about, and whether that pane
/// is already locked onto this hull (#169).
///
/// `None` is a pane that draws no map — a tree's post has a card, and `object.follow` on it is refused by name
/// (`the pane draws no map`). A control that says "no" every time is not a control, so the row is not drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Follow {
    pub pane: ViewId,
    pub following: bool,
}

/// The card, placed at `placement` in the picture's own points.
///
/// `follow` is the card's one control: `Follow` on a pane that is not locked onto the hull, `Stop following` on one
/// that is, and either way the press is the control plane's `object.follow` — the same verb a phone sends.
pub fn card(card: &Card, placement: Placement, now: Instant, follow: Option<Follow>) -> El {
    let mut body = col()
        .key("card")
        .absolute(placement.x as f32, placement.y as f32)
        // Width fixed, **height by content**: forcing 0 drew the fill and the border into a
        // zero-height box while the children overflowed it, which is exactly what "the card has no
        // panel" looked like on the wall.
        .w(WIDTH as f32)
        .gap(tokens::CARD_GAP)
        .padding(tokens::PAD)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::CARD_BORDER, tokens::card_edge())
        .fill(tokens::panel())
        // A card is mostly a display. A press on it must not clear the selection behind it; the one control
        // inside it is a child and takes its own press (#240).
        .on_press(|_state: &mut crate::ui::State| {});

    body = body.child(
        text(card.unit.name.clone())
            .key("card-name")
            .font(tokens::TITLE)
            .color(tokens::ink())
            // The one multi-paragraph, feed-supplied text on the wall: each paragraph in the
            // direction of its own words, as a browser lays out `unicode-bidi: plaintext`. A Latin
            // name is one text and lays out exactly as before; a right-to-left one no longer leans
            // on the wall's own direction.
            .dir_by_paragraph(),
    );

    // The hull number is absent for some hulls, and an absent row is better than an empty one.
    if let Some(hull_number) = &card.unit.hull_number {
        body = body.child(
            text(hull_number.clone())
                .key("card-hull")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }

    // Badges: the same vocabulary the marker uses.
    for badge in card.badges() {
        body = body.child(
            text(badge.to_uppercase())
                .key(format!("card-badge-{badge}"))
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }

    // **The card's one control** (#240): a hull can be followed from the glass. The row is the press, like every
    // other control on this wall, and the verb is `object.follow` through the control plane — so the card, the
    // menu and a phone's `object.follow` are one sentence. It sits above the readings because a control an
    // operator has to hunt for under an age and a commander line is a control that is not there.
    if let Some(Follow { pane, following }) = follow {
        let (label, unit) = if following {
            (STOP_FOLLOWING, None)
        } else {
            (FOLLOW, Some(card.unit.id_unit))
        };
        body = body.child(
            row()
                .key("card-follow")
                .w_full()
                .gap(tokens::GAP)
                .items_center()
                .child(
                    text(label)
                        .key("card-follow-label")
                        .font(tokens::LABEL)
                        .color(tokens::accent())
                        .grow(1.0),
                )
                .on_press(move |state: &mut crate::ui::State| {
                    state
                        .apply(Intent::Follow {
                            pane,
                            unit,
                            zoom: None,
                        })
                        .ok();
                }),
        );
    }

    body = body.child(rule());
    for (label, value, measured) in card.motion_rows() {
        body = body.child(
            row()
                .w_full()
                .gap(tokens::GAP)
                .child(text(label).font(tokens::LABEL).color(tokens::ink_dim()))
                .child(spacer())
                .child(
                    text(value)
                        .key(format!("card-{label}"))
                        .font(tokens::LABEL)
                        // A dash is not a measurement, so it is drawn like the secondary text it is.
                        .color(if measured {
                            tokens::ink()
                        } else {
                            tokens::ink_dim()
                        }),
                ),
        );
    }

    let age = card.age_text(now);
    body = body.child(
        row()
            .w_full()
            .gap(tokens::GAP)
            .child(text("AGE").font(tokens::LABEL).color(tokens::ink_dim()))
            .child(spacer())
            .child(
                text(age)
                    .key("card-age")
                    .font(tokens::LABEL)
                    .color(tokens::ink()),
            ),
    );

    if let Some(recorded) = card.recorded_at {
        body = body.child(
            text(format!("taken {}", recorded.format("%H:%M:%SZ")))
                .key("card-recorded")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }

    // **Who asserted the fix, in words** (#288): a relayed point names the operator and the commander it
    // claims. A device fix has no line, because there is nobody to name.
    if let Some(line) = card.provenance_line() {
        body = body.child(
            text(line)
                .key("card-provenance")
                .font(tokens::LABEL)
                .color(tokens::ink_dim()),
        );
    }

    body = body.child(rule());
    let (label, value) = card.commander_line();
    body = body.child(
        row()
            .w_full()
            .gap(tokens::GAP)
            .child(text(label).font(tokens::LABEL).color(tokens::ink_dim()))
            .child(spacer())
            .child(
                text(value)
                    .key("card-commander")
                    .font(tokens::LABEL)
                    .color(tokens::ink()),
            ),
    );

    for (label, value) in card.specification_rows() {
        body = body.child(
            row()
                .w_full()
                .gap(tokens::GAP)
                .child(text(label).font(tokens::LABEL).color(tokens::ink_dim()))
                .child(spacer())
                .child(text(value).font(tokens::LABEL).color(tokens::ink())),
        );
    }

    body
}

/// A hairline between sections.
fn rule() -> El {
    el().w_full()
        .size(tokens::HAIRLINE, tokens::HAIRLINE)
        .fill(tokens::hairline())
}

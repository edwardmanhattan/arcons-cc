//! The card's drawing: a small floating panel beside its marker.
//!
//! Everything about *what* it says and *where* it goes is in [`crate::units::card`], which is pure and
//! tested. This file turns that into elements, and follows the rule the rest of the UI keeps: every
//! colour and size comes from [`crate::ui::tokens`].

use std::time::Instant;

use rune::{El, col, el, row, spacer, text};

use crate::ui::tokens;
use crate::units::card::{Card, Placement, WIDTH};

/// The card, placed at `placement` in the picture's own points.
pub fn card(card: &Card, placement: Placement, now: Instant) -> El {
    let mut body = col()
        .key("card")
        .absolute(placement.x as f32, placement.y as f32)
        .size(WIDTH as f32, 0.0)
        .gap(tokens::CARD_GAP)
        .padding(tokens::PAD)
        .radius(tokens::PANEL_RADIUS)
        .border(tokens::HAIRLINE, tokens::hairline())
        .fill(tokens::panel())
        // A card is a display. A press on it must not clear the selection behind it.
        .on_press(|_state: &mut crate::ui::State| {});

    body = body.child(
        text(card.unit.name.clone())
            .key("card-name")
            .font(tokens::TITLE)
            .color(tokens::ink()),
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

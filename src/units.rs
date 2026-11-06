//! The hull — the vocabulary the rest of the crate is written in.
//!
//! **A Unit is a hull, not a position.** Half the fleet may never have reported; those units belong
//! in the picture and not on the map (`INV-02`). What the map draws is a [`Fix`], and a Fix is
//! absent until one arrives.
//!
//! The words, as `CONTEXT.md` defines them:
//!
//! - **Unit** — a hull: `id_unit`, `name`, `hull_number`. Never "vessel" or "ship" in code.
//! - **Fix** — one accepted position report. Never "ping", "update" or "location".
//! - **Feed** — the standing picture: every live unit, whether or not it has ever reported.
//! - **Picture** — what the wall shows: the feed, plus the client-side judgements drawn on it.
//!
//! #3 deserialises the feed (`FeedPosition` in `docs/spec/minos-api.yaml`); #6 draws it. This file
//! holds only what both need: the identity of a hull, and the picture's membership.

/// A hull. `id_unit` is the join key for every other Minos endpoint, so it is never guessed.
///
/// `hull_number` is `Option` because the API types it `nullable` and omits it for hulls the source
/// lists without one. An absent hull number is not an empty string, and it is not a zero.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unit {
    /// `int64` in the pinned spec (`FeedPosition.id_unit`).
    pub id_unit: i64,
    pub name: String,
    pub hull_number: Option<String>,
}

/// The standing picture: every live unit, whether or not it has ever reported.
///
/// Today this is membership and nothing else. #3 fills it from `/live-feed/positions`, #4
/// reconciles socket arrivals into it by `id_unit`, and #6 adds the fixes. The type lives here,
/// rather than in `data`, deliberately: the picture is the display's model, and the transport is
/// not allowed to decide its shape.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Picture {
    units: Vec<Unit>,
}

impl Picture {
    /// How many hulls the wall knows about — including the ones that have never reported.
    pub fn len(&self) -> usize {
        self.units.len()
    }

    /// True before the first read completes, which is what tells "no units exist" apart from "not
    /// asked yet". `INV-09` is the same distinction one layer down.
    pub fn is_empty(&self) -> bool {
        self.units.is_empty()
    }

    /// Replace the membership with a completed picture read.
    ///
    /// The read is authoritative: it names every live hull, including the ones that have never
    /// reported, so a hull missing from it is missing from the fleet, not from our copy.
    pub fn replace_from_feed(&mut self, feed: &[crate::data::types::FeedPosition]) {
        self.units = feed
            .iter()
            .map(|position| Unit {
                id_unit: position.id_unit,
                name: position.name.clone(),
                hull_number: position.hull_number.clone(),
            })
            .collect();
    }
}

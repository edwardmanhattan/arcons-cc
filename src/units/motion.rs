//! Travel between two fixes — **a drawing decision, and only that**.
//!
//! A hull that moves 200 m between beacons should slide 200 m across the wall, not jump. So the marker
//! travels from where it was drawn to the new fix over a short window. Three rules make that safe:
//!
//! 1. **It never creates or mutates a [`Pos`].** The interpolation produces a new value for the
//!    renderer and nothing else; the fix stays exactly where the server put it (`INV-05`).
//! 2. **It is time-bounded.** [`DURATION`] is a fixed window, not the interval the fixes imply: the
//!    implied interval is a beacon gap (seconds to minutes), and interpolating over it would show a
//!    hull halfway between two reports for a minute after the second one arrived. The suggested
//!    interval is capped at `DURATION` for the same reason.
//! 3. **It refuses to travel when travelling would be a lie.** A `backfilled` fix, a fix after a long
//!    silence, or a jump too large to be a hull's own motion all **snap**: the marker appears where
//!    the server says it is, and the wall does not animate a teleport as though it were movement.
//!
//! Nothing here is in the track history. There is no track history: v0.1 draws the newest fix.

use std::time::{Duration, Instant};

use crate::units::Pos;

/// How long a marker takes to reach a new fix.
///
/// Long enough to read as movement (three frames at 60 fps is a stutter), short enough that the wall
/// is showing the server's answer within a fifth of a second.
pub const DURATION: Duration = Duration::from_millis(600);

/// Beyond this, a jump is not the hull's own motion — it is a new fix after a silence, and animating
/// it draws a boat sailing across the ocean. Snap instead.
pub const MAX_JUMP_DEGREES: f64 = 0.5;

/// A gap longer than this means the previous fix is not a point on the way here.
pub const MAX_GAP: Duration = Duration::from_secs(30);

/// One marker's travel from the position it was drawn at to the newest fix.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Travel {
    from: Pos,
    to: Pos,
    started: Instant,
}

impl Travel {
    /// Where the marker is `now`, clamped to the destination once the window has passed.
    pub fn at(&self, now: Instant) -> Pos {
        let elapsed = now.saturating_duration_since(self.started);
        let t = (elapsed.as_secs_f64() / DURATION.as_secs_f64()).clamp(0.0, 1.0);
        // Straight line in degrees. Over a few hundred metres the difference from a great-circle
        // interpolation is smaller than the marker, and a projection-aware curve would have to know
        // the camera — which is the renderer's job, not the model's.
        Pos::new(
            self.from.lat + (self.to.lat - self.from.lat) * t,
            self.from.lon + (self.to.lon - self.from.lon) * t,
        )
    }

    /// The fix this travel lands on.
    pub fn destination(&self) -> Pos {
        self.to
    }

    pub fn start(&self) -> Pos {
        self.from
    }

    /// When this travel began, so a caller can tell whether it is still in flight.
    pub fn started(&self) -> Instant {
        self.started
    }
}

/// Travel from `from` to `to`, or `None` when the marker should simply appear at `to`.
///
/// `gap` is the interval the fixes imply, when the caller knows it; a gap beyond [`MAX_GAP`] snaps.
pub fn travel(
    from: Pos,
    to: Pos,
    backfilled: bool,
    gap_seconds: f64,
    now: Instant,
) -> Option<Travel> {
    if backfilled {
        return None;
    }
    if from.apart(to) > MAX_JUMP_DEGREES {
        return None;
    }
    if gap_seconds > MAX_GAP.as_secs_f64() && gap_seconds > 0.0 {
        return None;
    }
    Some(Travel {
        from,
        to,
        started: now,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_marker_travels_from_where_it_was_to_where_the_fix_is() {
        let (from, to) = (Pos::new(-6.0, 106.0), Pos::new(-6.01, 106.01));
        let now = Instant::now();
        let travel = travel(from, to, false, 5.0, now).expect("a small live jump travels");
        assert_eq!(travel.at(now), from, "at once, it is still where it was");
        let midway = travel.at(now + DURATION / 2);
        assert!((midway.lat - (from.lat + to.lat) / 2.0).abs() < 1e-9);
        assert_eq!(travel.at(now + DURATION), to, "at the end, exactly the fix");
        assert_eq!(travel.at(now + DURATION * 3), to, "and it stays there");
    }

    #[test]
    fn interpolation_never_moves_the_fix_itself() {
        // INV-05: the drawn position is derived, and the fix it comes from is untouched — the value
        // the renderer reads now is not the value any calculation may use later.
        let (from, to) = (Pos::new(-6.0, 106.0), Pos::new(-6.05, 106.05));
        let now = Instant::now();
        let travel = travel(from, to, false, 5.0, now).expect("travels");
        let drawn = travel.at(now + DURATION / 3);
        assert_ne!(drawn, to, "INV-05: mid-flight the marker is not at the fix");
        assert_ne!(drawn, from, "and not still at the old one");
        assert_eq!(
            travel.destination(),
            to,
            "INV-05: the fix is exactly where the server put it"
        );
        // Nothing about a `Pos` can be mutated through the travel: they are values, copied in and out.
        assert_eq!(travel.start(), from);
    }

    #[test]
    fn a_backfilled_fix_snaps_instead_of_sailing_across_the_map() {
        // INV-03: history arrives where it happened. Animating it draws a hull crossing an ocean.
        let (from, to) = (Pos::new(-6.0, 106.0), Pos::new(-6.001, 106.001));
        assert!(travel(from, to, true, 5.0, Instant::now()).is_none());
    }

    #[test]
    fn a_jump_too_large_to_be_the_hulls_own_motion_snaps() {
        let (from, to) = (Pos::new(-6.0, 106.0), Pos::new(-7.0, 112.0));
        assert!(
            travel(from, to, false, 5.0, Instant::now()).is_none(),
            "a hundred miles is not a glide"
        );
    }

    #[test]
    fn a_fix_after_a_long_silence_snaps() {
        let (from, to) = (Pos::new(-6.0, 106.0), Pos::new(-6.001, 106.001));
        assert!(travel(from, to, false, MAX_GAP.as_secs_f64() + 1.0, Instant::now()).is_none());
        assert!(
            travel(from, to, false, 1.0, Instant::now()).is_some(),
            "but a normal beacon gap travels"
        );
    }
}

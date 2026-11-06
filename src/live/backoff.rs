//! Backoff for a reconnecting socket: exponential, capped, **fully jittered**.
//!
//! The SDK's stock strategy is exponential without jitter, and a fleet of displays reconnecting in
//! lockstep is exactly what jitter is for: one server restart, and every wall in the building asks
//! for a socket in the same millisecond, every millisecond after that.
//!
//! Full jitter (AWS's term, and the algorithm Google's SRE book recommends) is `random(0, bound]`
//! rather than `bound ± noise`: the mean wait is halved and two clients never align, at the cost of
//! the occasional early retry. The bound grows 1 s, 2 s, 4 s … to the cap.
//!
//! The randomness is injectable so the schedule can be **tested**, which is the only reason this is
//! its own module.

use std::time::Duration;

/// First retry: one second.
pub const BASE: Duration = Duration::from_secs(1);
/// Never wait longer than this, however long the outage.
pub const CAP: Duration = Duration::from_secs(30);

/// The reconnect schedule.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    base: Duration,
    cap: Duration,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            base: BASE,
            cap: CAP,
        }
    }
}

impl Backoff {
    pub fn new(base: Duration, cap: Duration) -> Self {
        Self { base, cap }
    }

    /// The bound for `attempt` (1-based): `base * 2^(attempt-1)`, capped.
    ///
    /// Saturating at 2^20 rather than shifting a `u64` until it wraps: an outage that lasts a week
    /// must not produce a shift overflow and a zero-delay hot loop.
    pub fn bound(&self, attempt: u32) -> Duration {
        let doublings = attempt.saturating_sub(1).min(20);
        let scaled = self.base.saturating_mul(1u32 << doublings);
        scaled.min(self.cap)
    }

    /// The wait before `attempt`, with full jitter drawn from `[0, bound]`.
    ///
    /// `roll` is a number in `[0, 1)` — the caller supplies it so a test can be deterministic and so
    /// the actor uses one RNG rather than seeding one per retry.
    pub fn delay(&self, attempt: u32, roll: f64) -> Duration {
        let bound = self.bound(attempt);
        let fraction = roll.clamp(0.0, 1.0);
        bound.mul_f64(fraction)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bound_doubles_from_one_second_and_stops_at_the_cap() {
        let backoff = Backoff::default();
        let bounds: Vec<f64> = (1..=6)
            .map(|attempt| backoff.bound(attempt).as_secs_f64())
            .collect();
        assert_eq!(
            bounds,
            vec![1.0, 2.0, 4.0, 8.0, 16.0, 30.0],
            "1 s doubling, capped at 30 s"
        );
        // And it stays at the cap, however long the outage runs.
        for attempt in [7, 12, 40, 10_000] {
            assert_eq!(
                backoff.bound(attempt),
                CAP,
                "attempt {attempt} must stay capped"
            );
        }
    }

    #[test]
    fn full_jitter_stays_inside_the_bound_and_reaches_both_intents() {
        let backoff = Backoff::default();
        for attempt in 1..=6 {
            let bound = backoff.bound(attempt);
            for roll in [0.0, 0.25, 0.5, 0.99, 1.0] {
                let delay = backoff.delay(attempt, roll);
                assert!(
                    delay <= bound,
                    "attempt {attempt} roll {roll}: {delay:?} exceeds {bound:?}"
                );
            }
            assert_eq!(
                backoff.delay(attempt, 0.0),
                Duration::ZERO,
                "roll 0 is an immediate retry"
            );
            assert_eq!(
                backoff.delay(attempt, 1.0),
                bound,
                "roll 1 is the whole bound"
            );
        }
    }

    #[test]
    fn jitter_spreads_a_fleet_instead_of_aligning_it() {
        // The property the stock strategy lacks: two displays rolling independently almost never
        // land on the same wait.
        let backoff = Backoff::default();
        let mut rolls = [0.11, 0.37, 0.52, 0.83, 0.94].into_iter().cycle();
        let waits: Vec<Duration> = (0..5)
            .map(|_| backoff.delay(5, rolls.next().unwrap()))
            .collect();
        let mut unique = waits.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            waits.len(),
            "five independent rolls produced duplicate waits: {waits:?}"
        );
    }

    #[test]
    fn a_short_cap_is_honoured_and_a_zero_roll_never_hot_loops_forever() {
        // A caller with a small cap (a test, or a local server) still gets a sane ladder.
        let backoff = Backoff::new(Duration::from_millis(100), Duration::from_millis(500));
        assert_eq!(backoff.bound(1), Duration::from_millis(100));
        assert_eq!(backoff.bound(4), Duration::from_millis(500));
        assert_eq!(backoff.delay(4, 1.0), Duration::from_millis(500));
    }
}

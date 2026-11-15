//! **A watch** (#221): the one thing on the control plane that is not request/answer.
//!
//! `docs/mobile-remote.md` decision 1 is unchanged by it: the projection is still the only truth a client has, the
//! phone still **reads before it acts**, and a watch is **not** a subscription it resumes — after a reconnect or a
//! return from sleep the first thing on the wire is a read (`docs/control.md` says the same, with the frames).
//!
//! What a watch adds is a way for a wall that **someone else** moved — a hand on the glass, a `ctl` script, another
//! phone — to reach a client without the client asking four times a second. One frame asks for it:
//!
//! ```text
//! {"v": 1, "watch": "wall"}                     → {"v": 1, "ok": true, "watch": "wall"}
//! {"v": 1, "ok": true, "watch": "wall", "wall": { … }}   and then one of these whenever it changed
//! ```
//!
//! **Coalesced to at most one per beat**, and the beat is [`BEAT`]: 4 Hz, the cadence #71 already gives a focused
//! pane's map. So a wall that changes nothing and a wall that changes sixty times a second cost the client the same —
//! and a wall that changed nothing at all writes nothing, which is [`Watching::due`] and the first thing this module
//! is tested for.
//!
//! **A watch is a carrier's capability, not a verb of the plane** (#195's seam doing the job it was made for). The
//! unix socket answers one request with one answer, because a `ctl` script must not be able to hold it open for ever:
//! [`NOT_CARRIED`] is its refusal, by name. A relay holds a connection open already, and carries one.

use std::time::{Duration, Instant};

/// **The beat a watch publishes on** (#221): 4 Hz, the cadence a focused pane's map already runs at (#71).
///
/// Named here rather than in the session loop because it is a promise to the client: a wall that changes nothing and a
/// wall that changes sixty times a second are the same number of lines and the same number of wakes.
pub const BEAT: Duration = Duration::from_millis(250);

/// **The refusal a carrier that cannot carry a watch gives** (#221), by name: a `ctl` script must not be able to hold
/// the wall's one socket open for ever, and a client that asked anyway is told which of the two walls it is talking to.
pub const NOT_CARRIED: &str = "this carrier does not carry a watch";

/// **What a client asks to watch** (#221): a word, not an intent.
///
/// One variant, and a closed list rather than a string: `read` has `wall` and `scene`, and a watch today has the
/// projection only — a phone draws the arrangement, and the picture is not a client's to read (`ADR-0005` decision 7).
/// A word this wall does not use is a named refusal, never a default (`INV-07`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Watch {
    /// The projection, published when it changed: `docs/control.md`'s read of `wall`, as a stream.
    Wall,
}

impl Watch {
    /// The word on the wire.
    pub fn word(self) -> &'static str {
        match self {
            Watch::Wall => "wall",
        }
    }

    /// The watch a word names, or `None` for one this wall does not carry (`INV-07`: no default).
    pub fn of_word(word: &str) -> Option<Watch> {
        [Watch::Wall].into_iter().find(|watch| watch.word() == word)
    }
}

/// **A watch that is open** (#221): the version of the projection the client already holds, and the beat's clock.
///
/// The version is the client's, not the wall's: it is what the client read before it asked, so the first publish is a
/// **change** and not a repeat of the read the client has just done. That is why a wall that publishes when nothing
/// changed is unrepresentable here rather than merely forbidden.
#[derive(Clone, Copy, Debug)]
pub struct Watching {
    /// What the client asked to watch, so a publish says which stream it belongs to.
    watch: Watch,
    /// The `seq` the client holds — the projection it read, or the last one published to it.
    published: u64,
    beat: Duration,
    /// When the last publish went out (or the watch opened), so the next one is at least a beat later.
    last: Instant,
}

impl Watching {
    /// A watch that starts where the wall is: `seq` is the projection the client read, and `now` is the clock its beat
    /// starts from — the session's own, so a test can walk a beat without sleeping through one.
    pub fn at(watch: Watch, seq: u64, now: Instant) -> Watching {
        Watching {
            watch,
            published: seq,
            beat: BEAT,
            last: now,
        }
    }

    /// What the client asked to watch.
    pub fn watch(&self) -> Watch {
        self.watch
    }

    /// The version the client holds.
    pub fn holds(&self) -> u64 {
        self.published
    }

    /// **How long a read of the carrier may wait** (#221): the rest of this beat, so the loop wakes no more often than
    /// the beat — and a client that is typing wakes it sooner, because a line is a request.
    pub fn waits(&self, now: Instant) -> Duration {
        (self.last + self.beat).saturating_duration_since(now)
    }

    /// **Whether a publish is due**: the projection has moved **and** the beat has elapsed.
    ///
    /// Nothing changed costs nothing; sixty changes inside one beat are one line, and it is the newest. A change that
    /// arrives inside the beat waits for it rather than spending a wake on each version.
    pub fn due(&mut self, seq: u64, now: Instant) -> bool {
        if seq == self.published || now < self.last + self.beat {
            return false;
        }
        self.published = seq;
        self.last = now;
        true
    }
}

/// **Whether an error is a deadline rather than a failure** (#221): a carrier read that waited its beat out.
///
/// A socket's read deadline and a channel's are the same two kinds, and a watch treats both as "nothing has happened
/// yet" — a beat is not a broken wire, and a session that ended on one would be a phone that went quiet because a wall
/// had nothing to say.
pub fn timed_out(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::{RefusalKind, Refused, refusal_kind};

    /// One beat, and the instants a test walks through it by. Named so a scenario reads as a scenario.
    ///
    /// **Not the wall clock**: a test about a 250 ms beat may not depend on how long the test before it took, and a
    /// claim whose two sides read two different `Instant::now()`s would be a claim about the machine.
    fn at(millis: u64) -> Instant {
        static BASE: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
        *BASE.get_or_init(Instant::now) + Duration::from_millis(millis)
    }

    /// **The ticket's first claim** (#221): two changes at two beats are two projections, two inside one beat are one,
    /// and a wall that changed nothing publishes nothing.
    ///
    /// The falsifier is the obvious implementation — publish on every change — which wakes a phone for each one, and
    /// the other obvious one — publish every beat — which wakes it for a wall that is standing still.
    #[test]
    fn the_beat_coalesces_changes_and_publishes_nothing_that_did_not_move() {
        let mut watch = Watching::at(Watch::Wall, 7, at(0));
        let mut published = Vec::new();

        // **Two changes in one beat are one projection**, and it is the newest: the wall moved to 8, then to 9,
        // and the client is told 9 once.
        for (millis, seq) in [(50, 8), (100, 9), (249, 9)] {
            if watch.due(seq, at(millis)) {
                published.push(seq);
            }
        }
        // **A change at a beat of its own is a second projection.**
        if watch.due(9, at(250)) {
            published.push(9);
        }
        assert_eq!(
            published,
            vec![9],
            "a change inside the beat published early, or the change at the beat did not: the beat is the promise"
        );

        // The line above consumed the version; the same projection is not published twice.
        assert!(
            !watch.due(9, at(300)),
            "the wall published the projection the client already holds: a phone woken for nothing (#221)"
        );
        assert_eq!(watch.holds(), 9, "and the client holds what was published");

        // **A second change, a beat later, is its own projection**: two changes at two beats are two projections.
        assert!(
            watch.due(10, at(500)),
            "a change one beat after the last publish must be published: the client cannot ask for it"
        );
        assert_eq!(watch.holds(), 10);

        // **None changed, none published** — however long the wall stands still.
        for millis in [750, 1_000, 5_000, 60_000] {
            assert!(
                !watch.due(10, at(millis)),
                "a wall that changed nothing published something at {millis} ms (#221): the phone wakes for nothing"
            );
        }
    }

    /// **The beat is the wait** (#221): a watched loop is told exactly how long it may block, so a wall with nothing
    /// to say costs one wake per beat and a line from the client still wakes it at once.
    #[test]
    fn the_beat_is_the_wait_and_a_publish_moves_it() {
        let mut watch = Watching::at(Watch::Wall, 1, at(0));
        assert!(
            watch.waits(at(0)) <= BEAT,
            "a watch must never wait longer than its beat, or a change sits on the wall for two of them"
        );
        assert_eq!(
            watch.waits(at(BEAT.as_millis() as u64 + 10)),
            Duration::ZERO,
            "a beat that has already elapsed leaves nothing to wait for: the loop asks the wall now"
        );
        assert!(watch.due(2, at(BEAT.as_millis() as u64 + 10)));
        assert!(
            watch.waits(at(BEAT.as_millis() as u64 + 10)) >= BEAT,
            "a publish starts the next beat, so two publishes are never closer together than one beat"
        );
    }

    /// **The word is the frame's, in one place** (#221): a client asks for `wall`, the answer says `wall`, and a word
    /// this wall does not carry names nothing at all (`INV-07`'s rule, where a default would be a guess).
    #[test]
    fn the_watch_word_is_one_word() {
        assert_eq!(Watch::Wall.word(), "wall");
        assert_eq!(Watch::of_word("wall"), Some(Watch::Wall));
        for other in ["scene", "", "WALL", "watch"] {
            assert_eq!(
                Watch::of_word(other),
                None,
                "{other:?} named a watch: a word this wall does not carry must be a refusal, not a default"
            );
        }
        // **The refusal, spelled out**: `tests/scaffold.rs` reads the source for every `Refused("…")` this crate can
        // send and fails when one has no category, so the name is written here where a reader can see it rather than
        // only inside `REFUSALS`.
        assert_eq!(
            Refused(NOT_CARRIED),
            Refused("this carrier does not carry a watch"),
            "the refusal is a name a client can branch on (`INV-07`), and the socket gives it"
        );
        assert_eq!(
            refusal_kind(NOT_CARRIED),
            Some(RefusalKind::Rule),
            "and its kind is the one a client acts on: a `rule` is a read, which is the fallback a watch has"
        );
    }
}

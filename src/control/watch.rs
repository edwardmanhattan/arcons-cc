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

// **The client's half of a watch** (#221): once the wall can publish, a client has two rules.
//
// The wall's half is the module above -- the beat, the coalescing, the frames. This is what a phone keeps, and both
// rules are already implied by docs/mobile-remote.md decision 1:
//
// 1. **A watch does not resume.** It belongs to a connection, and a connection that comes back knows nothing: the
//     client reads first and asks for the watch again (Session::is_open is false until both have happened). A watch
//     that resumed on its own would be a client believing what it remembered.
// 2. **A seq it did not expect is a read.** A publish is one version further on than the projection the client holds;
//     a number that skips or goes back is a line this client cannot place in the wall history, and the one thing it
//     may not do is reconstruct one. It reads.
//
// Session and Publish compile on every platform; the tests are unix-only because they drive control::client, and a
// Windows build has no plane (#197). It lives here rather than in a module of its own because a watch is one contract
// with two ends, and the two ends in one file is how they stay one contract -- control::sessions is the wall queue
// (#222), a different thing with a confusingly similar name.

/// **What a client does with a projection the wall published** (#221).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Publish {
    /// **The version after the one this client holds**: it is the wall now.
    Take,
    /// **A number it did not expect** — one that skipped ahead, or one behind it. The numbers and the client's history
    /// disagree (a version it never saw, or a wall that went backwards), so it **reads** and takes the read's
    /// projection. It never reconstructs a difference from what it holds.
    Read,
}

/// **A client's watch on one connection** (#221): the version it holds, and whether it is watching at all.
///
/// Not carried across a connection. [`Session::shut`] is what a drop, a hang-up or a read that failed all end in, and
/// the client asks again only after a read has landed — the whole of rule 1, in one place.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Session {
    /// The `seq` of the projection this client holds: from the read that landed, or from the last publish it took.
    held: u64,
    /// Whether the wall has granted a watch on this connection.
    open: bool,
}

impl Session {
    /// A session that holds nothing and is watching nothing: what a client has before its first read.
    pub fn new() -> Session {
        Session::default()
    }

    /// **A read landed** (#221): this is the version the client holds. A read does not open a watch — the frame does,
    /// and only after the read (`docs/mobile-remote.md` decision 1).
    pub fn read(&mut self, seq: u64) {
        self.held = seq;
    }

    /// **The wall granted a watch**: what follows is the same projection, published.
    pub fn opened(&mut self) {
        self.open = true;
    }

    /// **The connection is gone** — a drop, a hang-up, a read that did not land. Everything about the watch goes with
    /// it, because nothing on the far side of a new connection is a fact about this one.
    pub fn shut(&mut self) {
        *self = Session::default();
    }

    /// Whether a watch is open on this connection.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// The version the client holds.
    pub fn held(&self) -> u64 {
        self.held
    }

    /// **The rule for a publish** (#221): the version after this one is taken; anything else is a read.
    ///
    /// A publish is coalesced to the wall's beat, so versions can be skipped by *the wall's own doing* — and the wall
    /// numbers the projection it hands out, so the number a watcher sees is one further on than the last one it was
    /// handed. A number that is not is a line this client cannot place, and it reads rather than guesses.
    pub fn publish(&mut self, seq: u64) -> Publish {
        if self.held.checked_add(1) == Some(seq) {
            self.held = seq;
            Publish::Take
        } else {
            Publish::Read
        }
    }
}

#[cfg(all(test, unix))]
mod client_tests {
    use super::*;
    use crate::control::carrier::{Carrier, Loopback, Peer};
    use crate::control::client::{Client, Denied};
    use crate::control::projection::Wall as Projection;
    use crate::control::watch::Watch;
    use crate::control::wire::{self, Answer, Request};
    use std::sync::mpsc::{self, Receiver};
    use std::time::Duration;

    /// **A projection at a version, saying where it came from** (#221). The label is what makes the candidates a test
    /// can tell apart: the one the wall published, and the one the client read instead.
    fn projection(seq: u64, label: &str) -> Projection {
        let mut wall = crate::ui::projection::of(&crate::ui::State::default());
        wall.seq = seq;
        wall.workspaces[0].name = label.to_string();
        wall
    }

    /// The label a projection carries, so a test compares what the client **holds** and not what it was told.
    fn label(wall: &Projection) -> &str {
        &wall.workspaces[0].name
    }

    /// **A wall whose answers are this test's** (#221): the client's end, and every request the client wrote.
    ///
    /// A scripted wire rather than `control::client`'s real-wall harness, because one of the claims here is about a
    /// line **no wall would publish**: a `seq` that skipped. A wall that could do that is a wall with a bug; the client
    /// that survives it is what this module is for.
    fn script(
        mut answers: impl FnMut(&Request) -> Vec<Answer> + Send + 'static,
    ) -> (Peer, Receiver<String>) {
        let (wall_end, peer) = Loopback::pair();
        let (sent, requests) = mpsc::channel();
        std::thread::Builder::new()
            .name("sessions-test-wall".into())
            .spawn(move || {
                let mut wall_end = wall_end;
                while let Ok(Some(line)) = wall_end.read_line() {
                    let _ = sent.send(line.clone());
                    // A line the script cannot read is a test bug; silence would hang the client and say less than a
                    // test that fails.
                    let Ok(request) = wire::request(&line) else {
                        continue;
                    };
                    for answer in answers(&request) {
                        if wall_end.write_line(&wire::reply(answer)).is_err() {
                            return;
                        }
                    }
                }
            })
            .expect("the scripted wall's thread starts");
        (peer, requests)
    }

    /// **Poll the way a phone does**, bounded: the scripted wall writes its publish from its own thread, so a test
    /// that looked once would be racing it. `Ok(None)` is "nothing yet", which is what most frames of a watch are.
    fn published(client: &mut Client<Peer>) -> Projection {
        for _ in 0..1_000 {
            match client.published() {
                Ok(Some(wall)) => return wall,
                Ok(None) => std::thread::sleep(Duration::from_millis(2)),
                Err(error) => {
                    panic!("the client's wire failed where a publish was expected: {error:?}")
                }
            }
        }
        panic!("the wall's publish never arrived (#221)");
    }

    /// Every request a client has sent since the last look, waiting for the first one so a test never races the wire.
    fn next(sent: &Receiver<String>) -> String {
        sent.recv_timeout(Duration::from_secs(2))
            .expect("the client sent nothing, where a request was expected (#221)")
    }

    /// **A `seq` it did not expect makes the client read** (#221): the wall publishes version 6 while the client holds
    /// 4, so the numbers cannot be placed — the client sends a read and holds the projection the read answers with,
    /// never the one the publish carried.
    ///
    /// The falsifier is the obvious implementation: take every publish, which draws a wall from a history the client
    /// cannot account for and leaves its next intent composed from a number it cannot trust.
    #[test]
    fn a_missed_seq_re_reads() {
        let mut reads = 0;
        let (peer, sent) = script(move |request| match request {
            Request::Wall => {
                reads += 1;
                vec![Answer::Wall(if reads == 1 {
                    projection(4, "the wall it read")
                } else {
                    projection(6, "the wall it read again")
                })]
            }
            // **The grant, and a publish that skipped a version**: the beat coalesced 5 into 6, which a watcher
            // holding 4 cannot place.
            Request::Watch(_) => vec![
                Answer::Watching(Watch::Wall),
                Answer::Published(Watch::Wall, projection(6, "a version it never saw")),
            ],
            _ => Vec::new(),
        });
        let mut client = Client::new();
        client.connect(peer);
        assert_eq!(label(&client.read().expect("a read")), "the wall it read");
        assert_eq!(next(&sent), wire::request_line(&Request::Wall));
        client.watch().expect("a client that has read may watch");
        assert_eq!(
            next(&sent),
            wire::request_line(&Request::Watch(Watch::Wall))
        );

        let published = published(&mut client);
        assert_eq!(
            label(&published),
            "the wall it read again",
            "the client took the version it could not place: a missed seq must be read again, never reconstructed \
             (#221)"
        );
        assert_eq!(
            client.wall().map(label),
            Some("the wall it read again"),
            "and the projection it holds is the read's, not the skipped publish's"
        );
        assert_eq!(
            next(&sent),
            wire::request_line(&Request::Wall),
            "the read the skipped version earned is the next line on the wire"
        );
        assert_eq!(
            client.session().held(),
            6,
            "the read's version is the one it holds now, so the next publish is expected again"
        );
        assert!(
            sent.try_recv().is_err(),
            "the client wrote more than one read: a version it could not place costs one read, not a loop (#221)"
        );
    }

    /// **A watch resumes only after a read** (#221, `docs/mobile-remote.md` decision 1): a client that loses its
    /// connection holds no watch, may not take a publish on the new one, and is refused by its own rule when it asks to
    /// watch before reading — nothing is written on the new wire until it has read.
    #[test]
    fn a_watch_resumes_only_after_a_read() {
        let (peer, sent) = script(|request| match request {
            Request::Wall => vec![Answer::Wall(projection(1, "the wall"))],
            Request::Watch(_) => vec![Answer::Watching(Watch::Wall)],
            _ => Vec::new(),
        });
        let mut client = Client::new();
        client.connect(peer);
        client.read().expect("the first read");
        client
            .watch()
            .expect("a watch on the connection it read on");
        assert!(client.session().is_open());
        let _ = (next(&sent), next(&sent));

        // The connection goes — a drop, a sleep, a network that changed underneath.
        client.disconnect();
        assert_eq!(
            client
                .published()
                .expect_err("a dropped client watches nothing"),
            Denied::NoConnection,
        );

        // It comes back. **Nothing is known on a new connection**, whatever the last one held.
        let (peer, sent) = script(|request| match request {
            Request::Wall => vec![Answer::Wall(projection(9, "the wall it came back to"))],
            Request::Watch(_) => vec![Answer::Watching(Watch::Wall)],
            _ => Vec::new(),
        });
        client.connect(peer);
        assert_eq!(
            client.watch(),
            Err(Denied::NotRead),
            "a watch was granted before a read: a client that resumes a watch believes what it remembered (#221)"
        );
        assert_eq!(
            client
                .published()
                .expect_err("and nothing is published to it"),
            Denied::NotWatching,
        );
        assert!(
            sent.try_recv().is_err(),
            "the client wrote on the new connection before reading it: a watch that resumes without a read is the \
             falsifier (#221)"
        );

        // Read, then ask: the order decision 1 states, and the only order that works.
        client.read().expect("the read on resume");
        client
            .watch()
            .expect("a watch after the read is the rule working");
        assert_eq!(
            vec![next(&sent), next(&sent)],
            vec![
                wire::request_line(&Request::Wall),
                wire::request_line(&Request::Watch(Watch::Wall))
            ],
            "the first line after a resume is the read and only the read, and the watch follows it (#221)"
        );
    }

    /// **The rule itself** (#221): every version but the next one is a read, including the one the client already
    /// holds and the one behind it — and a taken version becomes the one it holds.
    #[test]
    fn the_next_version_is_taken_and_nothing_else_is() {
        let mut session = Session::new();
        session.read(41);
        assert_eq!(session.publish(42), Publish::Take);
        assert_eq!(session.held(), 42, "a taken publish is the version held");
        for unplaceable in [42, 41, 7, u64::MAX] {
            assert_eq!(
                session.publish(unplaceable),
                Publish::Read,
                "{unplaceable} was taken as the next version after 42: only 43 is (#221)"
            );
        }
        assert_eq!(
            session.held(),
            42,
            "a publish the client would not take moved what it holds"
        );
        // The top of the range is not a client's business to overflow into.
        let mut edge = Session::new();
        edge.read(u64::MAX);
        assert_eq!(edge.publish(u64::MAX), Publish::Read);
        assert_eq!(edge.publish(0), Publish::Read);
    }

    /// **A watch is a connection's** (#221): shutting a session forgets the version and the stream together, which is
    /// the whole of what a drop may leave behind.
    #[test]
    fn a_session_that_shuts_holds_nothing() {
        let mut session = Session::new();
        session.read(7);
        session.opened();
        assert!(session.is_open());
        session.shut();
        assert!(!session.is_open(), "a shut session is not watching");
        assert_eq!(session.held(), 0, "and holds no version to compare against");
        assert_eq!(
            Session::default(),
            Session::new(),
            "a session with nothing on it is the shape a client starts from"
        );
    }
}

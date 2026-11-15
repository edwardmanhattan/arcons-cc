//! **A session with a watch** (#221): the client's two rules for the one thing on the plane that is not
//! request/answer.
//!
//! [`crate::control::watch`] is the wall's half — the beat, the coalescing, the frames. This is the half a phone keeps,
//! and it is two rules that `docs/mobile-remote.md` decision 1 already implies:
//!
//! 1. **A watch does not resume.** It belongs to a connection, and a connection that comes back knows nothing: the
//!    client reads first and asks for the watch again ([`Session::is_open`] is false until both have happened). A
//!    watch that resumed on its own would be a client believing what it remembered.
//! 2. **A `seq` it did not expect is a read.** A publish is one version further on than the projection the client
//!    holds; a number that skips or goes back is a line this client cannot place in the wall's history, and the one
//!    thing it may not do is reconstruct one. It reads.
//!
//! Driven over the wall's own session loop by `cargo test control::sessions`, because a client is a client whether the
//! wire is a pair of channels in this process or a relay to a device in another room (#197).

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

// **Unix only, because a client is** (#197): the rules are driven through `control::client`, which a Windows build
// has no plane for (`src/control.rs` says the same where it gates the module). The rules themselves — `Session` and
// `Publish` — compile everywhere, so the type a client keeps is not a platform's.
#[cfg(all(test, unix))]
mod tests {
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
        let mut wall = crate::control::projection::of(&crate::ui::State::default());
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

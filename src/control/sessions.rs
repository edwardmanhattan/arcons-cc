//! **Two remotes at one wall** (#222): the wall's queue, and the rules between the carriers that feed it.
//!
//! A relay makes something reachable that was only theoretical before it (`docs/mobile-remote.md` listed "who wins a
//! refusal when two clients ask for contradictory things at once" as open): the wall can hold a unix `ctl`, a phone and
//! a viewer at once. Each is a **carrier** over one session loop (#195) — a line in, a `Job` for the wall, a line out —
//! and what this module holds is the part that is the *wall's* rather than any carrier's: one queue.
//!
//! **Arrival order is the order.** A carrier does not apply anything (that is `INV-05`), and it does not own a queue
//! either: every carrier holds an end of [`Queue`], the same one, so a phone's intent waits behind a `ctl`'s if it
//! arrived after it. Two queues drained in turn would be a wall where one carrier is privileged by the order of two
//! `while` loops in `poll` — a rule nobody wrote and nobody could see.
//!
//! The other four rules of the ticket are held where they belong, and named here because they are what the queue is
//! for: a stale premise is a `rule` and not a `busy` (`control::REFUSALS`), a carrier that hangs up takes its origin
//! with it (`State::answer`), the origin of a cursor is the **device's name** the carrier knows
//! (`control::carrier::Carrier::origin`), and two clients that agree are applied twice with the second answering
//! `changed: false`. `docs/control.md` is the prose; `cargo test control::sessions` and `cargo test control::authority`
//! are the proof.

use std::sync::mpsc::{self, Receiver, Sender};

use crate::ui::Origin;

/// **One thing a carrier has left for the wall** (#222): a request to answer, or a hand that has let go.
///
/// The queue is one channel, so the two are one type: a carrier's session says what happened to it — a line to answer,
/// or the end of it — and the thread that owns the state settles both. A second channel for "the client went away"
/// would be a second way to reach the wall. The type lives with [`Queue`], which carries it, so the listener depends
/// on the queue rather than the queue on the listener.
pub enum Job {
    /// **A parsed request**, waiting for the UI thread. The reply channel is how the listener says the answer back to
    /// the client without ever touching the wall itself.
    Request {
        /// What the client asked for: a change, a look at the wall, or the arrangement (#181, #201).
        request: super::wire::Request,
        /// **Whose hand asked** (#222): the origin the carrier knows its client by — the socket's `ctl`, a relay's
        /// device name — or `None` for a carrier that carries anybody. The door prefers it over an origin a frame typed.
        hand: Option<Origin>,
        answer: Sender<super::wire::Answer>,
    },
    /// **A carrier that hung up** (#222): the hand this origin was is gone, and its cursor goes with it. A carrier
    /// serves several hands over its life (a relay connection carries every paired phone), so the session reports each
    /// of them once, at the end.
    Departed { origin: Origin },
}

impl Job {
    /// The request this job carries, if it carries one. A driver that has a job and wants to know what it is asks this
    /// rather than matching twice.
    pub fn request(&self) -> Option<&super::wire::Request> {
        match self {
            Job::Request { request, .. } => Some(request),
            Job::Departed { .. } => None,
        }
    }

    /// Answer the client. Called by the UI thread after `control::apply` (or after reading the projection), and once
    /// per job — a departed hand has nobody to answer, and says nothing about it.
    pub fn answer(self, answer: super::wire::Answer) {
        // A client that hung up is not an error: the reply has nowhere to go, and the wall already changed.
        if let Job::Request { answer: to, .. } = self {
            let _ = to.send(answer);
        }
    }
}

/// **The wall's queue** (#222): one channel, every carrier feeding it, one thread draining it.
///
/// The type exists so "the queue is the wall's" is a shape rather than a habit. A carrier is handed a
/// [`Queue::sender`] — an end of *this* channel — and the thread that owns the state drains [`Queue::next`] between
/// frames, so the order the wall applies in is the order the carriers read in.
pub struct Queue {
    to: Sender<Job>,
    from: Receiver<Job>,
}

impl Queue {
    /// A queue with no carrier yet: a wall that never opens a plane still has one, empty.
    pub fn new() -> Queue {
        let (to, from) = mpsc::channel();
        Queue { to, from }
    }

    /// The end one carrier holds. Cloned per carrier — the socket's listener, the relay's dial — and every clone feeds
    /// the same channel.
    pub fn sender(&self) -> Sender<Job> {
        self.to.clone()
    }

    /// **The next thing a carrier has left for the wall**, if any. Never blocks: `poll` calls this between frames, and
    /// a wall that waited here would freeze for a client that has not finished typing (`INV-05`).
    pub fn next(&self) -> Option<Job> {
        self.from.try_recv().ok()
    }
}

impl Default for Queue {
    fn default() -> Queue {
        Queue::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::carrier::{Loopback, Peer};
    use crate::control::wire::{self, Answer};
    use crate::control::{self, Changed, REFUSALS, RefusalKind, refusal_kind};
    use crate::ui::State;
    use crate::verb::Intent;
    use crate::verb::{Origin, PaneContent};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::time::{Duration, Instant};

    /// Long enough for two threads and a channel on a slow machine, short enough that a wedged test fails rather than
    /// hangs a gate.
    const PATIENCE: Duration = Duration::from_secs(5);

    /// **One client on one carrier**, served by the wall's own session loop on its own thread — the door the socket's
    /// listener and the relay's dial both use (`server::serve_session`, #195). `name` is whose hand it is: a relay's
    /// carrier knows it from `devices.toml`, and a test's has to be told (#222).
    struct Client {
        /// The client's end. An `Option` so `Drop` can hang up *before* joining: a loop blocked in a read is woken by
        /// its peer going away, and joining first would wait for a line nobody is going to type.
        peer: Option<Peer>,
        thread: Option<std::thread::JoinHandle<()>>,
    }

    impl Client {
        fn connect(queue: &Queue, name: &str) -> Client {
            let (wall_end, peer) = Loopback::named(name);
            Client::serve(queue, wall_end, peer)
        }

        /// A carrier with no name — the local `ctl`'s honest state on a loopback, where nothing authenticated the
        /// connection and nothing had to.
        fn anonymous(queue: &Queue) -> Client {
            let (wall_end, peer) = Loopback::pair();
            Client::serve(queue, wall_end, peer)
        }

        fn serve(queue: &Queue, wall_end: Loopback, peer: Peer) -> Client {
            let tx = queue.sender();
            let stop = Arc::new(AtomicBool::new(false));
            let thread = std::thread::Builder::new()
                .name("sessions-test-carrier".to_string())
                .spawn(move || {
                    let _ = crate::control::server::serve_session(wall_end, &tx, &stop);
                })
                .expect("the session loop starts");
            Client {
                peer: Some(peer),
                thread: Some(thread),
            }
        }

        fn send(&self, line: &str) {
            self.peer.as_ref().expect("a live connection").send(line);
        }

        /// The next line the wall wrote, read with the client's own decoder — so a refusal arrives with the `kind`
        /// the wall sent rather than with this test's reading of it.
        fn answer(&self, what: &str) -> Result<wire::Answer, wire::Unreadable> {
            let line = self
                .peer
                .as_ref()
                .expect("a live connection")
                .recv_within(PATIENCE)
                .unwrap_or_else(|| panic!("{what}: the wall never answered"));
            wire::answer(&line)
        }
    }

    impl Drop for Client {
        fn drop(&mut self) {
            // End of file, exactly as a client closing its socket: the loop's read returns `None`, the loop says which
            // hands came in on it, and its thread ends (#222).
            self.peer = None;
            if let Some(thread) = self.thread.take() {
                let _ = thread.join();
            }
        }
    }

    /// The next job the wall has been handed, waited for within [`PATIENCE`]: a queue that never delivers fails with
    /// the sentence the test wrote rather than hanging.
    fn next(queue: &Queue, what: &str) -> Job {
        let deadline = Instant::now() + PATIENCE;
        loop {
            if let Some(job) = queue.next() {
                return job;
            }
            assert!(Instant::now() < deadline, "{what}");
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// One request line, in the wire's own shape.
    fn line(intent: &str) -> String {
        format!(r#"{{"v":{},"intent":{intent}}}"#, control::PROTOCOL)
    }

    /// A wall with two panes in its first workspace (the default wall has one), so "the next thing" can be about a
    /// pane that another client is about to close.
    fn two_panes() -> State {
        let mut state = State::default();
        let outcome = state.apply(Intent::SplitPane {
            pane: 1,
            content: PaneContent::Map,
        });
        assert!(
            outcome.is_ok(),
            "the wall this test drives must have two panes: {outcome:?}"
        );
        state
    }

    /// **A second carrier is not waited for by the first** (#222 rule 1).
    ///
    /// The falsifier is the ticket's own: *a second carrier waited for by the first* — a phone holds a watch, so a
    /// wall that served one connection at a time would be silent for as long as the first client stayed connected.
    /// Here the first carrier is connected and says nothing, and the second is answered anyway, on the wall's own
    /// queue and the thread that owns the state.
    #[test]
    fn a_second_carrier_is_not_waited_for_by_the_first() {
        let queue = Queue::new();
        let mut state = State::default();
        let silent = Client::anonymous(&queue);
        let phone = Client::connect(&queue, "phone");

        phone.send(&line(r#"{"verb":"workspace.new"}"#));
        let job = next(
            &queue,
            "the phone's request never reached the wall's queue: a carrier is holding the wall (#222)",
        );
        assert!(state.answer(job), "a new workspace is a change");
        assert_eq!(
            phone.answer("the phone's answer").expect("an answer"),
            Answer::Changed(Ok(Changed(true))),
            "the phone was answered while another carrier was still connected and silent"
        );
        assert_eq!(
            state.workspaces.len(),
            4,
            "and the change is the wall's, applied once on the thread that owns it"
        );
        drop(silent);
    }

    /// **Arrival order is the order, and no carrier is privileged** (#222 rule 1) — the queue is the wall's, so the
    /// intent that arrived first is applied first whatever carrier it came in on.
    ///
    /// The pair is contradictory on purpose: one client closes pane 2 and the other asks for pane 2's keys, and the
    /// request that arrives second **loses**. The test is run twice with the carriers' roles swapped, so a wall that
    /// favoured the socket, or the relay, would answer differently the second time — which is the only way a
    /// privilege can be seen.
    #[test]
    fn arrival_order_is_the_order_whoever_the_carrier_is() {
        let close = line(r#"{"verb":"pane.close","pane":2}"#);
        let focus = line(r#"{"verb":"pane.focus","pane":2}"#);

        for (first, second) in [("ctl", "phone"), ("phone", "ctl")] {
            let queue = Queue::new();
            let mut state = two_panes();
            let a = Client::connect(&queue, first);
            let b = Client::connect(&queue, second);

            // The close arrives first, and the wall applies it before the second request is even typed.
            a.send(&close);
            let job = next(&queue, "the first request never reached the wall's queue");
            assert!(state.answer(job), "closing a pane of two is a change");
            assert_eq!(
                a.answer("the first answer").expect("an answer"),
                Answer::Changed(Ok(Changed(true))),
                "the first arrival changed the wall"
            );

            // The focus arrives second, on the other carrier, and it is the loser: the premise is in the past.
            b.send(&focus);
            let job = next(&queue, "the second request never reached the wall's queue");
            assert!(!state.answer(job), "a refusal must not dirty the layout");
            let refusal = match b.answer("the second answer") {
                Err(wire::Unreadable::Refused(refusal)) => refusal,
                other => panic!(
                    "the loser of the race must be refused by name, not {other:?} ({first} first, {second} \
                     second)"
                ),
            };
            assert_eq!(
                refusal.why, "no such pane",
                "the pane is gone because the intent that arrived first closed it ({first} first)"
            );
            assert_eq!(
                refusal.kind,
                RefusalKind::Rule,
                "a premise the wall has lost is a rule, not a wall that is busy (#222 rule 2, #198)"
            );
            assert_eq!(
                state.focused(),
                1,
                "the focus stayed where the wall left it: the later request was refused, and an arrival order is not \
                 a race (`{first}` first)"
            );
            assert!(
                state.pane(2).is_none(),
                "and pane 2 is closed exactly once, whichever carrier asked"
            );
        }
    }

    /// **A stale premise is a `rule`, and `busy` is a wall that is not answering** (#222 rule 2).
    ///
    /// A client that reads `busy` into a lost premise retries a request that cannot succeed, on a wall that is
    /// perfectly well; the two words mean two different things to a phone (#198). So `busy` is only ever "the wall
    /// did not answer in time" or "the wall has stopped" and the two names beside them, and nothing a client can
    /// provoke by asking twice is one of them.
    #[test]
    fn a_stale_premise_is_a_rule_and_busy_is_a_wall_that_is_not_answering() {
        let queue = Queue::new();
        let mut state = two_panes();
        let ctl = Client::connect(&queue, "ctl");
        let phone = Client::connect(&queue, "phone");

        ctl.send(&line(r#"{"verb":"pane.close","pane":2}"#));
        let job = next(&queue, "the close never reached the wall's queue");
        assert!(state.answer(job));

        phone.send(&line(r#"{"verb":"pane.close","pane":2}"#));
        let job = next(&queue, "the second close never reached the wall's queue");
        assert!(!state.answer(job));
        let refusal = match phone.answer("the second close's answer") {
            Err(wire::Unreadable::Refused(refusal)) => refusal,
            other => panic!("a close on a pane another client closed is refused, not {other:?}"),
        };
        assert_eq!(refusal.why, "no such pane");
        assert_eq!(
            refusal.kind,
            RefusalKind::Rule,
            "the premise is gone and retrying will not bring it back, so this is a rule and never a busy (#222)"
        );

        // The claim from the other side: `busy` is the wall's silence, and it is four names, none of them a premise.
        let busy: Vec<&str> = REFUSALS
            .iter()
            .filter(|(_, kind)| *kind == RefusalKind::Busy)
            .map(|(why, _)| *why)
            .collect();
        assert_eq!(
            busy,
            [
                "the relay is not answering",
                "the wall did not answer",
                "the wall has no source of chance",
                "the wall has stopped"
            ],
            "`busy` is reserved for a wall that is not answering (#222): a fifth name here is a refusal a client \
             would retry when retrying cannot help"
        );
        for premise in [
            "no such pane",
            "no such workspace",
            "the last pane on the wall",
            "the pane is already in that workspace",
        ] {
            assert_eq!(
                refusal_kind(premise),
                Some(RefusalKind::Rule),
                "`{premise}` is the wall's own state and not a wall that is busy (#222)"
            );
        }
        drop(ctl);
        drop(phone);
    }

    /// **Two clients that agree are not a special case** (#222 rule 5): identical intents are applied twice, and the
    /// second answers `changed: false` because nothing on screen moved.
    ///
    /// The falsifier is a wall that special-cases agreement — coalescing the second into the first, or refusing it as
    /// a duplicate — which would make "did my press land?" a question a client answers by guessing.
    #[test]
    fn two_clients_that_agree_are_applied_twice_and_the_second_changed_nothing() {
        let queue = Queue::new();
        let mut state = two_panes();
        // A split focuses the pane it made, so the keys are put back on the wall's first pane: both clients are then
        // asking for a move, which is the pair of intents that agree.
        assert!(state.focus_pane(1), "the first pane has the keys");
        let first = Client::connect(&queue, "ctl");
        let second = Client::connect(&queue, "phone");
        let focus = line(r#"{"verb":"pane.focus","pane":2}"#);

        first.send(&focus);
        let job = next(&queue, "the first focus never reached the wall's queue");
        assert!(state.answer(job));
        assert_eq!(
            first.answer("the first answer").expect("an answer"),
            Answer::Changed(Ok(Changed(true))),
            "the first client moved the keys, and the wall says so"
        );

        second.send(&focus);
        let job = next(&queue, "the second focus never reached the wall's queue");
        assert!(
            !state.answer(job),
            "the same request on the same pane moves nothing"
        );
        assert_eq!(
            second.answer("the second answer").expect("an answer"),
            Answer::Changed(Ok(Changed(false))),
            "the second client's identical intent is applied and answers `changed: false` (#222 rule 5)"
        );
        assert_eq!(
            state.focused(),
            2,
            "both clients asked for the same pane, and it has the keys"
        );
        drop(first);
        drop(second);
    }

    /// **The origin of a cursor is the device's name, and the frame cannot choose it** (#222 rule 4).
    ///
    /// A relay's carrier knows which paired device a frame is from — the name `devices.toml` holds beside the hash —
    /// and the session loop stamps it on the request. So a cursor, a refusal and a log line name the same thing, and
    /// a phone that types somebody else's name in its frame is not that person's hand.
    #[test]
    fn a_cursor_is_named_by_the_carrier_that_carried_it() {
        let queue = Queue::new();
        let mut state = State::default();
        let phone = Client::connect(&queue, "phone");

        // The frame's own `origin` is a well-formed client name, and it is not the one the wall keeps.
        phone.send(&line(
            r#"{"verb":"cursor.move","at":[420.0,180.0],"origin":"the other phone"}"#,
        ));
        let job = next(&queue, "the cursor move never reached the wall's queue");
        assert!(state.answer(job));

        let drawn: Vec<&str> = state
            .cursors
            .keys()
            .map(crate::verb::Origin::as_str)
            .collect();
        assert_eq!(
            drawn,
            ["phone"],
            "the cursor is the carrier's client, not the name the frame typed (#222 rule 4)"
        );
        assert_eq!(
            state.cursors[&Origin::named("phone")].at,
            (420.0, 180.0),
            "and it is where the request put it"
        );
        assert!(
            !state
                .cursors
                .contains_key(&Origin::named("the other phone")),
            "a frame cannot move another client's cursor by naming it"
        );
        drop(phone);
    }

    /// **A carrier that carries anybody renounces nothing** (#222): the socket's own client has a name (`ctl`), and a
    /// loopback with none is the local client's default — so the fallback is the same origin `State::apply` gives an
    /// absent one, not a third spelling of "nobody".
    #[test]
    fn an_unnamed_carrier_leaves_the_origin_to_the_local_client() {
        let queue = Queue::new();
        let mut state = State::default();
        let ctl = Client::anonymous(&queue);
        ctl.send(&line(r#"{"verb":"cursor.move","at":[10.0,20.0]}"#));
        let job = next(&queue, "the cursor move never reached the wall's queue");
        assert!(state.answer(job));
        assert_eq!(
            state.cursors.keys().map(Origin::as_str).collect::<Vec<_>>(),
            [Origin::ctl().as_str()],
            "a request with no name and a carrier with no name is the local client"
        );
        drop(ctl);
    }
}

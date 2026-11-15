//! **A client's rules** (#197): `docs/mobile-remote.md` decision 1, as one value and as four tests.
//!
//! A phone that controls the wall is a **client**, and the client model is small enough to write down and easy
//! enough to get wrong that prose is not enough. The rules, in the note's order:
//!
//! 1. **Read, then act.** The projection is the only truth a client has, so it reads the wall before it changes
//!    anything, and reads again to see what the change did.
//! 2. **A drop is not a memory.** A client that loses its connection keeps nothing. It does not queue intents and
//!    it does not replay them: "I closed pane 4" is a fact about a wall that no longer exists.
//! 3. **A wall that changed elsewhere is read, not corrected.** When a client comes back, the first thing it does
//!    is read — the wall may have moved while it was away, and its next intent is composed from what it just read.
//! 4. **A version it cannot speak is a stop.** A refusal carries its kind (#198); `version` means *update the app*,
//!    not *retry*. A phone has no operator to read the sentence, so the kind is the whole decision.
//!
//! This module is the client, not a test-only shape: [`Client`] holds a [`Carrier`] (#195), so the same rules hold
//! whether the wire is a loopback pair in this process or a relay to a device in another room. `cargo test
//! control::client` drives all four over the wall's own session loop — the door the unix socket uses.
//!
//! **What it deliberately is not**: a renderer. It never reads the fleet, and it has no `Scene` (#201) — that read
//! belongs to a viewer, and a client that could draw the water would have the picture (`ADR-0005` decision 7).

use super::carrier::Carrier;
use super::projection::Wall;
use super::sessions::{self, Session};
use super::watch::Watch;
use super::wire::{self, Answer, Refusal, Request, Unreadable};
use super::{Changed, Intent, PROTOCOL, RefusalKind};
use crate::ui::ViewId;

/// **What a client may do about a refusal** (#198): the four categories of "no", as the four actions
/// `docs/control.md` names beside them.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Recourse {
    /// `rule`: the wall will not do that. **Do not send it again** — read the wall, because a rule changes when the
    /// wall does, and the projection is how a client learns that.
    Read,
    /// `input`: the request was malformed. Fix the call, then send it again.
    Fix,
    /// `version`: **this client speaks a version the wall does not.** Send nothing more; update the app.
    Update,
    /// `busy`: the wall is not answering now. Retry, and do not read it as a refusal of the change.
    Retry,
}

impl RefusalKind {
    /// **The one action this kind names** (#198). A client has no operator to read the sentence, so the category is
    /// the whole of what it must decide.
    ///
    /// The match is over all four kinds and has no catch-all (`INV-07`): a fifth kind is a compile error here, which
    /// is exactly where a client's behaviour should be decided rather than defaulted.
    pub fn recourse(self) -> Recourse {
        match self {
            RefusalKind::Rule => Recourse::Read,
            RefusalKind::Input => Recourse::Fix,
            RefusalKind::Version => Recourse::Update,
            RefusalKind::Busy => Recourse::Retry,
        }
    }
}

impl Refusal {
    /// The action this refusal's kind names.
    pub fn recourse(&self) -> Recourse {
        self.kind.recourse()
    }
}

/// **Why a client's request did not land**: the wall's no (#198), or this client's own rule.
///
/// A refusal was *sent and answered*; the other three are a client that never got that far, and saying which is the
/// difference between "the wall said no" and "there is no wall" — a distinction a phone draws on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Denied {
    /// **The wall said no**, with the kind its recourse comes from.
    Refused(Refusal),
    /// There is no connection: this client is not talking to a wall, yet or any more.
    NoConnection,
    /// **Nothing has been read on this connection.** Read, then act.
    NotRead,
    /// **No watch is open here** (#221): a publish is only ever the answer to a watch this client asked for and was
    /// granted, and a client that takes one without having asked is reading a wire it did not set up.
    NotWatching,
    /// The carrier failed, or the wall hung up, while the request was out — the connection went with it.
    Lost,
    /// The wall answered something this protocol does not have.
    Unreadable(Unreadable),
}

impl Denied {
    /// What a client may do about it, when the wall named a category. `None` is this client's own rule: it has to
    /// reconnect or read, and no wall said anything to decide for it.
    pub fn recourse(&self) -> Option<Recourse> {
        match self {
            Denied::Refused(refusal) => Some(refusal.recourse()),
            Denied::NoConnection
            | Denied::NotRead
            | Denied::NotWatching
            | Denied::Lost
            | Denied::Unreadable(_) => None,
        }
    }
}

/// **What this client knows.** One value, so "a projection from the connection before" is not a state the type can
/// express: connecting and disconnecting both leave it [`Known::Nothing`].
enum Known {
    /// Nothing has been read on this connection. A client in this state may read and may not act.
    Nothing,
    /// The projection it last read — **the only thing it knows about the wall**.
    Wall(Wall),
    /// A refusal a client may not argue with: the wall does not speak this client's version (#198's `version`).
    Stopped(Refusal),
}

/// **A client of the wall**: a carrier, the one protocol it speaks, and the projection it last read.
///
/// ```text
/// connect ─► read ─► act ─► read          one connection
///    ▲                                    a read is always before an act
///    └── new connection, known = nothing  a connection that came back knows nothing
/// ```
pub struct Client<C: Carrier> {
    /// The protocol this build speaks. One number: a client does not negotiate (`docs/control.md`'s "refused,
    /// never negotiated").
    version: u32,
    /// The connection, when there is one. **A drop takes the projection with it** — there is no field here that
    /// could outlive its wire, because that is how a stale projection happens.
    carrier: Option<C>,
    /// What it knows, which is only ever what it has just read.
    known: Known,
    /// **The watch this connection keeps** (#221): the version it holds, and whether the wall is publishing to it. It
    /// is shut by every way a connection can end, because a watch is a connection's and not a memory's — the client
    /// reads before it asks again (`docs/mobile-remote.md` decision 1).
    session: Session,
}

impl<C: Carrier> Client<C> {
    /// A client that speaks this build's protocol ([`PROTOCOL`]).
    pub fn new() -> Client<C> {
        Client::speaking(PROTOCOL)
    }

    /// **A client built against another protocol** — what an app from before the wall's update is. It sends its own
    /// version, and the wall refuses rather than negotiating (#198).
    pub fn speaking(version: u32) -> Client<C> {
        Client {
            version,
            carrier: None,
            known: Known::Nothing,
            session: Session::new(),
        }
    }

    /// **Connect, and know nothing.** A new connection is not a new memory: whatever the last one read is gone, and
    /// nothing is sent on this one until the client asks for the wall.
    pub fn connect(&mut self, carrier: C) {
        self.carrier = Some(carrier);
        self.known = Known::Nothing;
        // **A new connection is not a new memory**, and it is not a watch either (#221): the phone reads, then asks.
        self.session.shut();
    }

    /// Let go: the connection and the projection both. **Nothing is queued here**, because an intent is a change and
    /// a change composed against a wall nobody is reading is a change made wrongly.
    pub fn disconnect(&mut self) {
        self.carrier = None;
        self.known = Known::Nothing;
        self.session.shut();
    }

    /// Whether this client has a connection. It may still know nothing (`read` has not been called).
    pub fn is_connected(&self) -> bool {
        self.carrier.is_some()
    }

    /// The projection this client last read — the only thing it knows about the wall, or `None`.
    pub fn wall(&self) -> Option<&Wall> {
        match &self.known {
            Known::Wall(wall) => Some(wall),
            Known::Nothing | Known::Stopped(_) => None,
        }
    }

    /// The pane the wall had the keys on, **from the projection this client holds** — never from a memory of an
    /// earlier read.
    pub fn focused(&self) -> Option<ViewId> {
        self.wall().and_then(|wall| wall.focused)
    }

    /// **Read the wall** (#181): ask for the projection, keep what came back, and hand it to the caller.
    ///
    /// A read is the only way this client learns anything. A refusal stops it when the kind is `version`, and a line
    /// it cannot read leaves it knowing nothing — so the next act is refused until it has read again.
    pub fn read(&mut self) -> Result<Wall, Denied> {
        if let Known::Stopped(ref refusal) = self.known {
            return Err(Denied::Refused(refusal.clone()));
        }
        let Some(carrier) = self.carrier.as_mut() else {
            self.known = Known::Nothing;
            return Err(Denied::NoConnection);
        };
        let request = wire::request_line_at(self.version, &Request::Wall);
        let outcome = exchange(carrier, &request);
        let line = match outcome {
            Ok(line) => line,
            Err(denied) => {
                // **A carrier that failed is a connection that went**: a client that kept the projection through
                // a broken wire would draw a wall it can no longer see.
                self.disconnect();
                return Err(denied);
            }
        };
        match wire::answer(&line) {
            Ok(Answer::Wall(wall)) => {
                // **The version the read answers with is the one this client holds** (#221): a publish is expected to
                // be the next one, and anything else is a read.
                self.session.read(wall.seq);
                self.known = Known::Wall(wall.clone());
                Ok(wall)
            }
            // A read answered with anything else is a wall this client does not understand; forget what it held,
            // because it cannot say whether that is still true.
            Ok(other) => {
                self.known = Known::Nothing;
                Err(Denied::Unreadable(Unreadable::Malformed(format!(
                    "a read was answered with {other:?}"
                ))))
            }
            Err(Unreadable::Refused(refusal)) => Err(self.refused(refusal)),
            Err(unreadable) => {
                self.known = Known::Nothing;
                Err(Denied::Unreadable(unreadable))
            }
        }
    }

    /// **Act on the wall** — a settled change, never a gesture (`docs/control.md`).
    ///
    /// Refused **without touching the carrier** when this client has not read on this connection: that is the
    /// client's first rule, and the reason a drop cannot become a queue.
    pub fn act(&mut self, intent: Intent) -> Result<Changed, Denied> {
        match &self.known {
            Known::Stopped(refusal) => return Err(Denied::Refused(refusal.clone())),
            Known::Nothing => {
                return Err(if self.carrier.is_some() {
                    Denied::NotRead
                } else {
                    Denied::NoConnection
                });
            }
            Known::Wall(_) => {}
        }
        let Some(carrier) = self.carrier.as_mut() else {
            return Err(Denied::NoConnection);
        };
        let request = wire::request_line_at(self.version, &Request::Intent(intent));
        let outcome = exchange(carrier, &request);
        let line = match outcome {
            Ok(line) => line,
            Err(denied) => {
                self.disconnect();
                return Err(denied);
            }
        };
        match wire::answer(&line) {
            Ok(Answer::Changed(Ok(changed))) => Ok(changed),
            // A refusal moved nothing, so the projection the intent was composed from is still the wall's.
            Err(Unreadable::Refused(refusal)) => Err(self.refused(refusal)),
            // An unreadable answer cannot say whether the change landed. Forget the projection: read again.
            Ok(other) => {
                self.known = Known::Nothing;
                Err(Denied::Unreadable(Unreadable::Malformed(format!(
                    "an intent was answered with {other:?}"
                ))))
            }
            Err(unreadable) => {
                self.known = Known::Nothing;
                Err(Denied::Unreadable(unreadable))
            }
        }
    }

    /// **The watch this connection keeps** (#221): the version of the projection the client holds, and whether the
    /// wall is publishing to it.
    pub fn session(&self) -> &Session {
        &self.session
    }

    /// **Ask the wall to publish its projection whenever it changes** (#221) — the one request on this plane whose
    /// answer keeps coming.
    ///
    /// Refused by this client's own rule until it has **read on this connection** ([`Denied::NotRead`]): decision 1 is
    /// unchanged by a watch, and a watch that resumed without a read would be a client believing what it remembered.
    /// Refused by the *wall* with `rule` when the carrier cannot carry one — the socket answers one request with one
    /// answer, because a `ctl` script must not be able to hold it open for ever — and a `rule`'s recourse is to read,
    /// which is exactly the fallback a controller wants.
    pub fn watch(&mut self) -> Result<(), Denied> {
        match &self.known {
            Known::Stopped(refusal) => return Err(Denied::Refused(refusal.clone())),
            Known::Nothing => {
                return Err(if self.carrier.is_some() {
                    Denied::NotRead
                } else {
                    Denied::NoConnection
                });
            }
            Known::Wall(_) => {}
        }
        let Some(carrier) = self.carrier.as_mut() else {
            return Err(Denied::NoConnection);
        };
        let request = wire::request_line_at(self.version, &Request::Watch(Watch::Wall));
        let line = match exchange(carrier, &request) {
            Ok(line) => line,
            Err(denied) => {
                self.disconnect();
                return Err(denied);
            }
        };
        match wire::answer(&line) {
            Ok(Answer::Watching(_)) => {
                self.session.opened();
                Ok(())
            }
            // A refusal moved nothing, so the projection the ask was composed from is still the wall's.
            Err(Unreadable::Refused(refusal)) => Err(self.refused(refusal)),
            // An answer that is not a grant cannot say the stream exists. Forget the projection: read again.
            Ok(other) => {
                self.known = Known::Nothing;
                Err(Denied::Unreadable(Unreadable::Malformed(format!(
                    "a watch was answered with {other:?}"
                ))))
            }
            Err(unreadable) => {
                self.known = Known::Nothing;
                Err(Denied::Unreadable(unreadable))
            }
        }
    }

    /// **Take a projection the wall published**, without waiting (#221): `Ok(None)` is "nothing has arrived", which is
    /// what most frames of a watch are.
    ///
    /// A publish one version after the one this client holds is the wall now. **A number it did not expect makes it
    /// read** — the read's projection is what comes back — because the one thing a client may not do with a line it
    /// cannot place is reconstruct a history from it. And a client that is not watching takes nothing at all: a
    /// publish is only ever the answer to a watch this client asked for.
    pub fn published(&mut self) -> Result<Option<Wall>, Denied> {
        if !self.session.is_open() {
            return Err(if self.carrier.is_some() {
                Denied::NotWatching
            } else {
                Denied::NoConnection
            });
        }
        let Some(carrier) = self.carrier.as_mut() else {
            return Err(Denied::NoConnection);
        };
        // **Taken, never waited for** (#221): a phone polls its wire between frames, and a client that blocked here
        // would be a screen that stops drawing until the wall moves.
        let line = match carrier.read_line_within(std::time::Duration::ZERO) {
            Ok(Some(line)) => line,
            // End of file: the connection went with it, and so did the watch.
            Ok(None) => {
                self.disconnect();
                return Err(Denied::Lost);
            }
            // A deadline is not a failure: nothing has arrived yet.
            Err(error) if super::watch::timed_out(&error) => return Ok(None),
            Err(_) => {
                self.disconnect();
                return Err(Denied::Lost);
            }
        };
        match wire::answer(&line) {
            Ok(Answer::Published(_, wall)) => match self.session.publish(wall.seq) {
                sessions::Publish::Take => {
                    self.known = Known::Wall(wall.clone());
                    Ok(Some(wall))
                }
                // **A version it did not expect**: read, and hold what the read answers with.
                sessions::Publish::Read => self.read().map(Some),
            },
            Ok(other) => {
                self.known = Known::Nothing;
                self.session.shut();
                Err(Denied::Unreadable(Unreadable::Malformed(format!(
                    "a watch published {other:?}"
                ))))
            }
            Err(Unreadable::Refused(refusal)) => Err(self.refused(refusal)),
            Err(unreadable) => {
                self.known = Known::Nothing;
                self.session.shut();
                Err(Denied::Unreadable(unreadable))
            }
        }
    }

    /// **A refusal, and what it does to the client.** A `version` refusal is a stop (#198): the wall will never
    /// answer this build, so sending again is a client arguing with an update it needs. Every other kind leaves the
    /// connection usable — the wall is still there and the next request may be a fresh one.
    fn refused(&mut self, refusal: Refusal) -> Denied {
        if refusal.kind == RefusalKind::Version {
            self.known = Known::Stopped(refusal.clone());
        }
        Denied::Refused(refusal)
    }
}

impl<C: Carrier> Default for Client<C> {
    fn default() -> Client<C> {
        Client::new()
    }
}

/// **One request out and one answer line back** — the only place a client touches its carrier, so "nothing is sent
/// before a read" is one function and not a habit.
///
/// End of file is a wall that hung up: a lost connection, never a blank answer.
fn exchange<C: Carrier>(carrier: &mut C, request: &str) -> Result<String, Denied> {
    carrier.write_line(request).map_err(|_| Denied::Lost)?;
    match carrier.read_line() {
        Ok(Some(line)) => Ok(line),
        Ok(None) | Err(_) => Err(Denied::Lost),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::carrier::{Loopback, Peer};
    use crate::control::projection::{PaneView, Tiling, Wall as Projection};
    use crate::control::server::{self, Job};
    use crate::ui::{PaneContent, State};
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc::{self, Receiver, Sender};

    /// **A wall in this process.** One thread is the UI thread and does what `ui::poll` does between frames: it
    /// takes each [`Job`] the session loop hands over and answers it from one `State`. Every connection is the
    /// wall's own session loop — [`server::serve_session`] over a [`Loopback`] — so a test drives the door the unix
    /// socket drives and nothing about the plane is special-cased for it.
    struct Served {
        stop: Arc<AtomicBool>,
        jobs: Sender<Job>,
    }

    impl Served {
        /// The default wall: one map pane and three workspaces.
        fn new() -> Served {
            let (jobs, handed_over) = mpsc::channel::<Job>();
            std::thread::Builder::new()
                .name("client-test-wall".into())
                .spawn(move || {
                    let mut state = State::default();
                    while let Ok(job) = handed_over.recv() {
                        let (answer, _) =
                            crate::control::authority::answer(&mut state, &job.request);
                        job.answer(answer);
                    }
                })
                .expect("the wall's thread starts");
            Served {
                stop: Arc::new(AtomicBool::new(false)),
                jobs,
            }
        }

        /// **A connection, and the lines a client sends on it.** The tap is on the client's side of the wire, so a
        /// test can say what the client sent rather than infer it from what the wall happened to answer — which is
        /// the whole of "it sent nothing it had queued".
        fn connected(&self) -> (Tapped, Receiver<String>) {
            let (wall_end, peer) = Loopback::pair();
            let jobs = self.jobs.clone();
            let stop = Arc::clone(&self.stop);
            std::thread::Builder::new()
                .name("client-test-session".into())
                .spawn(move || server::serve_session(wall_end, &jobs, &stop))
                .expect("the session loop starts");
            Tapped::new(peer)
        }
    }

    /// A client's carrier with a tap: every line it writes is recorded and then carried, so a test hears the wire
    /// in the client's own voice.
    struct Tapped {
        peer: Peer,
        sent: Sender<String>,
    }

    impl Tapped {
        fn new(peer: Peer) -> (Tapped, Receiver<String>) {
            let (sent, lines) = mpsc::channel();
            (Tapped { peer, sent }, lines)
        }
    }

    impl Carrier for Tapped {
        fn read_line(&mut self) -> std::io::Result<Option<String>> {
            self.peer.read_line()
        }

        fn write_line(&mut self, line: &str) -> std::io::Result<()> {
            let _ = self.sent.send(line.to_string());
            self.peer.write_line(line)
        }

        fn read_line_within(
            &mut self,
            within: std::time::Duration,
        ) -> std::io::Result<Option<String>> {
            self.peer.read_line_within(within)
        }

        fn hang_up(&mut self) {
            self.peer.hang_up();
        }
    }

    /// Every line a client has sent since the test last looked.
    fn sent(lines: &Receiver<String>) -> Vec<String> {
        lines.try_iter().collect()
    }

    /// Every pane id a projection names, in chip and pane order.
    fn ids(wall: &Projection) -> Vec<ViewId> {
        panes(wall).iter().map(|pane| pane.id).collect()
    }

    fn panes(wall: &Projection) -> Vec<&PaneView> {
        wall.workspaces
            .iter()
            .flat_map(|workspace| workspace.panes.iter())
            .collect()
    }

    fn names(wall: &Projection) -> Vec<&str> {
        wall.workspaces
            .iter()
            .map(|workspace| workspace.name.as_str())
            .collect()
    }

    fn tiling_names(tiling: &Tiling, id: ViewId) -> bool {
        match tiling {
            Tiling::Leaf { pane } => *pane == id,
            Tiling::Split { a, b, .. } => tiling_names(a, id) || tiling_names(b, id),
        }
    }

    fn arranges(wall: &Projection, id: ViewId) -> bool {
        wall.workspaces
            .iter()
            .filter_map(|workspace| workspace.tiling.as_ref())
            .any(|tiling| tiling_names(tiling, id))
    }

    /// **Read, then act, then read** (#197 rule 1): a client connects, reads the wall, closes a pane, reads again —
    /// and the two projections differ in exactly the pane it closed.
    ///
    /// The falsifier is a close that took a neighbour with it, or a second read answered from the client's own
    /// memory: either way a pane row or a workspace differs that nobody closed.
    #[test]
    fn a_read_an_act_and_a_read_move_exactly_one_pane() {
        let wall = Served::new();
        let (carrier, lines) = wall.connected();
        let mut client = Client::new();
        client.connect(carrier);

        let before = client.read().expect("a connected client may read");
        assert_eq!(
            ids(&before),
            vec![
                before
                    .focused
                    .expect("the default wall has a pane with the keys")
            ],
            "the wall a test starts from is one map pane, and the client must read it"
        );
        assert_eq!(
            sent(&lines),
            vec![wire::request_line(&Request::Wall)],
            "a read is one line, and the projection it earned is the only thing the client knows"
        );

        // Two panes, so there is one to close that is not the last one on the wall.
        let first = before.focused.expect("a pane with the keys");
        client
            .act(Intent::SplitPane {
                pane: first,
                content: PaneContent::Map,
            })
            .expect("a client that has just read may split a pane");
        let split = client.read().expect("the read after the split");
        let second = ids(&split)
            .into_iter()
            .find(|id| *id != first)
            .expect("the split made a second pane, and the read must name it");
        // The split put the keys on the new pane; give them back, so the only thing this test closes is a pane
        // nobody has focused.
        client
            .act(Intent::FocusPane(first))
            .expect("the first pane is still on the wall");
        let two = client.read().expect("the read after giving the keys back");
        assert_eq!(
            two.focused,
            Some(first),
            "the projection must say where the keys are before this test moves a pane nobody focused"
        );

        client
            .act(Intent::ClosePane(second))
            .expect("a pane beside another is not the last pane on the wall");
        let after = client.read().expect("the read after the close");

        assert_eq!(
            ids(&after),
            vec![first],
            "closing pane {second} must leave a projection naming exactly the pane that was left"
        );
        // Everything but the pane: the wall, its workspaces, its focus and its full-size pane are where they were.
        assert_eq!(after.active, two.active, "the active workspace moved");
        assert_eq!(
            after.focused, two.focused,
            "closing a pane nobody had focused must not move the keys"
        );
        assert_eq!(after.maximised, two.maximised, "the full-size pane moved");
        assert_eq!(
            names(&after),
            names(&two),
            "closing a pane must not rename or remove a workspace"
        );
        for pane in panes(&after) {
            let was = panes(&two)
                .into_iter()
                .find(|before| before.id == pane.id)
                .expect("every pane left after the close was there before it");
            assert_eq!(
                pane, was,
                "pane {} changed in a projection where only {second} was closed",
                pane.id
            );
        }
        assert!(
            !arranges(&after, second),
            "the arrangement still names pane {second}, which the wall closed"
        );
        assert!(
            arranges(&two, second),
            "the arrangement before the close did not name pane {second}: the projection is not what the wall held"
        );
    }

    /// **A drop is not a memory** (#197 rule 2): a client that loses the connection mid-sequence, reconnects and
    /// reads sends nothing it had queued — the wall is handed no request between the drop and the read.
    ///
    /// The falsifier is the obvious implementation: keep the intent in a queue, flush it when the wire comes back.
    /// The tap on the client's side of the carrier is what makes that visible; a test that watched the wall's
    /// answers would see the queued close land after a read that looked perfectly normal.
    #[test]
    fn a_client_that_drops_sends_nothing_it_had_queued() {
        let wall = Served::new();
        let (carrier, lines) = wall.connected();
        let mut client = Client::new();
        client.connect(carrier);
        let before = client.read().expect("the first read");
        let queued = before
            .focused
            .expect("the intent the client meant to send names a pane");
        assert_eq!(
            sent(&lines),
            vec![wire::request_line(&Request::Wall)],
            "the first read is one request"
        );

        // The connection goes with an intent still in the client's hand.
        client.disconnect();
        let denied = client
            .act(Intent::ClosePane(queued))
            .expect_err("a client with no connection cannot act");
        assert_eq!(
            denied,
            Denied::NoConnection,
            "a client that cannot send must say so, not keep the intent for later (#197)"
        );
        assert!(
            sent(&lines).is_empty(),
            "the client wrote on a connection it had dropped"
        );

        // It comes back. A wall that changed while the client was away would be changed wrongly by a replay, so
        // the first thing on the new wire is a read and nothing else.
        let (carrier, lines) = wall.connected();
        client.connect(carrier);
        assert!(
            sent(&lines).is_empty(),
            "the client sent something on reconnecting: a drop is not a memory (#197)"
        );
        let after = client.read().expect("the read on resume");
        assert_eq!(
            after, before,
            "the wall changed between the drop and the read: the client replayed an intent"
        );
        assert_eq!(
            sent(&lines),
            vec![wire::request_line(&Request::Wall)],
            "the first line after a resume is the read and only the read"
        );
        // A queue flushed *after* the read would still be a replay, and the projection the read returned could not
        // show it — so ask the wire again, and then ask the wall.
        assert!(
            sent(&lines).is_empty(),
            "the client sent something after the read that it had queued across the drop (#197)"
        );
        assert_eq!(
            client.read().expect("a second read").workspaces[0]
                .panes
                .len(),
            1,
            "the pane the client meant to close is still open: the drop kept nothing"
        );
    }

    /// **A wall that changed elsewhere is read, not corrected** (#197 rule 3): another client moves the keys while
    /// the first is away; the first reads, sees the change, and its next intent is composed from what it read.
    ///
    /// The falsifier is a client that composes from its remembered focus: it closes the pane it left behind, which
    /// is a wall corrected back to a state that no longer exists.
    #[test]
    fn a_wall_that_changed_while_a_client_was_away_is_read_not_corrected() {
        let wall = Served::new();
        let (carrier, _lines) = wall.connected();
        let mut away = Client::new();
        away.connect(carrier);

        let start = away.read().expect("the first read");
        let first = start.focused.expect("a pane with the keys");
        away.act(Intent::SplitPane {
            pane: first,
            content: PaneContent::Map,
        })
        .expect("a client that has read may split a pane");
        let split = away.read().expect("the read after the split");
        let other = ids(&split)
            .into_iter()
            .find(|id| *id != first)
            .expect("the split made a second pane");
        // The split leaves the keys on the new pane; the first client gives them back before it goes away, so the
        // change the second client makes is a change to a wall the first one has read.
        away.act(Intent::FocusPane(first))
            .expect("the first pane is still on the wall");
        let two = away.read().expect("the read after giving the keys back");
        assert_eq!(
            two.focused,
            Some(first),
            "the first client must leave with the keys on the first pane, so the second client's change is visible"
        );

        // The first client goes away mid-sequence.
        away.disconnect();

        // Another client gives the other pane the keys while the first is gone.
        let (carrier, _lines) = wall.connected();
        let mut elsewhere = Client::new();
        elsewhere.connect(carrier);
        let seen = elsewhere
            .read()
            .expect("the second client reads before it acts");
        assert_eq!(seen.focused, Some(first));
        elsewhere
            .act(Intent::FocusPane(other))
            .expect("the second client may give a pane the keys");
        elsewhere.disconnect();

        // The first comes back, reads, and composes its next intent from what it just read.
        let (carrier, _lines) = wall.connected();
        away.connect(carrier);
        let resumed = away.read().expect("the read on resume");
        assert_eq!(
            resumed.focused,
            Some(other),
            "the client read the wall and must see the pane another client focused — not the one it remembered"
        );
        let composed = resumed
            .focused
            .expect("the projection names the pane the next intent is about");
        away.act(Intent::ClosePane(composed))
            .expect("the pane it read as focused is not the last one");

        let after = away.read().expect("the read after the close");
        assert!(
            !ids(&after).contains(&other),
            "the client closed pane {composed} but the pane that changed is {other}: it corrected the wall \
             from its memory instead of reading it (#197)"
        );
        assert!(
            ids(&after).contains(&first),
            "the client closed the pane it remembered ({first}) instead of the one it just read ({other})"
        );
        assert_eq!(
            after.focused,
            Some(first),
            "closing the focused pane must leave the keys on the pane that is left"
        );
    }

    /// **A version it cannot speak is a stop** (#197 rule 4): a client whose protocol the wall does not speak is
    /// told so by name, and maps the `version` kind to *update* — not to *retry*, and not to a sentence a phone
    /// would have to read.
    #[test]
    fn a_version_the_wall_does_not_speak_stops_the_client() {
        let wall = Served::new();
        let (carrier, lines) = wall.connected();
        let mut client = Client::speaking(PROTOCOL + 1);
        client.connect(carrier);

        let denied = client
            .read()
            .expect_err("a client from another protocol cannot read this wall");
        assert_eq!(
            sent(&lines),
            vec![wire::request_line_at(PROTOCOL + 1, &Request::Wall)],
            "the refused request is the only line the client wrote"
        );
        let refusal = match &denied {
            Denied::Refused(refusal) => refusal,
            other => panic!("the wall must refuse by name (#198), got {other:?}"),
        };
        assert_eq!(
            refusal.why, "unsupported protocol version",
            "a version the wall does not speak is refused by name, never negotiated"
        );
        assert_eq!(
            refusal.kind,
            RefusalKind::Version,
            "the refusal's kind is `version`; a client cannot decide from the sentence (#198)"
        );
        assert_eq!(
            refusal.recourse(),
            Recourse::Update,
            "a version refusal is an update, not a retry: the wall will never answer this build (#197)"
        );
        assert_ne!(
            refusal.recourse(),
            Recourse::Retry,
            "retrying a version refusal is a phone arguing with an update it needs"
        );

        // **A stop**: sending again is a client retrying a wall that can never answer it.
        assert_eq!(
            client.read().expect_err("a stopped client keeps saying so"),
            Denied::Refused(refusal.clone()),
            "a stopped client must give the same answer, not a new request"
        );
        assert!(
            sent(&lines).is_empty(),
            "the stopped client sent another line: a version it cannot speak is a stop (#197)"
        );
        assert!(
            client.wall().is_none(),
            "a client the wall stopped must not hold a projection"
        );
    }

    /// **Every kind names exactly one recourse** (`INV-07`'s rule for the client): four kinds, four actions, and no
    /// default. A kind whose action is "whatever the last arm said" is a client that cannot decide for a phone.
    #[test]
    fn every_refusal_kind_names_one_recourse() {
        let kinds = [
            RefusalKind::Rule,
            RefusalKind::Input,
            RefusalKind::Version,
            RefusalKind::Busy,
        ];
        for kind in kinds {
            let _ = kind.recourse();
        }
        let recourses: Vec<Recourse> = kinds.iter().map(|kind| kind.recourse()).collect();
        assert_eq!(
            recourses,
            vec![
                Recourse::Read,
                Recourse::Fix,
                Recourse::Update,
                Recourse::Retry
            ],
            "INV-07: every refusal kind maps to exactly one action, and a client decides from the kind (#198)"
        );

        let wrong = Denied::Refused(Refusal {
            why: "unsupported protocol version".to_string(),
            kind: RefusalKind::Version,
        });
        assert_eq!(
            wrong.recourse(),
            Some(Recourse::Update),
            "a denial carries its kind's recourse"
        );
        assert_eq!(
            Denied::NoConnection.recourse(),
            None,
            "the client's own reasons are not the wall's no"
        );
    }
}

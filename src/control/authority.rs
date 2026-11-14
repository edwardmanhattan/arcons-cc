//! **The authority, as a role** (#202, `ADR-0006`).
//!
//! In a fleet, one process owns the wall's state and every viewer draws from a `Scene` it is given (#201). This is the
//! seam that makes that a *role* rather than a second program: the same `apply`, the same refusals, the same `Scene`,
//! and the only difference is **which thread calls `apply`**.
//!
//! - [`Role::Local`] is today's wall: it owns everything, and no network exists anywhere in its path. A capture is
//!   `Local`, which is why 45 scenes keep rendering with no authority in the loop at all.
//! - [`Role::Remote`] is a viewer: it asks the authority for a `Scene` and applies what came back. Its **own** inputs
//!   still go through `control::apply` locally until #204 makes them intents on the wire — the role is the plumbing
//!   first, because "who owns the state" is the question a fleet cannot get wrong.
//!
//! **No push, only pull.** A viewer asks for the newest scene (`read: "scene"`) and the `seq` tells it whether anything
//! moved, so a wall that has changed nothing costs one small answer. A push channel is #196's business, where a relay
//! already has to hold a connection open; inventing one here would be two ways to learn about a change.

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, Sender};

use crate::control::carrier::Carrier;
use crate::control::wire::{Answer, Request};
use crate::control::{Changed, Refused};
use crate::ui::State;

/// **Whose state this is** (#202).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Role {
    /// This process is the wall: it applies, it draws, and there is no network.
    Local,
    /// Another process is the wall: this one draws the scenes it is given.
    Remote(PathBuf),
}

impl Role {
    /// The role `ARCONS_AUTHORITY` names.
    ///
    /// Absent or empty is [`Role::Local`] — the default, and the one a capture, a test and a single wall all take. The
    /// word `socket` is this machine's wall (the control socket), and anything else is a path to one: a viewer next to
    /// the wall it mirrors is the case that needs no network at all, and it is the case a fleet's first screen is.
    pub fn from_env() -> Role {
        Role::of(std::env::var("ARCONS_AUTHORITY").ok().as_deref())
    }

    /// The same decision, from a value — so the rule can be tested without a process-wide environment.
    pub fn of(value: Option<&str>) -> Role {
        match value.map(str::trim) {
            None | Some("") => Role::Local,
            Some("socket") => Role::Remote(this_walls_socket()),
            Some(path) => Role::Remote(PathBuf::from(path)),
        }
    }

    /// The words `--self-check` uses.
    pub fn label(&self) -> String {
        match self {
            Role::Local => "this wall (no ARCONS_AUTHORITY)".to_string(),
            Role::Remote(path) => format!("a viewer of {}", path.display()),
        }
    }
}

/// This machine's wall, as a path.
///
/// Unix has one; Windows has no plane at all (a named pipe is a decision nobody has taken), so the word resolves to
/// something a `--self-check` row and a log line can print and nothing can connect to. The refusal belongs where the
/// transport is, not in a parser: `Role::of` stays portable so the row can say what was configured.
fn this_walls_socket() -> PathBuf {
    #[cfg(unix)]
    {
        crate::control::socket_path()
    }
    #[cfg(not(unix))]
    {
        PathBuf::from("socket")
    }
}

/// **Answer one request against the wall that owns it.** The authority's whole vocabulary, in one function.
///
/// A wall's control loop and a viewer's session both come here, so there is one place that decides what a request
/// means and one place that could grow a second opinion (`INV-07`).
pub fn answer(state: &mut State, request: &Request) -> (Answer, bool) {
    match request {
        Request::Intent(intent) => {
            let result = crate::control::apply(state, intent.clone());
            let changed = result.as_ref().map(|changed| changed.0).unwrap_or(false);
            (Answer::Changed(result), changed)
        }
        Request::Wall => (Answer::Wall(crate::control::projection::of(state)), false),
        // **The arrangement, not the projection** (#201): what a viewer's picture is computed from. Built on the frame
        // that asks, which is what "what is on the wall" means.
        Request::Scene => (Answer::Scene(Box::new(state.scene())), false),
    }
}

/// **A viewer's session** (#202): one carrier, one thread, one request at a time.
///
/// The thread owns the carrier and does the blocking, because the UI thread never waits for a client (`INV-05`). The
/// viewer asks (`send`) and picks answers up in `poll` (`next`), which is the same shape the wall's own listener has.
pub struct Session {
    to: Sender<Request>,
    answers: Receiver<Answer>,
    /// Whether a request is out: a viewer that asked again before the answer would queue work it cannot use, and the
    /// scene it wants is the *newest* one, not the next one in a line.
    in_flight: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Session {
    /// Take a carrier and start asking. The carrier is the transport (#195): a socket next to the wall, a relay
    /// later (#196).
    pub fn new(mut carrier: Box<dyn Carrier>) -> std::io::Result<Session> {
        let (to, requests) = mpsc::channel::<Request>();
        let (answers, from) = mpsc::channel::<Answer>();
        let in_flight = Arc::new(AtomicBool::new(false));
        let busy = Arc::clone(&in_flight);
        let thread = std::thread::Builder::new()
            .name("authority-session".into())
            .spawn(move || {
                for request in requests {
                    let line = crate::control::wire::request_line(&request);
                    if carrier.write_line(&line).is_err() {
                        break;
                    }
                    match carrier.read_line() {
                        Ok(Some(line)) => match crate::control::wire::answer(&line) {
                            Ok(answer) => {
                                if answers.send(answer).is_err() {
                                    break;
                                }
                            }
                            // **A refusal is an answer, not a broken line** (`INV-07`): the wall said no by name, the
                            // session says so and carries on. A viewer whose session ended on the first refused press
                            // would be a screen that stops working the first time somebody asks for something
                            // impossible. (Showing it is #198's `kind` and the next ticket; today it is a log line.)
                            Err(why) => {
                                tracing::warn!("authority: {why}");
                            }
                        },
                        // End of file, or a broken carrier: the viewer is on its own until something reconnects.
                        _ => break,
                    }
                    busy.store(false, Ordering::SeqCst);
                }
                // The loop is over for whatever reason; a `send` after this is refused rather than blocking.
                busy.store(true, Ordering::SeqCst);
            })?;
        Ok(Session {
            to,
            answers: from,
            in_flight,
            thread: Some(thread),
        })
    }

    /// **Ask, always** (#204): a viewer's own change is queued even while a scene is out, so a press cannot be lost to
    /// a poll in flight. The queue is ordered, and the loop answers strictly in turn.
    ///
    /// Returns whether the session is still alive; a viewer that cannot ask is a viewer whose next `next()` is `None`.
    pub fn ask(&self, request: Request) -> bool {
        self.in_flight.store(true, Ordering::SeqCst);
        self.to.send(request).is_ok()
    }

    /// Ask for something, unless a request is already out. `false` is "not sent", never "failed".
    pub fn send(&self, request: Request) -> bool {
        if self.in_flight.swap(true, Ordering::SeqCst) {
            return false;
        }
        if self.to.send(request).is_err() {
            // The session has ended: nothing more will be answered, and the caller's next `next()` returns `None`.
            return false;
        }
        true
    }

    /// Take one answer, if one has arrived. Never blocks — `poll` calls this every frame.
    pub fn next(&self) -> Option<Answer> {
        self.answers.try_recv().ok()
    }

    /// Whether a request is still out.
    pub fn busy(&self) -> bool {
        self.in_flight.load(Ordering::SeqCst)
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Dropping the sender ends the loop after the request in flight, so the thread is not left waiting for input
        // that will not come; the join then happens here rather than at process exit.
        let (dead, _) = mpsc::channel();
        drop(std::mem::replace(&mut self.to, dead));
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// A refusal a viewer can say out loud when its authority is not there — one name, one place.
pub const NO_AUTHORITY: &str = "the authority is not reachable";

/// What an authority answers when it cannot apply something: kept here so a viewer's refusal and a wall's are the same
/// words rather than two spellings of the same refusal.
pub fn changed(ok: bool) -> Answer {
    Answer::Changed(Ok(Changed(ok)))
}

/// A refusal, in the same shape a wall sends.
pub fn refused(why: &'static str) -> Answer {
    Answer::Changed(Err(Refused(why)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::carrier::Loopback;

    /// **Local unless the environment names something** (#202): the default is a wall with no network anywhere in its
    /// path, which is what a capture, a test and a single display all are.
    #[test]
    fn a_role_is_this_wall_unless_a_path_is_named() {
        assert_eq!(Role::of(None), Role::Local);
        assert_eq!(Role::of(Some("")), Role::Local);
        assert_eq!(Role::of(Some("   ")), Role::Local);
        assert_eq!(
            Role::of(Some("socket")),
            Role::Remote(this_walls_socket()),
            "the word `socket` is this machine's wall"
        );
        #[cfg(unix)]
        assert_eq!(
            this_walls_socket(),
            crate::control::socket_path(),
            "and on a unix machine that is the socket the wall listens on"
        );
        assert_eq!(
            Role::of(Some("/run/other/control.sock")),
            Role::Remote(PathBuf::from("/run/other/control.sock"))
        );
        assert!(Role::Local.label().contains("this wall"));
    }

    /// **One place answers a request**, whether it is a wall's loop or a viewer's session.
    #[test]
    fn the_authority_answers_an_intent_and_a_scene() {
        let mut state = State::default();
        let (reply, changed) = answer(
            &mut state,
            &Request::Intent(crate::control::Intent::NewWorkspace),
        );
        assert_eq!(reply, Answer::Changed(Ok(Changed(true))));
        assert!(changed, "a new workspace is a change");

        let (reply, changed) = answer(&mut state, &Request::Scene);
        let Answer::Scene(scene) = reply else {
            panic!("a scene request is answered with a scene");
        };
        assert!(!changed, "a read changes nothing");
        assert_eq!(
            scene.workspace.len(),
            state.workspaces.len(),
            "the scene is the wall it was asked of"
        );
        assert_eq!(scene.seq, state.scene_seq, "and says how old it is");
    }

    /// **A viewer applies only what the scene carried** (#202), over a carrier it has never seen (#195).
    ///
    /// The wall on the left owns four workspaces; the viewer on the right starts with one and does nothing but ask.
    /// After one round trip it draws the same arrangement — and before it, it is exactly as it was: nothing at the
    /// transport's end reached into the viewer's state.
    #[test]
    fn a_viewer_applies_only_what_the_scene_carried() {
        let mut wall_state = State::default();
        let start = wall_state.workspaces.len();
        // A wall with something to say: two more workspaces than the default wall has.
        for _ in 0..2 {
            let _ = answer(
                &mut wall_state,
                &Request::Intent(crate::control::Intent::NewWorkspace),
            );
        }
        assert_eq!(wall_state.workspaces.len(), start + 2);

        let (wall_end, peer) = Loopback::pair();
        // The authority, on its own thread, exactly as a wall's loop answers: one request, one answer.
        let serving = std::thread::spawn(move || {
            let mut carrier = wall_end;
            while let Ok(Some(line)) = carrier.read_line() {
                let Ok(request) = crate::control::wire::request(&line) else {
                    break;
                };
                let (answer, _) = answer(&mut wall_state, &request);
                if carrier
                    .write_line(&crate::control::wire::reply(answer))
                    .is_err()
                {
                    break;
                }
            }
        });

        let session = Session::new(Box::new(peer)).expect("a session over a loopback");
        let mut viewer = State::default();
        assert_eq!(
            viewer.workspaces.len(),
            start,
            "the viewer starts as a default wall"
        );

        assert!(session.send(Request::Scene), "the viewer asks once");
        let reply = loop {
            if let Some(reply) = session.next() {
                break reply;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        };
        let Answer::Scene(scene) = reply else {
            panic!("a scene request is answered with a scene");
        };
        assert_eq!(
            viewer.workspaces.len(),
            start,
            "nothing changed the viewer before the scene was applied"
        );

        assert!(viewer.apply_scene(&scene), "the scene is applied");
        assert_eq!(
            viewer.workspaces.len(),
            start + 2,
            "the viewer now draws the wall the authority owns"
        );

        drop(session);
        serving.thread().unpark();
        let _ = serving.join();
    }
    /// **A viewer's press is a request, not a change** (#204): on a screen that draws somebody else's wall, an intent
    /// goes to the authority and **nothing moves here** — `changed: false` is the truth, because no pixel did. The
    /// change arrives with the next `Scene`, which is the only thing that may move a viewer's picture.
    ///
    /// The falsification this is written against: applying locally "because the wire was busy" would move a pane the
    /// authority had not agreed to, and then move it back when the scene landed — a wall that visibly argues with
    /// itself.
    // The field these drive is `State::session`, which only a unix build has (#203).
    #[cfg(unix)]
    #[test]
    fn a_viewer_s_press_asks_and_does_not_change_the_wall() {
        let (wall_end, peer) = Loopback::pair();
        let mut wall_state = State::default();
        let start = wall_state.workspaces.len();

        let serving = std::thread::spawn(move || {
            let mut carrier = wall_end;
            while let Ok(Some(line)) = carrier.read_line() {
                let Ok(request) = crate::control::wire::request(&line) else {
                    break;
                };
                let (answer, _) = answer(&mut wall_state, &request);
                if carrier
                    .write_line(&crate::control::wire::reply(answer))
                    .is_err()
                {
                    break;
                }
            }
        });

        // A viewer: a state with a session and nothing of its own.
        let mut viewer = State::default();
        viewer.session = Some(Session::new(Box::new(peer)).expect("a session"));
        assert!(viewer.is_viewer());

        // The press. The same door every handler uses.
        let outcome = crate::control::apply(&mut viewer, crate::control::Intent::NewWorkspace);
        assert_eq!(
            outcome,
            Ok(Changed(false)),
            "a viewer's intent is a request: nothing changed on this screen"
        );
        assert_eq!(
            viewer.workspaces.len(),
            start,
            "and the picture did not move before the authority agreed"
        );

        // The press's answer: the authority **agreed**, and said so. Then, as `poll` does every frame, the viewer asks
        // for the arrangement — the scene is the only thing that moves its picture.
        let mut agreed = None;
        let mut seen = None;
        for _ in 0..400 {
            while let Some(answer) = viewer.session.as_ref().and_then(Session::next) {
                match answer {
                    Answer::Changed(Ok(Changed(changed))) if agreed.is_none() => {
                        agreed = Some(changed)
                    }
                    Answer::Scene(scene) => seen = Some(scene),
                    _ => {}
                }
            }
            if let (Some(_), None, Some(session)) =
                (agreed.as_ref(), seen.as_ref(), viewer.session.as_ref())
            {
                let _ = session.ask(Request::Scene);
            }
            if seen.is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        assert_eq!(agreed, Some(true), "the authority agreed to the press");
        let scene = seen.expect("the authority answered a scene");
        assert!(viewer.apply_scene(&scene), "the scene is what moves it");
        assert_eq!(
            viewer.workspaces.len(),
            start + 1,
            "and the change is the authority's, applied exactly once"
        );

        if let Some(session) = viewer.session.take() {
            drop(session);
        }
        let _ = serving.join();
    }

    /// **A refusal does not end a viewer's session** (#204): the wall says no by name, the session says so and stays
    /// open, and the next request is answered. A screen that stopped working the first time somebody asked for
    /// something impossible would be a wall that had to be restarted to be driven.
    ///
    /// *Showing* the refusal on the glass waits for #198 (`kind`), which is where a refusal grows a category a client
    /// can act on; today the far end logs it.
    // The field these drive is `State::session`, which only a unix build has (#203).
    #[cfg(unix)]
    #[test]
    fn a_refusal_does_not_end_a_viewer_s_session() {
        let (wall_end, peer) = Loopback::pair();
        let serving = std::thread::spawn(move || {
            let mut carrier = wall_end;
            while let Ok(Some(line)) = carrier.read_line() {
                let Ok(request) = crate::control::wire::request(&line) else {
                    break;
                };
                // A wall that says no: the last pane on the wall (#141).
                let answer = Answer::Changed(Err(Refused(crate::control::LAST_PANE)));
                let _ = &request;
                if carrier
                    .write_line(&crate::control::wire::reply(answer))
                    .is_err()
                {
                    break;
                }
            }
        });

        let mut viewer = State::default();
        viewer.session = Some(Session::new(Box::new(peer)).expect("a session"));
        let asked = crate::control::apply(&mut viewer, crate::control::Intent::ClosePane(1));
        eprintln!("DEBUG asked={asked:?} session={}", viewer.session.is_some());

        // The refused press: the session logs it and stays open.
        std::thread::sleep(std::time::Duration::from_millis(100));
        // And the next request is answered — the authority's word for it, not permission to act.
        let asked = viewer
            .session
            .as_ref()
            .is_some_and(|session| session.ask(Request::Scene));
        assert!(asked, "the session is still open after a refusal");
        if let Some(session) = viewer.session.take() {
            drop(session);
        }
        let _ = serving.join();
    }
}

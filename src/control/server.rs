//! **The listener** — one client at a time on a unix socket, and the jobs the UI thread drains (#165).
//!
//! `docs/adr/0005-control-plane.md` decided the transport; this is it. The thread here **parses and answers
//! nothing about the wall**: it hands a [`Job`] to the UI thread and waits for the answer, because `apply`
//! changes `State` and `State` belongs to the loop that draws it. A socket read on the UI thread would break the
//! rule the rest of the crate is built on ("the UI thread never blocks"); a state change on this thread would be
//! two threads mutating one wall.
//!
//! **The socket is the credential** (`INV-08`): mode `0600`, inside a `0700` directory, no token and no
//! handshake. The directory is what closes the window between `bind` and `set_permissions` — nobody else can
//! enter it to race the chmod.

use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use super::{Refused, SOCKET_MODE};

/// How long a client waits for the wall to answer before it is told the wall did not.
///
/// A wall mid-frame is a wall that will answer in a frame; a wall that took five seconds is a wall with a
/// problem, and a client hanging on a socket for ever cannot tell that from a slow one.
const ANSWER_WITHIN: Duration = Duration::from_secs(5);

/// **One parsed request, waiting for the UI thread.** The reply channel is how the listener says the answer
/// back to the client without ever touching the wall itself.
pub struct Job {
    /// What the client asked for: a change, or a look at the wall (#181).
    pub request: super::wire::Request,
    answer: Sender<super::wire::Answer>,
}

impl Job {
    /// Answer the client. Called by the UI thread after `control::apply` (or after reading the projection), and
    /// once per job.
    pub fn answer(self, answer: super::wire::Answer) {
        // A client that hung up is not an error: the reply has nowhere to go, and the wall already changed.
        let _ = self.answer.send(answer);
    }
}

/// The listening socket, the thread that accepts on it, and the queue the UI thread drains.
pub struct Server {
    path: PathBuf,
    jobs: Receiver<Job>,
    listening: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    /// The connection being served, so a stopping wall can hang it up rather than wait for a client that may
    /// never type anything again.
    current: Arc<Mutex<Option<UnixStream>>>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Server {
    /// Bind `path` and start accepting. The bind happens **here**, before the thread, so a wall that cannot own
    /// its socket says so at start-up rather than in a log line nobody reads.
    ///
    /// A socket left behind by a wall that was killed is removed; a socket that **answers** is another wall, and
    /// two walls driving one display is refused rather than made to share.
    pub fn spawn(path: PathBuf) -> std::io::Result<Server> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
            // The directory is the real permission: created 0700, so the socket inside it is unreachable while
            // its own mode is being set.
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
        if path.exists() {
            match UnixStream::connect(&path) {
                Ok(_) => {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::AddrInUse,
                        "another wall is already listening on this socket",
                    ));
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(SOCKET_MODE))?;

        let (tx, jobs) = mpsc::channel();
        let listening = Arc::new(AtomicBool::new(true));
        let stop = Arc::new(AtomicBool::new(false));
        let current = Arc::new(Mutex::new(None));
        let thread = {
            let listening = listening.clone();
            let stop = stop.clone();
            let path = path.clone();
            let current = current.clone();
            std::thread::Builder::new()
                .name("control".to_string())
                .spawn(move || serve(&listener, &tx, &current, &listening, &stop, &path))?
        };
        Ok(Server {
            path,
            jobs,
            listening,
            stop,
            current,
            thread: Some(thread),
        })
    }

    /// The socket's path, for the log line and `--self-check`.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Whether the listener is accepting — `false` after it stops, which `--self-check` reports rather than
    /// claiming a wall is drivable when it is not.
    pub fn listening(&self) -> bool {
        self.listening.load(Ordering::Relaxed)
    }

    /// **The next request, if one is waiting.** Called from `poll`, between frames, and never blocking: a wall
    /// that waited here would freeze for a client that has not finished typing its line.
    pub fn next_job(&self) -> Option<Job> {
        self.jobs.try_recv().ok()
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::SeqCst);
        // A client mid-line is hung up first: it is blocked in a read that nothing is going to answer, and a
        // `join` that waited for it would wait for a person.
        if let Some(client) = self
            .current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .take()
        {
            let _ = client.shutdown(std::net::Shutdown::Both);
        }
        // And waking `accept`: a listener blocked in `accept` does not see a flag, so the wall connects to
        // itself once and lets the loop notice the stop on the way out.
        let _ = UnixStream::connect(&self.path);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
        let _ = std::fs::remove_file(&self.path);
    }
}

/// Accept one client at a time, and serve it until it hangs up.
///
/// **One client at a time** is a decision, not a limitation: the wall is one display, and two clients whose
/// requests interleave would be a `ctl` script racing a phone. The second client waits for the first to finish,
/// which is what a queue means.
fn serve(
    listener: &UnixListener,
    tx: &Sender<Job>,
    current: &Mutex<Option<UnixStream>>,
    listening: &AtomicBool,
    stop: &AtomicBool,
    path: &Path,
) {
    for stream in listener.incoming() {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        let Ok(stream) = stream else {
            // A failed accept is not a wall failure: the socket may be going away underneath us, and the loop
            // ends when the path does.
            if !path.exists() {
                break;
            }
            continue;
        };
        // A clone shares the socket, so shutting it down from `Drop` stops the read below.
        if let Ok(handle) = stream.try_clone() {
            *current
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner()) = Some(handle);
        }
        let _ = serve_one(stream, tx, stop);
        *current
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = None;
    }
    listening.store(false, Ordering::Relaxed);
}

/// One connection: read lines, answer each one, stop at end of file or at the wall's stop — which `Drop` says
/// by shutting this socket down (the read then returns end-of-file, and a half-typed line goes with it).
fn serve_one(stream: UnixStream, tx: &Sender<Job>, stop: &AtomicBool) -> std::io::Result<()> {
    serve_session(super::carrier::Unix::new(stream)?, tx, stop)
}

/// **The session loop, over whatever carrier the wall has** (#195).
///
/// This is the whole of the plane's behaviour — read a line, hand it to the UI thread, write the answer back — and it
/// knows the transport only as [`super::carrier::Carrier`]. A relay (#196) and a viewer's connection (#202) reuse it,
/// so there is one vocabulary, one refusal path and one place that could break `INV-05`.
fn serve_session(
    mut carrier: impl super::carrier::Carrier,
    tx: &Sender<Job>,
    stop: &AtomicBool,
) -> std::io::Result<()> {
    while let Some(line) = carrier.read_line()? {
        if stop.load(Ordering::SeqCst) {
            break;
        }
        if line.trim().is_empty() {
            continue;
        }
        let answer = match super::wire::request(&line) {
            Ok(request) => ask_the_wall(tx, request),
            Err(why) => super::wire::Answer::Changed(Err(why)),
        };
        // **A refused request is a line, never a closed connection** (`INV-07`): a client that asked for
        // something impossible can ask for something else without reconnecting.
        carrier.write_line(&super::wire::reply(answer))?;
    }
    Ok(())
}

/// Hand one intent to the UI thread and wait for its answer.
fn ask_the_wall(tx: &Sender<Job>, request: super::wire::Request) -> super::wire::Answer {
    let (answer, reply) = mpsc::channel();
    if tx.send(Job { request, answer }).is_err() {
        // The wall has stopped: the loop is gone, and saying so is better than a client waiting for ever.
        return super::wire::Answer::Changed(Err(Refused("the wall has stopped")));
    }
    match reply.recv_timeout(ANSWER_WITHIN) {
        Ok(answer) => answer,
        Err(RecvTimeoutError::Timeout) => {
            super::wire::Answer::Changed(Err(Refused("the wall did not answer")))
        }
        Err(RecvTimeoutError::Disconnected) => {
            super::wire::Answer::Changed(Err(Refused("the wall has stopped")))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::control::wire::Answer;
    use crate::control::{self, Changed, Intent, LAST_PANE};
    use crate::ui::State;
    use std::io::{BufRead, BufReader, Write};

    /// A socket in this test's own directory, so two runs of the gate cannot collide and nothing is left in the
    /// operator's run directory by a test.
    fn test_socket(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("arcons-cc-test-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("control.sock")
    }

    /// Wait for the listener thread to hand over a request. The production path is `next_job`, which never
    /// blocks (a wall that waited on a client would freeze for one that has not finished typing); a test knows
    /// the request is coming and waits for the thread instead of racing it.
    fn wait_job(server: &Server) -> Job {
        for _ in 0..1_000 {
            if let Some(job) = server.next_job() {
                return job;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
        panic!("the listener never handed over a request");
    }

    /// **The wall's own answer, over a real socket**: a line in, a line out, the state changed, and the file
    /// gone when the server is.
    #[test]
    fn a_request_changes_the_wall_and_the_socket_goes_when_it_does() {
        let path = test_socket("round-trip");
        let server = Server::spawn(path.clone()).expect("the socket binds");
        assert!(
            server.listening(),
            "a server that bound a socket is listening"
        );
        // The socket is the credential: owner read and write, nothing for anyone else.
        let mode = std::fs::metadata(&path)
            .expect("the socket exists")
            .permissions()
            .mode();
        assert_eq!(
            mode & 0o777,
            0o600,
            "the control socket must be 0600 (`INV-08`), got {:o}",
            mode & 0o777
        );

        let mut client = UnixStream::connect(&path).expect("a client connects");
        client
            .write_all(b"{\"v\":1,\"intent\":{\"verb\":\"workspace.new\"}}\n")
            .expect("the request goes");
        // The UI answers between frames: here that is this thread, applying to a wall of its own.
        let mut state = State::default();
        let job = wait_job(&server);
        let intent = match &job.request {
            super::super::wire::Request::Intent(intent) => intent.clone(),
            other => panic!("a read where a change was asked for: {other:?}"),
        };
        let result = control::apply(&mut state, intent);
        job.answer(Answer::Changed(result));

        let mut reply = String::new();
        BufReader::new(client)
            .read_line(&mut reply)
            .expect("the reply comes");
        assert_eq!(
            reply.trim(),
            r#"{"changed":true,"ok":true,"v":1}"#,
            "a client is told what happened, with the version on it"
        );
        drop(server);
        assert!(
            !path.exists(),
            "a wall that stopped leaves no socket behind for the next launch to trip on"
        );
    }

    /// A refusal travels as a line too, with the rule's name in it — the same name `apply` returned.
    #[test]
    fn a_refusal_is_a_line_with_the_rules_name() {
        let path = test_socket("refusal");
        let server = Server::spawn(path.clone()).expect("the socket binds");
        let mut client = UnixStream::connect(&path).expect("a client connects");
        client
            .write_all(b"{\"v\":1,\"intent\":{\"verb\":\"pane.close\",\"pane\":1}}\n")
            .expect("the request goes");
        let mut state = State::default();
        let only = state.focused();
        let job = wait_job(&server);
        assert_eq!(
            job.request,
            super::super::wire::Request::Intent(Intent::ClosePane(only))
        );
        let intent = match &job.request {
            super::super::wire::Request::Intent(intent) => intent.clone(),
            other => panic!("a read where a change was asked for: {other:?}"),
        };
        let result = control::apply(&mut state, intent);
        assert_eq!(result, Err(Refused(LAST_PANE)));
        job.answer(Answer::Changed(result));
        let mut reply = String::new();
        BufReader::new(client)
            .read_line(&mut reply)
            .expect("the reply comes");
        assert_eq!(
            reply.trim(),
            format!(r#"{{"ok":false,"v":1,"why":"{LAST_PANE}"}}"#),
            "the client gets the rule's name, not a generic failure"
        );
    }

    /// **A read is answered over the same socket, from the wall's own state** (#181): the projection a client gets
    /// is the projection `State` describes, and it is a body rather than a `changed` flag.
    #[test]
    fn a_read_comes_back_as_the_walls_projection() {
        let path = test_socket("read");
        let server = Server::spawn(path.clone()).expect("the socket binds");
        let mut client = UnixStream::connect(&path).expect("a client connects");
        client
            .write_all(b"{\"v\":1,\"read\":\"wall\"}\n")
            .expect("the request goes");
        let state = State::default();
        let job = wait_job(&server);
        assert_eq!(job.request, super::super::wire::Request::Wall);
        let answer = Answer::Wall(crate::control::projection::of(&state));
        job.answer(answer);
        let mut reply = String::new();
        BufReader::new(client)
            .read_line(&mut reply)
            .expect("the reply comes");
        let value: serde_json::Value = serde_json::from_str(reply.trim()).expect("JSON");
        assert_eq!(value["ok"], serde_json::json!(true));
        assert_eq!(
            value["wall"]["workspaces"].as_array().map(Vec::len),
            Some(state.workspaces.len()),
            "every workspace is in the projection, empty ones included"
        );
        assert_eq!(
            value["wall"]["focused"],
            serde_json::json!(state.focused()),
            "and the pane with the keys"
        );
        // The read changed nothing.
        assert_eq!(state.pane_count(), 1);
    }

    /// **Garbage in, a named refusal out, and the connection stays open** — a client that made a typo should
    /// not have to reconnect to fix it.
    #[test]
    fn a_bad_line_is_refused_and_the_client_can_ask_again() {
        let path = test_socket("bad-line");
        let server = Server::spawn(path.clone()).expect("the socket binds");
        let mut client = UnixStream::connect(&path).expect("a client connects");
        client.write_all(b"not json\n").expect("the request goes");
        let mut reply = String::new();
        let mut reader = BufReader::new(client.try_clone().expect("clone"));
        reader.read_line(&mut reply).expect("the reply comes");
        assert_eq!(reply.trim(), r#"{"ok":false,"v":1,"why":"not JSON"}"#);
        // And the same connection still works afterwards.
        client
            .write_all(b"{\"v\":1,\"intent\":{\"verb\":\"surface.close\"}}\n")
            .expect("the second request goes");
        let job = wait_job(&server);
        job.answer(Answer::Changed(Ok(Changed(false))));
        let mut second = String::new();
        reader
            .read_line(&mut second)
            .expect("the second reply comes");
        assert_eq!(second.trim(), r#"{"changed":false,"ok":true,"v":1}"#);
    }
    /// **The session loop knows the transport only as a `Carrier`** (#195): the same loop that serves a unix socket
    /// serves a pair of in-memory ends, and nothing about intents, answers or the wall changed to allow it.
    ///
    /// It also holds `INV-05`: the loop does not apply anything. It hands the wall a `Job` and waits, so the client
    /// hears nothing until the UI thread has finished — asserted, not assumed, by asking the peer for its line before
    /// answering and getting nothing.
    #[test]
    fn the_session_loop_serves_a_carrier_it_has_never_seen() {
        let (wall_end, peer) = crate::control::carrier::Loopback::pair();
        let (tx, jobs) = mpsc::channel::<Job>();
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = Arc::clone(&stop);
        let serving = std::thread::spawn(move || serve_session(wall_end, &tx, &stopping));

        peer.send(r#"{"v":1,"intent":{"verb":"workspace.new"}}"#);
        // Bounded: a loop that answered for itself would never send a job, and a test that hangs tells you less than
        // one that fails with a sentence.
        let job = jobs.recv_timeout(Duration::from_secs(2)).expect(
            "the loop did not hand the request to the wall: something answered for the UI thread (INV-05)",
        );
        assert!(
            peer.recv_within(Duration::from_millis(50)).is_none(),
            "the loop answered before the wall did: a carrier must never apply a change itself (INV-05)"
        );

        // The UI thread, played by this test: apply, then answer.
        let mut state = State::default();
        let answer = match job.request.clone() {
            super::super::wire::Request::Intent(intent) => {
                super::super::wire::Answer::Changed(control::apply(&mut state, intent))
            }
            super::super::wire::Request::Wall => {
                super::super::wire::Answer::Wall(crate::control::projection::of(&state))
            }
        };
        job.answer(answer);
        let line = peer
            .recv_within(Duration::from_secs(2))
            .expect("the answer came back through the carrier");
        // The reply is built by `wire::reply`, so this compares against the same function the socket uses rather than
        // against a hand-written copy of its key order.
        assert_eq!(
            line,
            super::super::wire::reply(super::super::wire::Answer::Changed(Ok(
                crate::control::Changed(true)
            ))),
            "the answer is the same line the socket sends"
        );
        assert_eq!(
            state.workspaces.len(),
            4,
            "the wall changed once, on the thread that owns it"
        );

        drop(peer);
        stop.store(true, Ordering::SeqCst);
        serving
            .join()
            .expect("the loop thread ends")
            .expect("a hung-up peer is end of file, not an error");
    }
}

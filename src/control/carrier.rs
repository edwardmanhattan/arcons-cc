//! **A carrier** — the two things a transport must do, and nothing else (#195).
//!
//! `ADR-0006` and `docs/mobile-remote.md` both need the same control plane over more than a unix socket: a relay the
//! wall dials out to (a phone), a network session (a viewer on another machine). The cheap way to have that is to make
//! the transport a *seam* rather than a second copy of "read a line, ask the wall, write the answer" — two copies of
//! that is how a protocol grows a second refusal vocabulary.
//!
//! So a carrier knows **lines**, and nothing about intents, answers, `Job`s or the wall. The unix socket is one
//! ([`Unix`]); a loopback pair is another, and it is what a test drives when it wants the real session loop without a
//! filesystem ([`Loopback`]). A relay and a TCP session are the same six methods (#196, #202).
//!
//! **The UI thread is not in here** (`INV-05`): a carrier blocks its *own* thread, and the session hands the wall a
//! [`super::sessions::Job`] and waits — which is the rule this seam must not tempt anyone to break.

use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use crate::ui::Origin;

/// One line in, one line out, and a way to hang up.
///
/// `read_line` returns `Ok(None)` at end of file, which is a client that hung up rather than a wall failure.
pub trait Carrier: Send {
    fn read_line(&mut self) -> std::io::Result<Option<String>>;
    fn write_line(&mut self, line: &str) -> std::io::Result<()>;

    /// Hang the connection up, so a thread blocked in [`Carrier::read_line`] wakes. Called when the wall stops: a
    /// client that typed half a line must not hold the process open.
    fn hang_up(&mut self) {}

    /// **Whether this carrier can carry a watch** (#221) — a stream of lines, not one answer to one request.
    ///
    /// The socket the wall has always had says **no**: it answers one request with one answer, because a `ctl` script
    /// must not be able to hold the wall's one connection open for ever. A relay holds a connection open already, and
    /// says **yes**. So a watch is a capability of a transport and not a verb of the plane, and a client that asks the
    /// wrong one is refused by name ([`super::watch::NOT_CARRIED`]).
    fn carries_a_watch(&self) -> bool {
        false
    }

    /// **Read a line, waiting at most `within`** (#221): what a watched session loop uses to make a publish due.
    ///
    /// `Ok(None)` is end of file, exactly as [`Carrier::read_line`]'s is. A deadline is **not a failure**: it is an
    /// error of kind [`std::io::ErrorKind::WouldBlock`] or `TimedOut` (see [`super::watch::timed_out`]), and the loop
    /// treats it as "nothing has happened yet". Every carrier can be told how long to wait — the one that carries no
    /// watch is simply never asked.
    fn read_line_within(&mut self, within: Duration) -> std::io::Result<Option<String>>;

    /// **Whose hand the line just read came in on** (#222) — the name this carrier knows its client by.
    ///
    /// The session loop stamps it on the request, so a cursor cannot be moved under somebody else's name, and it is
    /// the origin the wall drops when this carrier hangs up. A unix socket answers [`Origin::ctl`] (its credential is
    /// the file's mode, so it has no name of its own), a relay answers the **device's name** from `devices.toml` — never
    /// its key and never its address (`INV-08`) — and a carrier that carries anybody, as a loopback pair in a test
    /// does, answers `None` and renounces nothing.
    fn origin(&self) -> Option<Origin> {
        None
    }
}

/// The unix socket the wall has always used (#165) — 0600 inside a 0700 directory, one client at a time.
#[cfg(unix)]
pub struct Unix {
    reader: std::io::BufReader<std::os::unix::net::UnixStream>,
    writer: std::os::unix::net::UnixStream,
}

#[cfg(unix)]
impl Unix {
    /// Take a stream, and split it into a reader and a writer that share the socket — so `hang_up` on one end wakes a
    /// read blocked on the other.
    pub fn new(stream: std::os::unix::net::UnixStream) -> std::io::Result<Unix> {
        let writer = stream.try_clone()?;
        Ok(Unix {
            reader: std::io::BufReader::new(stream),
            writer,
        })
    }
}

#[cfg(unix)]
impl Carrier for Unix {
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        use std::io::BufRead;
        let mut line = String::new();
        match self.reader.read_line(&mut line) {
            // A last line with no newline is still a line; a read of nothing is the end of the connection.
            Ok(0) => Ok(None),
            Ok(_) => Ok(Some(line)),
            Err(error) => Err(error),
        }
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        use std::io::Write;
        writeln!(self.writer, "{line}")?;
        self.writer.flush()
    }

    /// The socket's reads have no deadline: one request, one answer, and a client that has gone quiet is moved along by
    /// `hang_up` rather than by a timer. The method exists because every carrier answers it, and the wait it is given
    /// is honoured — this carrier simply never carries a watch (#221).
    fn read_line_within(&mut self, within: Duration) -> std::io::Result<Option<String>> {
        self.reader.get_ref().set_read_timeout(Some(within))?;
        let outcome = self.read_line();
        let _ = self.reader.get_ref().set_read_timeout(None);
        outcome
    }

    fn hang_up(&mut self) {
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
    }

    /// **The client with no identity of its own**: the socket's credential is its file's mode (`INV-08`), so `ctl` is
    /// the honest name for whoever is on the other end, and it is the origin its cursor is drawn and dropped under
    /// (#222).
    fn origin(&self) -> Option<Origin> {
        Some(Origin::ctl())
    }
}

/// **Two ends of one wire, in memory** — the carrier a test uses to drive the session loop without a socket.
///
/// A real implementation of the trait rather than a special case in production code: whatever the loop does with a
/// `Unix` it does with this, which is what makes "the transport is a seam" a claim a test can hold.
pub struct Loopback {
    from_peer: Receiver<String>,
    to_peer: Sender<String>,
    /// Who this end says is speaking, when a test needs a carrier that knows its client — a phone on a relay (#222).
    origin: Option<Origin>,
}

/// The other end of a [`Loopback`]: what a client holds in a test.
pub struct Peer {
    from_wall: Receiver<String>,
    to_wall: Sender<String>,
}

impl Loopback {
    /// Two ends, each of which reads what the other writes. **Nameless**: a carrier that carries anybody, which is what
    /// a test's wire is until the test says otherwise.
    pub fn pair() -> (Loopback, Peer) {
        Loopback::of(None)
    }

    /// The same two ends, with the wall's end knowing whose hand this is: a phone's session, or a `ctl`'s, so a test
    /// can drive the per-client rules of #222 — a cursor named by its carrier, and dropped when the carrier hangs up.
    pub fn named(name: &str) -> (Loopback, Peer) {
        Loopback::of(Some(Origin::named(name)))
    }

    fn of(origin: Option<Origin>) -> (Loopback, Peer) {
        let (to_peer, from_wall) = mpsc::channel();
        let (to_wall, from_peer) = mpsc::channel();
        (
            Loopback {
                from_peer,
                to_peer,
                origin,
            },
            Peer { from_wall, to_wall },
        )
    }
}

impl Carrier for Loopback {
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        // A dropped peer is end of file, exactly as a closed socket is: the caller cannot tell them apart, which is
        // the point.
        Ok(self.from_peer.recv().ok())
    }

    /// **This pair stands in for the socket** (#195), and a socket carries no watch (#221) — so the corpus and every
    /// session test that drives this carrier meet the same refusal a `ctl` script would. A test that wants a watch
    /// carries one with its own `Carrier`, which is the claim: the capability belongs to the transport.
    fn read_line_within(&mut self, within: Duration) -> std::io::Result<Option<String>> {
        within_of(&self.from_peer, within)
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        self.to_peer
            .send(line.to_string())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the peer hung up"))
    }

    fn origin(&self) -> Option<Origin> {
        self.origin.clone()
    }
}

impl Carrier for Peer {
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        Ok(self.from_wall.recv().ok())
    }

    fn read_line_within(&mut self, within: Duration) -> std::io::Result<Option<String>> {
        within_of(&self.from_wall, within)
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        self.to_wall
            .send(line.to_string())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the wall hung up"))
    }
}

/// **A channel read with a deadline** (#221): `Ok(None)` is end of file and a deadline is `WouldBlock`, which is the
/// same pair a socket's read gives a watched loop.
fn within_of(from: &Receiver<String>, within: Duration) -> std::io::Result<Option<String>> {
    match from.recv_timeout(within) {
        Ok(line) => Ok(Some(line)),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(std::io::Error::new(
            std::io::ErrorKind::WouldBlock,
            "nothing arrived within the beat",
        )),
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Ok(None),
    }
}

impl Peer {
    /// Send one line to the wall's carrier.
    pub fn send(&self, line: &str) {
        let _ = self.to_wall.send(line.to_string());
    }

    /// Take the next line the wall wrote, or `None` if it hung up.
    pub fn recv(&self) -> Option<String> {
        self.from_wall.recv().ok()
    }

    /// Take a line the wall has already written, without waiting. The corpus driver needs it because the
    /// session loop answers a line it cannot parse **itself** and hands the wall only the ones it can, so a
    /// driver waiting on one channel or the other has to look at both.
    pub fn try_recv(&self) -> Option<String> {
        self.from_wall.try_recv().ok()
    }

    /// Take the next line, waiting at most `within`.
    pub fn recv_within(&self, within: std::time::Duration) -> Option<String> {
        self.from_wall.recv_timeout(within).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **A carrier carries lines and nothing else**: what goes in comes out, in order, and a hung-up peer is end of
    /// file rather than an error the caller has to distinguish from a wall failure.
    #[test]
    fn a_loopback_carries_lines_in_order_and_a_dropped_peer_is_end_of_file() {
        let (mut wall, peer) = Loopback::pair();
        peer.send("{\"verb\":\"pane.focus\",\"pane\":2}");
        assert_eq!(
            wall.read_line().expect("no error").as_deref(),
            Some("{\"verb\":\"pane.focus\",\"pane\":2}")
        );
        wall.write_line("{\"ok\":true}").expect("writes");
        assert_eq!(peer.recv().as_deref(), Some("{\"ok\":true}"));

        drop(peer);
        assert_eq!(
            wall.read_line().expect("a closed peer is not an error"),
            None,
            "a peer that goes away is end of file (`Ok(None)`), which is what a closed socket gives the loop"
        );
    }
}

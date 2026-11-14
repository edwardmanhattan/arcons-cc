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
//! [`super::server::Job`] and waits — which is the rule this seam must not tempt anyone to break.

use std::sync::mpsc::{self, Receiver, Sender};

/// One line in, one line out, and a way to hang up.
///
/// `read_line` returns `Ok(None)` at end of file, which is a client that hung up rather than a wall failure.
pub trait Carrier: Send {
    fn read_line(&mut self) -> std::io::Result<Option<String>>;
    fn write_line(&mut self, line: &str) -> std::io::Result<()>;

    /// Hang the connection up, so a thread blocked in [`Carrier::read_line`] wakes. Called when the wall stops: a
    /// client that typed half a line must not hold the process open.
    fn hang_up(&mut self) {}
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

    fn hang_up(&mut self) {
        let _ = self.writer.shutdown(std::net::Shutdown::Both);
    }
}

/// **Two ends of one wire, in memory** — the carrier a test uses to drive the session loop without a socket.
///
/// A real implementation of the trait rather than a special case in production code: whatever the loop does with a
/// `Unix` it does with this, which is what makes "the transport is a seam" a claim a test can hold.
pub struct Loopback {
    from_peer: Receiver<String>,
    to_peer: Sender<String>,
}

/// The other end of a [`Loopback`]: what a client holds in a test.
pub struct Peer {
    from_wall: Receiver<String>,
    to_wall: Sender<String>,
}

impl Loopback {
    /// Two ends, each of which reads what the other writes.
    pub fn pair() -> (Loopback, Peer) {
        let (to_peer, from_wall) = mpsc::channel();
        let (to_wall, from_peer) = mpsc::channel();
        (Loopback { from_peer, to_peer }, Peer { from_wall, to_wall })
    }
}

impl Carrier for Loopback {
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        // A dropped peer is end of file, exactly as a closed socket is: the caller cannot tell them apart, which is
        // the point.
        Ok(self.from_peer.recv().ok())
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        self.to_peer
            .send(line.to_string())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the peer hung up"))
    }
}

impl Carrier for Peer {
    fn read_line(&mut self) -> std::io::Result<Option<String>> {
        Ok(self.from_wall.recv().ok())
    }

    fn write_line(&mut self, line: &str) -> std::io::Result<()> {
        self.to_wall
            .send(line.to_string())
            .map_err(|_| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "the wall hung up"))
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

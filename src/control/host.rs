//! **The wall's carriers, in one value** (#222): the queue every carrier feeds, the listener and the relay.
//!
//! The host is a mailbox, not a second owner of the wall. It hands the state one job at a time
//! ([`Host::next_job`]); `State::answer` settles it on the thread that owns the state and answers the
//! carrier. No carrier reads a field of the wall and no job is applied off the UI thread (`INV-05`).
//!
//! The queue and the listener are unix-only because the transport is; a Windows build has the
//! vocabulary and no host.

use super::relay::Relay;
use super::server::{Job, Server};
use super::sessions::Queue;

/// The carriers this process owns.
pub struct Host {
    queue: Queue,
    listener: Option<Server>,
    relay: Option<Relay>,
}

impl Host {
    /// A wall that binds nothing: a capture, a test, or a plane turned off.
    pub fn new() -> Host {
        Host {
            queue: Queue::new(),
            listener: None,
            relay: None,
        }
    }

    /// The end a carrier is handed. Every carrier feeds the same queue.
    pub fn queue(&self) -> &Queue {
        &self.queue
    }

    /// **The next thing a carrier has left for the wall**, if any. Never blocks (`INV-05`).
    pub fn next_job(&self) -> Option<Job> {
        self.queue.next()
    }

    /// Keep the listener alive for the wall's life.
    pub fn listen(&mut self, listener: Server) {
        self.listener = Some(listener);
    }

    /// Keep the relay alive for the wall's life.
    pub fn relay(&mut self, relay: Relay) {
        self.relay = Some(relay);
    }

    /// Whether this wall has a unix socket. A wall with no listener is still a wall.
    pub fn listening(&self) -> bool {
        self.listener.as_ref().is_some_and(Server::listening)
    }

    /// Whether the outbound carrier is connected. A wall with no relay is the supported default.
    pub fn relay_connected(&self) -> bool {
        self.relay.as_ref().is_some_and(Relay::connected)
    }
}

impl Default for Host {
    fn default() -> Host {
        Host::new()
    }
}

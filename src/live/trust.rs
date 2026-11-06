//! **INV-09** as a type: after a reconnect, nothing is trusted until a picture read completes.
//!
//! `live-feed` keeps no history and no presence (`realtime-protocol.md` §4). A reconnect therefore
//! loses every fix published while the socket was down, and the server will not replay them. The
//! markers still on screen are from before the gap, and the socket cannot tell us what happened in
//! it — so they are **not** current, and presenting them as current is the one lie this display must
//! never tell.
//!
//! Hence: a disconnect makes the picture [`Trust::Unproven`], and only a completed read makes it
//! [`Trust::Proven`] again. While unproven, publications are still *received* — they carry fresh
//! fixes and throwing them away would widen the gap — but they are applied only after the read that
//! re-establishes the picture, so a marker cannot be drawn as current on the strength of one
//! publication that arrived before the read.
//!
//! The type is this small on purpose: it is the whole state machine, so the actor has nowhere to
//! keep a second copy of the answer.

/// Whether the markers on screen may be presented as the current picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Trust {
    /// The socket has been (re)connected and no picture read has completed since: what is on screen
    /// is from before the gap.
    #[default]
    Unproven,
    /// A completed `/live-feed/positions` read has landed since the last disconnect.
    Proven,
}

impl Trust {
    /// A disconnect happened: the picture is not current any more.
    ///
    /// Applies to every disconnect, including one the client asked for. "It was only a blip" is the
    /// assumption that puts a stale hull in the wrong ocean.
    pub fn after_disconnect(self) -> Self {
        Trust::Unproven
    }

    /// A picture read completed: the picture is the server's answer, and it is current as of now.
    pub fn after_picture(self) -> Self {
        Trust::Proven
    }

    /// Whether a marker may be drawn as the current picture.
    pub fn is_current(self) -> bool {
        matches!(self, Trust::Proven)
    }

    /// Whether a publication may be applied to the picture.
    ///
    /// Only once the picture it applies to is proven: before that, the publication is a fact about a
    /// unit in a picture we cannot draw yet.
    pub fn accepts_publication(self) -> bool {
        self.is_current()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The events the actor sees, as far as trust is concerned.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Event {
        Connected,
        Publication,
        Disconnect,
        Picture,
    }

    fn apply(trust: Trust, event: Event) -> Trust {
        match event {
            // Being connected proves nothing: the socket is a transport, and the gap it just left is
            // exactly what has to be filled by a read.
            Event::Connected | Event::Publication => trust,
            Event::Disconnect => trust.after_disconnect(),
            Event::Picture => trust.after_picture(),
        }
    }

    #[test]
    fn a_connect_alone_never_makes_a_picture_current() {
        // The bug this type exists to prevent: "connected, therefore live". There is no replay on
        // this channel, so a connect after a gap proves only that the socket is back.
        let trust = apply(
            apply(apply(Trust::Proven, Event::Disconnect), Event::Connected),
            Event::Publication,
        );
        assert_eq!(trust, Trust::Unproven);
        assert!(
            !trust.accepts_publication(),
            "INV-09: a publication after a reconnect is not a picture"
        );
    }

    #[test]
    fn a_completed_read_proves_the_picture_again() {
        let trust = apply(
            apply(apply(Trust::Proven, Event::Disconnect), Event::Connected),
            Event::Picture,
        );
        assert!(trust.is_current());
        assert!(trust.accepts_publication());
    }

    #[test]
    fn no_sequence_of_events_leaves_a_stale_picture_trusted() {
        // A small model check over every sequence up to length 6: the invariant is not "the happy
        // path works", it is "there is no order of events in which a disconnect is followed by
        // trusting a publication before a read".
        let events = [
            Event::Connected,
            Event::Publication,
            Event::Disconnect,
            Event::Picture,
        ];
        for length in 0..=6u32 {
            for mut code in 0..events.len().pow(length) {
                let mut sequence = Vec::new();
                for _ in 0..length {
                    sequence.push(events[code % events.len()]);
                    code /= events.len();
                }

                let mut trust = Trust::Proven;
                let mut disconnected_since_read = false;
                for event in &sequence {
                    trust = apply(trust, *event);
                    match event {
                        Event::Disconnect => disconnected_since_read = true,
                        Event::Picture => disconnected_since_read = false,
                        _ => {}
                    }
                    if trust.accepts_publication() {
                        assert!(
                            !disconnected_since_read,
                            "INV-09: {sequence:?} trusted a publication after a disconnect and before a read"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_default_is_unproven() {
        // A process that has not read anything yet has not proved anything.
        assert!(!Trust::default().is_current());
    }
}

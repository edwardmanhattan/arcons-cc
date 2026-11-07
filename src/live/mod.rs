//! The Centrifugo socket actor: movement, on its own thread, with its own runtime.
//!
//! The socket is an **accelerator over the read**, never the source of truth. `live-feed` has no
//! replay and no presence, so after any reconnect the picture is re-read and only then trusted
//! (`INV-09`). Everything else here follows from the pinned protocol:
//!
//! - **The token rides in the connect frame's `data`** as `{"token": "…"}` — never an
//!   `Authorization` header, never the frame's `token` field. Upstream `tokio-centrifuge` 0.2.6
//!   cannot set it, which is why [`crate::live::connect`] builds the bytes and the vendored copy
//!   carries them.
//! - **The envelope is not uniform.** Centrifugo adds one `data` level, and the channels differ
//!   beneath it: `live-feed` is a **bare** event at `pub.data`; `game:<id>` puts a `GameEvent` at
//!   `pub.data.type` with its payload at `pub.data.data.*` (two levels); `game:<id>:positions` is
//!   bare, has **no** `type`, and lives at `pub.data.positions`. [`wire`] parses the first, and
//!   refuses the others rather than returning `undefined` with no error.
//! - **Refusals are total** (`INV-07`): [`Refusal`] has six variants and one [`Action`] each, both
//!   exhaustive matches, and no catch-all over the wire enum.
//! - **A `backfilled` fix is real and did not just happen** (`INV-03`): it reaches the UI with its
//!   flag intact, and never as "here now".
//!
//! v0.1 subscribes to `live-feed` and nothing else. There is no personal channel and no game channel
//! until there is a feature that needs one; a subscription to a channel nobody reads is a `107`
//! waiting to be explained.

pub mod actor;
pub mod backoff;
pub mod trust;
pub mod wire;

pub use actor::{LiveThread, Update};
pub use backoff::Backoff;
pub use trust::Trust;
pub use wire::LiveFix;

/// A Centrifugo client-protocol refusal: the number decides whether to come back.
///
/// Six codes, and no more: `realtime-protocol.md` §7 is the authority, and `tests/doc_claims.rs`
/// fails when the document and this enum disagree. That is the point — a new code upstream must be a
/// **compile-visible decision** here, never a silent default (`INV-07`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// `100` — our side failed, not yours. Marked temporary: come back.
    Internal,
    /// `101` — bad, absent or unverifiable token. Do **not** come back with the same one.
    Unauthorized,
    /// `103` — the account may not watch that channel. Do not retry.
    PermissionDenied,
    /// `105` — already subscribed. Not an error.
    AlreadySubscribed,
    /// `107` — an invalid channel name, which means a name was built client-side.
    InvalidChannelName,
    /// `109` — expired. Refresh the token, then retry.
    TokenExpired,
}

/// What a refusal means for the connection. One [`Refusal`], exactly one action.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    /// Stop. Signing in again is a human's decision, not a retry loop's.
    Stop,
    /// Refresh the token, then retry (`realtime-protocol.md` §8).
    RefreshThenRetry,
    /// Retry with backoff: the failure is temporary on the server's side.
    RetryWithBackoff,
    /// Not an error: nothing is wrong, do nothing.
    Ignore,
}

impl Refusal {
    /// Every refusal this client knows, for exhaustive tests and for the doc-claim check.
    pub const ALL: &[Refusal] = &[
        Refusal::Internal,
        Refusal::Unauthorized,
        Refusal::PermissionDenied,
        Refusal::AlreadySubscribed,
        Refusal::InvalidChannelName,
        Refusal::TokenExpired,
    ];

    /// The wire code. Exhaustive on purpose: adding a variant must fail to compile here first.
    pub const fn code(self) -> u16 {
        match self {
            Refusal::Internal => 100,
            Refusal::Unauthorized => 101,
            Refusal::PermissionDenied => 103,
            Refusal::AlreadySubscribed => 105,
            Refusal::InvalidChannelName => 107,
            Refusal::TokenExpired => 109,
        }
    }

    /// What to do about it.
    pub const fn action(self) -> Action {
        match self {
            Refusal::Internal => Action::RetryWithBackoff,
            Refusal::Unauthorized | Refusal::PermissionDenied | Refusal::InvalidChannelName => {
                Action::Stop
            }
            Refusal::TokenExpired => Action::RefreshThenRetry,
            Refusal::AlreadySubscribed => Action::Ignore,
        }
    }

    /// The refusal a code names, or `None` for a code this client has never seen.
    ///
    /// `None` is not a default: the actor logs the unknown code and does not invent an action for it,
    /// because guessing would be how a future protocol addition gets swallowed.
    pub const fn from_code(code: u16) -> Option<Refusal> {
        match code {
            100 => Some(Refusal::Internal),
            101 => Some(Refusal::Unauthorized),
            103 => Some(Refusal::PermissionDenied),
            105 => Some(Refusal::AlreadySubscribed),
            107 => Some(Refusal::InvalidChannelName),
            109 => Some(Refusal::TokenExpired),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_codes_round_trip_and_no_two_share_a_number() {
        for refusal in Refusal::ALL {
            assert_eq!(
                Refusal::from_code(refusal.code()),
                Some(*refusal),
                "{refusal:?} does not round-trip"
            );
        }
        for (i, a) in Refusal::ALL.iter().enumerate() {
            for b in &Refusal::ALL[i + 1..] {
                assert_ne!(a.code(), b.code(), "{a:?} and {b:?} share a code");
            }
        }
        assert_eq!(
            Refusal::from_code(199),
            None,
            "an unknown code is a visible None, never a guess"
        );
    }

    #[test]
    fn every_refusal_maps_to_exactly_one_action_and_the_two_similar_ones_differ() {
        // INV-07: 101 and 109 look alike from a distance — one says "never come back", the other
        // "refresh and come back". Treating them the same either retries forever with a dead token
        // or signs the wall out every hour.
        let mut seen = Vec::new();
        for refusal in Refusal::ALL {
            let action = refusal.action();
            assert_eq!(
                action,
                refusal.action(),
                "{refusal:?} must not change its mind"
            );
            seen.push((refusal.code(), action));
        }
        assert_eq!(
            seen.len(),
            6,
            "INV-07: six codes, one action each, no catch-all"
        );
        assert_eq!(Refusal::Unauthorized.action(), Action::Stop);
        assert_eq!(Refusal::TokenExpired.action(), Action::RefreshThenRetry);
        assert_ne!(
            Refusal::Unauthorized.action(),
            Refusal::TokenExpired.action()
        );
        assert_eq!(Refusal::Internal.action(), Action::RetryWithBackoff);
        assert_eq!(Refusal::AlreadySubscribed.action(), Action::Ignore);
        assert_eq!(Refusal::PermissionDenied.action(), Action::Stop);
        assert_eq!(Refusal::InvalidChannelName.action(), Action::Stop);
    }
}

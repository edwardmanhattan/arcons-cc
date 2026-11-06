//! The token, and the one place that decides when to refresh it.
//!
//! Two things matter here and nothing else does:
//!
//! 1. **Refresh is single-flight.** The wall's reads thread asks for a token from several tasks at
//!    once. Five concurrent callers must cause **one** refresh, not five: the server rotates refresh
//!    tokens, so five refreshes invalidate each other and sign the wall out.
//! 2. **The token never prints** (`INV-08`). It lives in a private field of a type whose `Debug` is
//!    written by hand, and it is handed out only through a method named `expose`-adjacent
//!    ([`Auth::access_token`]) so a reviewer sees every place it leaves.
//!
//! The 80% rule: a token is refreshed at 80% of the lifetime the server stated, not at 100%. Waiting
//! for expiry means a request can be in flight when it dies, and a 401 costs a round trip on the wall.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::data::Error;
use crate::data::types::TokenPair;

/// Refresh at this fraction of the stated lifetime.
const REFRESH_AT: f64 = 0.8;

/// Something that can turn a refresh token into a pair. A trait so the single-flight test can count
/// calls without a socket.
pub trait Refresher: Send + Sync {
    fn refresh(
        &self,
        refresh_token: &str,
    ) -> impl std::future::Future<Output = Result<TokenPair, Error>> + Send;
}

struct Current {
    access: String,
    /// Kept beside the access token so a rejected one can be recognised (`replace_rejected`).
    acquired: Instant,
    lifetime: Duration,
}

impl Current {
    fn new(pair: TokenPair, now: Instant) -> Self {
        let lifetime = Duration::from_secs(pair.expires_in().max(0) as u64);
        Self {
            access: pair.access_token().to_string(),
            acquired: now,
            lifetime,
        }
    }

    fn is_fresh(&self, now: Instant) -> bool {
        let usable = self.lifetime.mul_f64(REFRESH_AT);
        now.duration_since(self.acquired) < usable
    }
}

/// Holds the current token and refreshes it once, whichever task asks first.
pub struct Auth<R: Refresher> {
    refresher: R,
    /// The rotating refresh token, kept apart so it can be rotated without holding the token lock.
    refresh_token: Mutex<Option<String>>,
    /// The single-flight gate. Held across the refresh await: that is the whole mechanism.
    current: tokio::sync::Mutex<Option<Current>>,
}

impl<R: Refresher> Auth<R> {
    /// Already signed in: a token pair from `/auth/login`, or from an initial refresh.
    pub fn with_pair(refresher: R, pair: TokenPair) -> Self {
        let refresh = pair.refresh_token().map(|token| token.to_string());
        Self {
            refresher,
            refresh_token: Mutex::new(refresh),
            current: tokio::sync::Mutex::new(Some(Current::new(pair, Instant::now()))),
        }
    }

    /// A refresh token and nothing else: refresh on first use.
    pub fn with_refresh_token(refresher: R, refresh_token: String) -> Self {
        Self {
            refresher,
            refresh_token: Mutex::new(Some(refresh_token)),
            current: tokio::sync::Mutex::new(None),
        }
    }

    /// A usable access token, refreshing if the current one is past 80% of its life.
    pub async fn access_token(&self) -> Result<String, Error> {
        if let Some(token) = self.fresh_access() {
            return Ok(token);
        }
        let mut gate = self.current.lock().await;
        // Re-check under the gate: while we waited, another task may have refreshed. This double
        // check is what makes N callers cost one refresh.
        if let Some(current) = gate
            .as_ref()
            .filter(|current| current.is_fresh(Instant::now()))
        {
            return Ok(current.access.clone());
        }
        let pair = self.refresh_locked().await?;
        let access = pair.access_token().to_string();
        *gate = Some(Current::new(pair, Instant::now()));
        Ok(access)
    }

    /// The server rejected `rejected` with a 401: get a token that is not it, refreshing once.
    ///
    /// If another task has already replaced the rejected token, its token is returned instead of
    /// refreshing again — five tasks meeting the same 401 still cost one refresh.
    pub async fn replace_rejected(&self, rejected: &str) -> Result<String, Error> {
        let mut gate = self.current.lock().await;
        if let Some(current) = gate.as_ref().filter(|current| current.access != rejected) {
            return Ok(current.access.clone());
        }
        let pair = self.refresh_locked().await?;
        let access = pair.access_token().to_string();
        *gate = Some(Current::new(pair, Instant::now()));
        Ok(access)
    }

    /// How long the current access token still has, for `--self-check`. `None` before the first
    /// refresh, or once it is past its stated lifetime.
    pub fn remaining(&self) -> Option<Duration> {
        let current = self.current.try_lock().ok()?;
        let current = current.as_ref()?;
        current.lifetime.checked_sub(current.acquired.elapsed())
    }

    fn fresh_access(&self) -> Option<String> {
        // A short blocking lock: the value is copied out and the guard dropped before any await.
        let guard = self.current.try_lock().ok()?;
        let current = guard.as_ref()?;
        current
            .is_fresh(Instant::now())
            .then(|| current.access.clone())
    }

    /// Called only with the single-flight gate held.
    async fn refresh_locked(&self) -> Result<TokenPair, Error> {
        let token = self
            .refresh_token
            .lock()
            .map_err(|_| Error::NoCredential)?
            .clone()
            .ok_or(Error::NoCredential)?;
        let pair = self.refresher.refresh(&token).await?;
        // Refresh tokens rotate: keep the new one, or the next refresh replays a dead token and the
        // server refuses it (which is exactly how theft is made detectable).
        if let Some(next) = pair.refresh_token()
            && let Ok(mut slot) = self.refresh_token.lock()
        {
            *slot = Some(next.to_string());
        }
        Ok(pair)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct Counting {
        calls: AtomicUsize,
        expires_in: i64,
    }

    impl Counting {
        fn new(expires_in: i64) -> Self {
            Self {
                calls: AtomicUsize::new(0),
                expires_in,
            }
        }
    }

    impl Refresher for Counting {
        async fn refresh(&self, _refresh_token: &str) -> Result<TokenPair, Error> {
            let n = self.calls.fetch_add(1, Ordering::SeqCst);
            let body = format!(
                r#"{{"access_token":"access-{n}","token_type":"Bearer","expires_in":{},"refresh_token":"refresh-{n}"}}"#,
                self.expires_in
            );
            Ok(serde_json::from_str(&body).expect("the stub pair is valid"))
        }
    }

    fn pair(expires_in: i64) -> TokenPair {
        serde_json::from_str(&format!(
            r#"{{"access_token":"seed","token_type":"Bearer","expires_in":{expires_in},"refresh_token":"refresh-seed"}}"#
        ))
        .expect("the seed pair is valid")
    }

    #[tokio::test]
    async fn five_concurrent_callers_cause_one_refresh() {
        // The wall's reads thread asks from several tasks at once. Five refreshes rotate the refresh
        // token five times and invalidate four of them, which is a sign-out, not a retry.
        let auth = Auth::with_pair(Counting::new(900), pair(0)); // expires_in 0: stale immediately
        let (a, b, c, d, e) = tokio::join!(
            auth.access_token(),
            auth.access_token(),
            auth.access_token(),
            auth.access_token(),
            auth.access_token()
        );
        for result in [a, b, c, d, e] {
            assert!(result.is_ok(), "a caller failed: {result:?}");
        }
        assert_eq!(
            auth.refresher.calls.load(Ordering::SeqCst),
            1,
            "five callers must cost one refresh"
        );
    }

    #[tokio::test]
    async fn a_fresh_token_is_not_refreshed_at_all() {
        let auth = Auth::with_pair(Counting::new(900), pair(900));
        assert_eq!(auth.access_token().await.unwrap(), "seed");
        assert_eq!(auth.access_token().await.unwrap(), "seed");
        assert_eq!(auth.refresher.calls.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn a_token_past_eighty_percent_of_its_life_is_refreshed() {
        let auth = Auth::with_pair(Counting::new(900), pair(1));
        // One second of life: 80% is 0.8 s, so a short sleep is enough to cross the line without a
        // clock abstraction that would prove nothing about the real code path.
        tokio::time::sleep(Duration::from_millis(900)).await;
        assert_eq!(auth.access_token().await.unwrap(), "access-0");
        assert_eq!(auth.refresher.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn a_second_caller_meeting_the_same_401_does_not_refresh_twice() {
        // Both tasks hold the rejected token; the first refresh replaces it, the second sees a
        // different token and returns that one.
        let auth = Auth::with_pair(Counting::new(900), pair(0));
        let (a, b) = tokio::join!(auth.replace_rejected("seed"), auth.replace_rejected("seed"));
        assert_eq!(a.unwrap(), "access-0");
        assert_eq!(
            b.unwrap(),
            "access-0",
            "the second caller must use the token the first one fetched"
        );
        assert_eq!(auth.refresher.calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn the_rotated_refresh_token_is_kept() {
        struct Echo(std::sync::Mutex<Vec<String>>);
        impl Refresher for Echo {
            async fn refresh(&self, refresh_token: &str) -> Result<TokenPair, Error> {
                self.0.lock().unwrap().push(refresh_token.to_string());
                let n = self.0.lock().unwrap().len();
                Ok(serde_json::from_str(&format!(
                    r#"{{"access_token":"access-{n}","token_type":"Bearer","expires_in":0,"refresh_token":"refresh-{n}"}}"#
                ))
                .unwrap())
            }
        }
        let auth = Auth::with_pair(Echo(Default::default()), pair(0));
        auth.access_token().await.unwrap();
        auth.access_token().await.unwrap();
        let seen = auth.refresher.0.lock().unwrap().clone();
        assert_eq!(
            seen,
            vec!["refresh-seed".to_string(), "refresh-1".to_string()],
            "a replayed refresh token is refused by the server, so the rotated one must be kept"
        );
    }

    #[test]
    fn a_token_pair_never_prints_its_tokens() {
        // INV-08: the guarantee is the hand-written Debug, not a filter downstream.
        let pair = pair(900);
        let printed = format!("{pair:?}");
        assert!(
            !printed.contains("seed"),
            "the access token printed: {printed}"
        );
        assert!(
            !printed.contains("refresh-seed"),
            "the refresh token printed: {printed}"
        );
        assert!(
            printed.contains("<redacted>"),
            "the redaction must be visible, got {printed}"
        );
    }
}

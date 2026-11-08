//! The socket thread: connect, subscribe, hand updates to the view loop, survive.
//!
//! One thread, one current-thread runtime, one connection per attempt. The UI thread sends nothing
//! and awaits nothing: it drains [`Update`]s between frames and calls the waker it was given so Rune
//! draws the new picture immediately.
//!
//! The loop is deliberately blunt, because every clever version of it is a bug:
//!
//! 1. take a token (the reads thread's [`Http`] refreshes it at 80% of its life, and on a `401`);
//! 2. connect with the token **in the connect frame's `data`**;
//! 3. **re-read the picture** — `live-feed` has no replay, so this is the only thing that closes the
//!    gap — and only then mark it trusted (`INV-09`);
//! 4. subscribe, declaring it before the handshake and re-arming on every reconnect;
//! 5. wait for the connection to end, map its refusal to an [`Action`], and sleep a jittered
//!    backoff before starting again — or stop, when the refusal says stop.

use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio_centrifuge::client::Client;
use tokio_centrifuge::config::Config;

use crate::config::Minos;
use crate::data::Http;
use crate::data::types::FeedPosition;
use crate::live::wire::LiveFix;
use crate::live::{Action, Backoff, Refusal, Trust, wire};

/// The one channel this app subscribes to.
///
/// **A channel name is never built.** `live-feed` is fixed by the protocol and granted by the server;
/// a name assembled from an id (`game:<id>`) is refused with `107`, which is the server's way of
/// saying "I hand these out". v0.1 watches the feed and nothing else.
pub const LIVE_FEED: &str = "live-feed";

/// How long a handshake may take before it counts as a failed attempt. A connect that never answers
/// is a real failure mode, and an actor blocked on it forever is a wall that has silently stopped.
const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);

/// What the socket learned, for the view loop.
#[derive(Debug)]
pub enum Update {
    /// The socket is up. Says nothing about the picture: that arrives as [`Update::Picture`].
    Connected,
    /// The standing picture, re-read after a (re)connect. Until this lands, nothing on screen is
    /// current (`INV-09`).
    Picture(Vec<FeedPosition>),
    /// One live fix. `backfilled` travels with it: it is real and did not just happen (`INV-03`).
    Fix(LiveFix),
    /// A disconnect: whatever is on screen is from before the gap and must stop being presented as
    /// current until the next [`Update::Picture`].
    Untrusted,
    /// A refusal that stops the actor. `101`/`103`/`107` — a human has to fix something.
    Refused { code: u16, reason: String },
    /// `109`: the token expired. The reads thread refreshes; this connection retries.
    RefreshDue,
    /// A temporary failure. The actor will try again after `delay`.
    Retrying { attempt: u32, delay: Duration },
    /// The actor could not start at all (no credential, no route). The view shows it.
    Failed(String),
}

/// The handle the view loop holds.
pub struct LiveThread {
    updates: Receiver<Update>,
    stop: tokio::sync::mpsc::UnboundedSender<()>,
    /// Joined on drop: an actor still reconnecting while the process tears down is a thread outliving
    /// its socket, which is how a wall logs an abort on exit.
    join: Option<std::thread::JoinHandle<()>>,
}

impl LiveThread {
    /// Start the actor. `wake` runs after every update, so the view draws it now rather than on its
    /// next idle frame.
    pub fn spawn(
        minos: Minos,
        socket_url: String,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> LiveThread {
        Self::spawn_with(minos, socket_url, None, wake)
    }

    /// [`LiveThread::spawn`] with a **deliberate drop** after `drop_after`: the soak's
    /// `--reconnect-after`, so the reconnect path is exercised against a real socket rather than only in
    /// a model test (`INV-09`). Fires once, and says so in the log when it does.
    pub fn spawn_with(
        minos: Minos,
        socket_url: String,
        drop_after: Option<std::time::Duration>,
        wake: impl Fn() + Send + Sync + 'static,
    ) -> LiveThread {
        let (updates, incoming) = channel();
        let (stop, stopped) = tokio::sync::mpsc::unbounded_channel();
        let wake = Arc::new(wake);
        let join = std::thread::Builder::new()
            .name("arcons-live".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = updates.send(Update::Failed(format!(
                            "could not start the socket runtime: {error}"
                        )));
                        return;
                    }
                };
                runtime.block_on(run(minos, socket_url, drop_after, updates, stopped, wake));
            })
            .expect("the socket thread starts");
        LiveThread {
            updates: incoming,
            stop,
            join: Some(join),
        }
    }

    /// Drain what has arrived since the last frame.
    pub fn try_recv(&self) -> Option<Update> {
        self.updates.try_recv().ok()
    }
}

impl Drop for LiveThread {
    fn drop(&mut self) {
        let _ = self.stop.send(());
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

/// The connect frame's `data`: `{"token": "…"}` and nothing else.
///
/// This is the whole reason `vendor/tokio-centrifuge` exists. The token goes in the connect command's
/// `data`, **not** in the frame's `token` field (Centrifugo core would reject it) and **not** in an
/// `Authorization` header. Minos reads it here; the pinned protocol is literal about it.
pub fn connect_data(token: &str) -> Vec<u8> {
    // Built by hand rather than with a struct so the bytes are exactly this and a test can say so.
    serde_json::to_vec(&serde_json::json!({ "token": token })).unwrap_or_default()
}

async fn run(
    minos: Minos,
    socket_url: String,
    drop_after: Option<std::time::Duration>,
    updates: Sender<Update>,
    mut stopped: tokio::sync::mpsc::UnboundedReceiver<()>,
    wake: Arc<dyn Fn() + Send + Sync>,
) {
    // A heartbeat from the thread that never stops: the wall's own answer to "is this process alive",
    // independent of whether the compositor is drawing. The *view* logs what it received; this logs that
    // the process is still here and how much memory it is holding, which is what a soak reads (#42).
    tokio::spawn(async {
        let mut tick = tokio::time::interval(std::time::Duration::from_secs(30));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tick.tick().await;
            tracing::info!(
                uptime_ms = crate::platform::since_start() as u64,
                rss_mb = crate::platform::rss_mb()
                    .map(|mb| mb.round() as i64)
                    .unwrap_or(-1),
                "alive"
            );
        }
    });

    // The deliberate drop happens once: a wall that drops itself every N seconds is a wall with a bug,
    // and this flag is what makes the difference a fact in the code rather than a claim in a test.
    let mut forced = false;
    let mut sessions: u32 = 0;
    let http = match sign_in(&minos).await {
        Ok(http) => http,
        Err(error) => {
            let _ = updates.send(Update::Failed(format!(
                "could not sign in for the socket: {error}"
            )));
            wake();
            return;
        }
    };

    // The configured endpoint, not one derived here: a deployment may front the socket with a
    // different host or path from the REST API's.
    let url = socket_url;
    let backoff = Backoff::default();
    let mut attempt: u32 = 0;
    let mut roll = Roll::new();

    loop {
        if stopped.try_recv().is_ok() {
            return;
        }

        // 1. A token. `Http` refreshes at 80% of the stated life and on a 401, so this is either
        //    the current one or a fresh one.
        let token = match http.access_token().await {
            Ok(token) => token,
            Err(error) => {
                // A credential problem is a human's: stop rather than spin.
                let _ = updates.send(Update::Failed(format!(
                    "could not get a socket token: {error}"
                )));
                wake();
                return;
            }
        };

        sessions += 1;
        tracing::info!(session = sessions, "socket: connecting");
        let this_drop = if forced { None } else { drop_after };
        match session(
            &http,
            &url,
            &token,
            &updates,
            &mut stopped,
            &wake,
            this_drop,
            sessions,
        )
        .await
        {
            Session::Stopped => return,
            Session::Ended { code: _, reason } if reason.starts_with("soak:") => {
                forced = true;
                tracing::warn!("{reason} — reconnecting once, as the soak asked");
                wake();
            }
            Session::Ended { code, reason } => {
                let refusal = Refusal::from_code(u16::try_from(code).unwrap_or(0));
                match refusal.map(Refusal::action) {
                    Some(Action::Stop) => {
                        let _ = updates.send(Update::Refused {
                            code: code as u16,
                            reason,
                        });
                        wake();
                        return;
                    }
                    Some(Action::RefreshThenRetry) => {
                        let _ = updates.send(Update::RefreshDue);
                        wake();
                        // The next `access_token()` sees a token past 80% and refreshes it.
                        attempt = 0;
                    }
                    Some(Action::Ignore) => {
                        // `105`: already subscribed is not an error. Try again without waiting.
                        attempt = 0;
                    }
                    Some(Action::RetryWithBackoff) | None => {
                        // `100`, or a code this client has never seen: both are transient as far as
                        // we know, and an unknown code is logged rather than guessed at.
                        if refusal.is_none() {
                            eprintln!(
                                "arcons-cc: the socket was refused with unknown code {code}: {reason}"
                            );
                        }
                        attempt = attempt.saturating_add(1);
                        let delay = backoff.delay(attempt, roll.next());
                        let _ = updates.send(Update::Retrying { attempt, delay });
                        wake();
                        tokio::select! {
                            _ = tokio::time::sleep(delay) => {}
                            _ = stopped.recv() => return,
                        }
                    }
                }
            }
        }
    }
}

/// One connection, from handshake to its end.
enum Session {
    /// The view loop is going away.
    Stopped,
    /// The connection ended, with the code the server gave it (0 when there was none).
    Ended { code: u32, reason: String },
}

// Eight arguments, and every one of them is a different thing this function must not re-derive for
// itself: the two clients, the token, where updates go, how it is stopped, how it wakes the view, the
// soak's deliberate drop, and which session this is. A struct here would only move the same list.
#[allow(clippy::too_many_arguments)]
async fn session(
    http: &Http,
    url: &str,
    token: &str,
    updates: &Sender<Update>,
    stopped: &mut tokio::sync::mpsc::UnboundedReceiver<()>,
    wake: &Arc<dyn Fn() + Send + Sync>,
    drop_after: Option<Duration>,
    session_number: u32,
) -> Session {
    let config = Config::new()
        .with_name("arcons-cc")
        // The token, in `data`. Not `with_token`: see `connect_data`.
        .with_connect_data(connect_data(token));
    let client = Client::new(url, config);

    // Who is trusted: a disconnect makes the picture unproven until a read completes again
    // (`INV-09`). Shared with the callbacks because they outlive this scope.
    let trust = Arc::new(Mutex::new(Trust::default()));
    let (ended_tx, mut ended) = tokio::sync::mpsc::unbounded_channel::<(u32, String)>();

    {
        let updates = updates.clone();
        let wake = Arc::clone(wake);
        client.on_connected(move |_| {
            let _ = updates.send(Update::Connected);
            wake();
        });
    }
    {
        let updates = updates.clone();
        let wake = Arc::clone(wake);
        let trust = Arc::clone(&trust);
        client.on_disconnected(move |event| {
            // Every disconnect, including a requested one: "it was only a blip" is the assumption
            // that puts a stale hull in the wrong ocean.
            if let Ok(mut trust) = trust.lock() {
                *trust = trust.after_disconnect();
            }
            let _ = updates.send(Update::Untrusted);
            wake();
            let _ = ended_tx.send((event.code, event.reason.to_string()));
        });
    }

    // The subscription is declared **before** the handshake and fired without awaiting: awaiting
    // `subscribe()` while disconnected never resolves, because its reply needs the connection that
    // the await is standing in front of.
    let subscription = client.new_subscription(LIVE_FEED);
    {
        let updates = updates.clone();
        let wake = Arc::clone(wake);
        let trust = Arc::clone(&trust);
        subscription.on_publication(move |publication| {
            match wire::parse(&publication.data) {
                Ok(fix) => {
                    let trusted = trust
                        .lock()
                        .map(|trust| trust.accepts_publication())
                        .unwrap_or(false);
                    if trusted {
                        let _ = updates.send(Update::Fix(fix));
                        wake();
                    }
                    // Untrusted means a read is in flight or about to be: the publication is real but
                    // there is no picture to apply it to yet, and the read that follows supersedes
                    // it. Dropping it with the reason in one place beats applying it to the gap.
                }
                Err(error) => eprintln!("arcons-cc: ignoring a {LIVE_FEED} publication: {error}"),
            }
        });
    }
    let _ = subscription.subscribe();

    // 2. The handshake.
    match tokio::time::timeout(HANDSHAKE_TIMEOUT, client.connect()).await {
        Ok(Ok(())) => {}
        Ok(Err(())) => {
            // The refusal itself arrives through `on_disconnected`, which is why the reason travels
            // on `ended` rather than being invented here.
            return wait_for_end(stopped, &mut ended, drop_after).await;
        }
        Err(_) => {
            return Session::Ended {
                code: 0,
                reason: format!(
                    "no handshake reply within {} s",
                    HANDSHAKE_TIMEOUT.as_secs()
                ),
            };
        }
    }

    // 3. The read that closes the gap. Until it completes, nothing is current.
    match http.positions().await {
        Ok(picture) => {
            // The read that closes the gap, in the log with its session number: this is the evidence a
            // soak needs that a reconnect re-read the picture rather than resuming on what it had
            // (`INV-09`).
            tracing::info!(
                session = session_number,
                units = picture.len(),
                "socket: picture read after connect"
            );
            let _ = updates.send(Update::Picture(picture));
            if let Ok(mut trust) = trust.lock() {
                *trust = trust.after_picture();
            }
            wake();
        }
        Err(error) => {
            // The socket is up but the picture is not: say so, and keep the picture untrusted. The
            // next reconnect tries again; a wall that draws nothing is honest, one that draws the
            // pre-gap markers as current is not.
            let _ = updates.send(Update::Retrying {
                attempt: 0,
                delay: Duration::ZERO,
            });
            eprintln!("arcons-cc: the socket is up but the picture read failed: {error}");
        }
    }

    // 4. Hold the connection until it ends or the view loop goes away.
    wait_for_end(stopped, &mut ended, drop_after).await
}

async fn wait_for_end(
    stopped: &mut tokio::sync::mpsc::UnboundedReceiver<()>,
    ended: &mut tokio::sync::mpsc::UnboundedReceiver<(u32, String)>,
    drop_after: Option<Duration>,
) -> Session {
    tokio::select! {
        _ = stopped.recv() => Session::Stopped,
        // The soak's deliberate drop (`--reconnect-after`). A *reason* travels with it, because a reader
        // must be able to tell a deliberate gap from a network one (`INV-09` says a gap is a gap).
        _ = async {
            match drop_after {
                Some(after) => tokio::time::sleep(after).await,
                None => std::future::pending().await,
            }
        } => Session::Ended { code: 0, reason: "soak: deliberate drop".to_string() },
        code = ended.recv() => match code {
            Some((code, reason)) => Session::Ended { code, reason },
            // The client dropped its sender: treat it as an ended connection rather than hanging.
            None => Session::Ended { code: 0, reason: "the socket client stopped".to_string() },
        },
    }
}

/// What `--self-check` learned from the socket.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
    /// The handshake completed.
    pub connected: bool,
    /// The server confirmed the subscription — not merely that we asked.
    pub subscribed: bool,
    pub channel: String,
}

/// Connect, subscribe to `live-feed`, and report whether the server said yes.
///
/// The CLI path, so it may block: nothing is drawing yet. A probe that only checked "the TCP connect
/// worked" would pass on a refusal, which is why it waits for the **subscribed** reply.
pub fn probe(minos: &Minos) -> Result<Probe, String> {
    probe_url(minos, &minos.socket_url())
}

/// The same probe against a socket URL the configuration named explicitly (`centrifugo_url`), which
/// is not always derivable from the REST base.
pub fn probe_url(minos: &Minos, socket_url: &str) -> Result<Probe, String> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| format!("could not start a runtime: {error}"))?;
    runtime.block_on(async {
        let http = sign_in(minos).await.map_err(|error| error.to_string())?;
        let token = http
            .access_token()
            .await
            .map_err(|error| error.to_string())?;
        let config = Config::new()
            .with_name("arcons-cc")
            .with_connect_data(connect_data(&token));
        let client = Client::new(socket_url, config);

        // The probe reports why, not just that: a refused handshake carries a code, and "connected:
        // false" with no reason is a wall that cannot be debugged.
        let (refused_tx, mut refused) = tokio::sync::mpsc::unbounded_channel::<(u32, String)>();
        client.on_disconnected(move |event| {
            let _ = refused_tx.send((event.code, event.reason.to_string()));
        });
        let (subscribed_tx, mut subscribed) = tokio::sync::mpsc::unbounded_channel::<String>();
        let subscription = client.new_subscription(LIVE_FEED);
        subscription.on_subscribed(move |event| {
            let _ = subscribed_tx.send(event.channel.to_string());
        });
        let _ = subscription.subscribe();

        let connected = tokio::time::timeout(HANDSHAKE_TIMEOUT, client.connect())
            .await
            .map_err(|_| {
                format!(
                    "no handshake reply within {} s",
                    HANDSHAKE_TIMEOUT.as_secs()
                )
            })?
            .is_ok();
        if !connected {
            // Say WHY. "connected: false" with no reason is a wall nobody can debug.
            let refusal = refused.try_recv().ok().map(|(code, reason)| {
                match Refusal::from_code(code as u16) {
                    Some(refusal) => {
                        format!("{code} ({refusal:?} → {:?}): {reason}", refusal.action())
                    }
                    None => format!("{code} (unknown to this client): {reason}"),
                }
            });
            return Err(match refusal {
                Some(refusal) => format!("the socket refused the connection: {refusal}"),
                None => "the socket refused the connection with no refusal event".to_string(),
            });
        }
        let channel = tokio::time::timeout(Duration::from_secs(10), subscribed.recv())
            .await
            .ok()
            .flatten();
        Ok(Probe {
            connected: true,
            subscribed: channel.is_some(),
            channel: channel.unwrap_or_else(|| LIVE_FEED.to_string()),
        })
    })
}

async fn sign_in(minos: &Minos) -> Result<Http, crate::data::Error> {
    match minos.refresh_token() {
        Some(token) => Http::with_refresh_token(minos, token.to_string()).await,
        None => Http::sign_in(minos).await,
    }
}

/// A jitter source, seeded from the clock once per actor and advanced locally.
///
/// `std` has no RNG and this is not worth a dependency: the property that matters is that two
/// displays do not roll the same number, and a clock-seeded xorshift gives that.
struct Roll(u64);

impl Roll {
    fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15)
            | 1;
        Self(seed)
    }

    fn next(&mut self) -> f64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_connect_frame_carries_exactly_the_token_in_data() {
        // INV/§2, literal: `{"token": "…"}` in the connect command's `data`. Not a header, and not
        // the frame's `token` field. Asserted on the bytes the actor hands the transport.
        let bytes = connect_data("abc.def.ghi");
        assert_eq!(
            String::from_utf8(bytes.clone()).unwrap(),
            r#"{"token":"abc.def.ghi"}"#
        );
        let parsed: serde_json::Value = serde_json::from_slice(&bytes).expect("valid JSON");
        assert_eq!(
            parsed.as_object().map(|object| object.len()),
            Some(1),
            "exactly one key"
        );
        assert_eq!(parsed["token"], "abc.def.ghi");
        assert!(
            parsed.get("data").is_none(),
            "the payload is the data, not a wrapper around it"
        );
    }

    #[test]
    fn the_jitter_source_moves_and_stays_in_range() {
        let mut roll = Roll::new();
        let mut seen = Vec::new();
        for _ in 0..64 {
            let value = roll.next();
            assert!((0.0..1.0).contains(&value), "roll out of range: {value}");
            seen.push(value);
        }
        seen.dedup();
        assert!(seen.len() > 60, "the jitter source is repeating itself");
    }

    #[test]
    fn a_publication_before_the_first_read_is_not_trusted() {
        // The consequence of INV-09 at the actor's edge: the shared trust starts unproven, so the
        // first publications of a session — if any arrive before the read — are not applied.
        assert!(!Trust::default().accepts_publication());
    }
}

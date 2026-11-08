//! The backend thread: one current-thread Tokio runtime, one client, and a channel.
//!
//! **The UI thread never blocks.** The view loop sends a [`Command`] — a non-blocking `send` on an
//! unbounded channel — and drains [`Reply`]s on its next frame. Nothing here awaits on the view
//! loop's thread, and nothing on the view loop's thread awaits here.
//!
//! Why a current-thread runtime rather than `reqwest`'s blocking client: the socket actor (#4) wants
//! async anyway, both live on threads of their own, and one runtime per thread is the shape that
//! keeps blocking work out of a shared executor. The cost — no work stealing — is irrelevant for
//! two routes and their retries.

use std::sync::Arc;
use std::sync::mpsc::{Receiver, channel};

use tokio::sync::mpsc::{UnboundedSender, unbounded_channel};

use crate::config::Minos;
use crate::data::Error;
use crate::data::http::Http;
use crate::data::types::{FeedPosition, UnitCommander, UnitDetail};

/// What the view loop asks for. One variant per read the display performs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Command {
    /// The standing picture: every hull, whether or not it has reported.
    Picture,
    /// One hull's identity.
    Unit(i64),
    /// One hull's command history, newest first.
    Commander(i64),
    /// One hull's specification versions, newest first.
    Specifications(i64),
    /// The fleet register: every hull with its picture URL, in one read.
    Units,
    /// One hull's picture, by the presigned URL the register gave.
    Image(i64, String),
}

/// What comes back. It carries typed values or an [`Error`] — never a status code, and never a token.
///
/// Boxed where the payload is large, so a `Reply` is cheap to hold in a channel between frames: the
/// view loop drains these, and a big variant on the stack is pure waste there.
#[derive(Debug)]
pub enum Reply {
    Picture(Result<Vec<FeedPosition>, Error>),
    Unit(i64, Result<Box<UnitDetail>, Error>),
    Commander(i64, Result<Vec<UnitCommander>, Error>),
    /// The fleet register: picture URLs for every hull, in one read.
    Units(Result<Vec<crate::data::types::UnitSummary>, Error>),
    /// One hull's picture bytes, fetched off the UI thread and decoded by the view's cache.
    Image(i64, Result<Vec<u8>, Error>),
    Specifications(
        i64,
        Result<Vec<crate::data::types::UnitSpecification>, Error>,
    ),
    /// The backend could not start, or could not sign in. The view loop surfaces it once.
    Failed(String),
}

/// The handle the UI thread holds. `request` never blocks and `try_recv` never waits.
pub struct Backend {
    commands: UnboundedSender<Command>,
    replies: Receiver<Reply>,
}

impl Backend {
    /// Start the backend thread and sign in on it. Returns immediately; the sign-in happens off the
    /// caller's thread, and a failure arrives as [`Reply::Failed`].
    pub fn spawn(minos: Minos) -> Result<Backend, Error> {
        let (commands, mut incoming) = unbounded_channel::<Command>();
        let (outgoing, replies) = channel::<Reply>();
        let thread_replies = outgoing.clone();

        std::thread::Builder::new()
            .name("arcons-data".to_string())
            .spawn(move || {
                let runtime = match tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                {
                    Ok(runtime) => runtime,
                    Err(error) => {
                        let _ = thread_replies.send(Reply::Failed(format!(
                            "could not start the reads runtime: {error}"
                        )));
                        return;
                    }
                };
                runtime.block_on(async move {
                    let http = match sign_in(&minos).await {
                        Ok(http) => http,
                        Err(error) => {
                            let _ = thread_replies.send(Reply::Failed(format!(
                                "could not sign in to Minos: {error}"
                            )));
                            // Keep draining, so a late request does not panic on a closed channel.
                            while incoming.recv().await.is_some() {}
                            return;
                        }
                    };
                    let http = Arc::new(http);
                    while let Some(command) = incoming.recv().await {
                        let http = Arc::clone(&http);
                        let replies = thread_replies.clone();
                        // Each read is its own task: a slow picture read never delays a card.
                        tokio::spawn(async move {
                            let _ = replies.send(perform(&http, command).await);
                        });
                    }
                });
            })
            .map_err(|error| {
                Error::Transport(format!("could not start the reads thread: {error}"))
            })?;

        Ok(Backend { commands, replies })
    }

    /// Ask for something. Never blocks and never awaits: the UI thread calls this between frames.
    pub fn request(&self, command: Command) {
        // A closed channel means the backend thread is gone; the next frame sees no reply, and
        // `--self-check` or the connection badge says so. Dropping the error is deliberate.
        let _ = self.commands.send(command);
    }

    /// Drain what has arrived since the last frame.
    pub fn try_recv(&self) -> Option<Reply> {
        self.replies.try_recv().ok()
    }
}

async fn sign_in(minos: &Minos) -> Result<Http, Error> {
    match minos.refresh_token() {
        Some(token) => Http::with_refresh_token(minos, token.to_string()).await,
        None => Http::sign_in(minos).await,
    }
}

async fn perform(http: &Http, command: Command) -> Reply {
    match command {
        Command::Picture => Reply::Picture(http.positions().await),
        Command::Unit(id_unit) => Reply::Unit(id_unit, http.unit(id_unit).await.map(Box::new)),
        Command::Units => Reply::Units(http.units().await),
        Command::Image(id_unit, url) => Reply::Image(id_unit, http.image(&url).await),
        Command::Commander(id_unit) => Reply::Commander(id_unit, http.commander(id_unit).await),
        Command::Specifications(id_unit) => {
            Reply::Specifications(id_unit, http.specifications(id_unit).await)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// A stub Minos: `/auth/login` and `/live-feed/positions`, on loopback, in this process.
    fn stub_minos(body: &'static str) -> (String, std::thread::JoinHandle<()>) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().take(2) {
                let Ok(mut stream) = stream else { return };
                use std::io::{Read, Write};
                let mut request = [0u8; 4096];
                let read = stream.read(&mut request).unwrap_or(0);
                let request = String::from_utf8_lossy(&request[..read]).to_string();
                let payload = if request.starts_with("POST /api/v1/auth/login") {
                    r#"{"status_code":200,"message":"Successfull","data":{"access_token":"a","token_type":"Bearer","expires_in":900,"refresh_token":"r"}}"#
                } else {
                    body
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        (
            format!("http://127.0.0.1:{port}/api/v1"),
            std::thread::spawn(|| {}),
        )
    }

    const ONE_HULL: &str = r#"{"status_code":200,"message":"Successfull","data":[{"id_unit":8,"name":"KRI Abdul Halim Perdanakusuma","hull_number":"KRI-355","position":{"latitude":-7.88,"longitude":112.53,"speed_kn":0.08,"course_deg":255,"accuracy_m":7.2,"recorded_at":"2026-09-29T09:56:07+07:00","received_at":"2026-09-29T09:56:07+07:00","age_seconds":10.0,"backfilled":false}}]}"#;

    #[test]
    fn a_picture_arrives_without_the_ui_thread_waiting() {
        let (base, _stub) = stub_minos(ONE_HULL);
        let minos = Minos::new(
            base,
            crate::config::Credential::Password {
                identifier: "wall".into(),
                password: crate::config::Secret::new("not-a-real-password"),
            },
        );
        let backend = Backend::spawn(minos).expect("the backend thread starts");
        backend.request(Command::Picture);

        // The view loop's rule: never wait. Poll like a frame loop would.
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut reply = None;
        while Instant::now() < deadline {
            if let Some(got) = backend.try_recv() {
                reply = Some(got);
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        match reply {
            Some(Reply::Picture(Ok(picture))) => {
                assert_eq!(picture.len(), 1);
                assert_eq!(picture[0].id_unit, 8);
                assert_eq!(
                    picture[0].position.as_ref().and_then(|fix| fix.speed_kn),
                    Some(0.08)
                );
            }
            other => panic!("expected a picture, got {other:?}"),
        }
    }
}

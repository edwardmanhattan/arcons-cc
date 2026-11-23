//! **The desk's own write path** — one blocking POST to the relayed route, and the credential that carries it.
//!
//! This is deliberately **not** the wall's client (`arcons_cc::data::Http`). That client reads a picture; the desk
//! writes fixes, and ADR-0009 keeps the two apart on purpose: the wall's process must not hold a code path that can
//! put a vessel somewhere. What is shared is the configuration — the same deployment base and the same credential
//! directory — and the rule that nothing here logs a token, a host or a username (`INV-08`).
//!
//! A `401` is refreshed **once** and the same body is re-sent. The regime is not re-checked here: that is the
//! caller's, because it owns the clock, and a body re-sent a minute later may have crossed the live/backfill line.
//!
//! Blocking on purpose. The UI thread never calls this: the send runs on a worker, exactly as the wall's reads do,
//! and the frame loop only ever sees the answer.

use std::time::Duration;

use crate::report::{Answer, Body, PATH};
use crate::send::{Failure, Transport};

/// How long one request may take. A desk waiting on a ship's report cannot wait forever, and a hung socket is a
/// screen that never says what happened.
const TIMEOUT: Duration = Duration::from_secs(20);

/// A signed-in desk client. Cheap enough to move to a worker; it holds a token pair and a connection pool.
pub struct Client {
    base: String,
    agent: reqwest::blocking::Client,
    access: String,
    refresh: Option<String>,
}

impl Transport for Client {
    /// Post one body, refreshing once if the token has expired.
    fn post(&mut self, body: &Body) -> Result<Answer, Failure> {
        let first = self.attempt(body)?;
        if first.status != 401 {
            return Ok(first);
        }
        self.refresh()?;
        self.attempt(body)
    }
}

impl Client {
    /// Sign in against a deployment with an identifier and a password.
    ///
    /// The wall prefers a refresh token because a password at rest is a password to rotate; the desk starts with
    /// what an operator can type, and keeps the refresh token the login returns so a later start need not ask.
    pub fn sign_in(base: &str, identifier: &str, password: &str) -> Result<Self, Failure> {
        let base = base.trim_end_matches('/').to_string();
        let agent = agent()?;
        let answer = request(
            &agent,
            &base,
            "/auth/login",
            &serde_json::json!({
                "identifier": identifier,
                "password": password,
                // A desktop has no cookie jar: without this the pair arrives only as a cookie.
                "return_refresh_token": true,
            }),
            None,
        )?;
        let (access, refresh) = tokens(&answer)?;
        Ok(Self {
            base,
            agent,
            access,
            refresh: Some(refresh),
        })
    }

    /// A base URL and a credential from the environment: `MINOS_BASE_URL`, `MINOS_IDENTIFIER`, `MINOS_PASSWORD`.
    ///
    /// The environment rather than a file, because this is a deployment's setting and not the operator's: a value
    /// in the tree is a value in the repository, and `INV-08` keeps both the host and the credential out of it.
    pub fn from_environment() -> Result<Self, Failure> {
        let base = std::env::var("MINOS_BASE_URL").map_err(|_| {
            Failure::new("no deployment: set MINOS_BASE_URL (e.g. https://host/api/v1)")
        })?;
        let identifier = std::env::var("MINOS_IDENTIFIER")
            .map_err(|_| Failure::new("no credential: set MINOS_IDENTIFIER and MINOS_PASSWORD"))?;
        let password = std::env::var("MINOS_PASSWORD")
            .map_err(|_| Failure::new("no credential: set MINOS_IDENTIFIER and MINOS_PASSWORD"))?;
        Self::sign_in(&base, &identifier, &password)
    }

    /// The base this client talks to, without a credential in it.
    pub fn base(&self) -> &str {
        &self.base
    }

    fn attempt(&self, body: &Body) -> Result<Answer, Failure> {
        request(
            &self.agent,
            &self.base,
            PATH,
            &serde_json::to_value(body)
                .map_err(|_| Failure::new("the body would not serialise"))?,
            Some(&self.access),
        )
    }

    fn refresh(&mut self) -> Result<(), Failure> {
        let Some(refresh) = self.refresh.clone() else {
            return Err(Failure::new(
                "the credential expired and no refresh token was kept",
            ));
        };
        let answer = request(
            &self.agent,
            &self.base,
            "/auth/refresh",
            &serde_json::json!({ "refresh_token": refresh }),
            None,
        )?;
        let (access, next) = tokens(&answer)?;
        self.access = access;
        self.refresh = Some(next);
        Ok(())
    }
}

/// One request, and the answer's status and body — whatever the status.
///
/// A transport failure (no connection, a timeout, a body that never finished) is a [`Failure`]; an HTTP status is an
/// [`Answer`], because a `403` is Minos talking and not the network.
fn request(
    agent: &reqwest::blocking::Client,
    base: &str,
    path: &str,
    body: &serde_json::Value,
    bearer: Option<&str>,
) -> Result<Answer, Failure> {
    let mut call = agent.post(format!("{base}{path}")).json(body);
    if let Some(token) = bearer {
        call = call.bearer_auth(token);
    }
    let response = call.send().map_err(|error| Failure::new(short(&error)))?;
    let status = response.status().as_u16();
    let body = response
        .text()
        .map_err(|error| Failure::new(short(&error)))?;
    Ok(Answer::new(status, body))
}

/// The access and refresh tokens out of a login or a refresh answer.
fn tokens(answer: &Answer) -> Result<(String, String), Failure> {
    let envelope: serde_json::Value = serde_json::from_str(&answer.body)
        .map_err(|_| Failure::new("the credential answer was not JSON"))?;
    let data = envelope
        .get("data")
        .ok_or_else(|| Failure::new("the credential answer had no data"))?;
    let access = data
        .get("access_token")
        .and_then(|value| value.as_str())
        .ok_or_else(|| Failure::new("the credential answer had no access token"))?
        .to_string();
    let refresh = data
        .get("refresh_token")
        .and_then(|value| value.as_str())
        .unwrap_or_default()
        .to_string();
    Ok((access, refresh))
}

/// A `reqwest` failure in the operator's words, and **never** the URL: a host in a log line is what `INV-08` forbids.
fn short(error: &reqwest::Error) -> String {
    if error.is_timeout() {
        "the request timed out".to_string()
    } else if error.is_connect() {
        "could not reach the deployment".to_string()
    } else if error.is_body() || error.is_decode() {
        "the answer was not readable".to_string()
    } else {
        "the request failed".to_string()
    }
}

fn agent() -> Result<reqwest::blocking::Client, Failure> {
    reqwest::blocking::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(|error| Failure::new(short(&error)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    /// What one request looked like from the server's side.
    #[derive(Default)]
    struct Seen {
        paths: Vec<String>,
        bearers: Vec<String>,
        bodies: Vec<String>,
    }

    /// A Minos on loopback: login, refresh, and a queue of answers for the relayed route.
    struct Stub {
        base: String,
        seen: Arc<Mutex<Seen>>,
        refreshes: Arc<AtomicUsize>,
    }

    fn stub(answers: Vec<(u16, String)>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let base = format!(
            "http://127.0.0.1:{}/api/v1",
            listener.local_addr().unwrap().port()
        );
        let seen = Arc::new(Mutex::new(Seen::default()));
        let refreshes = Arc::new(AtomicUsize::new(0));
        let (thread_seen, thread_refreshes) = (Arc::clone(&seen), Arc::clone(&refreshes));
        let queue = Arc::new(Mutex::new(std::collections::VecDeque::from(answers)));

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buffer = [0u8; 16384];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let line = request.lines().next().unwrap_or_default().to_string();
                let path = line.split(' ').nth(1).unwrap_or_default().to_string();
                let bearer = request
                    .lines()
                    .find(|line| line.to_ascii_lowercase().starts_with("authorization:"))
                    .and_then(|line| line.split_once(' '))
                    .map(|(_, token)| token.trim().to_string())
                    .unwrap_or_default();
                let body = request
                    .split_once("\r\n\r\n")
                    .map(|(_, body)| body.to_string())
                    .unwrap_or_default();
                {
                    let mut seen = thread_seen.lock().unwrap();
                    seen.paths.push(path.clone());
                    seen.bearers.push(bearer);
                    seen.bodies.push(body);
                }

                let (status, payload) = if path.ends_with("/auth/login") {
                    (200u16, pair("access-0", "refresh-0"))
                } else if path.ends_with("/auth/refresh") {
                    let n = thread_refreshes.fetch_add(1, Ordering::SeqCst) + 1;
                    (200, pair(&format!("access-{n}"), &format!("refresh-{n}")))
                } else {
                    queue
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or((500, "{}".to_string()))
                };

                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });

        Stub {
            base,
            seen,
            refreshes,
        }
    }

    fn pair(access: &str, refresh: &str) -> String {
        format!(
            r#"{{"status_code":200,"message":"OK","data":{{"access_token":"{access}","refresh_token":"{refresh}"}}}}"#
        )
    }

    fn body() -> Body {
        Body {
            id_unit: 18,
            recorded_at: "2026-11-20T07:03:22.000Z".to_string(),
            latitude: -6.1023,
            longitude: 106.8712,
            speed_kn: Some(8.1),
            course_deg: Some(91.0),
            accuracy_m: Some(7.2),
        }
    }

    #[test]
    fn a_body_goes_out_to_the_relayed_route_with_the_bearer_token() {
        let stub = stub(vec![(201, r#"{"status_code":201,"data":{"id_unit":18,"recorded_at":"2026-11-20T07:03:22Z","received_at":"2026-11-20T07:04:01Z","latest":true}}"#.to_string())]);
        let mut client = Client::sign_in(&stub.base, "operator", "secret").expect("signed in");
        let answer = client.post(&body()).expect("an answer");

        assert_eq!(answer.status, 201);
        let seen = stub.seen.lock().unwrap();
        assert_eq!(seen.paths[1], "/api/v1/live-feed/fixes/relayed");
        assert_eq!(seen.bearers[1], "Bearer access-0");
        assert!(
            seen.bodies[1].contains("\"id_unit\":18")
                && seen.bodies[1].contains("\"recorded_at\":\"2026-11-20T07:03:22.000Z\""),
            "the body is the row's own: {}",
            seen.bodies[1]
        );
    }

    #[test]
    fn an_expired_token_is_refreshed_once_and_the_same_body_is_sent_again() {
        let stub = stub(vec![
            (401, r#"{"status_code":401,"message":"Unauthorized","data":null}"#.to_string()),
            (201, r#"{"status_code":201,"data":{"id_unit":18,"recorded_at":"x","received_at":"y","latest":true}}"#.to_string()),
        ]);
        let mut client = Client::sign_in(&stub.base, "operator", "secret").expect("signed in");
        let answer = client.post(&body()).expect("an answer");

        assert_eq!(answer.status, 201, "the retry after a refresh succeeded");
        assert_eq!(stub.refreshes.load(Ordering::SeqCst), 1, "refreshed once");
        let seen = stub.seen.lock().unwrap();
        let posts: Vec<&String> = seen
            .paths
            .iter()
            .zip(seen.bodies.iter())
            .filter(|(path, _)| path.ends_with("/live-feed/fixes/relayed"))
            .map(|(_, body)| body)
            .collect();
        assert_eq!(posts.len(), 2, "the same body went out twice");
        assert_eq!(
            posts[0], posts[1],
            "a retry re-sends the body, not a new one"
        );
    }

    #[test]
    fn a_second_401_is_an_answer_and_not_another_refresh() {
        let stub = stub(vec![
            (
                401,
                r#"{"status_code":401,"message":"Unauthorized","data":null}"#.to_string(),
            ),
            (
                401,
                r#"{"status_code":401,"message":"Unauthorized","data":null}"#.to_string(),
            ),
        ]);
        let mut client = Client::sign_in(&stub.base, "operator", "secret").expect("signed in");
        let answer = client.post(&body()).expect("an answer");

        assert_eq!(answer.status, 401);
        assert_eq!(
            stub.refreshes.load(Ordering::SeqCst),
            1,
            "a loop of refreshes is a lockout; the second 401 is the answer"
        );
    }

    #[test]
    fn a_refresh_that_fails_is_a_transport_failure_and_changes_nothing() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let base = format!(
            "http://127.0.0.1:{}/api/v1",
            listener.local_addr().unwrap().port()
        );
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buffer = [0u8; 16384];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let path = request
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .split(' ')
                    .nth(1)
                    .unwrap_or_default()
                    .to_string();
                // The login works and **nothing else does**: no second token can be had, which is the case
                // under test — the failure must come back as a `Failure`, never as a retry loop.
                let (status, payload) = if path.ends_with("/auth/login") {
                    (200u16, pair("access-0", "refresh-0"))
                } else {
                    (
                        401u16,
                        r#"{"status_code":401,"message":"Unauthorized","data":null}"#.to_string(),
                    )
                };
                let response = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{payload}",
                    payload.len()
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let mut client = Client::sign_in(&base, "operator", "secret").expect("signed in");
        let failure = client.post(&body()).expect_err("no answer arrived");
        assert!(
            failure.message.contains("refresh token") || failure.message.contains("credential"),
            "the failure names what the operator can do, got {}",
            failure.message
        );
        assert!(
            !failure.message.contains("access-0") && !failure.message.contains("127.0.0.1"),
            "a failure never carries a token or a host: {}",
            failure.message
        );
    }
}

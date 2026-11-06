//! The HTTP calls themselves: five endpoints, one retry rule.
//!
//! **Nothing here runs on the UI thread.** The backend thread (`data::thread`) owns one
//! current-thread Tokio runtime and this client; the view loop talks to it over a channel.
//!
//! The one rule worth stating twice: a `401` is **refresh once, then stop**. A second `401` after a
//! successful refresh is a real failure — the account is disabled, the token is for another server,
//! or the clocks are wrong — and retrying it forever is how a wall burns a rate limit all night.

use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::config::Minos;
use crate::data::Error;
use crate::data::auth::{Auth, Refresher};
use crate::data::types::{
    Envelope, ErrorResponse, FeedPosition, TokenPair, UnitCommander, UnitDetail,
};

/// A request timeout. The wall is on a LAN or a VPN; a read that takes longer than this is a stall to
/// retry, not a picture to wait for.
const TIMEOUT: Duration = Duration::from_secs(20);

/// The refresh call, as the auth layer sees it: a POST and nothing else.
struct HttpRefresher {
    base: String,
    client: reqwest::Client,
}

impl Refresher for HttpRefresher {
    async fn refresh(&self, refresh_token: &str) -> Result<TokenPair, Error> {
        let response = self
            .client
            .post(format!("{}/auth/refresh", self.base))
            .json(&serde_json::json!({ "refresh_token": refresh_token }))
            .send()
            .await
            .map_err(transport)?;
        let status = response.status();
        let body = response.text().await.map_err(transport)?;
        if !status.is_success() {
            return Err(failure(status.as_u16(), &body));
        }
        let envelope: Envelope<TokenPair> = serde_json::from_str(&body).map_err(decode)?;
        Ok(envelope.data)
    }
}

/// A signed-in Minos client. Cheap to clone; one per backend thread.
#[derive(Clone)]
pub struct Http {
    base: String,
    client: reqwest::Client,
    auth: std::sync::Arc<Auth<HttpRefresher>>,
}

impl Http {
    /// Sign in with a password (or identifier). The wall's preferred mode is a refresh token
    /// ([`Http::with_refresh_token`]); this exists because the dev account uses it.
    pub async fn sign_in(minos: &Minos) -> Result<Http, Error> {
        let (identifier, password) = minos.password().ok_or(Error::NoCredential)?.to_owned();
        let client = build_client()?;
        let base = minos.base_url().trim_end_matches('/').to_string();

        let response = client
            .post(format!("{base}/auth/login"))
            .json(&serde_json::json!({
                "identifier": identifier,
                "password": password,
                // A desktop has no cookie jar: without this the pair arrives only as a cookie.
                "return_refresh_token": true,
            }))
            .send()
            .await
            .map_err(transport)?;
        let status = response.status();
        let body = response.text().await.map_err(transport)?;
        if !status.is_success() {
            return Err(failure(status.as_u16(), &body));
        }
        let envelope: Envelope<TokenPair> = serde_json::from_str(&body).map_err(decode)?;

        let refresher = HttpRefresher {
            base: base.clone(),
            client: client.clone(),
        };
        Ok(Self {
            base,
            client,
            auth: std::sync::Arc::new(Auth::with_pair(refresher, envelope.data)),
        })
    }

    /// Start from a refresh token and validate it by refreshing once. A wall can hold one of these
    /// for a long time; a password at rest is a password to rotate.
    pub async fn with_refresh_token(minos: &Minos, refresh_token: String) -> Result<Http, Error> {
        let client = build_client()?;
        let base = minos.base_url().trim_end_matches('/').to_string();
        let refresher = HttpRefresher {
            base: base.clone(),
            client: client.clone(),
        };
        let auth = Auth::with_refresh_token(refresher, refresh_token);
        auth.access_token().await?;
        Ok(Self {
            base,
            client,
            auth: std::sync::Arc::new(auth),
        })
    }

    /// How long the current access token still has. `None` before the first refresh.
    pub fn token_remaining(&self) -> Option<Duration> {
        self.auth.remaining()
    }

    /// `GET /live-feed/positions` — the standing picture, **not paginated**.
    ///
    /// Needs the `read` action on `/system/live-feed`. Includes hulls that have never reported, with
    /// `position` absent.
    pub async fn positions(&self) -> Result<Vec<FeedPosition>, Error> {
        self.get("/live-feed/positions").await
    }

    /// `GET /live-feed/positions/{id}`.
    pub async fn position(&self, id_unit: i64) -> Result<FeedPosition, Error> {
        self.get(&format!("/live-feed/positions/{id_unit}")).await
    }

    /// `GET /units/{id}`. Needs `read` on `/system/units`.
    pub async fn unit(&self, id_unit: i64) -> Result<UnitDetail, Error> {
        self.get(&format!("/units/{id_unit}")).await
    }

    /// `GET /units/{id}/commander` — newest first; the current one is the entry with no `to_at`.
    /// An empty list is a valid answer: nobody has ever commanded this hull.
    pub async fn commander(&self, id_unit: i64) -> Result<Vec<UnitCommander>, Error> {
        self.get(&format!("/units/{id_unit}/commander")).await
    }

    /// `GET path`, with one refresh-and-retry on a `401`.
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        let token = self.auth.access_token().await?;
        match self.get_with(path, &token).await? {
            Attempt::Ok(value) => Ok(value),
            Attempt::Unauthorized => {
                // One refresh, one retry. A second 401 is the answer, not a reason to try again.
                let fresh = self.auth.replace_rejected(&token).await?;
                match self.get_with(path, &fresh).await? {
                    Attempt::Ok(value) => Ok(value),
                    Attempt::Unauthorized => Err(Error::Unauthorized),
                }
            }
        }
    }

    async fn get_with<T: DeserializeOwned>(
        &self,
        path: &str,
        token: &str,
    ) -> Result<Attempt<T>, Error> {
        let response = self
            .client
            .get(format!("{}{path}", self.base))
            .bearer_auth(token)
            .send()
            .await
            .map_err(transport)?;
        let status = response.status();
        let body = response.text().await.map_err(transport)?;
        if status.as_u16() == 401 {
            return Ok(Attempt::Unauthorized);
        }
        if !status.is_success() {
            return Err(failure(status.as_u16(), &body));
        }
        let envelope: Envelope<T> = serde_json::from_str(&body).map_err(decode)?;
        Ok(Attempt::Ok(envelope.data))
    }
}

enum Attempt<T> {
    Ok(T),
    Unauthorized,
}

/// The client every call is made with.
///
/// One timeout for all of them: a stalled read is a stall to surface, not a picture to wait for.
pub fn build_client() -> Result<reqwest::Client, Error> {
    reqwest::Client::builder()
        .timeout(TIMEOUT)
        .build()
        .map_err(transport)
}

/// A transport failure. **The URL is included, the headers are not**: an error must never carry the
/// bearer token into a log line (`INV-08`).
fn transport(error: reqwest::Error) -> Error {
    Error::Transport(error.to_string())
}

fn decode(error: serde_json::Error) -> Error {
    Error::Decode(error.to_string())
}

/// An error status, parsed into the code and message the server sent when it is shaped like an
/// [`ErrorResponse`]. Branching is on the status and the field names, never on the prose.
fn failure(status: u16, body: &str) -> Error {
    let response: Option<ErrorResponse> = serde_json::from_str(body).ok();
    let message = response
        .as_ref()
        .map(|response| response.message.clone())
        .unwrap_or_else(|| "no error body".to_string());
    match status {
        401 => Error::Unauthorized,
        403 => Error::Forbidden { message },
        429 => Error::RateLimited,
        _ => Error::Status {
            status,
            message: fields(&response).unwrap_or(message),
        },
    }
}

/// `data.errors` as `field: reason`, so a 400 says which field was wrong.
fn fields(response: &Option<ErrorResponse>) -> Option<String> {
    let errors = response.as_ref()?.data.as_ref()?.errors.clone();
    if errors.is_empty() {
        return None;
    }
    Some(
        errors
            .into_iter()
            .map(|(field, reason)| format!("{field}: {reason}"))
            .collect::<Vec<_>>()
            .join("; "),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use crate::config::{Credential, Minos, Secret};

    /// A Minos that lives on loopback, in this process: two auth routes and a queue of `GET`
    /// answers. Real sockets, real `reqwest`, no network and no stub of our own client.
    struct Stub {
        base: String,
        refreshes: Arc<AtomicUsize>,
        gets: Arc<AtomicUsize>,
    }

    fn stub(get_answers: Vec<(u16, String)>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let base = format!(
            "http://127.0.0.1:{}/api/v1",
            listener.local_addr().unwrap().port()
        );
        let refreshes = Arc::new(AtomicUsize::new(0));
        let gets = Arc::new(AtomicUsize::new(0));
        let (thread_refreshes, thread_gets) = (Arc::clone(&refreshes), Arc::clone(&gets));
        let answers = Arc::new(Mutex::new(VecDeque::from(get_answers)));

        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buffer = [0u8; 8192];
                let read = stream.read(&mut buffer).unwrap_or(0);
                let request = String::from_utf8_lossy(&buffer[..read]).to_string();
                let line = request.lines().next().unwrap_or_default().to_string();

                let (status, payload) = if line.starts_with("POST /api/v1/auth/login") {
                    (200u16, auth_envelope("access-seed", "refresh-seed"))
                } else if line.starts_with("POST /api/v1/auth/refresh") {
                    let n = thread_refreshes.fetch_add(1, Ordering::SeqCst);
                    (
                        200,
                        auth_envelope(&format!("access-{n}"), &format!("refresh-{n}")),
                    )
                } else if line.starts_with("GET ") {
                    thread_gets.fetch_add(1, Ordering::SeqCst);
                    answers
                        .lock()
                        .unwrap()
                        .pop_front()
                        .unwrap_or((500, "{}".to_string()))
                } else {
                    (404, "{}".to_string())
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
            refreshes,
            gets,
        }
    }

    fn auth_envelope(access: &str, refresh: &str) -> String {
        format!(
            r#"{{"status_code":200,"message":"Successfull","data":{{"access_token":"{access}","token_type":"Bearer","expires_in":900,"refresh_token":"{refresh}"}}}}"#
        )
    }

    async fn signed_in(stub: &Stub) -> Http {
        let minos = Minos::new(
            &stub.base,
            Credential::Password {
                identifier: "wall".into(),
                password: Secret::new("not-a-real-password"),
            },
        );
        Http::sign_in(&minos).await.expect("the stub signs us in")
    }

    const ONE_HULL: &str = r#"{"status_code":200,"message":"Successfull","data":[{"id_unit":8,"name":"KRI Abdul Halim Perdanakusuma","hull_number":"KRI-355"}]}"#;
    const NOT_AUTHORISED: &str = r#"{"status_code":401,"message":"Unauthorized","data":null}"#;

    #[tokio::test]
    async fn a_401_is_refreshed_once_and_the_read_is_retried() {
        let stub = stub(vec![
            (401, NOT_AUTHORISED.to_string()),
            (200, ONE_HULL.to_string()),
        ]);
        let http = signed_in(&stub).await;
        let picture = http.positions().await.expect("the retry succeeds");
        assert_eq!(picture.len(), 1);
        assert_eq!(
            stub.gets.load(Ordering::SeqCst),
            2,
            "one rejected read, one retry"
        );
        assert_eq!(
            stub.refreshes.load(Ordering::SeqCst),
            1,
            "one refresh, not a loop"
        );
    }

    #[tokio::test]
    async fn a_second_401_is_an_error_and_not_a_second_retry() {
        let stub = stub(vec![
            (401, NOT_AUTHORISED.to_string()),
            (401, NOT_AUTHORISED.to_string()),
        ]);
        let http = signed_in(&stub).await;
        let result = http.positions().await;
        assert!(
            matches!(result, Err(Error::Unauthorized)),
            "a second 401 is the answer: {result:?}"
        );
        assert_eq!(stub.gets.load(Ordering::SeqCst), 2);
        assert_eq!(
            stub.refreshes.load(Ordering::SeqCst),
            1,
            "the second 401 must not trigger another refresh"
        );
    }

    #[tokio::test]
    async fn a_429_is_rate_limited_and_never_retried() {
        let stub = stub(vec![(
            429,
            r#"{"status_code":429,"message":"Too many requests","data":null}"#.to_string(),
        )]);
        let http = signed_in(&stub).await;
        let result = http.positions().await;
        assert!(
            matches!(result, Err(Error::RateLimited)),
            "429 must surface as itself, got {result:?}"
        );
        assert_eq!(
            stub.gets.load(Ordering::SeqCst),
            1,
            "backing off means not asking again straight away"
        );
    }

    #[tokio::test]
    async fn a_400_names_the_field_that_was_wrong() {
        // Branching is on the code and the field names; the prose is for a human.
        let body = r#"{"status_code":400,"message":"Bad request","data":{"errors":{"identifier":"This field is required."}}}"#;
        let stub = stub(vec![(400, body.to_string())]);
        let http = signed_in(&stub).await;
        let result = http.unit(1).await;
        match result {
            Err(Error::Status { status, message }) => {
                assert_eq!(status, 400);
                assert!(
                    message.contains("identifier: This field is required."),
                    "the field and its reason must survive, got {message:?}"
                );
            }
            other => panic!("expected a 400 with its field, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn a_failed_sign_in_says_so_and_carries_no_token() {
        let stub = stub(vec![]);
        let minos = Minos::new(
            &stub.base,
            Credential::Password {
                identifier: "wall".into(),
                password: Secret::new("not-a-real-password"),
            },
        );
        // The stub's /auth/login succeeds, so drive the failure through a client pointed at a port
        // that nothing listens on: a transport error, which must not print a credential either.
        let closed = Minos::new("http://127.0.0.1:9/api/v1", minos_credential());
        let error = match Http::sign_in(&closed).await {
            Ok(_) => panic!("nothing listens on port 9, so a sign-in cannot succeed"),
            Err(error) => error,
        };
        let printed = format!("{error} {error:?}");
        assert!(
            !printed.contains("not-a-real-password"),
            "a credential reached an error message: {printed}"
        );
        // And the working stub still signs in.
        Http::sign_in(&minos).await.expect("the stub signs in");
    }

    fn minos_credential() -> Credential {
        Credential::Password {
            identifier: "wall".into(),
            password: Secret::new("not-a-real-password"),
        }
    }
}

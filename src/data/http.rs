//! The HTTP calls themselves: the reads the wall performs, one retry rule.
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
    Envelope, ErrorResponse, FeedPosition, Installation, InstallationType, NewsQuery, NewsStory,
    NewsStoryDetail, Pages, Register, StoryPage, TacticalObject, TacticalObjectFacets,
    TacticalObjectManifest, TacticalObjectPlacement, TniCommandPost, TniCommander, TokenPair,
    UnitCommander, UnitDetail, UnitOrganisation, UnitOrganisationDetail, UnitSummary, Viewpoint,
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

    /// The current access token, refreshed if needed.
    ///
    /// Handed to the socket actor for the connect frame's `data`. It is a bearer credential: callers
    /// pass it straight into the handshake and never log it (`INV-08`).
    pub async fn access_token(&self) -> Result<String, Error> {
        self.auth.access_token().await
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

    /// `GET /units/{id}/specifications` — append-only versions; the current one has `is_current`.
    pub async fn specifications(
        &self,
        id_unit: i64,
    ) -> Result<Vec<crate::data::types::UnitSpecification>, Error> {
        self.get(&format!("/units/{id_unit}/specifications")).await
    }

    /// `GET /units/{id}/commander` — newest first; the current one is the entry with no `to_at`.
    /// An empty list is a valid answer: nobody has ever commanded this hull.
    /// `GET /units` — the fleet register, **paged to the end**: picture URLs for every hull.
    ///
    /// One read per page, not one read per hull: the wall must not ask about a harbour one ship at a time.
    /// But one *page* is not the register. On the dev deployment `GET /units` answers 25 of 127 hulls unless
    /// asked otherwise, and the wall believed the 25 (`#52`): 14 picture URLs where the fleet has 62. So this
    /// asks for the server's maximum page size and follows `has_next_page` until the server says the list
    /// has ended, or until [`PAGE_CAP`] pages — because a backend that always says "more" must not spin the
    /// reads thread forever.
    pub async fn units(&self) -> Result<Register, Error> {
        let pages = self.paged::<UnitSummary>("/units").await?;
        Ok(Register {
            units: pages.rows,
            pages: pages.pages,
            total_records: pages.total_records,
            truncated: pages.truncated,
        })
    }

    /// `GET /viewpoint` — where the map opens. **Authentication only**, and an unconfigured
    /// deployment answers `{}`: both coordinates absent, never `0,0`.
    pub async fn viewpoint(&self) -> Result<Viewpoint, Error> {
        self.get("/viewpoint").await
    }

    /// `GET /installation-types` — the facility vocabulary, in picker order. Needs `read` on
    /// `/system/installations`.
    pub async fn installation_types(&self) -> Result<Vec<InstallationType>, Error> {
        self.get("/installation-types").await
    }

    /// `GET /installations?active_only=true` — the facilities, operational only. Needs `read` on
    /// `/system/installations`.
    ///
    /// `active_only` is the wall's read: a decommissioned post is a real answer to "what used to be
    /// here", but it is not a thing the picture draws. The route also filters by `id_service_branch`
    /// and `id_installation_type`; nothing asks for those yet, so this client does not send them.
    pub async fn installations(&self) -> Result<Vec<Installation>, Error> {
        self.get("/installations?active_only=true").await
    }

    /// `GET /tactical-objects?active=true` — the symbol library, every page. Needs `read` on
    /// `/system/tactical-objects`.
    ///
    /// The library is the picker's catalogue, not the map; a placement embeds its own symbol. It is
    /// paged like the register, for the same reason: page one is not the catalogue.
    pub async fn tactical_objects(&self) -> Result<Pages<TacticalObject>, Error> {
        self.paged("/tactical-objects?active=true").await
    }

    /// `GET /tactical-objects/manifest` — the whole active library, **unpaginated on purpose**, so a
    /// client can cache it and draw a symbol with no network.
    pub async fn tactical_object_manifest(&self) -> Result<TacticalObjectManifest, Error> {
        self.get("/tactical-objects/manifest").await
    }

    /// `GET /tactical-objects/facets` — the filter rows and their active counts.
    pub async fn tactical_object_facets(&self) -> Result<TacticalObjectFacets, Error> {
        self.get("/tactical-objects/facets").await
    }

    /// `GET /tactical-object-placements` — the operational map's marks, every page. Needs `read` on
    /// `/system/tactical-object-placements`.
    pub async fn tactical_object_placements(
        &self,
    ) -> Result<Pages<TacticalObjectPlacement>, Error> {
        self.paged("/tactical-object-placements").await
    }

    /// `GET /unit-organisations` — the standing orders of battle. Needs `read` on
    /// `/system/unit-organisations`.
    pub async fn unit_organisations(&self) -> Result<Vec<UnitOrganisation>, Error> {
        self.get("/unit-organisations").await
    }

    /// `GET /unit-organisations/{id}` — one structure and its forest of roots.
    pub async fn unit_organisation(&self, id: i64) -> Result<UnitOrganisationDetail, Error> {
        self.get(&format!("/unit-organisations/{id}")).await
    }

    /// `GET /tni-commander` — the supreme command. Needs `read` on `/system/tni-command`.
    /// `commander` is absent when the post is vacant, which is a real state.
    pub async fn tni_commander(&self) -> Result<TniCommandPost, Error> {
        self.get("/tni-commander").await
    }

    /// `GET /tni-commander/history` — every appointment, newest first.
    pub async fn tni_commander_history(&self) -> Result<Vec<TniCommander>, Error> {
        self.get("/tni-commander/history").await
    }

    /// `GET /banglinstra/stories` — **one page** of the newsroom, with the five parameters the wall
    /// asks with. Needs `read` on `/system/banglinstra`.
    ///
    /// The API's word for the route is `banglinstra`; this crate calls the thing the newsroom.
    ///
    /// Deliberately **not** `paged()`: that walks every page at [`PAGE_SIZE`] and answers "the whole
    /// catalogue", which is the register's question. The newsroom asks the operator's — page `n` of
    /// [`NEWS_PAGE_SIZE`] stories, ordered `first_seen` descending by the server — so a client that
    /// walked the pages would make the operator's page buttons meaningless and page 2 unreachable.
    pub async fn stories(&self, query: NewsQuery) -> Result<StoryPage, Error> {
        let path = format!(
            "/banglinstra/stories?scope={}&window={}&min_confidence={MIN_CONFIDENCE}&page_size={NEWS_PAGE_SIZE}&page_number={}",
            query.scope.as_str(),
            query.window.as_str(),
            query.page,
        );
        let envelope: Envelope<Vec<NewsStory>> = self.get_envelope(&path).await?;
        // `metadata` is required on this route: without it the panel cannot offer Next, and an
        // empty block would be a guess about a page the server never described.
        let pagination = envelope.metadata.ok_or_else(|| {
            Error::Decode(
                "the story list carried no metadata: a page that cannot say which page it is is not a page"
                    .to_string(),
            )
        })?;
        Ok(StoryPage {
            rows: envelope.data,
            pagination,
        })
    }

    /// `GET /banglinstra/stories/{id}` — the story and the documents that carried it, oldest first.
    /// Needs `read` on `/system/banglinstra`.
    pub async fn story(&self, id: i64) -> Result<NewsStoryDetail, Error> {
        self.get(&format!("/banglinstra/stories/{id}")).await
    }

    /// Every page of a paginated list, at the server's maximum page size.
    ///
    /// `path` may already carry a filter (`?active=true`); the page parameters are appended to it.
    /// The cap and the `truncated` flag are the register's rule (`#52`), shared so a second paged
    /// read cannot quietly grow a different one.
    async fn paged<T: DeserializeOwned>(&self, path: &str) -> Result<Pages<T>, Error> {
        let mut read = Pages::default();
        loop {
            let page = read.pages + 1;
            let separator = if path.contains('?') { '&' } else { '?' };
            let url = format!("{path}{separator}page_size={PAGE_SIZE}&page_number={page}");
            let envelope: Envelope<Vec<T>> = self.get_envelope(&url).await?;
            read.pages = page;
            read.total_records = envelope
                .metadata
                .as_ref()
                .map(|metadata| metadata.total_records);
            let more = envelope
                .metadata
                .is_some_and(|metadata| metadata.has_next_page);
            read.rows.extend(envelope.data);
            if !more {
                return Ok(read);
            }
            if page >= PAGE_CAP {
                read.truncated = true;
                tracing::warn!(
                    endpoint = path,
                    pages = read.pages,
                    rows = read.rows.len(),
                    total = read.total_records,
                    "a paginated read is longer than the page cap: the wall is drawing part of it"
                );
                return Ok(read);
            }
        }
    }

    /// The bytes at a **presigned** URL: a hull's photograph.
    ///
    /// No bearer token: the URL carries its own authorisation, and sending ours would be a credential in
    /// a request to a storage host we do not control. The whole body is read here because the cache wants
    /// bytes and the decode belongs off this thread anyway.
    pub async fn image(&self, url: &str) -> Result<Vec<u8>, Error> {
        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(transport)?
            .error_for_status()
            .map_err(|error| Error::Transport(format!("the picture: {error}")))?;
        Ok(response.bytes().await.map_err(transport)?.to_vec())
    }

    pub async fn commander(&self, id_unit: i64) -> Result<Vec<UnitCommander>, Error> {
        self.get(&format!("/units/{id_unit}/commander")).await
    }

    /// `GET path`, with one refresh-and-retry on a `401`.
    async fn get<T: DeserializeOwned>(&self, path: &str) -> Result<T, Error> {
        Ok(self.get_envelope(path).await?.data)
    }

    /// The whole envelope, for the routes whose `metadata` is part of the answer.
    async fn get_envelope<T: DeserializeOwned>(&self, path: &str) -> Result<Envelope<T>, Error> {
        let token = self.auth.access_token().await?;
        match self.get_with(path, &token).await? {
            Attempt::Ok(envelope) => Ok(envelope),
            Attempt::Unauthorized => {
                // One refresh, one retry. A second 401 is the answer, not a reason to try again.
                let fresh = self.auth.replace_rejected(&token).await?;
                match self.get_with(path, &fresh).await? {
                    Attempt::Ok(envelope) => Ok(envelope),
                    Attempt::Unauthorized => Err(Error::Unauthorized),
                }
            }
        }
    }

    async fn get_with<T: DeserializeOwned>(
        &self,
        path: &str,
        token: &str,
    ) -> Result<Attempt<Envelope<T>>, Error> {
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
        Ok(Attempt::Ok(envelope))
    }
}

enum Attempt<T> {
    Ok(T),
    Unauthorized,
}

/// The page size the register is read at: the largest the API documents, so a fleet of a few hundred hulls
/// is one or two reads rather than a dozen.
const PAGE_SIZE: u32 = 200;

/// The wall's confidence floor, sent as `min_confidence`. It drops stories whose geography is
/// weaker than this — and drops `unknown`-scope stories with them, because a story with no country
/// carries no confidence to compare. That is the intent: an operator asking for certainty is
/// asking not to be shown a story that was never located.
pub const MIN_CONFIDENCE: f64 = 0.5;

/// The wall's page, sent as `page_size`: what one screen of the newsroom holds, and the page the
/// operator pages through. Not [`PAGE_SIZE`], which is the register's *read* size rather than a
/// page anybody sees.
pub const NEWS_PAGE_SIZE: u32 = 25;

/// The most pages one register read will follow. Sixteen pages of [`PAGE_SIZE`] is 3 200 hulls; beyond that
/// the wall is being asked to hold a fleet it cannot draw, and a read that stops is better than one that
/// never ends.
const PAGE_CAP: u32 = 16;

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
    use crate::data::types::{NewsScope, NewsWindow};

    /// A Minos that lives on loopback, in this process: two auth routes and a queue of `GET`
    /// answers. Real sockets, real `reqwest`, no network and no stub of our own client.
    struct Stub {
        base: String,
        refreshes: Arc<AtomicUsize>,
        gets: Arc<AtomicUsize>,
        /// The request line of every `GET`, so a test can assert the route and its filters.
        paths: Arc<Mutex<Vec<String>>>,
    }

    fn stub(get_answers: Vec<(u16, String)>) -> Stub {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind loopback");
        let base = format!(
            "http://127.0.0.1:{}/api/v1",
            listener.local_addr().unwrap().port()
        );
        let refreshes = Arc::new(AtomicUsize::new(0));
        let gets = Arc::new(AtomicUsize::new(0));
        let paths = Arc::new(Mutex::new(Vec::new()));
        let (thread_refreshes, thread_gets) = (Arc::clone(&refreshes), Arc::clone(&gets));
        let thread_paths = Arc::clone(&paths);
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
                    thread_paths.lock().unwrap().push(line.clone());
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
            paths,
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

    fn fixture(relative: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("cannot read {}: {error}", path.display()))
    }

    /// **The register is every hull, not the first page.**
    ///
    /// `#52`: `GET /units` answers 25 of the dev fleet's 127 unless asked otherwise, and the wall asked once
    /// and believed it — 14 picture URLs where the fleet has 62. The three fixtures are recorded pages
    /// (`fixtures/rest/units-page-*.json`, trimmed to four rows each with the metadata as recorded), and the
    /// stub serves them in order.
    #[tokio::test]
    async fn the_register_is_read_to_its_last_page() {
        let stub = stub(vec![
            (200, fixture("fixtures/rest/units-page-1.json")),
            (200, fixture("fixtures/rest/units-page-2.json")),
            (200, fixture("fixtures/rest/units-page-3.json")),
        ]);
        let http = signed_in(&stub).await;
        let register = http.units().await.expect("the register reads");
        assert_eq!(register.pages, 3, "three recorded pages, three reads");
        assert_eq!(
            register.units.len(),
            12,
            "every row of every page, or the wall is drawing part of a fleet"
        );
        assert_eq!(
            register.total_records,
            Some(127),
            "the server's own count, so an operator can compare it with what the wall holds"
        );
        assert!(!register.truncated, "the last page says there is no more");
        assert_eq!(
            stub.gets.load(Ordering::SeqCst),
            3,
            "one read per page and no speculative fourth"
        );
        // The hulls are the recorded ones, in order: page 3's rows must be there, not just page 1's.
        let ids: Vec<i64> = register.units.iter().map(|unit| unit.id).collect();
        assert_eq!(ids.first(), Some(&262), "the first recorded hull");
        assert_eq!(
            ids.last(),
            Some(&88),
            "the last recorded hull, from the last page"
        );
    }

    /// A register that fits in one page stops after one read — no second request, no guessing.
    #[tokio::test]
    async fn a_register_that_ends_on_its_first_page_is_one_read() {
        let single = r#"{"status_code":200,"message":"Successfull","data":[{"id":8,"name":"KRI Abdul Halim Perdanakusuma","hull_number":"355"}],"metadata":{"total_records":1,"total_pages":1,"page_size":200,"current_page":1,"has_next_page":false,"has_previous_page":false}}"#;
        let stub = stub(vec![(200, single.to_string())]);
        let http = signed_in(&stub).await;
        let register = http.units().await.expect("the register reads");
        assert_eq!(register.pages, 1);
        assert_eq!(register.units.len(), 1);
        assert!(
            register.total_records.is_some(),
            "one page still reports its count"
        );
        assert_eq!(stub.gets.load(Ordering::SeqCst), 1);
    }

    /// **A backend that always says "more" is cut at the cap, and says so.**
    ///
    /// The cap exists so one broken server cannot spin the reads thread forever; `truncated` exists so the
    /// result is not a silent lie about the fleet (#52).
    #[tokio::test]
    async fn a_register_that_never_ends_is_cut_at_the_cap_and_says_so() {
        let endless = r#"{"status_code":200,"message":"Successfull","data":[{"id":1,"name":"KRI Cakra"}],"metadata":{"total_records":999999,"total_pages":999999,"page_size":1,"current_page":1,"has_next_page":true,"has_previous_page":false}}"#;
        let answers: Vec<(u16, String)> = (0..PAGE_CAP as usize + 5)
            .map(|_| (200u16, endless.to_string()))
            .collect();
        let stub = stub(answers);
        let http = signed_in(&stub).await;
        let register = http.units().await.expect("the register reads");
        assert_eq!(
            register.pages, PAGE_CAP,
            "the cap is the loop's guard, not a suggestion"
        );
        assert!(register.truncated, "a capped read must say it was capped");
        assert_eq!(
            stub.gets.load(Ordering::SeqCst) as u32,
            PAGE_CAP,
            "no request past the cap"
        );
    }

    /// **The operational resources are read from their own routes, with the filters the wall asks
    /// for** (#107). Each answer is the recorded fixture, and the request lines are asserted so a
    /// client that dropped `active_only=true` or the page parameters is caught here rather than by a
    /// server that quietly answered a different question.
    #[tokio::test]
    async fn the_operational_resources_read_from_their_own_routes() {
        let stub = stub(vec![
            (200, fixture("fixtures/rest/viewpoint.json")),
            (200, fixture("fixtures/rest/installation-types.json")),
            (200, fixture("fixtures/rest/installations.json")),
            (200, fixture("fixtures/rest/tactical-objects.json")),
            (200, fixture("fixtures/rest/tactical-objects-manifest.json")),
            (200, fixture("fixtures/rest/tactical-objects-facets.json")),
            (
                200,
                fixture("fixtures/rest/tactical-object-placements.json"),
            ),
            (200, fixture("fixtures/rest/unit-organisations.json")),
            (200, fixture("fixtures/rest/unit-organisation-1.json")),
            (200, fixture("fixtures/rest/tni-commander.json")),
            (200, fixture("fixtures/rest/tni-commander-history.json")),
        ]);
        let http = signed_in(&stub).await;

        assert_eq!(
            http.viewpoint()
                .await
                .expect("the viewpoint reads")
                .coordinates(),
            None,
            "an unset viewpoint is absent, not 0,0"
        );
        assert_eq!(
            http.installation_types().await.expect("types read").len(),
            5
        );
        assert!(
            http.installations()
                .await
                .expect("facilities read")
                .is_empty()
        );
        let library = http.tactical_objects().await.expect("the library reads");
        assert_eq!(library.rows.len(), 56);
        assert_eq!(library.pages, 1, "the recorded page says there is no more");
        assert_eq!(library.total_records, Some(56));
        assert_eq!(
            http.tactical_object_manifest()
                .await
                .expect("the manifest reads")
                .symbols
                .len(),
            56
        );
        assert_eq!(
            http.tactical_object_facets()
                .await
                .expect("the facets read")
                .total,
            56
        );
        assert_eq!(
            http.tactical_object_placements()
                .await
                .expect("the placements read")
                .rows
                .len(),
            3
        );
        assert_eq!(
            http.unit_organisations()
                .await
                .expect("the structures read")
                .len(),
            4
        );
        assert_eq!(
            http.unit_organisation(1)
                .await
                .expect("one structure reads")
                .organisation
                .id,
            1
        );
        assert!(
            http.tni_commander()
                .await
                .expect("the supreme command reads")
                .commander
                .is_none(),
            "the recorded post is vacant"
        );
        assert!(
            http.tni_commander_history()
                .await
                .expect("the history reads")
                .is_empty()
        );

        let paths = stub.paths.lock().unwrap().clone();
        for wanted in [
            "GET /api/v1/installations?active_only=true ",
            "GET /api/v1/tactical-objects?active=true&page_size=200&page_number=1 ",
            "GET /api/v1/tactical-object-placements?page_size=200&page_number=1 ",
        ] {
            assert!(
                paths.iter().any(|path| path.contains(wanted)),
                "the client did not ask {wanted:?}; it asked {paths:?}"
            );
        }
    }

    /// **The newsroom asks for ONE page, with the wall's five parameters, and never walks the
    /// pages** (#135). `paged()` would ask `page_size=200` and follow `has_next_page`, which makes the
    /// operator's page buttons meaningless; the falsifiable claim is the request line itself.
    #[tokio::test]
    async fn the_newsroom_asks_for_one_page_with_the_walls_parameters() {
        let stub = stub(vec![
            (200, fixture("fixtures/rest/newsroom-stories-page-1.json")),
            (200, fixture("fixtures/rest/newsroom-story-1.json")),
        ]);
        let http = signed_in(&stub).await;

        let page = http
            .stories(NewsQuery::latest(NewsScope::International))
            .await
            .expect("the page reads");
        assert_eq!(
            page.rows.len(),
            3,
            "the rows of the page that was asked for"
        );
        assert_eq!(page.pagination.current_page, 1);
        assert_eq!(page.pagination.total_records, 128);
        assert!(page.pagination.has_next_page);

        let detail = http.story(412).await.expect("the detail reads");
        assert_eq!(detail.story.id, 412);
        assert_eq!(detail.articles.len() as u32, detail.story.source_count);

        let paths = stub.paths.lock().unwrap().clone();
        assert_eq!(
            paths.len(),
            2,
            "one request per read: a paged() walk would have asked for more, got {paths:?}"
        );
        assert!(
            paths[0].starts_with(
                "GET /api/v1/banglinstra/stories?scope=international&window=latest&min_confidence=0.5&page_size=25&page_number=1 "
            ),
            "the list is one page, at the wall's confidence and page size, got {:?}",
            paths[0]
        );
        assert!(
            paths[1].starts_with("GET /api/v1/banglinstra/stories/412 "),
            "the detail is the story's own route, got {:?}",
            paths[1]
        );
    }

    /// The operator's page is a parameter, not a walk: page 2 asks for `page_number=2` once.
    #[tokio::test]
    async fn the_operators_page_is_the_page_that_is_asked_for() {
        let stub = stub(vec![
            (200, fixture("fixtures/rest/newsroom-stories-page-1.json")),
            (200, fixture("fixtures/rest/newsroom-stories-page-1.json")),
        ]);
        let http = signed_in(&stub).await;

        let query = NewsQuery {
            scope: NewsScope::International,
            window: NewsWindow::Latest,
            page: 2,
        };
        let page = http.stories(query).await.expect("the page reads");
        assert_eq!(
            page.pagination.current_page, 1,
            "the fixture is page 1 of 6; the client reports what the server said rather than relabelling it"
        );

        let paths = stub.paths.lock().unwrap().clone();
        assert_eq!(paths.len(), 1, "one page, one request: {paths:?}");
        assert!(
            paths[0].starts_with(
                "GET /api/v1/banglinstra/stories?scope=international&window=latest&min_confidence=0.5&page_size=25&page_number=2 "
            ),
            "page 2 is `page_number=2`, not a second walk at the read size, got {:?}",
            paths[0]
        );
    }

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

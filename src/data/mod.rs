//! The Minos REST client: the standing picture, the fleet register, and the operational resources.
//!
//! **Nothing here runs on the UI thread.** [`thread::Backend`] owns a current-thread Tokio runtime
//! and this module's client on a thread of its own; the view loop sends a [`thread::Command`] and
//! drains [`thread::Reply`]s between frames. A blocking `GET` in `view` is how a wall display freezes
//! and stays frozen.
//!
//! | need | call |
//! | --- | --- |
//! | the initial picture | `GET /live-feed/positions` — not paginated, includes hulls with no `position` |
//! | one hull | `GET /live-feed/positions/{id}` |
//! | identity | `GET /units/{id}` |
//! | who commands it | `GET /units/{id}/commander` — newest first; the current one has no `to_at` |
//! | where the map opens | `GET /viewpoint` — authentication only; `{}` when unset |
//! | the facility vocabulary | `GET /installation-types` — `symbol_code` is a code, not an id |
//! | the facilities | `GET /installations?active_only=true` |
//! | the symbol library | `GET /tactical-objects?active=true`, paged to the end |
//! | the offline cache | `GET /tactical-objects/manifest` — unpaginated on purpose |
//! | the picker's counts | `GET /tactical-objects/facets` |
//! | the operational marks | `GET /tactical-object-placements`, paged to the end |
//! | the standing orders of battle | `GET /unit-organisations`, `GET /unit-organisations/{id}` |
//! | the supreme command | `GET /tni-commander`, `GET /tni-commander/history` |
//! | sign in, refresh | `POST /auth/login` (`return_refresh_token: true`), `POST /auth/refresh` |
//!
//! Permissions, from the pinned spec and **confirmed against the dev server on 2026-11-10**: the
//! picture needs the `read` action on `/system/live-feed`, and `/units/…` (identity, commander) needs
//! `read` on `/system/units`. The operational resources each have their own subject —
//! `/system/installations`, `/system/tactical-objects`, `/system/tactical-object-placements`,
//! `/system/unit-organisations` and `/system/tni-command` — except `/viewpoint`, which is
//! authentication only. A read-only wall account therefore holds those `read` actions and nothing
//! else: it never posts a fix, an order, a placement or a message.
//!
//! Facts the code must not re-derive per call site, because they are the traps:
//!
//! - An absent or `null` measurement is **unmeasured**, never zero (`INV-01`). `Option<f64>`, never a
//!   default.
//! - `backfilled: true` is real, current-as-of-whenever-it-arrived, and **did not just happen**
//!   (`INV-03`).
//! - `live-feed` has no history: after a reconnect, re-read before trusting anything (`INV-09`).
//! - Unknown JSON fields must parse: the server adds fields without ceremony.
//! - Age comes from the server (`age_seconds`), never from this machine's clock against a timestamp.
//! - The library is **not** the map: a placement embeds its symbol, so drawing needs no second read.
//! - `unit-organisations/{id}.nodes` is a **forest**; more than one root is normal.
//! - A vacant supreme command is a real state, not an error: `commander` is absent.
//!
//! `401`/`403` are failures to surface, not to retry; `429` is real and backs off. A second `401`
//! after a successful refresh is [`Error::Unauthorized`] — the account is disabled, or the token is
//! for another server, and retrying it all night burns a rate limit for nothing.

pub mod auth;
pub mod http;
pub mod thread;
pub mod types;
pub mod wire;

pub use http::Http;
pub use thread::{Backend, Command, Reply};

/// Everything that can go wrong between here and the wire.
///
/// **No variant carries a token, a header or a password** (`INV-08`): the transport variant carries
/// the URL from `reqwest`'s own message, which never includes request headers.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// No credential is configured, or it was consumed by a failed refresh.
    #[error("no credential is configured")]
    NoCredential,
    /// The request never completed.
    #[error("could not reach Minos: {0}")]
    Transport(String),
    /// `401` — and a second `401` after a refresh is exactly this, not a reason to retry.
    #[error("not authorised (401): the credential was refused")]
    Unauthorized,
    /// `403` — authenticated, but not allowed, or the account is not `Active`.
    #[error("forbidden (403): {message}")]
    Forbidden { message: String },
    /// `429` — the server is asking for less traffic. Back off.
    #[error("rate limited (429)")]
    RateLimited,
    /// Any other failure status, with `data.errors` flattened into `field: reason` when present.
    #[error("HTTP {status}: {message}")]
    Status { status: u16, message: String },
    /// The body was not the envelope the spec describes.
    #[error("could not decode the response: {0}")]
    Decode(String),
}

impl Error {
    /// Whether **waiting** could fix this. A wall starts at boot, before the network is up, and a wall on a
    /// ship loses its route: those are transport and overload failures, and they must be retried rather
    /// than reported once and abandoned (#94). A refused credential is the opposite — no amount of waiting
    /// signs in an account that may not read this feed — and a decode failure is a contract change, which a
    /// human has to look at.
    pub fn retryable(&self) -> bool {
        match self {
            Error::Transport(_) | Error::RateLimited => true,
            Error::Status { status, .. } => *status >= 500,
            Error::NoCredential
            | Error::Unauthorized
            | Error::Forbidden { .. }
            | Error::Decode(_) => false,
        }
    }
}

#[cfg(test)]
mod retry_tests {
    use super::Error;

    /// #94: a wall that starts before its network is up has to wait, not die. A refused credential is the
    /// one thing waiting never fixes, and a decode failure is a contract change for a human.
    #[test]
    fn only_what_waiting_can_fix_is_retryable() {
        for transient in [
            Error::Transport("connect refused".into()),
            Error::RateLimited,
            Error::Status {
                status: 503,
                message: "unavailable".into(),
            },
        ] {
            assert!(transient.retryable(), "{transient} is worth retrying");
        }
        for permanent in [
            Error::NoCredential,
            Error::Unauthorized,
            Error::Forbidden {
                message: "no read on /system/live-feed".into(),
            },
            Error::Status {
                status: 404,
                message: "not found".into(),
            },
            Error::Decode("no such field".into()),
        ] {
            assert!(!permanent.retryable(), "{permanent} needs a human");
        }
    }
}

/// What `--self-check` learned from the live server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveCheck {
    /// Hulls in the standing picture — including the ones that have never reported.
    pub vessels: usize,
    /// Hulls with no fix. On the dev feed this is almost all of them.
    pub silent: usize,
    /// How long the access token still has. `None` if the server did not state a lifetime.
    pub token_remaining: Option<std::time::Duration>,
}

/// One operational resource, as `--self-check` reports it: the name of the row, and either what the
/// read returned or why it was refused.
///
/// **A refusal is not a crash.** Every row is attempted even when an earlier one failed, so an
/// account missing one `read` names the resource it cannot see instead of hiding the rest behind it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResourceCheck {
    pub name: &'static str,
    pub detail: Result<String, Error>,
}

/// One resource row: run the read, describe the value on success, keep the error on failure.
async fn resource<T, F>(
    name: &'static str,
    read: impl std::future::Future<Output = Result<T, Error>>,
    describe: F,
) -> ResourceCheck
where
    F: FnOnce(T) -> String,
{
    ResourceCheck {
        name,
        detail: read.await.map(describe),
    }
}

/// How many posts a forest holds, roots included.
fn count_nodes(nodes: &[types::UnitOrganisationNode]) -> usize {
    nodes
        .iter()
        .map(|node| 1 + count_nodes(&node.children))
        .sum()
}

/// A signed-in session, kept open for the CLI paths that ask two questions of one wall.
///
/// `start` is the login check and `read` is the read check, deliberately separate: "the credential is
/// refused" (exit 3) and "the credential works and the read fails" (exit 4) are different problems
/// with different fixes, and one function that does both cannot tell them apart.
///
/// This is the CLI path, not the display: it is allowed to block, because nothing is drawing yet.
pub struct Session {
    http: http::Http,
}

impl Session {
    /// Sign in, or exchange the refresh token once. **This is the only call that proves a
    /// credential**, so a failure here is exit 3 and nothing else.
    pub fn start(minos: &crate::config::Minos) -> Result<Session, Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| Error::Transport(format!("could not start a runtime: {error}")))?;
        let http = runtime.block_on(async {
            match minos.refresh_token() {
                // A refresh token is exchanged once at startup rather than held unproven: without
                // this, "signed in" would mean "the file has a string in it".
                Some(token) => {
                    let http = http::Http::with_refresh_token(minos, token.to_string()).await?;
                    http.access_token().await?;
                    Ok::<_, Error>(http)
                }
                None => http::Http::sign_in(minos).await,
            }
        })?;
        Ok(Session { http })
    }

    /// How long the access token still has. `None` when the server stated no lifetime.
    pub fn token_remaining(&self) -> Option<std::time::Duration> {
        self.http.token_remaining()
    }

    /// The **fleet register** as the server has it: how many hulls, and how many of them have a picture.
    ///
    /// `GET /units` needs `read` on a different subject (`/system/units`) than the live feed, so a wall
    /// account can pass every other check and still be unable to draw a single photograph. That is the
    /// case this line exists to name (`#53`), and it is why the count is printed rather than a boolean.
    pub fn register(&mut self) -> Result<(usize, usize), Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| Error::Transport(format!("could not start a runtime: {error}")))?;
        runtime.block_on(async {
            let register = self.http.units().await?;
            let with_picture = register
                .units
                .iter()
                .filter(|unit| unit.image_url.is_some())
                .count();
            Ok::<_, Error>((register.units.len(), with_picture))
        })
    }

    /// The picture as the server has it. A read, not a shape check: a `200` with an empty list would
    /// prove the credential works and nothing else.
    pub fn read(&mut self) -> Result<LiveCheck, Error> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|error| Error::Transport(format!("could not start a runtime: {error}")))?;
        let token_remaining = runtime.block_on(async {
            let picture = self.http.positions().await?;
            let silent = picture
                .iter()
                .filter(|hull| hull.position.is_none())
                .count();
            Ok::<_, Error>(LiveCheck {
                vessels: picture.len(),
                silent,
                token_remaining: self.http.token_remaining(),
            })
        })?;
        let _ = token_remaining.token_remaining;
        Ok(token_remaining)
    }

    /// **One row per operational resource** (`#107`), each naming what it read.
    ///
    /// Every route needs `read` on its own `/system/...` subject, and an account can hold some and not
    /// others. The rows are attempted independently for that reason: "the map is empty" is not a
    /// diagnosis, and a wall operator needs the name of the read that was refused.
    ///
    /// The structure detail needs an id, so it reads the list first and asks about the first
    /// structure the server named. An empty list is a valid answer — a deployment with no structures —
    /// and is reported as such rather than as a failure.
    pub fn resources(&mut self) -> Vec<ResourceCheck> {
        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => runtime,
            Err(error) => {
                return vec![ResourceCheck {
                    name: "resources",
                    detail: Err(Error::Transport(format!(
                        "could not start a runtime: {error}"
                    ))),
                }];
            }
        };
        runtime.block_on(async {
            let mut rows = Vec::new();

            rows.push(
                resource(
                    "viewpoint",
                    self.http.viewpoint(),
                    |viewpoint| match viewpoint.coordinates() {
                        Some((latitude, longitude)) => {
                            format!("view at {latitude:.4}, {longitude:.4}")
                        }
                        None => "the backend has not chosen a view (unset, not 0,0)".to_string(),
                    },
                )
                .await,
            );
            rows.push(
                resource("inst types", self.http.installation_types(), |kinds| {
                    format!("{} kinds", kinds.len())
                })
                .await,
            );
            rows.push(
                resource("facilities", self.http.installations(), |places| {
                    format!("{} active facilities", places.len())
                })
                .await,
            );
            rows.push(
                resource("symbols", self.http.tactical_objects(), |library| {
                    format!(
                        "{} symbols in {} page(s){}",
                        library.rows.len(),
                        library.pages,
                        if library.truncated { ", truncated" } else { "" }
                    )
                })
                .await,
            );
            rows.push(
                resource(
                    "manifest",
                    self.http.tactical_object_manifest(),
                    |manifest| {
                        format!(
                            "{} symbols, version {}",
                            manifest.symbols.len(),
                            manifest.version
                        )
                    },
                )
                .await,
            );
            rows.push(
                resource("facets", self.http.tactical_object_facets(), |facets| {
                    format!(
                        "{} active symbols, {} domains, {} categories",
                        facets.total,
                        facets.domains.len(),
                        facets.categories.len()
                    )
                })
                .await,
            );
            rows.push(
                resource(
                    "placements",
                    self.http.tactical_object_placements(),
                    |marks| {
                        format!(
                            "{} marks in {} page(s){}",
                            marks.rows.len(),
                            marks.pages,
                            if marks.truncated { ", truncated" } else { "" }
                        )
                    },
                )
                .await,
            );

            let organisations = self.http.unit_organisations().await;
            let detail = match &organisations {
                Ok(list) => match list.first() {
                    Some(first) => {
                        resource("org", self.http.unit_organisation(first.id), |detail| {
                            format!(
                                "{} (id {}): {} nodes, {} root(s)",
                                detail.organisation.name,
                                detail.organisation.id,
                                count_nodes(&detail.nodes),
                                detail.nodes.len()
                            )
                        })
                        .await
                    }
                    None => ResourceCheck {
                        name: "org",
                        detail: Ok("no structure to read".to_string()),
                    },
                },
                Err(error) => ResourceCheck {
                    name: "org",
                    detail: Err(error.clone()),
                },
            };
            rows.push(
                resource("orgs", async { organisations }, |list| {
                    format!("{} structures", list.len())
                })
                .await,
            );
            rows.push(detail);

            rows.push(
                resource(
                    "tni command",
                    self.http.tni_commander(),
                    |post| match post.commander {
                        Some(commander) => format!(
                            "{}, rank {}, held by {}",
                            post.position_title, post.echelon_rank, commander.name
                        ),
                        None => format!(
                            "{}, rank {}, vacant",
                            post.position_title, post.echelon_rank
                        ),
                    },
                )
                .await,
            );
            rows.push(
                resource(
                    "tni history",
                    self.http.tni_commander_history(),
                    |appointments| format!("{} appointments", appointments.len()),
                )
                .await,
            );

            rows
        })
    }
}

#[cfg(test)]
mod tests {
    use super::types::*;
    use std::path::PathBuf;

    fn fixture(name: &str) -> String {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures/rest")
            .join(name);
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
    }

    fn picture(name: &str) -> Vec<FeedPosition> {
        let envelope: Envelope<Vec<FeedPosition>> =
            serde_json::from_str(&fixture(name)).expect("the recorded picture must decode");
        envelope.data
    }

    #[test]
    fn an_absent_speed_is_none_and_never_zero() {
        // INV-01: the measurement is missing, not zero. A `Some(0.0)` here would draw a stopped hull
        // with all the confidence of a measured one.
        let hulls = picture("positions-speed-absent.json");
        let fix = hulls[0].position.as_ref().expect("this fixture has a fix");
        assert_eq!(fix.speed_kn, None, "absent speed_kn must decode as None");
        assert_eq!(
            fix.course_deg,
            Some(255.0),
            "course_deg is still there and must survive: absence is per field"
        );
        assert_ne!(
            fix.speed_kn,
            Some(0.0),
            "an absent measurement is not a measured zero"
        );
    }

    #[test]
    fn a_null_speed_is_none_and_never_zero() {
        // The API encodes the same thing both ways; both must mean "unmeasured".
        let hulls = picture("positions-speed-null.json");
        let fix = hulls[0].position.as_ref().expect("this fixture has a fix");
        assert_eq!(fix.speed_kn, None, "null speed_kn must decode as None");
        assert_ne!(fix.speed_kn, Some(0.0));
    }

    #[test]
    fn a_measured_zero_survives_as_zero() {
        // The other half of INV-01: `0.0` on the wire is a measured stop and must not become "unknown".
        let body =
            fixture("positions-speed-null.json").replace("\"speed_kn\": null", "\"speed_kn\": 0.0");
        let envelope: Envelope<Vec<FeedPosition>> =
            serde_json::from_str(&body).expect("valid after the edit");
        let fix = envelope.data[0].position.as_ref().expect("a fix");
        assert_eq!(fix.speed_kn, Some(0.0), "a measured zero is a measurement");
    }

    #[test]
    fn hulls_that_never_reported_are_in_the_picture_without_a_fix() {
        // INV-02 starts here: 126 of the 127 hulls on the dev feed have never reported. They are in
        // the list and they have no position; neither fact may be inferred from the other.
        let hulls = picture("positions.json");
        assert!(
            hulls.len() > 100,
            "the recorded picture should be the whole fleet, got {}",
            hulls.len()
        );
        let silent = hulls.iter().filter(|hull| hull.position.is_none()).count();
        assert_eq!(
            silent + 1,
            hulls.len(),
            "the recording has exactly one reported hull; if that changed, re-read the fixture before trusting this test"
        );
        let reported = hulls
            .iter()
            .find(|hull| hull.position.is_some())
            .expect("one hull has reported");
        let fix = reported.position.as_ref().unwrap();
        assert!(!fix.backfilled, "the recorded live fix is not a backfill");
        assert!(
            fix.age_seconds > 0.0,
            "the server states the age; it does not send zero for an old fix"
        );
    }

    #[test]
    fn a_hull_number_may_be_missing_or_null() {
        let body = r#"{"status_code":200,"message":"Successfull","data":[{"id_unit":1,"name":"KRI Cakra"},{"id_unit":2,"name":"KRI Nagapasa","hull_number":null},{"id_unit":3,"name":"KRI Bima Suci","hull_number":""}]}"#;
        let envelope: Envelope<Vec<FeedPosition>> = serde_json::from_str(body).expect("decodes");
        assert_eq!(envelope.data[0].hull_number, None, "absent is None");
        assert_eq!(envelope.data[1].hull_number, None, "null is None");
        assert_eq!(
            envelope.data[2].hull_number.as_deref(),
            Some(""),
            "an empty hull number is still a value the server sent; we do not invent one and we do not delete one"
        );
    }

    #[test]
    fn unknown_fields_do_not_break_parsing() {
        // docs/verification.md rule 8. The recorded login carries `must_change_password`, which the
        // pinned TokenPair does not have: the server adds fields without ceremony.
        let pair: Envelope<TokenPair> = serde_json::from_str(&fixture("auth-login.json"))
            .expect("unknown fields must be tolerated");
        assert_eq!(pair.data.expires_in(), 900);
        assert!(
            pair.data.access_token().starts_with("REDACTED"),
            "the fixture's tokens are redacted on purpose"
        );
    }

    #[test]
    fn a_unit_and_its_specification_decode() {
        let unit: Envelope<UnitDetail> =
            serde_json::from_str(&fixture("unit.json")).expect("unit decodes");
        assert!(unit.data.id > 0);
        assert!(
            !unit.data.unit_class.category.name.is_empty(),
            "the class carries its category"
        );

        let spec: Envelope<Vec<UnitSpecification>> =
            serde_json::from_str(&fixture("specifications.json")).expect("specification decodes");
        assert!(
            spec.data.iter().any(|version| version.is_current),
            "one version is current"
        );
    }

    #[test]
    fn a_commander_list_may_be_empty() {
        // "Nobody has ever commanded this hull" is an empty list, not a 404.
        let envelope: Envelope<Vec<UnitCommander>> =
            serde_json::from_str(&fixture("commander-empty.json")).expect("empty list decodes");
        assert!(envelope.data.is_empty());
        assert_eq!(envelope.status_code, 200);
    }

    #[test]
    fn the_current_commander_is_the_entry_with_no_to_at() {
        let body = r#"{"status_code":200,"message":"Successfull","data":[
            {"id_unit":1,"id_user":9,"user_name":"A","from_at":"2026-11-01T00:00:00Z","to_at":null,"assigned_by":1,"note":""},
            {"id_unit":1,"id_user":7,"user_name":"B","from_at":"2026-10-01T00:00:00Z","to_at":"2026-11-01T00:00:00Z","assigned_by":1,"note":""}]}"#;
        let envelope: Envelope<Vec<UnitCommander>> = serde_json::from_str(body).expect("decodes");
        let current: Vec<&UnitCommander> = envelope
            .data
            .iter()
            .filter(|entry| entry.is_current())
            .collect();
        assert_eq!(
            current.len(),
            1,
            "exactly one open assignment, by definition"
        );
        assert_eq!(current[0].user_name, "A");
    }

    #[test]
    fn an_error_envelope_carries_its_status_and_no_token() {
        let error: ErrorResponse =
            serde_json::from_str(&fixture("error-401.json")).expect("error decodes");
        assert_eq!(error.status_code, 401);
        assert_eq!(error.message, "Unauthorized");
        assert!(error.data.is_none(), "a 401 names no field");
        let not_found: ErrorResponse =
            serde_json::from_str(&fixture("error-404.json")).expect("error decodes");
        assert_eq!(not_found.status_code, 404);
    }

    // -- the operational resources (#107) -------------------------------------------------------

    /// **An unset viewpoint is absent, and absence is not `0,0`.** The deployment answers `{}`:
    /// both coordinates gone together, and a map that defaulted the pair to the origin would draw a
    /// chosen view in the Gulf of Guinea (`INV-01`).
    #[test]
    fn an_unset_viewpoint_is_absent_and_not_zero_zero() {
        let envelope: Envelope<Viewpoint> =
            serde_json::from_str(&fixture("viewpoint.json")).expect("viewpoint decodes");
        assert_eq!(envelope.status_code, 200);
        assert_eq!(envelope.data.latitude, None, "absent is not 0.0");
        assert_eq!(envelope.data.longitude, None);
        assert_eq!(
            envelope.data.coordinates(),
            None,
            "half a viewpoint is not a viewpoint"
        );
    }

    /// The facility vocabulary decodes in picker order, and `symbol_code` — a **code**, not an id —
    /// is absent on this deployment rather than invented.
    #[test]
    fn the_facility_vocabulary_decodes_with_an_absent_symbol_code() {
        let envelope: Envelope<Vec<InstallationType>> =
            serde_json::from_str(&fixture("installation-types.json")).expect("types decode");
        assert_eq!(envelope.data.len(), 5, "five kinds were recorded");
        let orders: Vec<i32> = envelope.data.iter().map(|kind| kind.sort_order).collect();
        let mut sorted = orders.clone();
        sorted.sort_unstable();
        assert_eq!(orders, sorted, "the picker's order, not the file's");
        assert_eq!(
            envelope.data[0].symbol_code, None,
            "an absent symbol_code is not an empty one, and not a made-up code"
        );
        assert_eq!(envelope.data[0].is_system, Some(true));
    }

    /// **An empty facility register is a valid answer**: the dev instance has registered no
    /// facilities. "Draw none" is not "the read failed", and the wall must not fall back to
    /// pretending it could not ask.
    #[test]
    fn an_empty_installation_list_is_a_valid_answer() {
        let envelope: Envelope<Vec<Installation>> =
            serde_json::from_str(&fixture("installations.json")).expect("installations decode");
        assert_eq!(envelope.status_code, 200);
        assert!(envelope.data.is_empty());
    }

    /// A facility's position is optional but indivisible: a row without one has no coordinates,
    /// and `0,0` is a place rather than "unknown".
    #[test]
    fn an_unlocated_facility_has_no_coordinates() {
        let body = r#"{"status_code":200,"message":"Successfull","data":[
            {"id":1,"id_installation_type":3,"type_name":"Pangkalan","id_service_branch":1,"name":"A","is_active":true},
            {"id":2,"id_installation_type":3,"type_name":"Pangkalan","id_service_branch":1,"name":"B","latitude":-6.1,"longitude":106.8,"is_active":true}]}"#;
        let envelope: Envelope<Vec<Installation>> = serde_json::from_str(body).expect("decodes");
        assert_eq!(
            envelope.data[0].coordinates(),
            None,
            "an unlocated facility is not at the origin"
        );
        assert_eq!(envelope.data[0].latitude, None);
        assert_eq!(
            envelope.data[1].coordinates(),
            Some((-6.1, 106.8)),
            "a located facility keeps both coordinates"
        );
    }

    /// The symbol library is the recorded page, with its `metadata` intact: one page of 56 of 56.
    #[test]
    fn the_symbol_library_is_one_recorded_page_with_its_metadata() {
        let envelope: Envelope<Vec<TacticalObject>> =
            serde_json::from_str(&fixture("tactical-objects.json")).expect("library decodes");
        assert_eq!(envelope.data.len(), 56, "the recorded catalogue");
        let metadata = envelope.metadata.expect("a paged route reports metadata");
        assert_eq!(metadata.total_records, 56);
        assert_eq!(metadata.total_pages, 1);
        assert!(!metadata.has_next_page, "one page and no more");

        let first = &envelope.data[0];
        assert_eq!(first.code, "BOYCAR_N");
        assert_eq!(
            first.category.domain.name, "Maritime",
            "the category carries its domain so the picker needs no second call"
        );
        let url = first
            .asset_url
            .as_deref()
            .expect("the recording signed a URL");
        assert!(
            url.starts_with("https://objects.example.test/"),
            "a presigned URL's host and query are redacted in the fixture, got {url}"
        );
        assert!(
            !url.contains("X-Amz-Credential") && !url.contains("X-Amz-Signature"),
            "INV-08: a fixture must not carry a presigned credential, got {url}"
        );
    }

    /// **The manifest is unpaginated and carries an entry for every symbol**, because a page of a
    /// cache manifest is an incomplete cache. A missing `asset_url` would be a missing picture, not a
    /// missing symbol.
    #[test]
    fn the_manifest_is_unpaginated_and_every_entry_carries_its_key() {
        let envelope: Envelope<TacticalObjectManifest> =
            serde_json::from_str(&fixture("tactical-objects-manifest.json"))
                .expect("manifest decodes");
        let manifest = envelope.data;
        assert_eq!(manifest.version, 1);
        assert_eq!(manifest.symbols.len(), 56);
        assert!(
            envelope.metadata.is_none(),
            "the manifest has no metadata: it is not paged"
        );
        for entry in &manifest.symbols {
            assert!(
                !entry.asset_key.is_empty(),
                "{} has no asset_key, so a client cannot recognise its artwork",
                entry.code
            );
        }
        let first = &manifest.symbols[0];
        assert_eq!(first.code, "BOYCAR_N");
        assert_eq!(first.category, "Kenavigasian");
        assert_eq!(first.domain, "Maritime");
    }

    /// The facets are the picker's two rows: the counts agree with each other and with the total, and
    /// a category with no symbols is returned with `count: 0` rather than omitted.
    #[test]
    fn the_facet_counts_agree_and_zero_count_rows_survive() {
        let envelope: Envelope<TacticalObjectFacets> =
            serde_json::from_str(&fixture("tactical-objects-facets.json")).expect("facets decode");
        let facets = envelope.data;
        assert_eq!(facets.total, 56);
        let by_domain: u64 = facets.domains.iter().map(|domain| domain.count).sum();
        let by_category: u64 = facets
            .categories
            .iter()
            .map(|category| category.count)
            .sum();
        assert_eq!(
            by_domain, facets.total,
            "a domain's count is the sum of its categories'"
        );
        assert_eq!(by_category, facets.total);
        assert!(
            facets.domains.iter().any(|domain| domain.count == 0),
            "the vocabulary is complete even where the catalogue is not: a zero row must survive"
        );
        for category in &facets.categories {
            assert!(
                facets
                    .domains
                    .iter()
                    .any(|domain| domain.id == category.id_domain),
                "category {} names a domain the response does not carry",
                category.name
            );
        }
    }

    /// A placement **embeds** its whole symbol, so drawing never needs a second lookup — and a mark
    /// with no label and no rotation decodes as `None`, not as `""` or `0.0` (`INV-01`).
    #[test]
    fn a_placement_embeds_its_symbol_and_an_absent_label_is_none() {
        let envelope: Envelope<Vec<TacticalObjectPlacement>> =
            serde_json::from_str(&fixture("tactical-object-placements.json"))
                .expect("placements decode");
        assert_eq!(envelope.data.len(), 3);
        let first = &envelope.data[0];
        assert_eq!(first.symbol.code, "LIGHTS_FIX");
        assert!(
            first.symbol.category.domain.name == "Maritime",
            "the embedded symbol carries its category and domain"
        );
        assert_eq!(
            first.label.as_deref(),
            Some(""),
            "the recording sent an empty label, which is a value the operator typed as empty"
        );

        // The absence case: the same recorded rows with `label` and `rotation_deg` removed, the shape
        // the API sends when neither is set.
        let absent = fixture("tactical-object-placements.json")
            .replace("\"label\": \"\",\n", "")
            .replace("\"rotation_deg\": 0,\n", "");
        let envelope: Envelope<Vec<TacticalObjectPlacement>> =
            serde_json::from_str(&absent).expect("the absence case decodes");
        for mark in &envelope.data {
            assert_eq!(mark.label, None, "an absent label is not an empty string");
            assert_eq!(
                mark.rotation_deg, None,
                "an absent rotation is not 0 degrees"
            );
        }
    }

    /// `nodes` is a **forest**: more than one root is normal, `children` is always an array, and a
    /// post with no level has no `echelon_rank` — never a rank of 0.
    #[test]
    fn the_standing_structure_is_a_forest_with_children_always_an_array() {
        let list: Envelope<Vec<UnitOrganisation>> =
            serde_json::from_str(&fixture("unit-organisations.json")).expect("list decodes");
        assert_eq!(list.data.len(), 4, "one structure per service branch");
        let navy = list
            .data
            .iter()
            .find(|organisation| organisation.id == 1)
            .expect("the recorded detail is for id 1");

        let detail: Envelope<UnitOrganisationDetail> =
            serde_json::from_str(&fixture("unit-organisation-1.json")).expect("detail decodes");
        assert_eq!(detail.data.organisation.id, navy.id);
        assert_eq!(detail.data.organisation.name, navy.name);
        assert_eq!(detail.data.nodes.len(), 1, "this structure has one root");

        let root = &detail.data.nodes[0];
        assert_eq!(root.parent_id, None, "a root has no parent");
        assert_eq!(
            root.echelon_rank, None,
            "a post with no level is not a post at rank 0"
        );
        assert_eq!(
            root.commander, None,
            "nobody is appointed to this recorded post"
        );
        assert!(
            root.position_title.is_some(),
            "the recorded root has a position title"
        );
        fn count(nodes: &[UnitOrganisationNode]) -> usize {
            nodes.iter().map(|node| 1 + count(&node.children)).sum()
        }
        assert_eq!(count(&detail.data.nodes), 7, "the recorded structure");
        assert_eq!(root.children.len(), 1, "the recorded root has one child");
        assert_eq!(
            root.children[0].children.len(),
            2,
            "the recorded second level has two children"
        );
        let leaf = root.children[0].children[0]
            .children
            .iter()
            .find(|node| node.id == 7)
            .expect("the recorded deepest post");
        assert!(
            leaf.children.is_empty(),
            "a leaf's children is an empty array, not null"
        );
    }

    /// **A vacant supreme command is not an error.** The post is shared by every branch and its
    /// incumbent is absent; the history is an empty list, which is a valid answer.
    #[test]
    fn a_vacant_supreme_command_is_not_an_error() {
        let envelope: Envelope<TniCommandPost> =
            serde_json::from_str(&fixture("tni-commander.json")).expect("the post decodes");
        let post = envelope.data;
        assert_eq!(post.position_title, "Panglima TNI");
        assert_eq!(post.echelon_rank, 7, "the top of the vocabulary");
        assert_eq!(
            post.commander, None,
            "a vacant post has no incumbent, and that is a real state"
        );
        assert!(
            post.echelon_name.is_some(),
            "the recorded post names its echelon"
        );

        let history: Envelope<Vec<TniCommander>> =
            serde_json::from_str(&fixture("tni-commander-history.json")).expect("history decodes");
        assert_eq!(history.status_code, 200);
        assert!(
            history.data.is_empty(),
            "nobody has been appointed on this deployment"
        );
    }
}

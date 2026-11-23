//! **One vessel's report, and the answer it comes back with** (#278).
//!
//! The desk writes `POST /live-feed/fixes/relayed`, not the beacon's `/live-feed/fixes` (ADR-0009's amendment of
//! 2026-11-23). The difference is the claim: a beacon fix says *this is where I am*, and the route requires the
//! caller to be the vessel's commander — the account that cannot reach this system from sea. A relayed fix says
//! *this is where that ship told me it is*, and the server records who asserted it and for whom.
//!
//! Two halves live here, and **neither needs a socket**: [`Body`] is the bytes one row becomes, and
//! [`Verdict`] is what an answer means. The transport comes after them, so this shape is pinned by exchanges
//! recorded from the wire (`fixtures/desk/report/`) instead of by a request nobody can re-run.
//!
//! Nothing here invents a wire shape. The fields, their order and their bounds are `dto.FeedRelayRequest` and
//! `dto.FeedRelayedResult` in Minos `496ae25`; the refusals are the bytes the development deployment answered on
//! 2026-11-23.

use serde::Serialize;

use crate::batch::{FixError, Sendable};

/// The route one vessel's fix is posted to. One vessel, one fix, one request (decision 2).
pub const PATH: &str = "/live-feed/fixes/relayed";

/// An instant as ISO 8601 in UTC with milliseconds — what `recorded_at` is written as.
///
/// The field edits WIB (decision 3) and the wire is UTC; the conversion is the field's own arithmetic, so the
/// only thing this function adds is the spelling. `Z` rather than `+00:00`, because the contract's own examples
/// use it and a desk that wrote both would make two identical instants look different in an audit trail.
pub fn iso_utc(epoch_ms: i64) -> String {
    let civil = crate::field::zoned::utc_civil(epoch_ms);
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}.{:03}Z",
        civil.year,
        civil.month,
        civil.day,
        civil.hour,
        civil.minute,
        civil.second,
        epoch_ms.rem_euclid(1_000)
    )
}

/// The body of one vessel's request.
///
/// **Absent is absent.** A measurement the source did not give is not serialised at all, because `0` is a claim
/// (`INV-01`): a speed of zero says the ship stopped, and a vessel whose speed nobody measured is not stopped.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Body {
    pub id_unit: u64,
    pub recorded_at: String,
    pub latitude: f64,
    pub longitude: f64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speed_kn: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub course_deg: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub accuracy_m: Option<f64>,
}

impl Body {
    /// The body a sendable row becomes, or the reason it must not be sent.
    ///
    /// The fence is [`crate::batch::Fix::check`]'s — the same one the tray drew its readiness from — so a
    /// row cannot be refused on screen and accepted here. What this adds is the one thing a row can lack that
    /// the fence allows: **coordinates**. A relay without a position is not a position.
    pub fn of(sendable: &Sendable) -> Result<Self, Refusal> {
        let fix = &sendable.fix;
        let latitude = fix.latitude.ok_or(Refusal::Missing("latitude"))?;
        let longitude = fix.longitude.ok_or(Refusal::Missing("longitude"))?;
        fix.check().map_err(Refusal::Fenced)?;
        Ok(Self {
            id_unit: sendable.id_unit,
            recorded_at: iso_utc(fix.recorded_at_ms),
            latitude,
            longitude,
            speed_kn: fix.speed_kn,
            course_deg: fix.course_deg,
            accuracy_m: fix.accuracy_m,
        })
    }

    /// The bytes, exactly as they go on the wire.
    pub fn json(&self) -> String {
        serde_json::to_string(self).expect("a body of numbers and strings always serialises")
    }
}

/// Why a row is not sent: a field the operator must fix, named as the wire names it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Refusal {
    /// A coordinate the row does not have.
    Missing(&'static str),
    /// The sanity fence: a value no vessel can do.
    Fenced(FixError),
}

impl Refusal {
    pub fn field(self) -> &'static str {
        match self {
            Self::Missing(field) => field,
            Self::Fenced(error) => error.field(),
        }
    }

    /// What the row says, in the operator's words.
    pub fn sentence(self) -> String {
        match self {
            Self::Missing("latitude") => {
                "no latitude: place the vessel on the chart first".to_string()
            }
            Self::Missing("longitude") => {
                "no longitude: place the vessel on the chart first".to_string()
            }
            Self::Missing(field) => format!("no {field}"),
            Self::Fenced(error) => error.to_string(),
        }
    }
}

/// One answer, exactly as it arrived: the status and the body's bytes.
///
/// The status is the transport's, not the body's `status_code`. They agree today; a proxy that answered `502`
/// with an HTML page has no `status_code` at all, and a client that read the body first would call it nothing.
#[derive(Debug, Clone, PartialEq)]
pub struct Answer {
    pub status: u16,
    pub body: String,
}

impl Answer {
    pub fn new(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into(),
        }
    }

    /// What the answer means for the row that asked.
    ///
    /// Every status the route documents is a branch here, and **anything else is `Unrecognised`** — a status
    /// nobody has seen is not a success (`INV-07`: no catch-all that reads as one).
    pub fn verdict(&self) -> Verdict {
        let envelope = serde_json::from_str::<Envelope>(&self.body).ok();
        let message = envelope
            .as_ref()
            .and_then(|envelope| envelope.message.clone())
            .unwrap_or_else(|| "the answer had no message".to_string());
        match self.status {
            201 => envelope
                .and_then(|envelope| envelope.data)
                .and_then(|data| created(&data))
                .unwrap_or(Verdict::Unrecognised {
                    status: 201,
                    message: "a 201 whose body was not the recorded result".to_string(),
                }),
            400 => refused(400, envelope.as_ref(), message),
            401 => Verdict::Unauthenticated { message },
            403 => Verdict::NoCommander {
                message: first_error(envelope.as_ref())
                    .unwrap_or((String::new(), message))
                    .1,
            },
            404 => Verdict::NoSuchVessel,
            422 => Verdict::Unreadable {
                message: first_error(envelope.as_ref())
                    .unwrap_or((String::new(), message))
                    .1,
            },
            status => Verdict::Unrecognised { status, message },
        }
    }
}

/// The envelope every answer shares: a status, a message, and a `data` this module reads by shape.
#[derive(serde::Deserialize)]
struct Envelope {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    data: Option<serde_json::Value>,
}

/// `data` of a `201`: the one fix the route recorded.
fn created(data: &serde_json::Value) -> Option<Verdict> {
    Some(Verdict::Created {
        id_unit: data.get("id_unit")?.as_u64()?,
        recorded_at: data.get("recorded_at")?.as_str()?.to_string(),
        received_at: data.get("received_at")?.as_str()?.to_string(),
        latest: data.get("latest")?.as_bool()?,
    })
}

/// The first `data.errors.<field>` an answer carries: the shape both a `400` and a `422` use.
fn first_error(envelope: Option<&Envelope>) -> Option<(String, String)> {
    let errors = envelope?.data.as_ref()?.get("errors")?.as_object()?;
    let (field, message) = errors.iter().next()?;
    Some((field.clone(), message.as_str()?.to_string()))
}

/// A `400`: a field Minos refused, or one whose reason is not in a field.
fn refused(status: u16, envelope: Option<&Envelope>, message: String) -> Verdict {
    match first_error(envelope) {
        Some((field, message)) => Verdict::Refused {
            status,
            field,
            message,
        },
        None => Verdict::Unrecognised { status, message },
    }
}

/// What an answer means for its row.
#[derive(Debug, Clone, PartialEq)]
pub enum Verdict {
    /// `201`: the fix was recorded. `latest` says whether the picture moved — a report older than what the
    /// vessel's own handset already sent is in the log and does **not** move the map, and saying "created" without
    /// that half would let an operator believe they corrected it.
    Created {
        id_unit: u64,
        recorded_at: String,
        received_at: String,
        latest: bool,
    },
    /// `400`: a field Minos refused, keyed exactly as it was sent. Fix that field and send this row again.
    Refused {
        status: u16,
        field: String,
        message: String,
    },
    /// `401`: the credential expired. Refresh once, re-check the regime, re-send the same body.
    Unauthenticated { message: String },
    /// `403`: the vessel has no commander appointed, so there is no report to attribute. The row stays whole;
    /// it becomes sendable again when a commander is assigned.
    NoCommander { message: String },
    /// `404`: no such hull. The row goes.
    NoSuchVessel,
    /// `422`: the route could not read the body. That is this client's bug, never the operator's fix.
    Unreadable { message: String },
    /// A status nobody has seen. Never read as success.
    Unrecognised { status: u16, message: String },
}

impl Verdict {
    /// What the row says under itself.
    pub fn sentence(&self) -> String {
        match self {
            Self::Created { latest: true, .. } => "recorded — the picture moved".to_string(),
            Self::Created { latest: false, .. } => {
                "recorded — older than what this vessel already reported, so the picture did not move"
                    .to_string()
            }
            Self::Refused { field, message, .. } => format!("Minos refused {field}: {message}"),
            Self::Unauthenticated { .. } => {
                "the credential expired: sign in again, then send this row".to_string()
            }
            Self::NoCommander { message } => {
                format!("no commander appointed, so there is no report to attribute: {message}")
            }
            Self::NoSuchVessel => "no such vessel: Minos does not have this hull".to_string(),
            Self::Unreadable { message } => {
                format!("this client sent a body Minos could not read: {message}")
            }
            Self::Unrecognised { status, message } => {
                format!("unrecognised answer {status}: {message}")
            }
        }
    }

    /// Whether the same row, unchanged, may be sent again.
    ///
    /// A refusal the operator can fix, an expired credential and an unappointed commander are all retryable once
    /// the named thing changes. A `201` is done, a missing hull is gone, and a body this client could not write is
    /// retried only after the client is fixed — so none of those are retryable as they stand.
    pub fn retryable(&self) -> bool {
        matches!(
            self,
            Self::Refused { .. } | Self::Unauthenticated { .. } | Self::NoCommander { .. }
        )
    }
}

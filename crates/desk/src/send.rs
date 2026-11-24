//! **The send** — one row's trip to Minos, and a batch of those trips (#278).
//!
//! The route takes one vessel's fix, so a batch is a loop and every row has its own answer (ADR-0009, decision 2).
//! There is no global "sent" here, and there could not be: a `201` for one hull says nothing about the next.
//!
//! **The transport is a trait, not a socket.** What this module has to prove is the shape of the trip — that a row
//! cannot be in flight twice, that an answer settles it, that a transport failure changes nothing, and that one
//! vessel's refusal never touches another's — and that is provable with a fake that answers in recorded bytes. The
//! HTTP client is a thin implementation of [`Transport`] beside it, and the gate never opens a socket.

use crate::batch::Plan;
use crate::report::{Answer, Body, Refusal, Verdict};

/// What one send needs from the world: a body in, an answer out, or a failure that is nobody's fault yet.
pub trait Transport {
    /// Post one vessel's fix. The answer is returned whatever its status: a `403` is an answer, not a failure.
    fn post(&mut self, body: &Body) -> Result<Answer, Failure>;
}

/// A transport failure: no answer arrived at all.
///
/// The message is for the operator and **never carries the body, the token or the host** (`INV-08`). It is the only
/// failure that changes nothing about the fix, which is why it is its own type rather than a ninth [`Verdict`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Failure {
    pub message: String,
}

impl Failure {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

/// One row's state. Four transitions, and the only way in or out of each is a method here.
///
/// **`InFlight` holds the body that is out**, so a second send of the same row has nothing to hand over — a vessel
/// cannot be in flight twice by construction, not by a flag somebody has to remember to clear.
#[derive(Debug, Clone, PartialEq)]
pub enum State {
    /// The row as typed. Nothing has been checked.
    Draft,
    /// The fix passes the fence, has both coordinates, and has its bytes.
    Validated(Body),
    /// The body is out. It comes back as [`State::Recorded`] or [`State::Refused`] when the answer lands.
    InFlight(Body),
    /// `201`: Minos recorded it. `latest` is whether the picture moved.
    Recorded { body: Body, latest: bool },
    /// A wire refusal: the answer named what is wrong, and the row is still here.
    Refused { body: Body, verdict: Verdict },
}

impl State {
    /// The body this row has, once it has one.
    pub fn body(&self) -> Option<&Body> {
        match self {
            Self::Draft => None,
            Self::Validated(body)
            | Self::InFlight(body)
            | Self::Recorded { body, .. }
            | Self::Refused { body, .. } => Some(body),
        }
    }

    /// Draft → Validated: the fence, the coordinates, and the bytes.
    ///
    /// Consumes the draft, so a refusal **changes nothing**: the operator's fix is still theirs to correct, and the
    /// caller never gets a half-checked row back.
    pub fn validated(self, sendable: &crate::batch::Sendable) -> Result<State, Refusal> {
        Body::of(sendable).map(State::Validated)
    }

    /// Validated → InFlight, handing the body over. `None` when the row is not validated, which is what stops the
    /// same row being sent twice.
    pub fn begin(&mut self) -> Option<Body> {
        match std::mem::replace(self, Self::Draft) {
            Self::Validated(body) => {
                let handed = body.clone();
                *self = Self::InFlight(body);
                Some(handed)
            }
            other => {
                *self = other;
                None
            }
        }
    }

    /// InFlight → Recorded or Refused. Anything else is not in flight and has no answer to settle.
    pub fn settle(&mut self, answer: &Answer) -> Option<&Verdict> {
        let Self::InFlight(body) = std::mem::replace(self, Self::Draft) else {
            *self = Self::Draft;
            return None;
        };
        match answer.verdict() {
            Verdict::Created { latest, .. } => {
                *self = Self::Recorded { body, latest };
                None
            }
            verdict => {
                *self = Self::Refused {
                    body,
                    verdict: verdict.clone(),
                };
                match self {
                    Self::Refused { verdict, .. } => Some(verdict),
                    _ => None,
                }
            }
        }
    }

    /// InFlight → Validated: no answer arrived, so the row is exactly where it was before the attempt.
    ///
    /// Not a retry: the loop does not retry by itself, because the regime may have crossed a line while the request
    /// waited. The row is left sendable and the operator (or the next send) decides.
    pub fn failed(&mut self) {
        if let Self::InFlight(body) = std::mem::replace(self, Self::Draft) {
            *self = Self::Validated(body);
        } else {
            *self = Self::Draft;
        }
    }
}

/// What became of one row.
#[derive(Debug, Clone, PartialEq)]
pub enum Fate {
    /// `201`, with whether the picture moved.
    Recorded { latest: bool },
    /// The wire refused it; the verdict's sentence is what the row says.
    Refused(Verdict),
    /// It never left: the fence, or a coordinate the row does not have.
    Unsendable(Refusal),
    /// No answer arrived. Nothing about the fix changed.
    Failed(Failure),
}

impl Fate {
    /// Whether the same row, unchanged, may be sent again.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Recorded { .. } | Self::Unsendable(_) => false,
            Self::Refused(verdict) => verdict.retryable(),
            Self::Failed(_) => true,
        }
    }

    /// What the row says under itself.
    pub fn sentence(&self) -> String {
        match self {
            Self::Recorded { latest: true } => "recorded — the picture moved".to_string(),
            Self::Recorded { latest: false } => {
                "recorded — older than what this vessel already reported, so the picture did not move".to_string()
            }
            Self::Refused(verdict) => verdict.sentence(),
            Self::Unsendable(refusal) => refusal.sentence(),
            Self::Failed(failure) => format!("no answer from Minos: {}", failure.message),
        }
    }
}

/// One vessel and what became of it, in the order the batch was built.
#[derive(Debug, Clone, PartialEq)]
pub struct Trip {
    pub id_unit: u64,
    /// The body as it was sent, or as it would have been. Absent only for a row that never got one.
    pub body: Option<Body>,
    pub fate: Fate,
}

/// Send every row of a plan, one request each, and report each answer against the vessel that asked.
///
/// The rows are independent: a refusal, a missing hull or a transport failure for one vessel leaves the next
/// untouched, and the loop does not stop for any of them. A transport failure is **not** retried here — the caller
/// decides, and re-validates first, because the regime may have crossed its line while the request waited.
pub fn send_all(plan: &Plan, transport: &mut impl Transport) -> Vec<Trip> {
    let mut trips = Vec::with_capacity(plan.sendable.len());
    for sendable in &plan.sendable {
        let mut state = match State::Draft.validated(sendable) {
            Ok(state) => state,
            Err(refusal) => {
                trips.push(Trip {
                    id_unit: sendable.id_unit,
                    body: None,
                    fate: Fate::Unsendable(refusal),
                });
                continue;
            }
        };
        let Some(body) = state.begin() else {
            trips.push(Trip {
                id_unit: sendable.id_unit,
                body: None,
                fate: Fate::Unsendable(Refusal::Missing("state")),
            });
            continue;
        };
        let fate = match transport.post(&body) {
            Ok(answer) => {
                state.settle(&answer);
                match &state {
                    State::Recorded { latest, .. } => Fate::Recorded { latest: *latest },
                    State::Refused { verdict, .. } => Fate::Refused(verdict.clone()),
                    _ => Fate::Unsendable(Refusal::Missing("state")),
                }
            }
            Err(failure) => {
                state.failed();
                Fate::Failed(failure)
            }
        };
        trips.push(Trip {
            id_unit: sendable.id_unit,
            body: Some(body),
            fate,
        });
    }
    trips
}

//! **The batch, without a window** — the parts of #277 that are rules rather than pixels.
//!
//! A batch is vessels, one row each, one request each (ADR-0009, decision 2). Nothing here sends anything;
//! #278 turns a [`Plan`] into requests, one per vessel. Nothing here decides a regime either: every time
//! question goes through [`crate::time::verdict`], so the field, the row and the send cannot disagree about
//! whether a fix is live or backfill.
//!
//! The model is deliberately smaller than the screen. A row holds the last observation of one hull and
//! cannot hold a second; a batch holds one row per hull and returns the row it displaced. The contract's
//! sixty fixes per request are not a shape the desk can reach: it writes one fix per vessel.

mod fix;
mod plan;
mod report;
mod row;

pub use fix::{
    ACCURACY_LIMIT_M, COURSE_LIMIT_DEG, Fix, FixError, LATITUDE_LIMIT_DEG, LONGITUDE_LIMIT_DEG,
    SPEED_LIMIT_KN,
};
pub use plan::{Plan, Sendable};
pub use report::Batch;
pub use row::{Readiness, Reason, Row};

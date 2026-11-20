//! **A plan** — which rows would leave, and which stay behind with a reason.
//!
//! A plan is a report about a moment, not a payload. Nothing here serialises, and nothing here sends: #278
//! turns each [`Sendable`] into its own request, after asking [`crate::time::verdict`] again, because time
//! has moved since the plan was drawn.

use super::fix::Fix;
use super::row::Reason;

/// One vessel and exactly one fix — the unit Minos accepts, because `id_unit` is top-level and a request
/// carries the fixes of one hull. There is no list of fixes here on purpose: a second fix for a vessel
/// would be a track nobody asked for (ADR-0009, decision 2).
#[derive(Debug, Clone, PartialEq)]
pub struct Sendable {
    pub id_unit: u64,
    pub name: String,
    pub fix: Fix,
}

/// What would be sent now, and what would not.
///
/// The contract is arithmetic: N vessels means **N requests**, one `id_unit` each. A plan that looked like
/// one payload with N fixes would describe an endpoint Minos does not have. `blocked` carries the row's
/// reason so the tray can say why it is still on the board.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Plan {
    pub sendable: Vec<Sendable>,
    pub blocked: Vec<(u64, Reason)>,
}

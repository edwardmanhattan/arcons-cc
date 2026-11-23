//! **The batch** — the report the operator is about to send, one row per vessel.
//!
//! A batch is vessels, one row per hull, one request each (ADR-0009, decision 2). It is a **draft, not a
//! queue**: it may survive a restart so a mis-click does not lose ten minutes of typing, but a send never
//! uses the time the draft was made, and nothing here schedules a retry. A queued report whose regime
//! changed between typing and sending is silently wrong, and that is the failure the ADR refuses.
//!
//! The [`source_note`](Batch::source_note) is decision 6: the payload has no provenance field, so the desk
//! keeps one free-text note per batch itself. It is optional and local — a mandatory note becomes `"x"` by
//! the second day, and an `"x"` that looks like evidence is worse than an empty field.

use super::plan::{Plan, Sendable};
use super::row::{Readiness, Row};

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Batch {
    rows: Vec<Row>,
    /// Where the operator learned this batch: "Plot 4 · phone call 14:03". Never sent, never required.
    pub source_note: Option<String>,
}

impl Batch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a row in the batch, replacing the hull's row if it already has one.
    ///
    /// Returns the displaced row, because replacing an observation is not a silent edit: the screen has to
    /// be able to undo it, and a hull with two rows would be a track the contract cannot carry. The
    /// replacement keeps the hull's place in the tray, so a row does not jump while it is being edited.
    pub fn insert(&mut self, row: Row) -> Option<Row> {
        match self
            .rows
            .iter_mut()
            .find(|held| held.id_unit == row.id_unit)
        {
            Some(held) => Some(std::mem::replace(held, row)),
            None => {
                self.rows.push(row);
                None
            }
        }
    }

    pub fn remove(&mut self, id_unit: u64) -> Option<Row> {
        let index = self.rows.iter().position(|row| row.id_unit == id_unit)?;
        Some(self.rows.remove(index))
    }

    pub fn get(&self, id_unit: u64) -> Option<&Row> {
        self.rows.iter().find(|row| row.id_unit == id_unit)
    }

    /// How many rows this hull holds. Zero or one, and the one place the list's badge asks.
    ///
    /// The answer is a count rather than a flag because that is what the badge says: a hull in the batch
    /// shows its count beside its name instead of a second row somewhere else. The model guarantees the
    /// count is never two, so a two would be the bug the badge exists to make impossible to draw.
    pub fn count(&self, id_unit: u64) -> usize {
        self.rows
            .iter()
            .filter(|row| row.id_unit == id_unit)
            .count()
    }

    /// The rows in the order they were added, which is the order the operator built the report in.
    pub fn rows(&self) -> &[Row] {
        &self.rows
    }

    pub fn len(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// What would be sent now, and what cannot be. A plan is a snapshot, not a payload.
    pub fn plan(&self, server_now: Option<i64>) -> Plan {
        let mut plan = Plan::default();
        for row in &self.rows {
            match row.readiness(server_now) {
                Readiness::Sendable => plan.sendable.push(Sendable {
                    id_unit: row.id_unit,
                    name: row.name.clone(),
                    fix: row.fix,
                }),
                Readiness::Blocked(reason) => plan.blocked.push((row.id_unit, reason)),
            }
        }
        plan
    }
}

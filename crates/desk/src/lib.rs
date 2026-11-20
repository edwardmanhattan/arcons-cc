//! Arcons Desk — the shore client that writes the Minos live feed.
//!
//! See `docs/adr/0009-arcons-desk.md`. This crate starts with the two things every later screen is built on
//! and neither of which needs a window: **who this installation is** ([`identity`]) and **what a typed time
//! will do** ([`time`]).

pub mod batch;
pub mod field;
pub mod identity;
pub mod time;

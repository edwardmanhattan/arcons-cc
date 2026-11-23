//! Arcons Desk — the shore client that writes the Minos live feed.
//!
//! See `docs/adr/0009-arcons-desk.md`. The crate holds the two things every later screen is built
//! on and neither of which needs a window — **who this installation is** ([`identity`]) and **what
//! a typed time will do** ([`time`]) — and the two things that do need one: the DOM-free model of
//! the time field ([`field`]) and the screen that composes it ([`ui`]), with the named states a
//! capture renders ([`scene`]).

pub mod batch;
pub mod chart;
pub mod field;
pub mod identity;
pub mod picker;
pub mod report;
pub mod scene;
pub mod send;
pub mod time;
pub mod ui;

//! Arcons Command Center, as a library.
//!
//! The binary (`src/main.rs`) is one use of this crate; the tests are the other. They have to be
//! able to name what they test — the refusal codes in `live`, the wire field lists in `data` — and
//! a binary-only crate cannot be imported. One binary is still produced.
//!
//! Two rules shape the module list, and neither is negotiable:
//!
//! - **The UI thread never blocks.** Reads (`data`), the socket (`live`) and the map (`map`) each
//!   own a thread and hand results to the view loop; the view loop only ever builds elements.
//! - **A ticket is done when `scripts/verify.sh` says so.** See `docs/verification.md` and
//!   `docs/invariants.md`; every ticket carries the command that decides it.

pub mod config;
pub mod data;
pub mod live;
pub mod map;
pub mod platform;
pub mod scene;
pub mod ui;
pub mod units;
pub mod verify;

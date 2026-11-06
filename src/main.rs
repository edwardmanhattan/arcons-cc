//! Arcons Command Center: the live maritime picture, on a wall.
//!
//! v0.1 is a map of every live vessel, moving as fixes arrive, where clicking a unit opens a
//! floating card with what is known about it. The plan lives in the issues — `#1` is the epic —
//! not in this file.
//!
//! Two rules shape the module list, and neither is negotiable:
//!
//! - **The UI thread never blocks.** Reads (`data`), the socket (`live`) and the map (`map`) each
//!   own a thread and hand results to the view loop; the view loop only ever builds elements.
//! - **A ticket is done when `scripts/verify.sh` says so.** See `docs/verification.md` and
//!   `docs/invariants.md`; every ticket carries the command that decides it.

mod config;
mod data;
mod live;
mod map;
mod ui;
mod units;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args = match config::Args::parse(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(message) => {
            eprintln!("arcons-cc: {message}\n\n{}", config::USAGE);
            return ExitCode::from(64); // EX_USAGE
        }
    };

    match args.command {
        config::Command::Run => ui::run(args.window, args.passthrough),
        config::Command::Version => {
            println!("arcons-cc {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        config::Command::Help => {
            println!("{}", config::USAGE);
            ExitCode::SUCCESS
        }
        config::Command::SelfCheck => config::self_check(),
    }
}

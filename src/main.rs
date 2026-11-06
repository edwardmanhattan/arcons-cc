//! Arcons Command Center: the live maritime picture, on a wall.
//!
//! v0.1 is a map of every live vessel, moving as fixes arrive, where clicking a unit opens a
//! floating card with what is known about it. The plan lives in the issues — `#1` is the epic —
//! not in this file.
//!
//! The crate is a library (`src/lib.rs`) with this one binary on top of it, so the tests can name
//! what they test. Everything worth reading is documented there.

use std::process::ExitCode;

use arcons_cc::{config, ui};

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

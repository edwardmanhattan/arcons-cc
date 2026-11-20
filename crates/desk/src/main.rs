//! `cargo run -p desk` — the desk's command line. The window arrives with the time field (#276, second
//! commit); what exists here is the installation's identity and the way to see it.

use std::process::ExitCode;

use desk::identity::{self, DeviceId};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("--version") => {
            println!("arcons desk {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--self-check") | None => self_check(),
        Some("--new-device-id") => new_device_id(),
        Some(flag) => {
            eprintln!("desk: unknown flag {flag}");
            eprintln!("  --self-check      what is configured, and this installation's device id");
            eprintln!(
                "  --new-device-id   mint a new id (a cloned disk image carries the old one)"
            );
            eprintln!("  --version         the version, no window");
            ExitCode::from(64)
        }
    }
}

fn self_check() -> ExitCode {
    let Some(dir) = identity::config_dir() else {
        eprintln!("desk: no config directory (set XDG_CONFIG_HOME or HOME)");
        return ExitCode::from(2);
    };
    println!("client       arcons desk {}", env!("CARGO_PKG_VERSION"));
    println!("config       {}", dir.display());
    match DeviceId::load_or_mint(&dir) {
        Ok(id) => {
            println!("device id    {id}");
            println!("device file  {}", DeviceId::path(&dir).display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("device id    {error}");
            ExitCode::from(2)
        }
    }
}

fn new_device_id() -> ExitCode {
    let Some(dir) = identity::config_dir() else {
        eprintln!("desk: no config directory (set XDG_CONFIG_HOME or HOME)");
        return ExitCode::from(2);
    };
    match DeviceId::rotate(&dir) {
        Ok(id) => {
            println!("device id    {id}");
            println!("device file  {}", DeviceId::path(&dir).display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("desk: {error}");
            ExitCode::from(2)
        }
    }
}

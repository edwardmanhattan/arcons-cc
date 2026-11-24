//! **The standing picture: the read, the cache, and the time that says it is not now** (#284).
//!
//! The desk reads `GET /live-feed/positions` on a worker and paints what it already has. Startup reads the
//! cache **before** it tries the network, so a cold start offline opens on the last picture it read; every
//! successful read writes the cache back atomically (a temp file in the same directory, then a rename).
//!
//! **A cached picture is not now.** The list's ages are the wire's own `age_seconds`, carried unchanged;
//! nothing here derives an age from the cache's timestamp. The timestamp is a label ("read at 14:02") and
//! the sentence that says a refresh failed, never an observation time.
//!
//! **The cache holds no secret, host or username** (`INV-08`). It holds the positions Minos returned —
//! hull ids, names, numbers and last known fixes — and the read's timestamp. The credential stays in
//! [`crate::deployment`]; it is never serialised beside the picture. The wall's `FeedFix` does not model
//! `reported_by`/`reported_for`, so those fields are dropped when the answer is parsed and cannot reach the
//! file; `tests/picture.rs` asserts the file's keys.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use arcons_cc::data::types::FeedPosition;
use serde::{Deserialize, Serialize};

use crate::field::zoned;
use crate::identity;
use crate::send::Failure;

/// The file inside the client's config directory that holds the last standing picture.
pub const FILE: &str = "desk-picture.json";

/// What the cache holds: the positions, and when they were read.
///
/// The positions are the wall's [`FeedPosition`] — the same type the list parses — so the cache cannot
/// disagree with the read about the picture's shape. It is serialised from the parsed values, not the raw
/// response body: the parse is what drops the fields the wall does not model, including the provenance
/// usernames the newest contract adds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Cached {
    pub read_at_ms: i64,
    pub positions: Vec<FeedPosition>,
}

/// What a successful read hands back to the frame loop.
#[derive(Debug, Clone, PartialEq)]
pub struct Picture {
    pub read_at_ms: i64,
    pub positions: Vec<FeedPosition>,
}

impl Picture {
    /// The list's copy of the picture, in the age order the ADR asks the list for.
    pub fn picker(&self) -> crate::picker::Picker {
        crate::picker::Picker::from_positions(&self.positions)
    }
}

/// **Where the picture is in its life.** The list is the picture already in memory; this is the sentence
/// beside it, which has to say "cached" when the list is not now.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    /// No cache and no successful read: an empty list and "no picture read yet", never an error.
    None,
    /// A read is out. `cached` is the picture still on screen, when there is one.
    Reading { cached: Option<i64> },
    /// The picture came from the file at startup and no read has answered yet.
    Cached { read_at_ms: i64 },
    /// The picture came from a successful read.
    Ready { read_at_ms: i64 },
    /// The refresh failed. `cached` is the picture still on screen; the list is never emptied by a lost
    /// connection.
    Failed {
        message: String,
        cached: Option<i64>,
    },
}

impl Status {
    /// The read the picture on screen came from, when there is one.
    pub fn read_at(&self) -> Option<i64> {
        match self {
            Self::None => None,
            Self::Reading { cached } => *cached,
            Self::Cached { read_at_ms } | Self::Ready { read_at_ms } => Some(*read_at_ms),
            Self::Failed { cached, .. } => *cached,
        }
    }

    pub fn is_reading(&self) -> bool {
        matches!(self, Self::Reading { .. })
    }

    /// The one sentence the rail puts under the list. `offset_ms` is the clock the operator reads (WIB),
    /// passed in so this module does not own a second copy of the field's zone.
    pub fn line(&self, offset_ms: i32) -> String {
        let at = |ms: i64| {
            let civil = zoned::at_offset(ms, offset_ms);
            format!("{:02}:{:02}", civil.hour, civil.minute)
        };
        match self {
            Self::None => "no picture read yet".to_string(),
            Self::Reading { cached: Some(ms) } => {
                format!("reading… showing the picture read at {}", at(*ms))
            }
            Self::Reading { cached: None } => "reading the standing picture…".to_string(),
            Self::Cached { read_at_ms } => format!("cached · read at {}", at(*read_at_ms)),
            Self::Ready { read_at_ms } => format!("read at {}", at(*read_at_ms)),
            Self::Failed {
                message,
                cached: Some(ms),
            } => format!(
                "refresh failed — showing the picture read at {} ({message})",
                at(*ms)
            ),
            Self::Failed {
                message,
                cached: None,
            } => format!("refresh failed — no picture read ({message})"),
        }
    }
}

/// The cache's path inside `dir`.
pub fn path_in(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// **The cache this installation has**, or `None` when there is none or it cannot be read.
///
/// A missing file is the first run and says nothing. A file that is not the cache we wrote — a truncated
/// write from an older build, a hand-edited file, bytes from another program — is **ignored with one line
/// in the log**, never a panic and never a silent empty picture.
pub fn load_from(dir: &Path) -> Option<Cached> {
    let path = path_in(dir);
    let text = match fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return None,
        Err(error) => {
            eprintln!(
                "desk: cannot read the picture cache at {}: {error}",
                path.display()
            );
            return None;
        }
    };
    match serde_json::from_str(&text) {
        Ok(cached) => Some(cached),
        Err(error) => {
            eprintln!(
                "desk: ignoring the picture cache at {}: {error}",
                path.display()
            );
            None
        }
    }
}

/// **Write the cache atomically**: a temp file in the same directory, fsynced, then renamed over the old
/// one. A crash leaves either the old picture or the new one, never half of either. Mode 0600: the picture
/// is not a secret, but it is this desk's record and nothing else on the machine has a reason to write it.
pub fn save_to(dir: &Path, positions: &[FeedPosition], read_at_ms: i64) -> std::io::Result<()> {
    fs::create_dir_all(dir)?;
    let target = path_in(dir);
    let temp = target.with_extension("new");
    let cached = Cached {
        read_at_ms,
        positions: positions.to_vec(),
    };
    {
        let mut options = OpenOptions::new();
        options.write(true).create(true).truncate(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        let mut file = options.open(&temp)?;
        serde_json::to_writer(&mut file, &cached).map_err(std::io::Error::other)?;
        file.write_all(b"\n")?;
        file.sync_all()?;
    }
    fs::rename(&temp, &target)
}

/// The cache in this installation's config directory.
pub fn load() -> Option<Cached> {
    identity::config_dir().and_then(|dir| load_from(&dir))
}

/// Write the cache to this installation's config directory.
pub fn save(positions: &[FeedPosition], read_at_ms: i64) -> std::io::Result<()> {
    let dir = identity::config_dir().ok_or_else(|| {
        std::io::Error::other("no config directory (set XDG_CONFIG_HOME or HOME)")
    })?;
    save_to(&dir, positions, read_at_ms)
}

/// The read's timestamp, as epoch milliseconds. The clock is the OS's; the desk owns no clock of its own.
pub fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as i64)
        .unwrap_or(0)
}

/// **One read of the standing picture, on the caller's worker.**
///
/// Signs in with the deployment's credential, `GET /live-feed/positions`, and writes the cache. A cache
/// write that fails does **not** discard the picture the operator can already see; it is one line in the
/// log, and the next successful read tries again.
pub fn read() -> Result<Picture, Failure> {
    let mut client = crate::http::Client::connect()?;
    let positions = client.positions()?;
    let read_at_ms = now_ms();
    if let Err(error) = save(&positions, read_at_ms) {
        eprintln!("desk: the standing picture was read but not cached: {error}");
    }
    Ok(Picture {
        read_at_ms,
        positions,
    })
}

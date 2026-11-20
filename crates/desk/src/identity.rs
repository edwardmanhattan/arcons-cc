//! **The installation's identity** — a UUIDv4 minted on first run (ADR-0009, decision 2).
//!
//! Minos's `POST /live-feed/fixes` carries a `device_id`, which the contract describes as "the handset".
//! A desk is not a handset, so it sends the identity of *this installation* — and that identity must say
//! nothing else about the machine. It is not derived from the hostname, the MAC address or the login name:
//! a value computed from those would name the machine and its user to Minos, and `INV-08`'s rule that no
//! host, username or secret leaves this process is not kept by a field that merely looks like an identifier.
//!
//! The id is **not a secret** — it travels on the wire by design — so `--self-check` prints it and
//! `--new-device-id` replaces it deliberately. That matters on a cloned disk image: four machines copied
//! from one image carry one config directory, and four workstations reporting under one id would make the
//! audit trail name the wrong one with nothing on screen to show it.

use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

/// The file inside the client's config directory that holds the id.
pub const FILE: &str = "desk-device-id";

/// The directory both binaries keep their configuration in, beside the wall's.
pub fn config_dir() -> Option<PathBuf> {
    if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
        if !xdg.is_empty() {
            return Some(PathBuf::from(xdg).join("arcons-cc"));
        }
    }
    std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config").join("arcons-cc"))
}

#[derive(Debug)]
pub enum IdError {
    /// The OS refused to give us 16 bytes. There is no degraded mode: an id guessed from a clock is an id
    /// two machines can share.
    NoSourceOfChance,
    /// A file that is not a version-4 UUID. Refused rather than replaced, so a corrupted file is visible.
    Shape(String),
    Io(std::io::Error),
}

impl fmt::Display for IdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSourceOfChance => f.write_str("the OS gave no source of chance"),
            Self::Shape(text) => write!(f, "not a version-4 UUID: {text:?}"),
            Self::Io(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for IdError {}

impl From<std::io::Error> for IdError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

/// A version-4 UUID, kept in its canonical hyphenated form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceId(String);

impl DeviceId {
    /// 16 bytes from the OS, with the version and variant bits set. Not a hash of anything.
    pub fn mint() -> Result<Self, IdError> {
        let mut bytes = [0u8; 16];
        getrandom::fill(&mut bytes).map_err(|_| IdError::NoSourceOfChance)?;
        bytes[6] = (bytes[6] & 0x0f) | 0x40; // version 4
        bytes[8] = (bytes[8] & 0x3f) | 0x80; // RFC 4122 variant
        Ok(Self(format_uuid(&bytes)))
    }

    /// Accept only what we would have written: 36 characters, hyphens where a UUID has them, a version-4
    /// nibble and an RFC 4122 variant. A v1 or v7 id is a different kind of identifier and is refused.
    pub fn parse(text: &str) -> Result<Self, IdError> {
        let text = text.trim();
        let bytes = text.as_bytes();
        let ok = bytes.len() == 36
            && bytes[8] == b'-'
            && bytes[13] == b'-'
            && bytes[18] == b'-'
            && bytes[23] == b'-'
            && bytes[14] == b'4'
            && matches!(bytes[19].to_ascii_lowercase(), b'8' | b'9' | b'a' | b'b')
            && bytes
                .iter()
                .enumerate()
                .all(|(i, b)| matches!(i, 8 | 13 | 18 | 23) || b.is_ascii_hexdigit());
        if !ok {
            return Err(IdError::Shape(text.to_string()));
        }
        Ok(Self(text.to_ascii_lowercase()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    pub fn path(dir: &Path) -> PathBuf {
        dir.join(FILE)
    }

    /// Read the id this installation already has.
    pub fn load(dir: &Path) -> Result<Self, IdError> {
        let text = fs::read_to_string(Self::path(dir))?;
        Self::parse(&text)
    }

    /// The id this installation will use: minted and stored on the first run, read on every run after.
    pub fn load_or_mint(dir: &Path) -> Result<Self, IdError> {
        match Self::load(dir) {
            Ok(id) => Ok(id),
            Err(IdError::Io(error)) if error.kind() == std::io::ErrorKind::NotFound => {
                let id = Self::mint()?;
                id.save(dir)?;
                Ok(id)
            }
            Err(other) => Err(other),
        }
    }

    /// Write the id at mode 0600: it is not a secret, but it is this installation's name in an audit trail,
    /// and nothing else on the machine has a reason to change it.
    pub fn save(&self, dir: &Path) -> Result<(), IdError> {
        fs::create_dir_all(dir)?;
        let target = Self::path(dir);
        let temp = target.with_extension("new");
        {
            let mut options = OpenOptions::new();
            options.write(true).create(true).truncate(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut file = options.open(&temp)?;
            file.write_all(self.0.as_bytes())?;
            file.write_all(b"\n")?;
            file.sync_all()?;
        }
        fs::rename(&temp, &target)?;
        Ok(())
    }

    /// Mint a new id and store it, replacing the old one.
    pub fn rotate(dir: &Path) -> Result<Self, IdError> {
        let id = Self::mint()?;
        id.save(dir)?;
        Ok(id)
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn format_uuid(bytes: &[u8; 16]) -> String {
    let mut out = String::with_capacity(36);
    for (i, byte) in bytes.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            out.push('-');
        }
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

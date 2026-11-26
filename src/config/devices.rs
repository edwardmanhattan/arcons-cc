//! `devices.toml` — **the phones that may drive the wall** (#194) — as opposed to the arrangement
//! (`layout.toml`), the preferences (`settings.toml`) and the credential (`credentials.toml`).
//!
//! One row per paired device: the name it gave, **the SHA-256 of its key**, and when it paired. The key
//! itself is never in this file, never in the tree and never in a log line (`INV-08`): a leaked
//! `devices.toml` is a list of names, not a set of keys, and the only copy of a key is the one the phone
//! took away at `pair.claim`.
//!
//! This module is the **file and its arithmetic**: which keys exist, what a usable row is, the fresh key
//! and its hash. It knows nothing about `State`: `ui::devices` is what turns a claim into a row, and
//! `ui::pairing` is the rules a code has to pass.
//!
//! Three rules, each of them `layout.toml`'s:
//!
//! - **Unreadable is not fatal.** A missing or corrupt file is a log line and a wall nobody has paired with.
//! - **Nothing is invented.** A row with no name, a key that is not a hex SHA-256, or two devices with one
//!   name is a file this wall refuses rather than guesses at.
//! - **Written atomically and privately** — [`super::write_atomically`], the same rename `layout.toml` uses.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The format's version, as written. Read is not gated on it: a file from a newer wall is read for what
/// this one understands, because refusing to pair is worse than ignoring a key from tomorrow.
pub const VERSION: u32 = 1;

/// A key's length: 32 bytes. Fresh at a claim, never written here — only its hash is.
pub const KEY_LEN: usize = 32;

/// A SHA-256, as lowercase hex: what a row's `key` field holds.
pub const HASH_CHARS: usize = 64;

/// What the display remembers about the devices allowed to drive it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Devices {
    /// [`VERSION`], as written.
    pub version: u32,
    /// The paired devices, in the order they paired.
    pub device: Vec<Device>,
}

/// One paired phone.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Device {
    /// The name the phone gave itself: what `pair.forget` names, and what a cursor's `origin` says.
    pub name: String,
    /// **The SHA-256 of the key, lowercase hex** — never the key. A leak of this file is a list of names.
    pub key: String,
    /// When it paired, as RFC 3339 in UTC. A fact for an operator reading the file, not a decision.
    pub paired_at: String,
}

impl Devices {
    /// Read the file, and say what happened in one line for the log — `layout.toml`'s contract.
    ///
    /// Never fails. `None` means "nobody is paired", which is a wall that keeps drawing; an absent file is
    /// that with no complaint, an unusable one is that with a sentence naming the file and the reason.
    pub fn read(path: &Path) -> (Option<Devices>, String) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (None, format!("devices: no {}", path.display()));
            }
            Err(error) => {
                return (
                    None,
                    format!("devices: {} is unreadable ({error})", path.display()),
                );
            }
        };
        match Devices::parse(&text) {
            Ok(devices) => (Some(devices), format!("devices: read {}", path.display())),
            Err(error) => (
                None,
                format!("devices: {} is not usable ({error})", path.display()),
            ),
        }
    }

    /// Parse and validate: every row a name, a hash and a time, and no name twice.
    pub fn parse(text: &str) -> Result<Devices, String> {
        let mut devices: Devices = toml::from_str(text).map_err(|error| error.to_string())?;
        devices.version = VERSION;
        for device in &mut devices.device {
            device.name = device.name.trim().to_string();
            if device.name.is_empty() {
                return Err("a device has no name".to_string());
            }
            if !is_hash(&device.key) {
                return Err(format!("{}'s key is not a SHA-256 hash", device.name));
            }
            device.key.make_ascii_lowercase();
        }
        for (index, device) in devices.device.iter().enumerate() {
            if devices.device[..index]
                .iter()
                .any(|earlier| earlier.name == device.name)
            {
                return Err(format!("two devices are named {}", device.name));
            }
        }
        Ok(devices)
    }

    /// Write the file atomically, and privately — `layout.toml`'s way, for `layout.toml`'s reason.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let text = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        super::write_atomically(path, &text)
    }

    /// Whether this name is taken. The comparison is on the trimmed name, which is what `parse` stores.
    pub fn has(&self, name: &str) -> bool {
        let name = name.trim();
        self.device.iter().any(|device| device.name == name)
    }

    /// Append a row. A name may not be taken — [`Devices::has`] is the rule `pair.claim` asks first.
    pub fn append(&mut self, device: Device) {
        self.version = VERSION;
        self.device.push(device);
    }

    /// Remove a device by name. `false` when there was no such row: forgetting nothing is a no-op, not an
    /// error, because a revocation that is a deletion has nothing to refuse.
    pub fn forget(&mut self, name: &str) -> bool {
        let name = name.trim();
        let before = self.device.len();
        self.device.retain(|device| device.name != name);
        self.device.len() != before
    }

    /// The names, in pairing order — what an operator reads off a surface that lists the rows.
    pub fn names(&self) -> Vec<&str> {
        self.device
            .iter()
            .map(|device| device.name.as_str())
            .collect()
    }
}

/// **A device's key**: 32 bytes made fresh at a claim, handed to the phone once, and never stored.
///
/// `Debug` is written by hand, so formatting a whole answer or a state is safe by construction (`INV-08`) —
/// the same decision [`crate::minos::Secret`] makes, for the same reason.
#[derive(Clone, PartialEq, Eq)]
pub struct Key([u8; KEY_LEN]);

impl Key {
    /// 32 bytes from the operating system's source of chance.
    pub fn fresh() -> Result<Key, String> {
        let mut bytes = [0_u8; KEY_LEN];
        getrandom::fill(&mut bytes).map_err(|error| error.to_string())?;
        Ok(Key(bytes))
    }

    /// **What the file keeps**: the SHA-256 of the key, lowercase hex.
    pub fn hash(&self) -> String {
        let digest = Sha256::digest(self.0);
        digest.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// **What the phone keeps**, and what crosses the carrier once: the key itself, lowercase hex.
    ///
    /// Named `expose`'s cousin rather than `to_string`, so every use reads like a handover.
    pub fn hex(&self) -> String {
        self.0.iter().map(|byte| format!("{byte:02x}")).collect()
    }

    /// A key read back from the wire, or `None` when the text is not 64 hex characters.
    pub fn from_hex(text: &str) -> Option<Key> {
        if text.len() != KEY_LEN * 2 || !text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return None;
        }
        let mut bytes = [0_u8; KEY_LEN];
        for (index, byte) in bytes.iter_mut().enumerate() {
            *byte = u8::from_str_radix(&text[index * 2..index * 2 + 2], 16).ok()?;
        }
        Some(Key(bytes))
    }
}

impl std::fmt::Debug for Key {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("<redacted>")
    }
}

/// Whether `text` is a SHA-256 as the file writes one: 64 hex characters, either case.
pub fn is_hash(text: &str) -> bool {
    text.len() == HASH_CHARS && text.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// `$XDG_CONFIG_HOME/arcons-cc/devices.toml`, a sibling of `layout.toml` and `settings.toml`.
pub fn devices_file() -> PathBuf {
    super::config_dir().join("devices.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("arcons-devices-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    fn phone(name: &str) -> Device {
        let key = Key::fresh().expect("a source of chance");
        Device {
            name: name.to_string(),
            key: key.hash(),
            paired_at: "2026-01-01T00:00:00Z".to_string(),
        }
    }

    /// **What the wall writes is what it reads**, mode 0600 and a rename — `layout.toml`'s contract for the
    /// sibling file (#194).
    #[test]
    fn what_the_wall_writes_is_what_it_reads() {
        let dir = temp_dir("round-trip");
        let path = dir.join("devices.toml");
        let mut written = Devices::default();
        written.append(phone("phone"));
        written.write(&path).expect("the file writes");
        let (read, note) = Devices::read(&path);
        let read = read.expect("a paired wall");
        assert_eq!(read, written, "{note}");
        assert_eq!(
            read.names(),
            vec!["phone"],
            "a row is a name, a hash and when"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path)
                .expect("the file exists")
                .permissions()
                .mode();
            assert_eq!(
                mode & 0o777,
                0o600,
                "devices.toml is private: a hash is not a key, but a list of who may drive the wall is \
                 nobody else's business (INV-08)"
            );
        }
        assert!(!dir.join("devices.toml.tmp").exists(), "the write renamed");
    }

    /// **A key is 32 bytes and its hash is the SHA-256**, lowercase hex; the key itself is not in the row
    /// (`INV-08`: a leaked file is a list of names, not a set of keys).
    #[test]
    fn a_row_holds_a_hash_and_never_the_key() {
        let key = Key::fresh().expect("a source of chance");
        let hex = key.hex();
        assert_eq!(hex.len(), KEY_LEN * 2, "a key is 32 bytes of hex");
        assert_eq!(
            Key::from_hex(&hex),
            Some(key.clone()),
            "the hex the client is given is the key the wall hashed"
        );
        let hash = key.hash();
        assert!(is_hash(&hash), "the row's key is a SHA-256, got {hash:?}");
        assert_ne!(hash, hex, "a row that holds the key is a file of keys");
        assert!(
            !format!("{key:?}").contains(&hex),
            "a key that prints is a key in a log line (INV-08)"
        );
        assert!(format!("{key:?}").contains("<redacted>"));
    }

    /// **Nothing is invented**: no name, a key that is not a hash, a key that is the key, and one name twice
    /// are each a file this wall refuses rather than guesses at.
    #[test]
    fn an_unusable_file_is_no_devices_and_a_sentence() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).expect("the temp dir");
        let path = dir.join("devices.toml");
        let good = Key::fresh().expect("a source of chance").hash();
        for text in [
            "version = 1\n[[device]]\nkey = \"abc\"\npaired_at = \"now\"".to_string(),
            format!(
                "[[device]]\nname = \"phone\"\nkey = \"{}\"\npaired_at = \"now\"",
                "z".repeat(64)
            ),
            format!(
                "[[device]]\nname = \"phone\"\nkey = \"{good}\"\npaired_at = \"now\"\nhost = \"minos\""
            ),
            format!(
                "[[device]]\nname = \"phone\"\nkey = \"{good}\"\npaired_at = \"now\"\n\
                 [[device]]\nname = \"phone\"\nkey = \"{good}\"\npaired_at = \"now\""
            ),
        ] {
            std::fs::write(&path, &text).expect("the file writes");
            let (devices, note) = Devices::read(&path);
            assert!(devices.is_none(), "{text:?} parsed");
            assert!(note.contains("not usable"), "{text:?}: {note}");
        }
        let (devices, note) = Devices::read(&dir.join("absent.toml"));
        assert!(devices.is_none());
        assert!(note.contains("no "), "{note}");
    }

    /// **An unknown key is not a device**: `deny_unknown_fields` is the rule, so a hand-written file cannot
    /// smuggle a key or a token in beside a name (`INV-08`).
    #[test]
    fn a_key_that_is_not_a_device_field_is_refused() {
        for text in [
            "token = \"abc\"",
            "[[device]]\nname = \"phone\"\nkey = \"abc\"\npaired_at = \"now\"\nsecret = \"x\"",
        ] {
            assert!(
                Devices::parse(text).is_err(),
                "{text:?} was accepted into devices.toml"
            );
        }
    }

    /// A device is forgotten by name, and forgetting one that is not there changes nothing — revocation is
    /// a deletion, not a rule to refuse.
    #[test]
    fn forget_removes_the_row_and_only_that_row() {
        let mut devices = Devices::default();
        devices.append(phone("phone"));
        devices.append(phone("tablet"));
        assert!(devices.has("phone"));
        assert!(devices.forget("phone"), "the row goes");
        assert!(!devices.has("phone"));
        assert_eq!(devices.names(), vec!["tablet"], "the other row stays");
        assert!(!devices.forget("phone"), "forgetting nothing is a no-op");
    }
}

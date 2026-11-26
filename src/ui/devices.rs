//! **The paired devices, as the wall's model** (#194): the row a claim appends, and where it is written.
//!
//! `config::devices` owns the file — which keys exist, what a usable row is, the fresh key and its hash. This
//! module owns the two things that are about a *wall* rather than a file:
//!
//! - **The write is off the UI thread** (`INV-05`). A claim already generates and hashes a key on the frame that
//!   answers it — pure and cheap — and hands the text to a thread for the syscall, exactly as `ui::saved` does
//!   for `layout.toml`. A `fsync` on the thread that draws the sea is a stutter a phone does not get to cause.
//! - **A capture never reads and never writes it** (`INV-11`). `devices.toml` is a sibling of `layout.toml`, so it
//!   takes the sibling's rule: a scene neither depends on the machine it was rendered on nor writes to it.
//!
//! The device key itself is not here. It is made at the claim, handed to the carrier once, and never stored
//! (`INV-08`); what a wall keeps is a list of names and hashes, which is what a `devices.toml` leak is worth.

use std::path::{Path, PathBuf};

use super::State;
use crate::config::devices::{self, Devices};

/// Where the wall's devices are remembered: `$XDG_CONFIG_HOME/arcons-cc/devices.toml`, beside the layout.
pub fn saved_devices_path() -> PathBuf {
    devices::devices_file()
}

impl State {
    /// **Restore the paired devices**, for a wall. A capture reads nothing, the layout file's rule (`INV-11`).
    pub fn with_saved_devices(self) -> State {
        let path = saved_devices_path();
        self.with_saved_devices_at(&path)
    }

    /// The same read, at a path the caller names — so a test can prove it against a file it owns, the way
    /// [`State::flush_layout_at`] does.
    pub fn with_saved_devices_at(mut self, path: &Path) -> State {
        if self.headless || rune::is_headless() {
            return self;
        }
        let (saved, note) = Devices::read(path);
        tracing::info!("{note}");
        if let Some(saved) = saved {
            self.devices = saved;
        }
        self
    }

    /// **The devices that may drive this wall** — names and hashes, never a key.
    pub fn devices(&self) -> &Devices {
        &self.devices
    }

    /// Write `devices.toml` now, **off the UI thread** (`INV-05`), the way the arrangement is written.
    ///
    /// Serialising happens here — it is pure and it is what proves the file can be built — and the syscall
    /// happens on a thread. A power cut mid-write comes back with the previous file: the writer renames a temp
    /// file into place (`config::write_atomically`).
    pub fn save_devices(&self) {
        // The capture rule, checked at the door as well as at the only two callers: a scene that claimed a device
        // must not write this machine's file (`INV-11`).
        if self.headless {
            return;
        }
        let saved = self.devices.clone();
        let path = self.devices_path.clone();
        std::thread::spawn(move || match saved.write(&path) {
            Ok(()) => tracing::debug!("devices: wrote {}", path.display()),
            Err(error) => tracing::warn!("devices: {error}"),
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::pairing as pair;
    use crate::verb::Intent;

    /// A directory this test owns, so no run touches the operator's config directory.
    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("arcons-ui-devices-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    /// A wall with a code on it, and the digits a phone would type.
    fn opened(path: &Path) -> (State, String) {
        let mut state = State {
            devices_path: path.to_path_buf(),
            ..State::default()
        };
        state.apply(Intent::PairBegin).expect("pairing opens");
        let digits = state.pairing().expect("a code on the wall").digits();
        (state, digits)
    }

    /// Wait for the thread a claim started to have written the file — bounded, so a missing write is a failing
    /// test with a sentence rather than a hang.
    ///
    /// Ten seconds rather than one: this directory is the build's `target/tmp`, and `sync_all` there competes with
    /// every other test in the run. The bound is what makes a missing write a failure; its width is not the claim.
    fn wait_for(path: &Path) -> String {
        for _ in 0..10_000 {
            if let Ok(text) = std::fs::read_to_string(path) {
                return text;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!(
            "devices.toml was never written at {}: a claim writes on a thread (INV-05), and this thread never \
             finished",
            path.display()
        );
    }

    /// **A claim writes the file atomically, with a hash in it and no key** (#194): a row appears, the mode is
    /// 0600, the rename happened, and the key the client was handed is nowhere in the text (`INV-08`).
    #[test]
    fn a_claim_writes_a_hashed_row_atomically() {
        let dir = temp_dir("claim");
        let path = dir.join("devices.toml");
        let (mut state, digits) = opened(&path);
        let paired = pair::claim(&mut state, &digits, "phone").expect("the code pairs");
        let text = wait_for(&path);

        let read = Devices::parse(&text).expect("the file the wall wrote is a devices file");
        assert_eq!(read.names(), vec!["phone"], "the claim appended one row");
        assert_eq!(
            read.device[0].key,
            paired.key.hash(),
            "the row is the SHA-256 of the key, and nothing else"
        );
        assert!(
            !text.contains(&paired.key.hex()),
            "INV-08: devices.toml holds the key, not its hash: {text}"
        );
        assert!(
            !dir.join("devices.toml.tmp").exists(),
            "the write renamed a temp file into place, as layout.toml does"
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
                "devices.toml is 0600, the mode every file in this directory has (INV-08)"
            );
        }
        assert_eq!(
            state.devices().names(),
            vec!["phone"],
            "and the model has the row the file has"
        );
    }

    /// **A wall comes back knowing its devices, and a capture never looks** (`INV-11`, the layout rule):
    /// `with_saved_devices_at` reads a file a wall owns, and a headless state is the default whatever the file says.
    #[test]
    fn a_wall_reads_its_devices_and_a_capture_reads_none() {
        let dir = temp_dir("read");
        let path = dir.join("devices.toml");
        let mut saved = Devices::default();
        let key = devices::Key::fresh().expect("a source of chance");
        saved.append(devices::Device {
            name: "phone".to_string(),
            key: key.hash(),
            paired_at: "2026-01-01T00:00:00Z".to_string(),
        });
        saved.write(&path).expect("the file writes");

        let wall = State::default().with_saved_devices_at(&path);
        assert_eq!(wall.devices().names(), vec!["phone"]);

        let capture = State {
            headless: true,
            ..State::default()
        }
        .with_saved_devices_at(&path);
        assert!(
            capture.devices().names().is_empty(),
            "INV-11: a capture reads no devices.toml, so a golden cannot depend on this machine"
        );
    }

    /// **A capture writes nothing**, even if a scene claimed a device: `save_devices` is the capture rule at the
    /// door, and `devices.toml` stays absent.
    #[test]
    fn a_capture_never_writes_the_file() {
        let dir = temp_dir("capture");
        let path = dir.join("devices.toml");
        let (mut state, digits) = opened(&path);
        state.headless = true;
        pair::claim(&mut state, &digits, "phone").expect("the code pairs in memory");
        state.save_devices();
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert!(
            !path.exists(),
            "INV-11: a headless run wrote devices.toml at {}",
            path.display()
        );
    }
}

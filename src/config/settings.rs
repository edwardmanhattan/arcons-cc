//! `settings.toml` — what somebody clicked on the wall, as opposed to what a deployment configured.
//!
//! **Two files, two owners** (#82). `config.toml` is the operator's: it names the server, the channel, the
//! wall's start point, and the display only ever *reads* it. This file is the display's own: it holds the
//! preferences a person changes at 2 a.m. with a mouse, and the wall writes it. Keeping them apart is what
//! stops "somebody toggled the gaps" from being a change to a deployment's configuration — and it is the
//! file a saved layout will need later (#83, #85).
//!
//! Three rules follow from that, and they are not preferences:
//!
//! - **Written atomically** (a temp file and a rename in the same directory): a wall that loses power
//!   mid-write must come back with the previous settings, not half a file.
//! - **Mode 0600**, like the credential file, because a wall's config directory is where the credential
//!   lives and a file everyone can read there is a habit that will write a secret into one eventually.
//!   Nothing secret is in here; nothing secret may be added.
//! - **Unreadable is not fatal**: a missing, corrupt or hand-edited file is a log line and the defaults.
//!   The wall's job is to draw the sea, and it cannot be made to refuse that by a typo in a preference.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The widest gap the display offers, in points. Above this the panes stop reading as one picture.
pub const GAP_MAX: f32 = 24.0;

/// What the display itself may change, and what it remembers between runs.
///
/// Every field is `#[serde(default)]` so that a file written by an older version — or an operator's
/// hand-written one with one key — is a settings file, not an error.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields, default)]
pub struct Settings {
    /// The space between panes, in points. `0` is panes that share a border, which is the look the wall
    /// had before this file existed — so a wall that has never opened its settings draws exactly the
    /// picture it drew yesterday.
    pub pane_gap: f32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings { pane_gap: 0.0 }
    }
}

impl Settings {
    /// Read the file, and say what happened in one line for the log.
    ///
    /// Never fails: an absent file is the defaults with no complaint (most walls will never have one), and
    /// an unusable one is the defaults with a sentence that names the file and the reason.
    pub fn read(path: &Path) -> (Settings, String) {
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return (
                    Settings::default(),
                    format!("settings: no {}, using the defaults", path.display()),
                );
            }
            Err(error) => {
                return (
                    Settings::default(),
                    format!(
                        "settings: {} is unreadable ({error}), using the defaults",
                        path.display()
                    ),
                );
            }
        };
        match Settings::parse(&text) {
            Ok(settings) => (settings, format!("settings: read {}", path.display())),
            Err(error) => (
                Settings::default(),
                format!(
                    "settings: {} is not usable ({error}), using the defaults",
                    path.display()
                ),
            ),
        }
    }

    /// Parse and clamp. A hand-edited `pane_gap = 400` is a wall with no picture, so it is clamped and
    /// reported rather than honoured — the value in memory is always one the layout can draw.
    pub fn parse(text: &str) -> Result<Settings, String> {
        let mut settings: Settings = toml::from_str(text).map_err(|error| error.to_string())?;
        settings.pane_gap = settings.pane_gap.clamp(0.0, GAP_MAX);
        Ok(settings)
    }

    /// Write the file atomically, and privately.
    pub fn write(&self, path: &Path) -> Result<(), String> {
        let directory = path
            .parent()
            .ok_or_else(|| format!("{} has no directory", path.display()))?;
        std::fs::create_dir_all(directory)
            .map_err(|error| format!("{}: {error}", directory.display()))?;
        // A temp file **in the same directory**, so the rename is a rename and not a copy across
        // filesystems: `settings.toml.tmp` next to `settings.toml` is atomic; `/tmp/...` is not.
        let temp = path.with_extension("toml.tmp");
        let text = toml::to_string_pretty(self).map_err(|error| error.to_string())?;
        let mut file =
            std::fs::File::create(&temp).map_err(|error| format!("{}: {error}", temp.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            // 0600 **before** the content is in it: a window in which the file exists, readable and
            // empty, is shorter than one where it is readable and full, but it is free to close.
            if let Err(error) = file.set_permissions(std::fs::Permissions::from_mode(0o600)) {
                let _ = std::fs::remove_file(&temp);
                return Err(format!("{}: {error}", temp.display()));
            }
        }
        if let Err(error) = file
            .write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
        {
            let _ = std::fs::remove_file(&temp);
            return Err(format!("{}: {error}", temp.display()));
        }
        drop(file);
        std::fs::rename(&temp, path).map_err(|error| {
            let _ = std::fs::remove_file(&temp);
            format!("{}: {error}", path.display())
        })
    }
}

/// `$XDG_CONFIG_HOME/arcons-cc/settings.toml`, beside the configuration and the credential.
pub fn settings_file() -> PathBuf {
    super::config_dir().join("settings.toml")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("arcons-settings-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        path
    }

    /// #82: a wall that has never had its settings changed draws what it drew before — the default gap is
    /// zero, which is panes that share a border.
    #[test]
    fn the_defaults_are_the_look_the_wall_already_had() {
        assert_eq!(Settings::default().pane_gap, 0.0);
        let (settings, note) = Settings::read(&temp_dir("absent").join("settings.toml"));
        assert_eq!(settings, Settings::default());
        assert!(note.contains("no "), "{note}");
    }

    /// The file round-trips, and a wall that wrote it reads it back.
    #[test]
    fn what_the_wall_writes_is_what_it_reads() {
        let dir = temp_dir("round-trip");
        let path = dir.join("settings.toml");
        let written = Settings { pane_gap: 12.0 };
        written.write(&path).expect("the file writes");
        let (read, note) = Settings::read(&path);
        assert_eq!(read, written, "{note}");
        assert_eq!(read.pane_gap, 12.0);
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
                "a settings file is private, like the credential"
            );
        }
        // No temp file is left behind: the write is a rename, and a wall that crashed mid-write would
        // leave one, so its absence is the evidence the write went through the rename.
        assert!(
            !dir.join("settings.toml.tmp").exists(),
            "the temp file was renamed"
        );
    }

    /// A corrupt file is a log line and the defaults — never a wall that refuses to draw.
    #[test]
    fn a_corrupt_file_is_the_defaults_and_a_sentence() {
        let dir = temp_dir("corrupt");
        std::fs::create_dir_all(&dir).expect("the temp dir");
        let path = dir.join("settings.toml");
        for text in [
            "pane_gap = \"eight\"".to_string(),
            "pane_gap = ".to_string(),
            "panes_gap = 8.0".to_string(), // a typo, refused rather than silently ignored
            "\u{0}\u{1}\u{2}".to_string(),
        ] {
            std::fs::write(&path, &text).expect("the file writes");
            let (settings, note) = Settings::read(&path);
            assert_eq!(settings, Settings::default(), "{text:?} parsed");
            assert!(note.contains("not usable"), "{text:?}: {note}");
        }
    }

    /// A value the layout cannot draw is clamped, not honoured: `pane_gap = 400` is a wall with no picture.
    #[test]
    fn a_gap_the_layout_cannot_draw_is_clamped() {
        assert_eq!(
            Settings::parse("pane_gap = 400.0").unwrap().pane_gap,
            GAP_MAX
        );
        assert_eq!(Settings::parse("pane_gap = -3.0").unwrap().pane_gap, 0.0);
        assert_eq!(Settings::parse("").unwrap(), Settings::default());
    }
}

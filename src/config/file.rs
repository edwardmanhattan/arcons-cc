//! `config.toml` — the part of the configuration that is not a secret.
//!
//! Every field is optional: a file that says one thing is not a file that must say everything, and
//! a missing field is filled from the environment or the default. **Unknown keys are refused**: a
//! typo (`map_cetner`) that silently falls back to the default is a wall showing the wrong ocean
//! with a configuration that looks correct.
//!
//! This file never holds a credential. It holds the *path* of one (`credentials`), which is why it
//! may be readable by anyone who can read the directory it lives in.

use std::path::{Path, PathBuf};

use crate::map;

/// What a config file may say. `None` means "not set here", never "set to nothing".
#[derive(Debug, Default, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct File {
    /// The REST base, including its path prefix (`…/api/v1`).
    pub minos_url: Option<String>,
    /// The Centrifugo endpoint, when it is not derivable from `minos_url`.
    pub centrifugo_url: Option<String>,
    /// The credential file, relative to this file's directory (or absolute).
    pub credentials: Option<String>,
    /// Where the wall starts looking: `[lat, lon]` in degrees.
    pub map_center: Option<[f64; 2]>,
    /// 0–22, in the projection's own units.
    pub map_zoom: Option<f64>,
    /// A fix older than this draws as stale. Seconds, above zero.
    pub stale_after_s: Option<f64>,
}

impl File {
    /// Parse and check. A message says what is wrong *and* which key it is about, because a wall
    /// operator reads one line and not a backtrace.
    pub fn parse(text: &str) -> Result<File, String> {
        let file: File = toml::from_str(text).map_err(|error| error.to_string())?;
        file.check()?;
        Ok(file)
    }

    fn check(&self) -> Result<(), String> {
        if let Some(url) = &self.minos_url
            && url.trim().is_empty()
        {
            return Err("minos_url is set but empty".into());
        }
        if let Some(url) = &self.centrifugo_url
            && !(url.starts_with("ws://") || url.starts_with("wss://"))
        {
            // A REST URL here is a socket that never connects, and the error it gives names nothing.
            return Err(format!(
                "centrifugo_url must be ws:// or wss:// (got {url:?})"
            ));
        }
        if let Some(credentials) = &self.credentials
            && credentials.trim().is_empty()
        {
            return Err("credentials is set but empty".into());
        }
        if let Some([lat, lon]) = self.map_center
            && !(lat.is_finite()
                && lon.is_finite()
                && (-90.0..=90.0).contains(&lat)
                && (-180.0..=180.0).contains(&lon))
        {
            return Err(format!(
                "map_center is [lat, lon] in degrees (got [{lat}, {lon}])"
            ));
        }
        if let Some(zoom) = self.map_zoom
            && !zoom.is_finite()
        {
            return Err("map_zoom must be a number".into());
        }
        if let Some(seconds) = self.stale_after_s
            && !(seconds.is_finite() && seconds > 0.0)
        {
            // Zero would draw every fix as stale the instant it arrives; negative is nonsense.
            return Err(format!("stale_after_s must be above zero (got {seconds})"));
        }
        Ok(())
    }

    /// Read the file at `path`. Absent is not an error: the environment alone is a valid setup.
    pub fn read(path: &Path) -> Result<File, String> {
        match std::fs::read_to_string(path) {
            Ok(text) => File::parse(&text).map_err(|error| format!("{}: {error}", path.display())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(File::default()),
            Err(error) => Err(format!("{}: {error}", path.display())),
        }
    }

    /// The credential file this configuration names, resolved against the config file's directory.
    pub fn credential_path(&self, config_path: &Path) -> Option<PathBuf> {
        let named = self.credentials.as_ref()?;
        let path = Path::new(named);
        Some(if path.is_absolute() {
            path.to_path_buf()
        } else {
            config_path.parent().unwrap_or(Path::new(".")).join(path)
        })
    }
}

/// The seed the map needs to draw without a network (`#5`). Named here so `--self-check` can print
/// it beside the rest of the configuration.
pub fn seed_present() -> bool {
    map::seed_present()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_file_may_say_one_thing_and_leave_the_rest_to_the_environment() {
        let file = File::parse("minos_url = \"https://api.example.test/api/v1\"\n").unwrap();
        assert_eq!(
            file.minos_url.as_deref(),
            Some("https://api.example.test/api/v1")
        );
        assert_eq!(file.map_center, None);
        assert_eq!(file.stale_after_s, None);
    }

    #[test]
    fn an_absent_file_is_an_empty_configuration_not_an_error() {
        let missing = std::env::temp_dir().join("arcons-cc-does-not-exist.toml");
        assert_eq!(File::read(&missing).unwrap(), File::default());
    }

    #[test]
    fn a_typo_is_refused_rather_than_defaulted() {
        // The failure this exists for: `map_cetner` silently draws the wrong ocean.
        let error = File::parse("map_cetner = [-6.1, 106.9]\n").unwrap_err();
        assert!(
            error.contains("map_cetner") || error.contains("unknown field"),
            "the message must name the key: {error}"
        );
    }

    #[test]
    fn each_field_is_checked_with_its_own_key_in_the_message() {
        for (text, key) in [
            ("minos_url = \"  \"\n", "minos_url"),
            (
                "centrifugo_url = \"https://x/connection\"\n",
                "centrifugo_url",
            ),
            ("credentials = \"\"\n", "credentials"),
            ("map_center = [91.0, 0.0]\n", "map_center"),
            ("map_center = [0.0, 181.0]\n", "map_center"),
            ("map_zoom = nan\n", "map_zoom"),
            ("stale_after_s = 0.0\n", "stale_after_s"),
            ("stale_after_s = -5.0\n", "stale_after_s"),
        ] {
            let error = File::parse(text).expect_err(text);
            assert!(error.contains(key), "{key:?} is not named in {error:?}");
        }
    }

    #[test]
    fn a_named_credential_resolves_beside_the_file_not_the_cwd() {
        let file = File::parse("credentials = \"wall-credentials.toml\"\n").unwrap();
        assert_eq!(
            file.credential_path(Path::new("/etc/arcons-cc/config.toml")),
            Some(PathBuf::from("/etc/arcons-cc/wall-credentials.toml"))
        );
        // An absolute path is left alone: a deployment may put the secret somewhere else entirely.
        let file = File::parse("credentials = \"/run/arcons/creds.toml\"\n").unwrap();
        assert_eq!(
            file.credential_path(Path::new("/etc/arcons-cc/config.toml")),
            Some(PathBuf::from("/run/arcons/creds.toml"))
        );
        assert_eq!(
            File::default().credential_path(Path::new("/x/config.toml")),
            None
        );
    }
}

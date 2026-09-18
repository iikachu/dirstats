// SPDX-License-Identifier: GPL-3.0-or-later
// by dirstats contributors

//! The few preferences that outlive a run, kept in a plain `key=value`
//! file under the platform's per-user configuration directory. There is
//! no dependency on a config crate: one file, a handful of keys.

use std::io;
use std::path::PathBuf;

/// Preferences that persist between runs.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Settings {
    /// Windows only: the user has acknowledged that permanent deletion
    /// skips the Recycle Bin, so front ends may offer it directly.
    pub permanent_delete: bool,
}

const PERMANENT_DELETE: &str = "permanent_delete";

impl Settings {
    /// Read the settings file, or defaults when there is none or it is unreadable.
    #[must_use]
    pub fn load() -> Self {
        Self::path().and_then(|p| std::fs::read_to_string(p).ok()).map(|text| Self::parse(&text)).unwrap_or_default()
    }

    /// Write the settings file, creating its directory.
    pub fn save(&self) -> io::Result<()> {
        let path = Self::path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "no configuration directory"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(path, self.serialise())
    }

    fn parse(text: &str) -> Self {
        let mut settings = Self::default();
        for line in text.lines() {
            let Some((key, value)) = line.split_once('=') else { continue };
            if key.trim() == PERMANENT_DELETE {
                settings.permanent_delete = value.trim() == "true";
            }
        }
        settings
    }

    fn serialise(&self) -> String {
        format!("{PERMANENT_DELETE}={}\n", self.permanent_delete)
    }

    /// `%APPDATA%\dirstats\settings` on Windows,
    /// `~/Library/Application Support/dirstats/settings` on macOS,
    /// `$XDG_CONFIG_HOME/dirstats/settings` or `~/.config/dirstats/settings` elsewhere.
    #[must_use]
    pub fn path() -> Option<PathBuf> {
        let base = if cfg!(windows) {
            PathBuf::from(std::env::var_os("APPDATA")?)
        } else if cfg!(target_os = "macos") {
            PathBuf::from(std::env::var_os("HOME")?).join("Library/Application Support")
        } else if let Some(xdg) = std::env::var_os("XDG_CONFIG_HOME") {
            PathBuf::from(xdg)
        } else {
            PathBuf::from(std::env::var_os("HOME")?).join(".config")
        };
        Some(base.join("dirstats").join("settings"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_ignores_junk() {
        let on = Settings { permanent_delete: true };
        assert_eq!(Settings::parse(&on.serialise()), on);
        assert_eq!(Settings::parse("garbage\npermanent_delete = true \nother=1"), on);
        assert_eq!(Settings::parse("permanent_delete=yes"), Settings::default());
        assert_eq!(Settings::parse(""), Settings::default());
    }
}

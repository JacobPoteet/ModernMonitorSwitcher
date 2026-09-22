/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Application settings, stored next to the profiles.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Profile name to keyboard accelerator, in the form Tauri's global
    /// shortcut plugin expects, e.g. "CmdOrControl+Alt+1".
    pub hotkeys: BTreeMap<String, String>,

    /// Check GitHub for a newer release on launch, and daily thereafter.
    pub check_for_updates: bool,

    /// Nicknames for monitors, keyed by device path, so "Left" and "Middle"
    /// can stand in for two monitors that both report the same model name.
    ///
    /// The device path encodes the EDID, so a nickname follows its monitor
    /// across reboots and between ports.
    pub monitor_names: BTreeMap<String, String>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            hotkeys: BTreeMap::new(),
            check_for_updates: true,
            monitor_names: BTreeMap::new(),
        }
    }
}

impl Settings {
    /// `%APPDATA%\ModernMonitorSwitcher\settings.json`
    pub fn default_path() -> Option<PathBuf> {
        std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .map(|base| base.join("ModernMonitorSwitcher").join("settings.json"))
    }

    /// Load settings, falling back to defaults.
    ///
    /// A missing file is normal on first run. A corrupt one is logged and
    /// replaced with defaults rather than blocking startup — losing a hotkey
    /// binding is a far smaller problem than an application that will not
    /// start.
    pub fn load(path: &Path) -> Settings {
        let Ok(text) = fs::read_to_string(path) else {
            return Settings::default();
        };
        match serde_json::from_str(&text) {
            Ok(settings) => settings,
            Err(e) => {
                tracing::warn!(path = %path.display(), error = %e, "unreadable settings; using defaults");
                Settings::default()
            }
        }
    }

    pub fn save(&self, path: &Path) -> std::io::Result<()> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self).expect("settings always serialize");
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, json)?;
        fs::rename(&tmp, path)
    }

    /// Drop hotkeys whose profile no longer exists.
    ///
    /// Without this, deleting a profile leaves a binding that silently holds a
    /// system-wide accelerator hostage.
    pub fn prune_hotkeys(&mut self, existing: &[String]) -> bool {
        let before = self.hotkeys.len();
        self.hotkeys.retain(|name, _| existing.contains(name));
        self.hotkeys.len() != before
    }

    /// Which profile, if any, is bound to this accelerator.
    pub fn profile_for_accelerator(&self, accelerator: &str) -> Option<&str> {
        self.hotkeys
            .iter()
            .find(|(_, a)| a.as_str() == accelerator)
            .map(|(name, _)| name.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("msw-settings-{tag}-{}.json", std::process::id()))
    }

    #[test]
    fn defaults_are_sensible() {
        let s = Settings::default();
        assert!(
            s.check_for_updates,
            "updating quietly is the point of this rewrite"
        );
        assert!(s.hotkeys.is_empty());
    }

    #[test]
    fn missing_file_yields_defaults() {
        let s = Settings::load(Path::new("Z:/definitely/not/here.json"));
        assert!(s.hotkeys.is_empty());
        assert!(s.check_for_updates);
    }

    #[test]
    fn corrupt_file_yields_defaults_rather_than_failing() {
        let path = temp_path("corrupt");
        fs::write(&path, "{ this is not json").unwrap();

        let s = Settings::load(&path);
        assert!(s.check_for_updates);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn round_trips_through_disk() {
        let path = temp_path("roundtrip");
        let mut s = Settings::default();
        s.hotkeys.insert("Work".into(), "CmdOrControl+Alt+1".into());
        s.check_for_updates = false;
        s.save(&path).unwrap();

        let loaded = Settings::load(&path);
        assert_eq!(loaded.hotkeys.get("Work").unwrap(), "CmdOrControl+Alt+1");
        assert!(!loaded.check_for_updates);

        fs::remove_file(&path).ok();
    }

    #[test]
    fn unknown_fields_and_partial_files_still_load() {
        let path = temp_path("partial");
        fs::write(&path, r#"{"future_option": 42}"#).unwrap();

        let s = Settings::load(&path);
        assert!(
            s.check_for_updates,
            "missing fields should fall back to the default"
        );
        assert!(s.hotkeys.is_empty());

        fs::remove_file(&path).ok();
    }

    #[test]
    fn pruning_drops_bindings_for_deleted_profiles() {
        let mut s = Settings::default();
        s.hotkeys.insert("Work".into(), "CmdOrControl+Alt+1".into());
        s.hotkeys.insert("Gone".into(), "CmdOrControl+Alt+2".into());

        assert!(s.prune_hotkeys(&["Work".to_string()]));
        assert_eq!(s.hotkeys.len(), 1);
        assert!(s.hotkeys.contains_key("Work"));

        assert!(
            !s.prune_hotkeys(&["Work".to_string()]),
            "pruning twice changes nothing"
        );
    }

    #[test]
    fn accelerators_resolve_back_to_their_profile() {
        let mut s = Settings::default();
        s.hotkeys.insert("Play".into(), "CmdOrControl+Alt+2".into());

        assert_eq!(
            s.profile_for_accelerator("CmdOrControl+Alt+2"),
            Some("Play")
        );
        assert_eq!(s.profile_for_accelerator("CmdOrControl+Alt+9"), None);
    }
}

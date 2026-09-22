/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Saved profiles and where they live on disk.

use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::ccd;
use crate::error::{Error, Result};
use crate::model::DisplayConfig;

/// On-disk format version. Bumped only for changes that older builds cannot
/// read; additive fields do not need it.
pub const FORMAT_VERSION: u32 = 1;

/// A named display configuration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,

    #[serde(default = "default_format_version")]
    pub format_version: u32,

    /// RFC 3339 timestamp, recorded when the profile was captured.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_at: Option<String>,

    /// Which application version captured it, for bug reports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub saved_by: Option<String>,

    /// Names of the monitors this profile leaves switched on. Purely
    /// descriptive — the authority is `config` — but it makes a profile file
    /// readable at a glance and gives the UI something to show.
    #[serde(default)]
    pub active_monitors: Vec<String>,

    pub config: DisplayConfig,
}

fn default_format_version() -> u32 {
    FORMAT_VERSION
}

impl Profile {
    /// Capture the machine's current display configuration under a name.
    pub fn capture(name: impl Into<String>) -> Result<Profile> {
        // AllPaths, not ActiveOnly: a profile has to describe the monitors it
        // turns *off* as well as the ones it turns on.
        let config = ccd::query(ccd::QueryScope::AllPaths)?;
        Ok(Profile::from_config(name, config))
    }

    pub fn from_config(name: impl Into<String>, config: DisplayConfig) -> Profile {
        let active_monitors = config.active_monitor_labels();
        Profile {
            name: name.into(),
            format_version: FORMAT_VERSION,
            saved_at: Some(now_rfc3339()),
            saved_by: Some(format!("msw {}", env!("CARGO_PKG_VERSION"))),
            active_monitors,
            config,
        }
    }

    /// Is this the profile currently on screen?
    ///
    /// Judged by which monitors are active, not by the exact modes, so it
    /// stays true after Windows adjusts a refresh rate.
    pub fn is_active(&self, current: &DisplayConfig) -> bool {
        self.config.has_same_active_monitors(current)
    }

    /// One-line summary for menus and CLI output.
    pub fn summary(&self) -> String {
        match self.active_monitors.len() {
            0 => "no active monitors".to_string(),
            1 => self.active_monitors[0].clone(),
            n => format!("{n} monitors: {}", self.active_monitors.join(", ")),
        }
    }
}

/// Current time as RFC 3339, without pulling in a date library.
///
/// Only ever displayed, never parsed or compared, so seconds resolution in UTC
/// is plenty.
fn now_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0) as i64;

    // Days since the epoch, then civil date via Howard Hinnant's algorithm.
    let days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400);

    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };

    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y,
        m,
        d,
        time_of_day / 3600,
        (time_of_day % 3600) / 60,
        time_of_day % 60
    )
}

/// Characters Windows refuses in a filename, plus the path separators.
const FORBIDDEN: &[char] = &['<', '>', ':', '"', '/', '\\', '|', '?', '*'];

/// Names Windows reserves regardless of extension.
const RESERVED: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

/// Is this a name we can safely turn into a filename?
pub fn is_valid_name(name: &str) -> bool {
    let trimmed = name.trim();
    if trimmed.is_empty() || trimmed.len() > 64 {
        return false;
    }
    if trimmed
        .chars()
        .any(|c| FORBIDDEN.contains(&c) || c.is_control())
    {
        return false;
    }
    // A trailing dot or space is legal to create but not to open again.
    if trimmed.ends_with('.') {
        return false;
    }
    let stem = trimmed.split('.').next().unwrap_or(trimmed);
    if RESERVED.iter().any(|r| stem.eq_ignore_ascii_case(r)) {
        return false;
    }
    true
}

/// A directory of profiles.
pub struct Store {
    dir: PathBuf,
}

impl Store {
    /// The default location: `%APPDATA%\ModernMonitorSwitcher\profiles`.
    pub fn default_location() -> Result<Store> {
        let base = std::env::var_os("APPDATA")
            .map(PathBuf::from)
            .ok_or(Error::NoDataDir)?;
        Ok(Store::at(
            base.join("ModernMonitorSwitcher").join("profiles"),
        ))
    }

    pub fn at(dir: impl Into<PathBuf>) -> Store {
        Store { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn ensure_dir(&self) -> Result<()> {
        fs::create_dir_all(&self.dir).map_err(|e| Error::io(&self.dir, e))
    }

    fn path_for(&self, name: &str) -> PathBuf {
        self.dir.join(format!("{}.json", name.trim()))
    }

    /// Every profile in the directory, sorted by name, case-insensitively.
    ///
    /// Unreadable files are logged and skipped rather than failing the whole
    /// listing — one corrupt profile should not hide the others.
    pub fn list(&self) -> Result<Vec<Profile>> {
        if !self.dir.exists() {
            return Ok(Vec::new());
        }

        let entries = fs::read_dir(&self.dir).map_err(|e| Error::io(&self.dir, e))?;
        let mut out = Vec::new();

        for entry in entries {
            let entry = entry.map_err(|e| Error::io(&self.dir, e))?;
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            match self.read_file(&path) {
                Ok(profile) => out.push(profile),
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "skipping unreadable profile")
                }
            }
        }

        out.sort_by_key(|p| p.name.to_lowercase());
        Ok(out)
    }

    /// Just the names, which is all a tray menu needs.
    pub fn list_names(&self) -> Result<Vec<String>> {
        Ok(self.list()?.into_iter().map(|p| p.name).collect())
    }

    fn read_file(&self, path: &Path) -> Result<Profile> {
        let text = fs::read_to_string(path).map_err(|e| Error::io(path, e))?;
        let mut profile: Profile =
            serde_json::from_str(&text).map_err(|source| Error::ProfileParse {
                path: path.to_path_buf(),
                source,
            })?;

        // The filename is authoritative, so a profile stays findable even if
        // someone edits the name field by hand.
        if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
            if profile.name != stem {
                profile.name = stem.to_string();
            }
        }
        Ok(profile)
    }

    pub fn load(&self, name: &str) -> Result<Profile> {
        let path = self.path_for(name);
        if !path.exists() {
            return Err(Error::NoSuchProfile(name.to_string()));
        }
        self.read_file(&path)
    }

    pub fn exists(&self, name: &str) -> bool {
        self.path_for(name).exists()
    }

    /// Write a profile, replacing any existing one of the same name.
    ///
    /// Writes to a temporary file and renames, so an interrupted save cannot
    /// leave a half-written profile behind.
    pub fn save(&self, profile: &Profile) -> Result<PathBuf> {
        if !is_valid_name(&profile.name) {
            return Err(Error::InvalidProfileName(profile.name.clone()));
        }
        self.ensure_dir()?;

        let path = self.path_for(&profile.name);
        let tmp = path.with_extension("json.tmp");

        let json = serde_json::to_string_pretty(profile).expect("a profile always serializes");
        fs::write(&tmp, json).map_err(|e| Error::io(&tmp, e))?;
        fs::rename(&tmp, &path).map_err(|e| Error::io(&path, e))?;

        Ok(path)
    }

    /// Capture the current configuration and store it under `name`.
    pub fn capture(&self, name: &str) -> Result<Profile> {
        if !is_valid_name(name) {
            return Err(Error::InvalidProfileName(name.to_string()));
        }
        let profile = Profile::capture(name.trim())?;
        self.save(&profile)?;
        Ok(profile)
    }

    pub fn delete(&self, name: &str) -> Result<()> {
        let path = self.path_for(name);
        if !path.exists() {
            return Err(Error::NoSuchProfile(name.to_string()));
        }
        fs::remove_file(&path).map_err(|e| Error::io(&path, e))
    }

    pub fn rename(&self, from: &str, to: &str) -> Result<()> {
        if !is_valid_name(to) {
            return Err(Error::InvalidProfileName(to.to_string()));
        }
        let mut profile = self.load(from)?;
        if self.exists(to) && !from.eq_ignore_ascii_case(to) {
            return Err(Error::ProfileExists(to.to_string()));
        }
        let old_path = self.path_for(from);
        profile.name = to.trim().to_string();
        self.save(&profile)?;
        if old_path != self.path_for(to) {
            fs::remove_file(&old_path).map_err(|e| Error::io(&old_path, e))?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::*;

    fn empty_config() -> DisplayConfig {
        DisplayConfig {
            paths: Vec::new(),
            modes: Vec::new(),
            monitors: Vec::new(),
        }
    }

    fn temp_store(tag: &str) -> Store {
        let dir = std::env::temp_dir().join(format!(
            "msw-test-{tag}-{}-{:?}",
            std::process::id(),
            std::thread::current().id()
        ));
        let _ = fs::remove_dir_all(&dir);
        Store::at(dir)
    }

    #[test]
    fn valid_names_are_accepted() {
        for name in ["Work", "Play", "Work 2", "3-monitor setup", "café"] {
            assert!(is_valid_name(name), "{name:?} should be valid");
        }
    }

    #[test]
    fn invalid_names_are_rejected() {
        for name in [
            "",
            "   ",
            "a/b",
            "a\\b",
            "a:b",
            "a*b",
            "a?b",
            "con",
            "CON",
            "NUL",
            "PRN.json",
            "trailing.",
            "a\u{7}b",
        ] {
            assert!(!is_valid_name(name), "{name:?} should be rejected");
        }
        assert!(!is_valid_name(&"x".repeat(65)));
    }

    #[test]
    fn save_and_load_round_trip() {
        let store = temp_store("roundtrip");
        let profile = Profile::from_config("Work", empty_config());

        store.save(&profile).unwrap();
        let loaded = store.load("Work").unwrap();

        assert_eq!(loaded.name, "Work");
        assert_eq!(loaded.config, profile.config);
        assert_eq!(loaded.format_version, FORMAT_VERSION);

        fs::remove_dir_all(store.dir()).unwrap();
    }

    #[test]
    fn listing_is_sorted_and_skips_junk() {
        let store = temp_store("listing");
        for name in ["Play", "work", "Arcade"] {
            store
                .save(&Profile::from_config(name, empty_config()))
                .unwrap();
        }
        fs::write(store.dir().join("broken.json"), "{ not json").unwrap();
        fs::write(store.dir().join("notes.txt"), "ignore me").unwrap();

        let names = store.list_names().unwrap();
        assert_eq!(names, vec!["Arcade", "Play", "work"]);

        fs::remove_dir_all(store.dir()).unwrap();
    }

    #[test]
    fn filename_wins_over_a_hand_edited_name_field() {
        let store = temp_store("filename-wins");
        store
            .save(&Profile::from_config("Work", empty_config()))
            .unwrap();

        let path = store.dir().join("Work.json");
        let text = fs::read_to_string(&path)
            .unwrap()
            .replace("\"Work\"", "\"Something Else\"");
        fs::write(&path, text).unwrap();

        assert_eq!(store.load("Work").unwrap().name, "Work");

        fs::remove_dir_all(store.dir()).unwrap();
    }

    #[test]
    fn saving_leaves_no_temporary_files_behind() {
        let store = temp_store("no-temps");
        store
            .save(&Profile::from_config("Work", empty_config()))
            .unwrap();

        let leftovers: Vec<_> = fs::read_dir(store.dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("tmp"))
            .collect();
        assert!(leftovers.is_empty());

        fs::remove_dir_all(store.dir()).unwrap();
    }

    #[test]
    fn missing_profile_is_an_error_not_a_panic() {
        let store = temp_store("missing");
        assert!(matches!(
            store.load("Nope"),
            Err(Error::NoSuchProfile(name)) if name == "Nope"
        ));
        assert!(matches!(store.delete("Nope"), Err(Error::NoSuchProfile(_))));
    }

    #[test]
    fn listing_an_absent_directory_is_empty_not_an_error() {
        let store = temp_store("absent");
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn invalid_names_are_refused_at_save_time() {
        let store = temp_store("bad-name");
        let profile = Profile::from_config("bad/name", empty_config());
        assert!(matches!(
            store.save(&profile),
            Err(Error::InvalidProfileName(_))
        ));
    }

    #[test]
    fn rename_moves_the_file() {
        let store = temp_store("rename");
        store
            .save(&Profile::from_config("Old", empty_config()))
            .unwrap();

        store.rename("Old", "New").unwrap();

        assert!(!store.exists("Old"));
        assert_eq!(store.load("New").unwrap().name, "New");

        fs::remove_dir_all(store.dir()).unwrap();
    }

    #[test]
    fn rename_onto_an_existing_profile_is_refused() {
        let store = temp_store("rename-clash");
        store
            .save(&Profile::from_config("A", empty_config()))
            .unwrap();
        store
            .save(&Profile::from_config("B", empty_config()))
            .unwrap();

        assert!(matches!(
            store.rename("A", "B"),
            Err(Error::ProfileExists(_))
        ));
        assert!(
            store.exists("A"),
            "the source must survive a refused rename"
        );

        fs::remove_dir_all(store.dir()).unwrap();
    }

    #[test]
    fn timestamp_looks_like_rfc3339() {
        let stamp = now_rfc3339();
        assert_eq!(stamp.len(), 20, "{stamp}");
        assert!(stamp.ends_with('Z'));
        assert_eq!(stamp.as_bytes()[4], b'-');
        assert_eq!(stamp.as_bytes()[10], b'T');

        let year: i32 = stamp[..4].parse().unwrap();
        assert!((2024..2100).contains(&year), "implausible year in {stamp}");
    }

    #[test]
    fn summary_reads_naturally() {
        let mut profile = Profile::from_config("Work", empty_config());
        assert_eq!(profile.summary(), "no active monitors");

        profile.active_monitors = vec!["DELL U2720Q".into()];
        assert_eq!(profile.summary(), "DELL U2720Q");

        profile.active_monitors = vec!["A".into(), "B".into()];
        assert_eq!(profile.summary(), "2 monitors: A, B");
    }
}

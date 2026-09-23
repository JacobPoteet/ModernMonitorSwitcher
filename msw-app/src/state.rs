/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Shared application state and the view types the settings window consumes.

use std::path::PathBuf;
use std::sync::Mutex;

use msw_core::Store;
use serde::Serialize;

use crate::settings::Settings;

/// Running from `tools/run-sandbox.ps1` rather than as the installed copy.
///
/// The script points `%APPDATA%` at a scratch folder, which already keeps
/// settings, profiles and the log away from the real ones. This covers what
/// does not live there: the single-instance lock, the Run key autostart
/// writes, and the updater, which installs over the real copy.
pub fn is_sandbox() -> bool {
    std::env::var_os("MSW_SANDBOX").is_some_and(|v| !v.is_empty())
}

/// Set when the user has actually asked to quit.
///
/// The exit handler otherwise vetoes every exit, because hiding the last
/// window must not end a tray application. Relying on the exit code to tell
/// the two apart is too subtle — an explicit flag says what was meant.
#[derive(Default)]
pub struct QuitFlag(pub std::sync::atomic::AtomicBool);

pub struct AppState {
    pub store: Store,
    pub settings: Mutex<Settings>,
    pub settings_path: PathBuf,
}

impl AppState {
    pub fn new() -> AppState {
        let store = Store::default_location().unwrap_or_else(|e| {
            // Without APPDATA there is nowhere canonical to put profiles.
            // Fall back to beside the executable so the application still runs.
            tracing::error!(error = %e, "no application data directory; falling back to the executable directory");
            let dir = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.to_path_buf()))
                .unwrap_or_else(|| PathBuf::from("."));
            Store::at(dir.join("profiles"))
        });

        let settings_path = Settings::default_path()
            .unwrap_or_else(|| store.dir().join("..").join("settings.json"));
        let settings = Settings::load(&settings_path);

        AppState {
            store,
            settings: Mutex::new(settings),
            settings_path,
        }
    }

    /// Persist the current settings, logging rather than failing on error.
    pub fn save_settings(&self) {
        let settings = self.settings.lock().expect("settings mutex poisoned");
        if let Err(e) = settings.save(&self.settings_path) {
            tracing::error!(path = %self.settings_path.display(), error = %e, "could not save settings");
        }
    }
}

/// A profile as the settings window sees it.
#[derive(Debug, Clone, Serialize)]
pub struct ProfileView {
    pub name: String,
    pub summary: String,
    pub monitors: Vec<String>,
    pub saved_at: Option<String>,
    /// Is this the configuration currently on screen?
    pub active: bool,
    /// Accelerator bound to this profile, if any.
    pub hotkey: Option<String>,
}

/// A monitor as the settings window sees it.
#[derive(Debug, Clone, Serialize)]
pub struct MonitorView {
    /// Stable identity, used as the key when setting a nickname.
    pub key: String,
    /// What Windows calls it, e.g. "DELL U2724D".
    pub model: String,
    /// The nickname, if one has been set.
    pub nickname: Option<String>,
    pub active: bool,
    /// Resolution and desktop position, which is how someone tells two
    /// identical monitors apart well enough to name them.
    pub resolution: Option<String>,
    pub position: Option<String>,
    /// Same geometry as `resolution`/`position`, unformatted, for drawing the
    /// arrangement diagram — virtual-desktop coordinates, so a monitor to the
    /// left of the primary has a negative `x`.
    pub x: Option<i32>,
    pub y: Option<i32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

/// What is on screen right now.
#[derive(Debug, Clone, Serialize)]
pub struct CurrentStatus {
    pub active_monitors: Vec<String>,
    pub inactive_monitors: Vec<String>,
    /// Name of the saved profile that matches, if one does.
    pub matching_profile: Option<String>,
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Profile operations shared by the tray menu, the hotkeys and the window.
//!
//! Everything that changes profiles goes through here so the tray, the
//! settings window and the tooltip cannot drift out of step: each operation
//! ends by refreshing the tray and telling the window to reload.

use std::collections::BTreeMap;

use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, CurrentStatus, MonitorView, ProfileView, ScreenView};
use crate::tray;

/// Event the settings window listens for to reload its list.
pub const PROFILES_CHANGED: &str = "profiles-changed";

/// The monitor nicknames, copied out so the settings lock is not held while
/// rendering.
pub fn nicknames(app: &AppHandle) -> BTreeMap<String, String> {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().expect("settings mutex poisoned");
    settings.monitor_names.clone()
}

/// One-line description of what a profile leaves switched on.
///
/// Recomputed from the stored configuration rather than read from the
/// profile's `active_monitors` field, so a nickname assigned after the profile
/// was saved still shows up.
fn summarize(
    profile: &msw_core::Profile,
    nicknames: &BTreeMap<String, String>,
) -> (String, Vec<String>) {
    let labels = profile.config.active_monitor_labels_with(nicknames);
    let summary = match labels.len() {
        0 => "no active monitors".to_string(),
        1 => labels[0].clone(),
        n => format!("{n} monitors: {}", labels.join(", ")),
    };
    (summary, labels)
}

/// Read every profile, annotated for display.
pub fn list(app: &AppHandle) -> Result<Vec<ProfileView>, String> {
    let state = app.state::<AppState>();
    tracing::debug!(dir = %state.store.dir().display(), "listing profiles");
    let profiles = state.store.list().map_err(|e| {
        tracing::error!(error = %e, "could not list profiles");
        e.to_string()
    })?;
    tracing::debug!(count = profiles.len(), "profiles read");

    // A failure to read the current configuration should not stop the list
    // from rendering; it only means nothing can be marked as active.
    let current = msw_core::current_config().ok();
    let settings = state.settings.lock().expect("settings mutex poisoned");
    let nicknames = settings.monitor_names.clone();

    Ok(profiles
        .into_iter()
        .map(|p| {
            let (summary, monitors) = summarize(&p, &nicknames);
            let (screens, off) = layout(&p.config, &nicknames);
            ProfileView {
                active: current.as_ref().is_some_and(|c| p.is_active(c)),
                summary,
                monitors,
                saved_at: p.saved_at.clone(),
                hotkey: settings.hotkeys.get(&p.name).cloned(),
                screens,
                off,
                name: p.name,
            }
        })
        .collect())
}

/// The geometry of every monitor a configuration turns on, and the names of
/// the ones it leaves off, so the window can draw a profile as the desk it
/// describes rather than a list of names.
fn layout(
    config: &msw_core::DisplayConfig,
    nicknames: &BTreeMap<String, String>,
) -> (Vec<ScreenView>, Vec<String>) {
    let labels = config.active_monitor_labels_with(nicknames);
    let screens = config
        .active_monitors()
        .into_iter()
        .zip(labels)
        .filter_map(|(m, label)| {
            active_mode(config, m).map(|mode| ScreenView {
                label,
                x: mode.position.x,
                y: mode.position.y,
                width: mode.width,
                height: mode.height,
            })
        })
        .collect();

    let off = config
        .monitors
        .iter()
        .filter(|m| !is_active(config, m))
        .map(|m| m.label_with(nicknames))
        .collect();

    (screens, off)
}

/// Describe what is on screen right now.
pub fn current_status(app: &AppHandle) -> Result<CurrentStatus, String> {
    let state = app.state::<AppState>();
    let config = msw_core::current_config().map_err(|e| e.to_string())?;
    let nicknames = nicknames(app);

    let active_monitors = config.active_monitor_labels_with(&nicknames);

    let inactive_monitors = config
        .monitors
        .iter()
        .filter(|m| !is_active(&config, m))
        .map(|m| m.label_with(&nicknames))
        .collect();

    let matching_profile = state
        .store
        .list()
        .unwrap_or_default()
        .into_iter()
        .find(|p| p.is_active(&config))
        .map(|p| p.name);

    Ok(CurrentStatus {
        active_monitors,
        inactive_monitors,
        matching_profile,
    })
}

/// Is this monitor on an active path?
fn is_active(config: &msw_core::DisplayConfig, monitor: &msw_core::MonitorInfo) -> bool {
    config.paths.iter().any(|p| {
        p.is_active() && p.target.id == monitor.id && p.target.adapter_id == monitor.adapter_id
    })
}

/// `DISPLAYCONFIG_ROTATION` values that turn a monitor on its side. The
/// source mode's width and height need swapping for either of them.
const ROTATE_90: i32 = 2;
const ROTATE_270: i32 = 4;

/// The source mode behind an active monitor's path — its resolution and its
/// position in the virtual desktop.
///
/// An inactive monitor has none, which is fine: it is also the one the user
/// is least able to identify.
///
/// `DISPLAYCONFIG_SOURCE_MODE` reports the panel's native width and height —
/// *before* the target's rotation is applied — so a portrait monitor still
/// comes back landscape-shaped here unless that rotation is applied. Every
/// caller wants what is actually on screen (the overlay's geometry, the
/// arrangement diagram, the resolution text), so it is applied once, here.
fn active_mode(
    config: &msw_core::DisplayConfig,
    monitor: &msw_core::MonitorInfo,
) -> Option<msw_core::model::SourceMode> {
    let path = config.paths.iter().find(|p| {
        p.is_active() && p.target.id == monitor.id && p.target.adapter_id == monitor.adapter_id
    })?;

    let mode = config.modes.iter().find_map(|mode| match mode.mode {
        msw_core::model::ModeKind::Source(s)
            if mode.id == path.source.id && mode.adapter_id == path.source.adapter_id =>
        {
            Some(s)
        }
        _ => None,
    })?;

    Some(match path.target.rotation {
        ROTATE_90 | ROTATE_270 => msw_core::model::SourceMode {
            width: mode.height,
            height: mode.width,
            ..mode
        },
        _ => mode,
    })
}

/// Every monitor Windows currently knows about, for the nickname editor.
pub fn list_monitors(app: &AppHandle) -> Result<Vec<MonitorView>, String> {
    let config = msw_core::current_config().map_err(|e| e.to_string())?;
    let nicknames = nicknames(app);
    let models = config.monitor_model_labels_with(&nicknames);

    Ok(config
        .monitors
        .iter()
        .zip(models)
        .map(|(m, model)| {
            let active = is_active(&config, m);
            let mode = active_mode(&config, m);

            MonitorView {
                key: m.key(),
                model,
                nickname: nicknames.get(&m.key()).cloned(),
                active,
                resolution: mode.map(|s| format!("{}x{}", s.width, s.height)),
                position: mode.map(|s| format!("{}, {}", s.position.x, s.position.y)),
                x: mode.map(|s| s.position.x),
                y: mode.map(|s| s.position.y),
                width: mode.map(|s| s.width),
                height: mode.map(|s| s.height),
            }
        })
        .collect())
}

/// Where to put an "Identify" overlay for one active monitor, and what it
/// should say.
pub struct IdentifyTarget {
    pub label: String,
    pub x: i32,
    pub y: i32,
    pub width: u32,
    pub height: u32,
}

/// Geometry and label for every active monitor, for the Identify overlay.
///
/// Labels reuse [`msw_core::DisplayConfig::active_monitor_labels_with`], so
/// the number an overlay shows is the same one the settings window would
/// disambiguate two identically named monitors with.
pub fn identify_targets(app: &AppHandle) -> Result<Vec<IdentifyTarget>, String> {
    let config = msw_core::current_config().map_err(|e| e.to_string())?;
    let nicknames = nicknames(app);
    let labels = config.active_monitor_labels_with(&nicknames);

    Ok(config
        .active_monitors()
        .into_iter()
        .zip(labels)
        .filter_map(|(m, label)| {
            active_mode(&config, m).map(|mode| IdentifyTarget {
                label,
                x: mode.position.x,
                y: mode.position.y,
                width: mode.width,
                height: mode.height,
            })
        })
        .collect())
}

/// Which saved profile matches the screen right now, if any.
pub fn active_profile_name(app: &AppHandle) -> Option<String> {
    let state = app.state::<AppState>();
    let config = msw_core::current_config().ok()?;
    state
        .store
        .list()
        .ok()?
        .into_iter()
        .find(|p| p.is_active(&config))
        .map(|p| p.name)
}

/// Switch to a profile.
pub fn apply(app: &AppHandle, name: &str) -> Result<String, String> {
    let state = app.state::<AppState>();
    let profile = state.store.load(name).map_err(|e| e.to_string())?;

    let outcome = msw_core::apply_profile(&profile).map_err(|e| e.to_string())?;

    let mut message = format!("Switched to {name}");
    if outcome.strategy != msw_core::Strategy::Verbatim {
        message.push_str(&format!(" ({})", outcome.strategy.describe()));
    }
    if outcome.lenient {
        message.push_str("; Windows adjusted some settings to make it fit");
    }

    tracing::info!(profile = name, strategy = ?outcome.strategy, lenient = outcome.lenient, "switched");
    refresh(app);
    Ok(message)
}

/// Capture the current configuration under a name.
pub fn save(app: &AppHandle, name: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.store.capture(name).map_err(|e| e.to_string())?;
    refresh(app);
    Ok(())
}

pub fn delete(app: &AppHandle, name: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.store.delete(name).map_err(|e| e.to_string())?;

    // Release any accelerator the deleted profile was holding.
    let removed = {
        let mut settings = state.settings.lock().expect("settings mutex poisoned");
        settings.hotkeys.remove(name).is_some()
    };
    if removed {
        state.save_settings();
        crate::hotkeys::reregister(app);
    }

    refresh(app);
    Ok(())
}

pub fn rename(app: &AppHandle, from: &str, to: &str) -> Result<(), String> {
    let state = app.state::<AppState>();
    state.store.rename(from, to).map_err(|e| e.to_string())?;

    // Carry any hotkey across to the new name.
    let moved = {
        let mut settings = state.settings.lock().expect("settings mutex poisoned");
        match settings.hotkeys.remove(from) {
            Some(accelerator) => {
                settings.hotkeys.insert(to.to_string(), accelerator);
                true
            }
            None => false,
        }
    };
    if moved {
        state.save_settings();
        crate::hotkeys::reregister(app);
    }

    refresh(app);
    Ok(())
}

/// Rebuild the tray and tell the settings window to reload.
pub fn refresh(app: &AppHandle) {
    if let Err(e) = tray::rebuild(app) {
        tracing::error!(error = %e, "could not rebuild the tray menu");
    }
    if let Err(e) = app.emit(PROFILES_CHANGED, ()) {
        tracing::debug!(error = %e, "no listener for the profile change event");
    }
}

/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Profile operations shared by the tray menu, the hotkeys and the window.
//!
//! Everything that changes profiles goes through here so the tray, the
//! settings window and the tooltip cannot drift out of step: each operation
//! ends by refreshing the tray and telling the window to reload.

use tauri::{AppHandle, Emitter, Manager};

use crate::state::{AppState, CurrentStatus, ProfileView};
use crate::tray;

/// Event the settings window listens for to reload its list.
pub const PROFILES_CHANGED: &str = "profiles-changed";

/// Read every profile, annotated for display.
pub fn list(app: &AppHandle) -> Result<Vec<ProfileView>, String> {
    let state = app.state::<AppState>();
    let profiles = state.store.list().map_err(|e| e.to_string())?;

    // A failure to read the current configuration should not stop the list
    // from rendering; it only means nothing can be marked as active.
    let current = msw_core::current_config().ok();
    let settings = state.settings.lock().expect("settings mutex poisoned");

    Ok(profiles
        .into_iter()
        .map(|p| ProfileView {
            active: current.as_ref().is_some_and(|c| p.is_active(c)),
            summary: p.summary(),
            monitors: p.active_monitors.clone(),
            saved_at: p.saved_at.clone(),
            hotkey: settings.hotkeys.get(&p.name).cloned(),
            name: p.name,
        })
        .collect())
}

/// Describe what is on screen right now.
pub fn current_status(app: &AppHandle) -> Result<CurrentStatus, String> {
    let state = app.state::<AppState>();
    let config = msw_core::current_config().map_err(|e| e.to_string())?;

    let active_monitors = config.active_monitor_labels();

    let inactive_monitors = config
        .monitors
        .iter()
        .filter(|m| {
            !config.paths.iter().any(|p| {
                p.is_active() && p.target.id == m.id && p.target.adapter_id == m.adapter_id
            })
        })
        .map(|m| m.label())
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

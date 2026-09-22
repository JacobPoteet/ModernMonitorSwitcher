/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Commands invoked from the settings window.
//!
//! Each one is a thin wrapper: the real work lives in `profiles`, `hotkeys`
//! and `updater`, so the tray and the window cannot diverge in behaviour.

use tauri::{AppHandle, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::settings::Settings;
use crate::state::{AppState, CurrentStatus, ProfileView};
use crate::updater::UpdateStatus;
use crate::{hotkeys, profiles, window};

#[tauri::command]
pub fn list_profiles(app: AppHandle) -> Result<Vec<ProfileView>, String> {
    profiles::list(&app)
}

#[tauri::command]
pub fn current_status(app: AppHandle) -> Result<CurrentStatus, String> {
    profiles::current_status(&app)
}

#[tauri::command]
pub fn apply_profile(app: AppHandle, name: String) -> Result<String, String> {
    profiles::apply(&app, &name)
}

#[tauri::command]
pub fn save_profile(app: AppHandle, name: String, overwrite: bool) -> Result<(), String> {
    let name = name.trim().to_string();

    if !msw_core::profile::is_valid_name(&name) {
        return Err(
            "Use a name without \\ / : * ? \" < > | that is not a reserved Windows name."
                .to_string(),
        );
    }

    let exists = app.state::<AppState>().store.exists(&name);
    if exists && !overwrite {
        return Err(format!("A profile named {name} already exists."));
    }

    profiles::save(&app, &name)
}

#[tauri::command]
pub fn delete_profile(app: AppHandle, name: String) -> Result<(), String> {
    profiles::delete(&app, &name)
}

#[tauri::command]
pub fn rename_profile(app: AppHandle, from: String, to: String) -> Result<(), String> {
    let to = to.trim().to_string();
    if !msw_core::profile::is_valid_name(&to) {
        return Err(
            "Use a name without \\ / : * ? \" < > | that is not a reserved Windows name."
                .to_string(),
        );
    }
    profiles::rename(&app, &from, &to)
}

/// Check whether a profile would apply, changing nothing on screen.
#[tauri::command]
pub fn preflight_profile(app: AppHandle, name: String) -> Result<Vec<String>, String> {
    let state = app.state::<AppState>();
    let profile = state.store.load(&name).map_err(|e| e.to_string())?;

    let options = msw_core::preflight_profile(&profile).map_err(|e| e.to_string())?;

    Ok(options
        .into_iter()
        .map(|(strategy, lenient)| {
            let mut text = strategy.describe().to_string();
            if lenient {
                text.push_str(" (Windows would adjust some settings)");
            }
            text
        })
        .collect())
}

#[tauri::command]
pub fn get_settings(app: AppHandle) -> Settings {
    let state = app.state::<AppState>();
    let settings = state.settings.lock().expect("settings mutex poisoned");
    settings.clone()
}

/// Bind or clear a profile's hotkey.
///
/// Passing `None` clears it. Rejects a combination already bound to a
/// different profile, because the second registration would silently lose.
#[tauri::command]
pub fn set_hotkey(app: AppHandle, name: String, accelerator: Option<String>) -> Result<(), String> {
    {
        let state = app.state::<AppState>();
        let mut settings = state.settings.lock().expect("settings mutex poisoned");

        match accelerator {
            Some(accelerator) if !accelerator.trim().is_empty() => {
                let accelerator = accelerator.trim().to_string();

                // Reject here rather than at registration time, where the
                // failure would only reach a log file.
                if accelerator
                    .parse::<tauri_plugin_global_shortcut::Shortcut>()
                    .is_err()
                {
                    return Err(format!("{accelerator} is not a usable key combination."));
                }

                if let Some(other) = settings.profile_for_accelerator(&accelerator) {
                    if other != name {
                        return Err(format!("{accelerator} is already assigned to {other}."));
                    }
                }

                settings.hotkeys.insert(name, accelerator);
            }
            _ => {
                settings.hotkeys.remove(&name);
            }
        }
    }

    app.state::<AppState>().save_settings();
    hotkeys::reregister(&app);
    profiles::refresh(&app);
    Ok(())
}

#[tauri::command]
pub fn set_check_for_updates(app: AppHandle, enabled: bool) {
    {
        let state = app.state::<AppState>();
        let mut settings = state.settings.lock().expect("settings mutex poisoned");
        settings.check_for_updates = enabled;
    }
    app.state::<AppState>().save_settings();
}

#[tauri::command]
pub fn get_autostart(app: AppHandle) -> Result<bool, String> {
    app.autolaunch().is_enabled().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn set_autostart(app: AppHandle, enabled: bool) -> Result<(), String> {
    let manager = app.autolaunch();
    if enabled {
        manager.enable().map_err(|e| e.to_string())
    } else {
        manager.disable().map_err(|e| e.to_string())
    }
}

#[tauri::command]
pub async fn check_for_update(app: AppHandle) -> Result<UpdateStatus, String> {
    crate::updater::check(&app).await
}

#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    crate::updater::install(&app).await
}

#[tauri::command]
pub fn monitors_off() {
    // The broadcast blocks until every top-level window has handled it, so it
    // must not run on the thread serving this command.
    std::thread::spawn(msw_core::power::all_monitors_off);
}

/// Restore whatever layout Windows remembers for the monitors connected now.
#[tauri::command]
pub fn reset_display_config() -> Result<(), String> {
    let status = msw_core::ccd::reset_to_database_current();
    if msw_core::ccd::is_success(status) {
        Ok(())
    } else {
        Err(format!(
            "Windows refused to restore its remembered layout (error {status})."
        ))
    }
}

#[tauri::command]
pub fn open_profiles_folder(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    let state = app.state::<AppState>();
    let dir = state.store.dir().to_path_buf();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

/// Open the project page in the real browser.
///
/// The URL is fixed here rather than passed in from the page, so this cannot
/// become a general "open anything" capability.
#[tauri::command]
pub fn open_repository(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;

    app.opener()
        .open_url(
            "https://github.com/JacobPoteet/ModernMonitorSwitcher",
            None::<&str>,
        )
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn app_version(app: AppHandle) -> String {
    app.package_info().version.to_string()
}

#[tauri::command]
pub fn hide_window(app: AppHandle) {
    window::hide(&app);
}

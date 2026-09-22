/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Global hotkeys.
//!
//! Bindings are unregistered and re-registered wholesale whenever they change.
//! The original application accumulated bugs from incremental hotkey
//! bookkeeping — stale registrations that held an accelerator after a profile
//! was renamed — and doing it wholesale makes that class of bug impossible.

use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut, ShortcutState};

use crate::state::AppState;
use crate::{profiles, window};

/// Register every configured hotkey, replacing whatever was registered before.
pub fn reregister(app: &AppHandle) {
    let shortcuts = app.global_shortcut();

    if let Err(e) = shortcuts.unregister_all() {
        tracing::warn!(error = %e, "could not clear existing hotkeys");
    }

    let bindings: Vec<(String, String)> = {
        let state = app.state::<AppState>();
        let settings = state.settings.lock().expect("settings mutex poisoned");
        settings
            .hotkeys
            .iter()
            .map(|(name, accelerator)| (name.clone(), accelerator.clone()))
            .collect()
    };

    for (name, accelerator) in bindings {
        match accelerator.parse::<Shortcut>() {
            Ok(shortcut) => {
                if let Err(e) = shortcuts.register(shortcut) {
                    // Another application may already own this combination.
                    tracing::warn!(
                        profile = %name,
                        accelerator = %accelerator,
                        error = %e,
                        "could not register hotkey"
                    );
                }
            }
            Err(e) => {
                tracing::warn!(
                    profile = %name,
                    accelerator = %accelerator,
                    error = %e,
                    "unusable accelerator"
                );
            }
        }
    }
}

/// Handle a hotkey press by switching to the profile bound to it.
pub fn on_shortcut(app: &AppHandle, shortcut: &Shortcut, state: ShortcutState) {
    // Fire on press, not release, and ignore the auto-repeat that holding the
    // keys down would otherwise produce.
    if state != ShortcutState::Pressed {
        return;
    }

    let accelerator = shortcut.into_string();
    let name = {
        let app_state = app.state::<AppState>();
        let settings = app_state.settings.lock().expect("settings mutex poisoned");
        settings
            .profile_for_accelerator(&accelerator)
            .map(str::to_string)
    };

    let Some(name) = name else {
        tracing::debug!(accelerator = %accelerator, "hotkey fired with no profile bound");
        return;
    };

    let app = app.clone();
    std::thread::spawn(move || {
        if let Err(e) = profiles::apply(&app, &name) {
            tracing::error!(profile = %name, error = %e, "hotkey switch failed");
            window::report_error(&app, &format!("Could not switch to {name}:\n\n{e}"));
        }
    });
}

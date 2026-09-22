/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The settings window.
//!
//! The window is hidden rather than closed, so reopening it from the tray is
//! instant and the application keeps running with no window at all.

use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

pub const MAIN: &str = "main";

/// Bring the settings window up, creating nothing — it exists from startup.
pub fn show(app: &AppHandle) {
    let Some(window) = app.get_webview_window(MAIN) else {
        tracing::error!("the main window is missing");
        return;
    };

    if let Err(e) = window.show() {
        tracing::error!(error = %e, "could not show the window");
        return;
    }
    let _ = window.unminimize();
    let _ = window.set_focus();
}

pub fn hide(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(MAIN) {
        let _ = window.hide();
    }
}

/// Show an error to the user.
///
/// Used for failures that arrive from a tray click or a hotkey, where there is
/// no window in front of the user to display them in.
pub fn report_error(app: &AppHandle, message: &str) {
    app.dialog()
        .message(message)
        .kind(MessageDialogKind::Error)
        .title("Modern Monitor Switcher")
        .blocking_show();
}

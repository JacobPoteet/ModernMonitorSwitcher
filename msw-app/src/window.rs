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

/// Size the window for the screen it opens on, then centre it.
///
/// A fixed size is wrong at both ends: fine on a laptop, lost in the middle
/// of a 4K monitor. The height is a share of the work area, and the width
/// follows from a fixed aspect ratio rather than its own share, which on a
/// widescreen monitor made a long, shallow window. Both are bounded so it is
/// never cramped or sprawling, and never larger than the screen. The size in
/// `tauri.conf.json` stands if the monitor cannot be read.
pub fn fit_to_screen(app: &AppHandle) {
    /// Logical pixels.
    const MIN: (f64, f64) = (1000.0, 700.0);
    const MAX: (f64, f64) = (1340.0, 940.0);
    /// Share of the work area's height to take.
    const HEIGHT_SHARE: f64 = 0.74;
    /// Width over height; a little squarer than 16:10.
    const ASPECT: f64 = 1.42;
    /// Never cover more than this much of the work area.
    const FIT: f64 = 0.94;

    let Some(window) = app.get_webview_window(MAIN) else {
        return;
    };
    let monitor = match window.current_monitor() {
        Ok(Some(m)) => m,
        _ => match window.primary_monitor() {
            Ok(Some(m)) => m,
            _ => return,
        },
    };

    let scale = monitor.scale_factor();
    let area = monitor.work_area().size.to_logical::<f64>(scale);

    let height = (area.height * HEIGHT_SHARE)
        .clamp(MIN.1, MAX.1)
        .min(area.height * FIT);
    let width = (height * ASPECT).clamp(MIN.0, MAX.0).min(area.width * FIT);

    tracing::debug!(width, height, scale, "sizing the window to the screen");
    let _ = window.set_size(tauri::LogicalSize::new(width, height));
    let _ = window.center();
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

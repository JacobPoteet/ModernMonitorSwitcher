/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The "Identify" overlay: a label on every active monitor naming it, the
//! same way the button in Windows' own Display Settings shows a number on
//! each screen.
//!
//! It exists because two monitors of the same model report the same name —
//! exactly the situation a nickname is meant to fix — so there has to be a
//! way to tell which physical screen is which before naming them.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl,
    WebviewWindowBuilder,
};

use crate::profiles::{self, IdentifyTarget};

/// Every window this module creates carries this prefix, so they can be
/// found and closed again without a separate registry of labels.
const WINDOW_PREFIX: &str = "identify-";

const VISIBLE_FOR: Duration = Duration::from_millis(3500);

/// Distinguishes one call to [`show`] from the next, so a delayed close left
/// over from an earlier call cannot dismiss overlays a later call just
/// opened.
static NEXT_BATCH: AtomicU64 = AtomicU64::new(0);

/// Show a label naming each active monitor, and dismiss it a few seconds
/// later.
pub fn show(app: &AppHandle) -> Result<(), String> {
    // Clear out anything still on screen from a previous call before opening
    // a fresh set, rather than letting them pile up.
    close_all(app);

    let targets = profiles::identify_targets(app)?;
    if targets.is_empty() {
        return Err("No active monitors to identify.".to_string());
    }

    let batch = NEXT_BATCH.fetch_add(1, Ordering::Relaxed);

    for (index, target) in targets.iter().enumerate() {
        if let Err(e) = spawn_overlay(app, batch, index, target) {
            tracing::error!(error = %e, "could not open an identify overlay");
        }
    }

    let app = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(VISIBLE_FOR);
        close_batch(&app, batch);
    });

    Ok(())
}

fn window_label(batch: u64, index: usize) -> String {
    format!("{WINDOW_PREFIX}{batch}-{index}")
}

fn spawn_overlay(
    app: &AppHandle,
    batch: u64,
    index: usize,
    target: &IdentifyTarget,
) -> tauri::Result<()> {
    let label = window_label(batch, index);

    // A JSON string literal is valid JS and escapes everything a nickname
    // could contain, so the label reaches the page without ever being parsed
    // as anything else.
    let script = format!(
        "window.__IDENTIFY_LABEL__ = {};",
        serde_json::to_string(&target.label).expect("a string always serializes")
    );

    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("identify.html".into()))
        .initialization_script(script)
        .title("Identify")
        .inner_size(target.width as f64, target.height as f64)
        .position(target.x as f64, target.y as f64)
        .decorations(false)
        .transparent(true)
        .shadow(false)
        .resizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()?;

    // The builder's position and size above are in logical pixels; the
    // geometry from DISPLAYCONFIG is physical, so it is reapplied here to
    // land the overlay exactly over its monitor regardless of DPI scaling.
    window.set_position(Position::Physical(PhysicalPosition::new(
        target.x, target.y,
    )))?;
    window.set_size(Size::Physical(PhysicalSize::new(
        target.width,
        target.height,
    )))?;

    // The overlay is purely informational; clicks should reach whatever is
    // underneath it, not get eaten by a window the user never asked to focus.
    let _ = window.set_ignore_cursor_events(true);

    window.show()?;
    Ok(())
}

/// Close every identify overlay, regardless of which call opened it.
fn close_all(app: &AppHandle) {
    for (label, window) in app.webview_windows() {
        if label.starts_with(WINDOW_PREFIX) {
            let _ = window.close();
        }
    }
}

/// Close only the overlays a specific call to [`show`] opened.
fn close_batch(app: &AppHandle, batch: u64) {
    let prefix = format!("{WINDOW_PREFIX}{batch}-");
    for (label, window) in app.webview_windows() {
        if label.starts_with(&prefix) {
            let _ = window.close();
        }
    }
}

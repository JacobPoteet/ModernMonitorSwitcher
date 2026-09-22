/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Updating from GitHub releases.
//!
//! Update packages are signed with a minisign key whose public half is baked
//! into the application, so a tampered release cannot be installed even if the
//! download itself were intercepted.
//!
//! Two paths in: a quiet check at startup that only speaks up when there is
//! something to install, and an explicit check from the tray that always says
//! what it found.

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_dialog::{DialogExt, MessageDialogButtons, MessageDialogKind};
use tauri_plugin_updater::UpdaterExt;

/// What a check turned up, for the settings window.
#[derive(Debug, Clone, Serialize)]
pub struct UpdateStatus {
    pub available: bool,
    pub current_version: String,
    pub new_version: Option<String>,
    pub notes: Option<String>,
}

/// Look for an update without touching the UI.
pub async fn check(app: &AppHandle) -> Result<UpdateStatus, String> {
    let current_version = app.package_info().version.to_string();

    let updater = app.updater().map_err(|e| e.to_string())?;
    let update = updater.check().await.map_err(|e| e.to_string())?;

    Ok(match update {
        Some(update) => UpdateStatus {
            available: true,
            current_version,
            new_version: Some(update.version.clone()),
            notes: update.body.clone(),
        },
        None => UpdateStatus {
            available: false,
            current_version,
            new_version: None,
            notes: None,
        },
    })
}

/// Download and install, then restart into the new version.
pub async fn install(app: &AppHandle) -> Result<(), String> {
    let updater = app.updater().map_err(|e| e.to_string())?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        return Err("There is no update to install.".to_string());
    };

    update
        .download_and_install(|_chunk, _total| {}, || {})
        .await
        .map_err(|e| e.to_string())?;

    tracing::info!("update installed; restarting");
    app.restart();
}

/// The quiet check run at startup.
///
/// Says nothing at all unless there is an update, so a machine that boots with
/// no network does not greet the user with an error.
pub fn check_quietly(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match check(&app).await {
            Ok(status) if status.available => {
                let version = status.new_version.clone().unwrap_or_default();
                tracing::info!(version = %version, "update available");
                offer(&app, status);
            }
            Ok(_) => tracing::debug!("already up to date"),
            Err(e) => tracing::debug!(error = %e, "update check failed"),
        }
    });
}

/// The explicit check from the tray menu, which always reports back.
pub fn check_interactively(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        match check(&app).await {
            Ok(status) if status.available => offer(&app, status),
            Ok(status) => {
                app.dialog()
                    .message(format!(
                        "Modern Monitor Switcher {} is the latest version.",
                        status.current_version
                    ))
                    .title("No update available")
                    .blocking_show();
            }
            Err(e) => {
                app.dialog()
                    .message(format!("Could not check for updates:\n\n{e}"))
                    .kind(MessageDialogKind::Warning)
                    .title("Update check failed")
                    .blocking_show();
            }
        }
    });
}

/// Ask whether to install, and do it if so.
fn offer(app: &AppHandle, status: UpdateStatus) {
    let version = status.new_version.clone().unwrap_or_default();

    let mut message = format!(
        "Version {version} is available. You have {}.",
        status.current_version
    );
    if let Some(notes) = status.notes.as_ref().filter(|n| !n.trim().is_empty()) {
        message.push_str("\n\n");
        message.push_str(notes.trim());
    }
    message.push_str("\n\nInstall it now? The application will restart.");

    let app_for_dialog = app.clone();
    app.dialog()
        .message(message)
        .title("Update available")
        .buttons(MessageDialogButtons::OkCancelCustom(
            "Install".to_string(),
            "Not now".to_string(),
        ))
        .show(move |install_it| {
            if !install_it {
                return;
            }
            let app = app_for_dialog.clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = install(&app).await {
                    tracing::error!(error = %e, "update failed");
                    app.dialog()
                        .message(format!("Could not install the update:\n\n{e}"))
                        .kind(MessageDialogKind::Error)
                        .title("Update failed")
                        .blocking_show();
                }
            });
        });
}

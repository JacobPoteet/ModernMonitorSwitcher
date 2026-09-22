/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// No console window. The application lives in the tray; `msw.exe` is the
// command line front end.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod hotkeys;
mod profiles;
mod settings;
mod state;
mod tray;
mod updater;
mod window;

use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use state::AppState;

/// Passed by the autostart entry so a boot does not pop the window open.
const ARG_MINIMIZED: &str = "--minimized";

fn main() {
    init_logging();

    tauri::Builder::default()
        // Must be first: it takes effect before the rest of the application
        // starts, so a second launch never gets as far as a second tray icon.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            tracing::info!(?argv, "second instance; focusing the existing one");
            // A second launch means the user tried to start the app again,
            // so show them the window they were looking for.
            window::show(app);
        }))
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            Some(vec![ARG_MINIMIZED]),
        ))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    hotkeys::on_shortcut(app, shortcut, event.state());
                })
                .build(),
        )
        .invoke_handler(tauri::generate_handler![
            commands::list_profiles,
            commands::current_status,
            commands::apply_profile,
            commands::save_profile,
            commands::delete_profile,
            commands::rename_profile,
            commands::preflight_profile,
            commands::get_settings,
            commands::set_hotkey,
            commands::set_check_for_updates,
            commands::get_autostart,
            commands::set_autostart,
            commands::check_for_update,
            commands::install_update,
            commands::monitors_off,
            commands::reset_display_config,
            commands::open_profiles_folder,
            commands::open_repository,
            commands::app_version,
            commands::hide_window,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(AppState::new());

            // Drop hotkey bindings for profiles that no longer exist, which
            // would otherwise hold an accelerator for nothing.
            prune_stale_hotkeys(&handle);

            tray::create(&handle)?;
            hotkeys::reregister(&handle);

            // Set the tooltip and check marks from the real current state.
            profiles::refresh(&handle);

            let check_updates = {
                let state = handle.state::<AppState>();
                let settings = state.settings.lock().expect("settings mutex poisoned");
                settings.check_for_updates
            };
            if check_updates {
                updater::check_quietly(&handle);
            }

            // Show the window on a normal launch, but not when Windows started
            // us at login.
            if !std::env::args().any(|a| a == ARG_MINIMIZED) {
                window::show(&handle);
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window hides it. Quitting is done from the tray, so
            // the application stays available after the window is dismissed.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error building the application")
        .run(|_app, event| {
            // Without this, hiding the last window would exit the process.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                if code.is_none() {
                    api.prevent_exit();
                }
            }
        });
}

fn prune_stale_hotkeys(app: &tauri::AppHandle) {
    let state = app.state::<AppState>();

    let Ok(existing) = state.store.list_names() else {
        return;
    };

    let changed = {
        let mut settings = state.settings.lock().expect("settings mutex poisoned");
        settings.prune_hotkeys(&existing)
    };

    if changed {
        tracing::info!("removed hotkeys for profiles that no longer exist");
        state.save_settings();
    }
}

fn init_logging() {
    let filter = std::env::var("MSW_LOG").unwrap_or_else(|_| "msw_app=info,msw_core=info".into());
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .try_init();
}

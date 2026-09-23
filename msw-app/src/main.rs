/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

// No console window. The application lives in the tray; `msw.exe` is the
// command line front end.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod hotkeys;
mod identify;
mod profiles;
mod settings;
mod state;
mod tray;
mod updater;
mod watcher;
mod window;

use std::sync::atomic::Ordering;

use tauri::{Manager, WindowEvent};
use tauri_plugin_autostart::MacosLauncher;

use state::{AppState, QuitFlag};

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
            commands::list_monitors,
            commands::set_monitor_name,
            commands::identify_monitors,
            commands::get_settings,
            commands::set_hotkey,
            commands::set_check_for_updates,
            commands::get_autostart,
            commands::set_autostart,
            commands::check_for_update,
            commands::reset_display_config,
            commands::open_profiles_folder,
            commands::open_repository,
            commands::app_version,
            commands::hide_window,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            app.manage(AppState::new());
            app.manage(QuitFlag::default());

            // Drop hotkey bindings for profiles that no longer exist, which
            // would otherwise hold an accelerator for nothing.
            prune_stale_hotkeys(&handle);

            tray::create(&handle)?;
            hotkeys::reregister(&handle);

            // Profiles can be changed by msw.exe or by hand while this is
            // running, so the tray tracks the directory rather than trusting
            // what it read at startup.
            watcher::start(&handle);

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
            // Only the settings window hides instead of closing, so the
            // application stays available after it is dismissed. Other
            // windows — the identify overlays — must be free to actually
            // close, or `.close()` would just hide them and leak one set on
            // every use.
            if window.label() != window::MAIN {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .build(tauri::generate_context!())
        .expect("error building the application")
        .run(|app, event| {
            // Hiding the last window must not end the process, because the
            // application lives in the tray. But a quit asked for explicitly
            // has to go through.
            if let tauri::RunEvent::ExitRequested { api, code, .. } = event {
                let quitting = app.state::<QuitFlag>().0.load(Ordering::SeqCst);
                tracing::info!(?code, quitting, "exit requested");

                if !quitting {
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

/// Where the log is written.
///
/// `%APPDATA%\ModernMonitorSwitcher\msw.log`, beside the profiles.
fn log_path() -> Option<std::path::PathBuf> {
    std::env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .map(|base| base.join("ModernMonitorSwitcher").join("msw.log"))
}

/// Log to a file.
///
/// This is a GUI application with no console, so there is nowhere for stdout
/// to go: redirecting it from a shell does not reliably reach a process built
/// for the windows subsystem. Without a file there is no way to find out what
/// the application did — not for a bug report, and not while developing it.
///
/// The file is opened for appending, never truncated on open. A second launch
/// is normally a short-lived process that hands over to the running instance
/// and exits, and truncating would let it destroy the log of the instance
/// actually doing the work — which is exactly the log anyone would want to
/// read. Instead it is cleared only when it has grown past a limit, so it
/// cannot grow without bound either.
fn init_logging() {
    // The default filter has to name this crate as the compiler knows it. The
    // binary target is called ModernMonitorSwitcher, so that — not "msw_app" —
    // is the target on every event this crate emits, and a filter naming the
    // package silently matches nothing at all.
    let filter = std::env::var("MSW_LOG")
        .unwrap_or_else(|_| format!("{}=info,msw_core=info", env!("CARGO_CRATE_NAME")));

    /// Clear the log once it passes this, checked only at startup.
    const MAX_LOG_BYTES: u64 = 1024 * 1024;

    let file = log_path().and_then(|path| {
        path.parent().map(std::fs::create_dir_all);

        let too_big = std::fs::metadata(&path).is_ok_and(|m| m.len() > MAX_LOG_BYTES);

        std::fs::OpenOptions::new()
            .create(true)
            .append(!too_big)
            .truncate(too_big)
            .write(true)
            .open(&path)
            .ok()
    });

    let Some(file) = file else {
        // No log file, so fall back to stdout. Better than nothing when run
        // from a terminal, and harmless otherwise.
        let _ = tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_target(false)
            .try_init();
        return;
    };

    // Keep a handle for reporting a failure to install the subscriber. Such a
    // failure cannot be logged through tracing, for obvious reasons, and
    // silently swallowing it leaves an empty log file and no explanation.
    let mut report = file.try_clone().ok();

    let result = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_ansi(false)
        .with_writer(move || file.try_clone().expect("the log file handle can be cloned"))
        .try_init();

    if let Err(e) = result {
        if let Some(report) = report.as_mut() {
            use std::io::Write;
            let _ = writeln!(report, "could not install the log subscriber: {e}");
        }
        return;
    }

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        pid = std::process::id(),
        "Modern Monitor Switcher starting"
    );
}

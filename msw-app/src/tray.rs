/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The system tray icon and its menu.
//!
//! The menu is rebuilt from disk whenever profiles change, rather than kept in
//! sync incrementally. Profiles can be edited by the command line or by hand
//! while the application is running, so re-reading is both simpler and more
//! truthful.

use tauri::menu::{CheckMenuItem, Menu, MenuBuilder, MenuItem, SubmenuBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Manager};

use crate::state::AppState;
use crate::{profiles, updater, window};

pub const TRAY_ID: &str = "msw-tray";

// Menu item id prefixes. Profile names are user-supplied, so they are always
// carried in the suffix and never parsed for meaning beyond the first colon.
const APPLY_PREFIX: &str = "apply:";
const OVERWRITE_PREFIX: &str = "overwrite:";
const ID_SAVE_NEW: &str = "save-new";
const ID_SETTINGS: &str = "settings";
const ID_MONITORS_OFF: &str = "monitors-off";
const ID_CHECK_UPDATES: &str = "check-updates";
const ID_QUIT: &str = "quit";

/// Asks the settings window to open its save dialog, so "New profile..." in
/// the tray lands on the name field rather than on the window in general.
const OPEN_SAVE_DIALOG: &str = "open-save-dialog";

/// Create the tray icon. Called once, at startup.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let menu = build_menu(app)?;
    tracing::info!("tray created");

    TrayIconBuilder::with_id(TRAY_ID)
        .icon(tray_icon()?)
        .menu(&menu)
        // The menu belongs on right click; a left click opens the window,
        // which is what people expect of a tray application on Windows.
        .show_menu_on_left_click(false)
        .tooltip(tooltip_text(None))
        .on_menu_event(handle_menu_event)
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                window::show(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

/// The tray icon, embedded at compile time so it cannot go missing at runtime.
fn tray_icon() -> tauri::Result<tauri::image::Image<'static>> {
    tauri::image::Image::from_bytes(include_bytes!("../icons/tray-32.png"))
}

/// Rebuild the menu and tooltip from what is on disk.
pub fn rebuild(app: &AppHandle) -> tauri::Result<()> {
    let Some(tray) = app.tray_by_id(TRAY_ID) else {
        // Nothing to update before the tray exists.
        return Ok(());
    };

    let menu = build_menu(app)?;
    tray.set_menu(Some(menu))?;

    let active = profiles::active_profile_name(app);
    tray.set_tooltip(Some(tooltip_text(active.as_deref())))?;

    Ok(())
}

/// Tray tooltip, which doubles as the answer to "which profile am I in?".
fn tooltip_text(active: Option<&str>) -> String {
    match active {
        Some(name) => format!("Modern Monitor Switcher\nOn screen: {name}"),
        None => "Modern Monitor Switcher".to_string(),
    }
}

fn build_menu(app: &AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let state = app.state::<AppState>();
    let profiles = state.store.list().unwrap_or_else(|e| {
        tracing::error!(error = %e, "could not read profiles for the tray menu");
        Vec::new()
    });

    let current = msw_core::current_config().ok();
    let settings = state.settings.lock().expect("settings mutex poisoned");

    let mut builder = MenuBuilder::new(app);

    if profiles.is_empty() {
        let empty = MenuItem::with_id(app, "no-profiles", "No profiles yet", false, None::<&str>)?;
        builder = builder.item(&empty);
    } else {
        for profile in &profiles {
            let active = current.as_ref().is_some_and(|c| profile.is_active(c));
            let accelerator = settings.hotkeys.get(&profile.name).map(|s| s.as_str());

            // A check mark beside the profile that is currently on screen.
            let item = CheckMenuItem::with_id(
                app,
                format!("{APPLY_PREFIX}{}", profile.name),
                &profile.name,
                true,
                active,
                accelerator,
            )?;
            builder = builder.item(&item);
        }
    }

    builder = builder.separator();

    // Save: as a new profile, or in place of an existing one. With nothing to
    // replace, a submenu holding a single item is just an extra hover.
    if profiles.is_empty() {
        let save_new = MenuItem::with_id(
            app,
            ID_SAVE_NEW,
            "Save current layout...",
            true,
            None::<&str>,
        )?;
        builder = builder.item(&save_new);
    } else {
        let save_new = MenuItem::with_id(app, ID_SAVE_NEW, "New profile...", true, None::<&str>)?;
        let mut save_menu = SubmenuBuilder::new(app, "Save current layout")
            .item(&save_new)
            .separator();
        for profile in &profiles {
            let item = MenuItem::with_id(
                app,
                format!("{OVERWRITE_PREFIX}{}", profile.name),
                format!("Replace {}", profile.name),
                true,
                None::<&str>,
            )?;
            save_menu = save_menu.item(&item);
        }
        builder = builder.item(&save_menu.build()?);
    }

    let monitors_off = MenuItem::with_id(
        app,
        ID_MONITORS_OFF,
        "Turn off displays",
        true,
        None::<&str>,
    )?;
    let settings_item = MenuItem::with_id(
        app,
        ID_SETTINGS,
        "Open Monitor Switcher",
        true,
        None::<&str>,
    )?;
    let check_updates = MenuItem::with_id(
        app,
        ID_CHECK_UPDATES,
        "Check for updates",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, ID_QUIT, "Quit", true, None::<&str>)?;

    builder
        .item(&monitors_off)
        .separator()
        .item(&settings_item)
        .item(&check_updates)
        .item(&quit)
        .build()
}

fn handle_menu_event(app: &AppHandle, event: tauri::menu::MenuEvent) {
    let id = event.id().as_ref().to_string();
    tracing::info!(menu_item = %id, "tray menu event");
    let app = app.app_handle().clone();

    if let Some(name) = id.strip_prefix(APPLY_PREFIX) {
        let name = name.to_string();
        // Applying blocks while Windows reconfigures the displays, which can
        // take a second or two. Keep it off the UI thread.
        std::thread::spawn(move || {
            if let Err(e) = profiles::apply(&app, &name) {
                tracing::error!(profile = %name, error = %e, "could not switch profile");
                window::report_error(&app, &format!("Could not switch to {name}:\n\n{e}"));
            }
        });
        return;
    }

    if let Some(name) = id.strip_prefix(OVERWRITE_PREFIX) {
        let name = name.to_string();
        std::thread::spawn(move || {
            if let Err(e) = profiles::save(&app, &name) {
                tracing::error!(profile = %name, error = %e, "could not save profile");
                window::report_error(&app, &format!("Could not save {name}:\n\n{e}"));
            }
        });
        return;
    }

    match id.as_str() {
        ID_SETTINGS => window::show(&app),
        ID_SAVE_NEW => {
            window::show(&app);
            if let Err(e) = app.emit(OPEN_SAVE_DIALOG, ()) {
                tracing::debug!(error = %e, "no listener for the save dialog event");
            }
        }
        ID_MONITORS_OFF => {
            // The broadcast blocks until every window has handled it.
            std::thread::spawn(msw_core::power::all_monitors_off);
        }
        ID_CHECK_UPDATES => updater::check_interactively(&app),
        ID_QUIT => {
            tracing::info!("quitting");
            // Record the intent before asking to exit. The exit handler vetoes
            // anything it has not been told about, so that hiding the last
            // window cannot end a tray application by accident.
            app.state::<crate::state::QuitFlag>()
                .0
                .store(true, std::sync::atomic::Ordering::SeqCst);
            app.exit(0);
        }
        _ => {}
    }
}

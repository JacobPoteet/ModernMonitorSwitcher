/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Watching the profiles directory.
//!
//! The tray menu is built from what is on disk, and this application is not
//! the only thing that writes there: `msw.exe` saves and deletes profiles, and
//! the files are plain JSON that anyone can edit by hand. Without a watcher the
//! tray would keep showing whatever it read at startup, which is exactly the
//! bug this module exists to fix — a profile saved from the command line
//! appeared in the settings window, because that reloads when it gains focus,
//! but never in the tray.

use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use notify::{EventKind, RecursiveMode, Watcher};
use tauri::{AppHandle, Manager};

use crate::profiles;
use crate::state::AppState;

/// How long to wait for a burst of file events to settle.
///
/// Saving a profile writes a temporary file and renames it, which is several
/// events for one logical change; a rebuild per event would be wasteful and
/// could read the directory mid-rename.
const DEBOUNCE: Duration = Duration::from_millis(300);

/// Start watching the profiles directory, rebuilding the tray when it changes.
///
/// Failures here are logged rather than fatal: a missing watcher means the
/// tray can go stale, which is far better than refusing to start.
pub fn start(app: &AppHandle) {
    let dir = app.state::<AppState>().store.dir().to_path_buf();

    // The watcher needs the directory to exist before it can watch it, and on
    // a first run nothing has created it yet.
    if let Err(e) = std::fs::create_dir_all(&dir) {
        tracing::warn!(dir = %dir.display(), error = %e, "could not create the profiles directory");
        return;
    }

    let app = app.clone();
    std::thread::spawn(move || run(app, dir));
}

fn run(app: AppHandle, dir: PathBuf) {
    let (tx, rx) = mpsc::channel();

    let mut watcher = match notify::recommended_watcher(tx) {
        Ok(w) => w,
        Err(e) => {
            tracing::warn!(error = %e, "could not create a file watcher; the tray menu will not track external changes");
            return;
        }
    };

    if let Err(e) = watcher.watch(&dir, RecursiveMode::NonRecursive) {
        tracing::warn!(dir = %dir.display(), error = %e, "could not watch the profiles directory");
        return;
    }

    tracing::debug!(dir = %dir.display(), "watching profiles directory");

    // The watcher stops working the moment it is dropped, so it has to stay
    // alive for as long as this loop runs.
    loop {
        let Ok(first) = rx.recv() else {
            // The sender is gone, which means the application is shutting down.
            tracing::debug!("profile watcher stopping");
            return;
        };

        if !is_interesting(&first) {
            continue;
        }

        // Swallow the rest of the burst before doing anything.
        while rx.recv_timeout(DEBOUNCE).is_ok() {}

        tracing::debug!("profiles changed on disk; rebuilding the tray");
        profiles::refresh(&app);
    }
}

/// Is this event worth rebuilding for?
///
/// Access events fire constantly and change nothing. Anything that creates,
/// removes or modifies a file might have changed the profile list.
fn is_interesting(event: &notify::Result<notify::Event>) -> bool {
    match event {
        Ok(event) => matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_) | EventKind::Any
        ),
        Err(e) => {
            tracing::debug!(error = %e, "file watch error");
            false
        }
    }
}

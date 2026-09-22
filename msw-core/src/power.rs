/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Monitor power state.

use windows::Win32::Foundation::{LPARAM, WPARAM};
use windows::Win32::UI::WindowsAndMessaging::{
    SendMessageW, HWND_BROADCAST, SC_MONITORPOWER, WM_SYSCOMMAND,
};

const MONITOR_OFF: isize = 2;
const MONITOR_STANDBY: isize = 1;
const MONITOR_ON: isize = -1;

fn set_monitor_power(state: isize) {
    // SAFETY: a broadcast WM_SYSCOMMAND with no pointer parameters. The call
    // blocks until every top-level window has processed it, which is why this
    // should not be called from a UI thread that also needs to stay responsive.
    unsafe {
        SendMessageW(
            HWND_BROADCAST,
            WM_SYSCOMMAND,
            Some(WPARAM(SC_MONITORPOWER as usize)),
            Some(LPARAM(state)),
        );
    }
}

/// Switch every monitor off. Any input wakes them again.
///
/// This is a power state, not a configuration change: it does not alter the
/// display topology and nothing needs to be restored afterwards.
pub fn all_monitors_off() {
    set_monitor_power(MONITOR_OFF);
}

/// Put every monitor into standby.
pub fn all_monitors_standby() {
    set_monitor_power(MONITOR_STANDBY);
}

/// Wake every monitor.
pub fn all_monitors_on() {
    set_monitor_power(MONITOR_ON);
}

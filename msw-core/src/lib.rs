/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Capture and restore Windows display configurations.
//!
//! The whole library is a wrapper over the Windows CCD API:
//! [`capture`] reads the current monitor topology, [`apply`] puts a saved one
//! back. The interesting problem is that adapter identifiers change between
//! boots, so restoring is not simply replaying what was recorded — see
//! [`apply`] for how that is handled.
//!
//! ```no_run
//! # fn main() -> Result<(), msw_core::Error> {
//! let store = msw_core::Store::default_location()?;
//! store.capture("Work")?;              // save what is on screen now
//! msw_core::apply_profile(&store.load("Work")?)?;   // and put it back later
//! # Ok(())
//! # }
//! ```
//!
//! ## Attribution
//!
//! The adapter-matching strategies are ported from MonitorSwitcher by
//! Martin Kraemer (<https://sourceforge.net/projects/monitorswitcher>),
//! likewise under the MPL 2.0.

#![cfg(windows)]

pub mod apply;
pub mod ccd;
pub mod error;
pub mod model;
pub mod power;
pub mod profile;

pub use apply::{ApplyOutcome, Strategy};
pub use error::{Error, Result};
pub use model::{DisplayConfig, MonitorInfo};
pub use profile::{Profile, Store};

/// Read the display configuration currently on screen.
pub fn capture(name: impl Into<String>) -> Result<Profile> {
    Profile::capture(name)
}

/// Read the current configuration without naming it.
pub fn current_config() -> Result<DisplayConfig> {
    ccd::query(ccd::QueryScope::AllPaths)
}

/// Restore a saved profile.
pub fn apply_profile(profile: &Profile) -> Result<ApplyOutcome> {
    apply::apply(&profile.config)
}

/// Check whether a profile could be restored, without touching the screen.
pub fn preflight_profile(profile: &Profile) -> Result<Vec<(Strategy, bool)>> {
    apply::preflight(&profile.config)
}

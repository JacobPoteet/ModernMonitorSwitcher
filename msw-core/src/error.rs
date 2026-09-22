/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

use std::path::PathBuf;

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("GetDisplayConfigBufferSizes failed (Win32 error {0})")]
    BufferSizes(u32),

    #[error("QueryDisplayConfig failed (Win32 error {0})")]
    Query(u32),

    #[error("could not apply the display configuration; every strategy was rejected (last Win32 error {last_status})")]
    ApplyFailed { last_status: i32 },

    #[error("no profile named {0:?}")]
    NoSuchProfile(String),

    #[error("a profile named {0:?} already exists")]
    ProfileExists(String),

    #[error("{0:?} is not a usable profile name")]
    InvalidProfileName(String),

    #[error("profile {path} is not valid JSON: {source}")]
    ProfileParse {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },

    #[error("could not read or write {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("could not locate the per-user application data directory")]
    NoDataDir,
}

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
}

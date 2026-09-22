/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Serde-friendly mirrors of the Win32 CCD structures.
//!
//! The Win32 types are `#[repr(C)]` with anonymous unions and bitfields, which
//! makes them awkward to serialize directly and ties the on-disk format to a
//! particular `windows` crate version. These mirrors are plain data: they
//! round-trip losslessly to and from the Win32 types (see `ccd.rs`), and the
//! JSON they produce is stable and hand-editable.

use serde::{Deserialize, Serialize};

/// Locally unique adapter identifier.
///
/// Windows reassigns these on every boot and on GPU driver changes. Every
/// remapping strategy in `apply.rs` exists to deal with that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Luid {
    pub low: u32,
    pub high: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Rational {
    pub numerator: u32,
    pub denominator: u32,
}

impl Rational {
    /// Refresh rate in Hz, for display purposes only.
    pub fn as_hz(&self) -> f64 {
        if self.denominator == 0 {
            0.0
        } else {
            self.numerator as f64 / self.denominator as f64
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Region2D {
    pub cx: u32,
    pub cy: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Point {
    pub x: i32,
    pub y: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Rect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
}

/// Sentinel meaning "this path has no associated mode info entry".
pub const MODE_IDX_INVALID: u32 = 0xffff_ffff;

/// `DISPLAYCONFIG_PATH_ACTIVE` — the path is part of the active topology.
pub const PATH_ACTIVE: u32 = 0x0000_0001;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathSourceInfo {
    pub adapter_id: Luid,
    pub id: u32,
    /// Raw union value. Depending on the `SDC_VIRTUAL_MODE_AWARE` flag this is
    /// either a plain mode index or a packed (cloneGroupId, sourceModeInfoIdx)
    /// pair. Storing it raw preserves both readings.
    pub mode_info_idx: u32,
    pub status_flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathTargetInfo {
    pub adapter_id: Luid,
    pub id: u32,
    /// Raw union value; see [`PathSourceInfo::mode_info_idx`].
    pub mode_info_idx: u32,
    pub output_technology: i32,
    pub rotation: i32,
    pub scaling: i32,
    pub refresh_rate: Rational,
    pub scanline_ordering: i32,
    pub target_available: bool,
    pub status_flags: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PathInfo {
    pub source: PathSourceInfo,
    pub target: PathTargetInfo,
    pub flags: u32,
}

impl PathInfo {
    pub fn is_active(&self) -> bool {
        self.flags & PATH_ACTIVE != 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct VideoSignalInfo {
    pub pixel_rate: u64,
    pub h_sync_freq: Rational,
    pub v_sync_freq: Rational,
    pub active_size: Region2D,
    pub total_size: Region2D,
    /// Raw union value: `videoStandard` overlapped with a bitfield carrying
    /// vSyncFreqDivider. Kept raw for the same reason as `mode_info_idx`.
    pub video_standard: u32,
    pub scanline_ordering: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceMode {
    pub width: u32,
    pub height: u32,
    pub pixel_format: i32,
    pub position: Point,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TargetMode {
    pub video_signal_info: VideoSignalInfo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct DesktopImageInfo {
    pub path_source_size: Point,
    pub desktop_image_region: Rect,
    pub desktop_image_clip: Rect,
}

/// Which arm of the `DISPLAYCONFIG_MODE_INFO` union is live, tagged by the
/// structure's own `infoType` field.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ModeKind {
    Source(SourceMode),
    Target(TargetMode),
    DesktopImage(DesktopImageInfo),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModeInfo {
    pub id: u32,
    pub adapter_id: Luid,
    #[serde(flatten)]
    pub mode: ModeKind,
}

impl ModeInfo {
    pub fn is_source(&self) -> bool {
        matches!(self.mode, ModeKind::Source(_))
    }

    pub fn is_target(&self) -> bool {
        matches!(self.mode, ModeKind::Target(_))
    }
}

/// Human-facing identity of a monitor, from `DISPLAYCONFIG_TARGET_DEVICE_NAME`.
///
/// `device_path` is the stable one — it encodes the EDID and survives reboots
/// and port changes. `friendly_name` is what the user recognizes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MonitorInfo {
    pub adapter_id: Luid,
    pub id: u32,
    pub friendly_name: Option<String>,
    pub device_path: Option<String>,
    pub output_technology: i32,
    pub edid_manufacture_id: u16,
    pub edid_product_code_id: u16,
    pub connector_instance: u32,
}

impl MonitorInfo {
    /// Best available label for this monitor.
    pub fn label(&self) -> String {
        self.friendly_name
            .clone()
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| format!("Display {}", self.id))
    }
}

/// A complete display topology: what Windows hands back from
/// `QueryDisplayConfig`, plus the monitor identities needed to re-match it
/// against a different boot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayConfig {
    pub paths: Vec<PathInfo>,
    pub modes: Vec<ModeInfo>,
    #[serde(default)]
    pub monitors: Vec<MonitorInfo>,
}

impl DisplayConfig {
    /// Monitors attached to an active path, in path order — the ones a user
    /// would say are "on" in this configuration.
    pub fn active_monitors(&self) -> Vec<&MonitorInfo> {
        self.paths
            .iter()
            .filter(|p| p.is_active())
            .filter_map(|p| {
                self.monitors
                    .iter()
                    .find(|m| m.id == p.target.id && m.adapter_id == p.target.adapter_id)
            })
            .collect()
    }

    pub fn active_path_count(&self) -> usize {
        self.paths.iter().filter(|p| p.is_active()).count()
    }

    /// Labels for the active monitors, disambiguated when two report the same
    /// name.
    ///
    /// Identical monitors are common — a matched pair is the usual reason
    /// someone needs this application at all — and two entries both reading
    /// "DELL U2724D" tell the user nothing. Repeats get a numeric suffix in
    /// path order.
    pub fn active_monitor_labels(&self) -> Vec<String> {
        let monitors = self.active_monitors();

        let mut counts: Vec<(String, usize)> = Vec::new();
        for m in &monitors {
            let label = m.label();
            match counts.iter_mut().find(|(l, _)| *l == label) {
                Some((_, n)) => *n += 1,
                None => counts.push((label, 1)),
            }
        }

        let mut seen: Vec<(String, usize)> = Vec::new();
        monitors
            .iter()
            .map(|m| {
                let label = m.label();
                let total = counts
                    .iter()
                    .find(|(l, _)| *l == label)
                    .map(|(_, n)| *n)
                    .unwrap_or(1);
                if total < 2 {
                    return label;
                }
                let nth = match seen.iter_mut().find(|(l, _)| *l == label) {
                    Some((_, n)) => {
                        *n += 1;
                        *n
                    }
                    None => {
                        seen.push((label.clone(), 1));
                        1
                    }
                };
                format!("{label} #{nth}")
            })
            .collect()
    }

    /// A stable fingerprint of which monitors this configuration leaves on.
    ///
    /// Built from device paths, which survive reboots and adapter renumbering,
    /// so two configurations captured at different times can be compared. Used
    /// to work out which saved profile matches what is on screen now.
    pub fn active_identity(&self) -> Vec<String> {
        let mut out: Vec<String> = self
            .active_monitors()
            .iter()
            .map(|m| {
                m.device_path
                    .clone()
                    .unwrap_or_else(|| format!("{}:{}", m.adapter_id.low, m.id))
            })
            .collect();
        out.sort();
        out.dedup();
        out
    }

    /// Does this configuration light up the same monitors as `other`?
    ///
    /// Deliberately compares only *which* monitors are on, not their
    /// resolutions or arrangement. That is the distinction the user cares
    /// about when asking "am I in Work or Play right now?", and it stays true
    /// after Windows nudges a refresh rate.
    pub fn has_same_active_monitors(&self, other: &DisplayConfig) -> bool {
        let mine = self.active_identity();
        !mine.is_empty() && mine == other.active_identity()
    }

    /// Every distinct adapter LUID referenced anywhere in this config.
    pub fn adapter_ids(&self) -> Vec<Luid> {
        let mut out: Vec<Luid> = Vec::new();
        for p in &self.paths {
            for l in [p.source.adapter_id, p.target.adapter_id] {
                if !out.contains(&l) {
                    out.push(l);
                }
            }
        }
        for m in &self.modes {
            if !out.contains(&m.adapter_id) {
                out.push(m.adapter_id);
            }
        }
        out
    }

    /// Rewrite every occurrence of one adapter LUID with another.
    pub fn replace_adapter(&mut self, from: Luid, to: Luid) {
        for p in &mut self.paths {
            if p.source.adapter_id == from {
                p.source.adapter_id = to;
            }
            if p.target.adapter_id == from {
                p.target.adapter_id = to;
            }
        }
        for m in &mut self.modes {
            if m.adapter_id == from {
                m.adapter_id = to;
            }
        }
        for mon in &mut self.monitors {
            if mon.adapter_id == from {
                mon.adapter_id = to;
            }
        }
    }
}

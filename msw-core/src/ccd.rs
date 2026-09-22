/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! The Windows Connecting and Configuring Displays (CCD) API.
//!
//! This is the only module that touches Win32 directly. Everything above it
//! works on the plain types in [`crate::model`].

use crate::error::{Error, Result};
use crate::model::*;

use windows::Win32::Devices::Display::*;
use windows::Win32::Foundation::{ERROR_SUCCESS, LUID, POINTL, RECTL};

/// Flags for a normal "restore this exact configuration" apply.
///
/// `SDC_SAVE_TO_DATABASE` is what makes Windows remember the arrangement for
/// this set of monitors, so it survives sleep and unplug cycles.
pub const APPLY_FLAGS: SET_DISPLAY_CONFIG_FLAGS = SET_DISPLAY_CONFIG_FLAGS(
    SDC_APPLY.0
        | SDC_USE_SUPPLIED_DISPLAY_CONFIG.0
        | SDC_SAVE_TO_DATABASE.0
        | SDC_NO_OPTIMIZATION.0,
);

/// As above, but letting Windows adjust modes it considers invalid.
///
/// Tried only after the strict apply fails: it can silently land you on a
/// different refresh rate or resolution than the one you saved.
pub const APPLY_FLAGS_LENIENT: SET_DISPLAY_CONFIG_FLAGS =
    SET_DISPLAY_CONFIG_FLAGS(APPLY_FLAGS.0 | SDC_ALLOW_CHANGES.0);

/// Ask Windows whether a configuration *would* apply, without applying it.
pub const VALIDATE_FLAGS: SET_DISPLAY_CONFIG_FLAGS =
    SET_DISPLAY_CONFIG_FLAGS(SDC_VALIDATE.0 | SDC_USE_SUPPLIED_DISPLAY_CONFIG.0);

pub const VALIDATE_FLAGS_LENIENT: SET_DISPLAY_CONFIG_FLAGS =
    SET_DISPLAY_CONFIG_FLAGS(VALIDATE_FLAGS.0 | SDC_ALLOW_CHANGES.0);

// ---------------------------------------------------------------------------
// Conversions: Win32 -> model
// ---------------------------------------------------------------------------

impl From<LUID> for Luid {
    fn from(v: LUID) -> Self {
        Luid {
            low: v.LowPart,
            high: v.HighPart,
        }
    }
}

impl From<Luid> for LUID {
    fn from(v: Luid) -> Self {
        LUID {
            LowPart: v.low,
            HighPart: v.high,
        }
    }
}

impl From<DISPLAYCONFIG_RATIONAL> for Rational {
    fn from(v: DISPLAYCONFIG_RATIONAL) -> Self {
        Rational {
            numerator: v.Numerator,
            denominator: v.Denominator,
        }
    }
}

impl From<Rational> for DISPLAYCONFIG_RATIONAL {
    fn from(v: Rational) -> Self {
        DISPLAYCONFIG_RATIONAL {
            Numerator: v.numerator,
            Denominator: v.denominator,
        }
    }
}

impl From<DISPLAYCONFIG_2DREGION> for Region2D {
    fn from(v: DISPLAYCONFIG_2DREGION) -> Self {
        Region2D { cx: v.cx, cy: v.cy }
    }
}

impl From<Region2D> for DISPLAYCONFIG_2DREGION {
    fn from(v: Region2D) -> Self {
        DISPLAYCONFIG_2DREGION { cx: v.cx, cy: v.cy }
    }
}

impl From<POINTL> for Point {
    fn from(v: POINTL) -> Self {
        Point { x: v.x, y: v.y }
    }
}

impl From<Point> for POINTL {
    fn from(v: Point) -> Self {
        POINTL { x: v.x, y: v.y }
    }
}

impl From<RECTL> for Rect {
    fn from(v: RECTL) -> Self {
        Rect {
            left: v.left,
            top: v.top,
            right: v.right,
            bottom: v.bottom,
        }
    }
}

impl From<Rect> for RECTL {
    fn from(v: Rect) -> Self {
        RECTL {
            left: v.left,
            top: v.top,
            right: v.right,
            bottom: v.bottom,
        }
    }
}

fn path_from_win32(p: &DISPLAYCONFIG_PATH_INFO) -> PathInfo {
    // SAFETY: reading the `modeInfoIdx` arm of the union is always valid; both
    // arms are 32 bits wide and we keep the raw value rather than interpreting it.
    let source_idx = unsafe { p.sourceInfo.Anonymous.modeInfoIdx };
    let target_idx = unsafe { p.targetInfo.Anonymous.modeInfoIdx };

    PathInfo {
        source: PathSourceInfo {
            adapter_id: p.sourceInfo.adapterId.into(),
            id: p.sourceInfo.id,
            mode_info_idx: source_idx,
            status_flags: p.sourceInfo.statusFlags,
        },
        target: PathTargetInfo {
            adapter_id: p.targetInfo.adapterId.into(),
            id: p.targetInfo.id,
            mode_info_idx: target_idx,
            output_technology: p.targetInfo.outputTechnology.0,
            rotation: p.targetInfo.rotation.0,
            scaling: p.targetInfo.scaling.0,
            refresh_rate: p.targetInfo.refreshRate.into(),
            scanline_ordering: p.targetInfo.scanLineOrdering.0,
            target_available: p.targetInfo.targetAvailable.as_bool(),
            status_flags: p.targetInfo.statusFlags,
        },
        flags: p.flags,
    }
}

fn path_to_win32(p: &PathInfo) -> DISPLAYCONFIG_PATH_INFO {
    let mut out = DISPLAYCONFIG_PATH_INFO::default();
    out.sourceInfo.adapterId = p.source.adapter_id.into();
    out.sourceInfo.id = p.source.id;
    out.sourceInfo.Anonymous.modeInfoIdx = p.source.mode_info_idx;
    out.sourceInfo.statusFlags = p.source.status_flags;

    out.targetInfo.adapterId = p.target.adapter_id.into();
    out.targetInfo.id = p.target.id;
    out.targetInfo.Anonymous.modeInfoIdx = p.target.mode_info_idx;
    out.targetInfo.outputTechnology =
        DISPLAYCONFIG_VIDEO_OUTPUT_TECHNOLOGY(p.target.output_technology);
    out.targetInfo.rotation = DISPLAYCONFIG_ROTATION(p.target.rotation);
    out.targetInfo.scaling = DISPLAYCONFIG_SCALING(p.target.scaling);
    out.targetInfo.refreshRate = p.target.refresh_rate.into();
    out.targetInfo.scanLineOrdering = DISPLAYCONFIG_SCANLINE_ORDERING(p.target.scanline_ordering);
    out.targetInfo.targetAvailable = p.target.target_available.into();
    out.targetInfo.statusFlags = p.target.status_flags;

    out.flags = p.flags;
    out
}

fn signal_from_win32(v: &DISPLAYCONFIG_VIDEO_SIGNAL_INFO) -> VideoSignalInfo {
    // SAFETY: as with modeInfoIdx, both union arms are 32 bits and we keep the
    // raw bits rather than interpreting the bitfield.
    let video_standard = unsafe { v.Anonymous.videoStandard };
    VideoSignalInfo {
        pixel_rate: v.pixelRate,
        h_sync_freq: v.hSyncFreq.into(),
        v_sync_freq: v.vSyncFreq.into(),
        active_size: v.activeSize.into(),
        total_size: v.totalSize.into(),
        video_standard,
        scanline_ordering: v.scanLineOrdering.0,
    }
}

// These build a zeroed struct and assign fields rather than using a struct
// literal, because the anonymous union member has to be written through
// `Anonymous` after the fact. Zeroing first is also what makes the padding and
// the union's unused bytes deterministic, which matters when Windows compares
// configurations.
#[allow(clippy::field_reassign_with_default)]
fn signal_to_win32(v: &VideoSignalInfo) -> DISPLAYCONFIG_VIDEO_SIGNAL_INFO {
    let mut out = DISPLAYCONFIG_VIDEO_SIGNAL_INFO::default();
    out.pixelRate = v.pixel_rate;
    out.hSyncFreq = v.h_sync_freq.into();
    out.vSyncFreq = v.v_sync_freq.into();
    out.activeSize = v.active_size.into();
    out.totalSize = v.total_size.into();
    out.Anonymous.videoStandard = v.video_standard;
    out.scanLineOrdering = DISPLAYCONFIG_SCANLINE_ORDERING(v.scanline_ordering);
    out
}

/// Returns `None` for entries Windows left as type zero, which
/// `QueryDisplayConfig` pads the array with.
fn mode_from_win32(m: &DISPLAYCONFIG_MODE_INFO) -> Option<ModeInfo> {
    let kind = match m.infoType {
        DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE => {
            // SAFETY: infoType says the sourceMode arm is the live one.
            let s = unsafe { m.Anonymous.sourceMode };
            ModeKind::Source(SourceMode {
                width: s.width,
                height: s.height,
                pixel_format: s.pixelFormat.0,
                position: s.position.into(),
            })
        }
        DISPLAYCONFIG_MODE_INFO_TYPE_TARGET => {
            // SAFETY: infoType says the targetMode arm is the live one.
            let t = unsafe { m.Anonymous.targetMode };
            ModeKind::Target(TargetMode {
                video_signal_info: signal_from_win32(&t.targetVideoSignalInfo),
            })
        }
        DISPLAYCONFIG_MODE_INFO_TYPE_DESKTOP_IMAGE => {
            // SAFETY: infoType says the desktopImageInfo arm is the live one.
            let d = unsafe { m.Anonymous.desktopImageInfo };
            ModeKind::DesktopImage(DesktopImageInfo {
                path_source_size: d.PathSourceSize.into(),
                desktop_image_region: d.DesktopImageRegion.into(),
                desktop_image_clip: d.DesktopImageClip.into(),
            })
        }
        _ => return None,
    };

    Some(ModeInfo {
        id: m.id,
        adapter_id: m.adapterId.into(),
        mode: kind,
    })
}

#[allow(clippy::field_reassign_with_default)]
fn mode_to_win32(m: &ModeInfo) -> DISPLAYCONFIG_MODE_INFO {
    let mut out = DISPLAYCONFIG_MODE_INFO::default();
    out.id = m.id;
    out.adapterId = m.adapter_id.into();

    match &m.mode {
        ModeKind::Source(s) => {
            out.infoType = DISPLAYCONFIG_MODE_INFO_TYPE_SOURCE;
            out.Anonymous.sourceMode = DISPLAYCONFIG_SOURCE_MODE {
                width: s.width,
                height: s.height,
                pixelFormat: DISPLAYCONFIG_PIXELFORMAT(s.pixel_format),
                position: s.position.into(),
            };
        }
        ModeKind::Target(t) => {
            out.infoType = DISPLAYCONFIG_MODE_INFO_TYPE_TARGET;
            out.Anonymous.targetMode = DISPLAYCONFIG_TARGET_MODE {
                targetVideoSignalInfo: signal_to_win32(&t.video_signal_info),
            };
        }
        ModeKind::DesktopImage(d) => {
            out.infoType = DISPLAYCONFIG_MODE_INFO_TYPE_DESKTOP_IMAGE;
            out.Anonymous.desktopImageInfo = DISPLAYCONFIG_DESKTOP_IMAGE_INFO {
                PathSourceSize: d.path_source_size.into(),
                DesktopImageRegion: d.desktop_image_region.into(),
                DesktopImageClip: d.desktop_image_clip.into(),
            };
        }
    }
    out
}

/// Decode a fixed-size, NUL-padded UTF-16 buffer.
fn wide_to_string(buf: &[u16]) -> Option<String> {
    let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
    if end == 0 {
        return None;
    }
    Some(String::from_utf16_lossy(&buf[..end]))
}

/// Ask Windows for a monitor's name and device path.
///
/// Fails harmlessly for targets that are not currently attached; the caller
/// treats that as "no identity available".
fn query_monitor_info(adapter_id: LUID, target_id: u32) -> Option<MonitorInfo> {
    let mut req = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
    req.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME;
    req.header.size = std::mem::size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32;
    req.header.adapterId = adapter_id;
    req.header.id = target_id;

    // SAFETY: `req` is a correctly sized and typed request packet, and the
    // header describes its own size to the API.
    let status = unsafe { DisplayConfigGetDeviceInfo(&mut req.header) };
    if status != ERROR_SUCCESS.0 as i32 {
        tracing::debug!(target_id, status, "no device name for target");
        return None;
    }

    Some(MonitorInfo {
        adapter_id: adapter_id.into(),
        id: target_id,
        friendly_name: wide_to_string(&req.monitorFriendlyDeviceName),
        device_path: wide_to_string(&req.monitorDevicePath),
        output_technology: req.outputTechnology.0,
        edid_manufacture_id: req.edidManufactureId,
        edid_product_code_id: req.edidProductCodeId,
        connector_instance: req.connectorInstance,
    })
}

/// Which paths to include in a query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueryScope {
    /// Every path the hardware could drive, active or not. This is what a
    /// saved profile needs — it has to describe monitors it is turning *off*.
    AllPaths,
    /// Only what is currently lit up. Useful for reporting current state.
    ActiveOnly,
}

impl QueryScope {
    fn flags(self) -> QUERY_DISPLAY_CONFIG_FLAGS {
        match self {
            QueryScope::AllPaths => QDC_ALL_PATHS,
            QueryScope::ActiveOnly => QDC_ONLY_ACTIVE_PATHS,
        }
    }
}

/// Read the current display topology.
pub fn query(scope: QueryScope) -> Result<DisplayConfig> {
    let mut path_count: u32 = 0;
    let mut mode_count: u32 = 0;

    // SAFETY: both out-params are valid, writable u32s.
    let status =
        unsafe { GetDisplayConfigBufferSizes(scope.flags(), &mut path_count, &mut mode_count) };
    if status != ERROR_SUCCESS {
        return Err(Error::BufferSizes(status.0));
    }

    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];

    // SAFETY: the buffers are sized exactly as GetDisplayConfigBufferSizes
    // asked, and the counts passed in match their lengths. Windows may write
    // back smaller counts, which we honour by truncating below.
    let status = unsafe {
        QueryDisplayConfig(
            scope.flags(),
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
    };
    if status != ERROR_SUCCESS {
        return Err(Error::Query(status.0));
    }

    paths.truncate(path_count as usize);
    modes.truncate(mode_count as usize);

    // Drop paths whose target is not physically present. Keeping them makes
    // SetDisplayConfig reject the whole configuration later.
    let paths: Vec<PathInfo> = paths
        .iter()
        .map(path_from_win32)
        .filter(|p| p.target.target_available)
        .collect();

    // Drop the type-zero padding entries Windows leaves in the mode array.
    let modes: Vec<ModeInfo> = modes.iter().filter_map(mode_from_win32).collect();

    // Collect monitor identities for every distinct target we saw.
    let mut monitors: Vec<MonitorInfo> = Vec::new();
    for p in &paths {
        let already = monitors
            .iter()
            .any(|m| m.id == p.target.id && m.adapter_id == p.target.adapter_id);
        if already {
            continue;
        }
        if let Some(info) = query_monitor_info(p.target.adapter_id.into(), p.target.id) {
            monitors.push(info);
        }
    }

    Ok(DisplayConfig {
        paths,
        modes,
        monitors,
    })
}

/// Hand a configuration to Windows.
///
/// Returns the raw Win32 status so callers can distinguish "rejected, try the
/// next strategy" from "something is badly wrong".
pub fn set_display_config(config: &DisplayConfig, flags: SET_DISPLAY_CONFIG_FLAGS) -> i32 {
    let paths: Vec<DISPLAYCONFIG_PATH_INFO> = config.paths.iter().map(path_to_win32).collect();
    let modes: Vec<DISPLAYCONFIG_MODE_INFO> = config.modes.iter().map(mode_to_win32).collect();

    // SAFETY: both slices live for the duration of the call, and their lengths
    // are derived from the slices themselves by the binding.
    unsafe { SetDisplayConfig(Some(&paths), Some(&modes), flags) }
}

/// Would this configuration apply cleanly, without actually applying it?
pub fn validates(config: &DisplayConfig, lenient: bool) -> bool {
    let flags = if lenient {
        VALIDATE_FLAGS_LENIENT
    } else {
        VALIDATE_FLAGS
    };
    set_display_config(config, flags) == ERROR_SUCCESS.0 as i32
}

/// Turn the display config back to whatever Windows has recorded for the
/// currently connected set of monitors. The escape hatch if a profile lands
/// somewhere unusable.
pub fn reset_to_database_current() -> i32 {
    // SAFETY: no buffers involved; this form of the call takes none.
    unsafe {
        SetDisplayConfig(
            None,
            None,
            SET_DISPLAY_CONFIG_FLAGS(SDC_APPLY.0 | SDC_USE_DATABASE_CURRENT.0),
        )
    }
}

pub fn is_success(status: i32) -> bool {
    status == ERROR_SUCCESS.0 as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_decoding_stops_at_nul() {
        let buf: Vec<u16> = "DELL U2720Q\0garbage".encode_utf16().collect();
        assert_eq!(wide_to_string(&buf).as_deref(), Some("DELL U2720Q"));
    }

    #[test]
    fn empty_wide_buffer_is_none() {
        assert_eq!(wide_to_string(&[0u16; 64]), None);
        assert_eq!(wide_to_string(&[]), None);
    }

    #[test]
    fn path_round_trips_through_win32() {
        let original = PathInfo {
            source: PathSourceInfo {
                adapter_id: Luid {
                    low: 0xdead,
                    high: 0,
                },
                id: 1,
                mode_info_idx: 0,
                status_flags: 1,
            },
            target: PathTargetInfo {
                adapter_id: Luid {
                    low: 0xdead,
                    high: 0,
                },
                id: 4231,
                mode_info_idx: 1,
                output_technology: 5,
                rotation: 1,
                scaling: 1,
                refresh_rate: Rational {
                    numerator: 143_856,
                    denominator: 2400,
                },
                scanline_ordering: 1,
                target_available: true,
                status_flags: 1,
            },
            flags: PATH_ACTIVE,
        };

        assert_eq!(path_from_win32(&path_to_win32(&original)), original);
    }

    #[test]
    fn source_mode_round_trips_through_win32() {
        let original = ModeInfo {
            id: 1,
            adapter_id: Luid { low: 7, high: 0 },
            mode: ModeKind::Source(SourceMode {
                width: 3840,
                height: 2160,
                pixel_format: 4,
                position: Point { x: -3840, y: 0 },
            }),
        };

        assert_eq!(mode_from_win32(&mode_to_win32(&original)), Some(original));
    }

    #[test]
    fn target_mode_round_trips_through_win32() {
        let original = ModeInfo {
            id: 4231,
            adapter_id: Luid { low: 7, high: 0 },
            mode: ModeKind::Target(TargetMode {
                video_signal_info: VideoSignalInfo {
                    pixel_rate: 1_185_750_000,
                    h_sync_freq: Rational {
                        numerator: 400_000,
                        denominator: 3,
                    },
                    v_sync_freq: Rational {
                        numerator: 143_856,
                        denominator: 2400,
                    },
                    active_size: Region2D { cx: 3840, cy: 2160 },
                    total_size: Region2D { cx: 4000, cy: 2222 },
                    video_standard: 0,
                    scanline_ordering: 1,
                },
            }),
        };

        assert_eq!(mode_from_win32(&mode_to_win32(&original)), Some(original));
    }

    #[test]
    fn zero_type_modes_are_dropped() {
        let padding = DISPLAYCONFIG_MODE_INFO::default();
        assert!(mode_from_win32(&padding).is_none());
    }
}

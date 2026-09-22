/* This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at http://mozilla.org/MPL/2.0/. */

//! Restoring a saved configuration onto the machine as it is right now.
//!
//! The difficulty is that adapter LUIDs are not stable. Windows hands out new
//! ones on every boot, and on driver updates, so a profile saved yesterday
//! refers to adapters that no longer exist by that name. Everything else in a
//! saved path — the source id, the target id, the monitor's device path — is
//! stable, so each strategy below reconstructs the LUIDs from a different
//! stable anchor.
//!
//! The strategy ladder is ported from Martin Kraemer's MonitorSwitcher, which
//! accumulated these fallbacks over years of bug reports against real hardware.
//! Two things are new here: candidates are checked with `SDC_VALIDATE` before
//! anything is applied, so a rejected strategy costs no screen flicker; and
//! remapping is done through an explicit old-to-new map in a single pass,
//! rather than in-place substitution that could chain one rewrite into another.

use std::collections::HashMap;

use crate::ccd;
use crate::error::{Error, Result};
use crate::model::*;

/// How a candidate configuration was reconstructed from the saved one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Strategy {
    /// Exactly as saved. Correct when nothing has changed since the save,
    /// which is the common case within a single boot.
    Verbatim,
    /// Adopt current adapter LUIDs by matching paths on (source id, target id).
    PathIds,
    /// Match monitors by their device path, which encodes the EDID and is the
    /// most stable identifier available.
    DevicePath,
    /// Match monitors by the name the user sees. Less reliable than device
    /// path when two identical monitors are attached.
    FriendlyName,
    /// Collapse everything onto the single adapter present. Only meaningful
    /// on a one-GPU machine, which is most of them.
    SingleAdapter,
}

impl Strategy {
    pub fn describe(self) -> &'static str {
        match self {
            Strategy::Verbatim => "as saved",
            Strategy::PathIds => "matched by path ids",
            Strategy::DevicePath => "matched by monitor device path",
            Strategy::FriendlyName => "matched by monitor name",
            Strategy::SingleAdapter => "collapsed onto the single adapter",
        }
    }
}

/// What a successful apply actually did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ApplyOutcome {
    pub strategy: Strategy,
    /// True when the configuration only went through with `SDC_ALLOW_CHANGES`,
    /// meaning Windows was permitted to adjust modes to make it fit. The result
    /// may differ from what was saved.
    pub lenient: bool,
}

/// One candidate configuration, and how it was derived.
struct Candidate {
    strategy: Strategy,
    config: DisplayConfig,
}

/// Rewrite adapter LUIDs through an explicit map, in one pass.
///
/// Doing this as a lookup rather than repeated in-place substitution matters:
/// substituting A->B and then B->C in sequence would wrongly carry the first
/// group into the second.
fn remap_adapters(config: &mut DisplayConfig, map: &HashMap<Luid, Luid>) {
    let lookup = |l: Luid| map.get(&l).copied().unwrap_or(l);
    for p in &mut config.paths {
        p.source.adapter_id = lookup(p.source.adapter_id);
        p.target.adapter_id = lookup(p.target.adapter_id);
    }
    for m in &mut config.modes {
        m.adapter_id = lookup(m.adapter_id);
    }
    for mon in &mut config.monitors {
        mon.adapter_id = lookup(mon.adapter_id);
    }
}

/// Adopt current adapter LUIDs by matching saved paths to current paths on the
/// pair of ids that stays constant across boots.
fn remap_by_path_ids(saved: &DisplayConfig, current: &DisplayConfig) -> Option<DisplayConfig> {
    let mut map: HashMap<Luid, Luid> = HashMap::new();

    for saved_path in &saved.paths {
        let hit = current
            .paths
            .iter()
            .find(|c| c.source.id == saved_path.source.id && c.target.id == saved_path.target.id);
        let Some(hit) = hit else { continue };
        map.entry(saved_path.source.adapter_id)
            .or_insert(hit.source.adapter_id);
        map.entry(saved_path.target.adapter_id)
            .or_insert(hit.target.adapter_id);
    }

    if map.is_empty() || map.iter().all(|(from, to)| from == to) {
        return None;
    }

    let mut out = saved.clone();
    remap_adapters(&mut out, &map);
    Some(out)
}

/// How many monitors in this list carry the given key.
fn key_occurrences(
    monitors: &[MonitorInfo],
    key: fn(&MonitorInfo) -> Option<&str>,
    needle: &str,
) -> usize {
    monitors
        .iter()
        .filter(|m| key(m).is_some_and(|k| k == needle))
        .count()
}

/// Match monitors on a stable identity, and adopt both the adapter LUID and
/// the target id that identity currently lives at.
///
/// The target id can move when a monitor is plugged into a different port, so
/// unlike [`remap_by_path_ids`] this also rewrites ids.
///
/// A key that is not unique on both sides is skipped rather than guessed. Two
/// monitors of the same model report the same friendly name, and mapping both
/// of them onto whichever one happened to be found first would silently
/// collapse two displays into one.
fn remap_by_monitor_identity(
    saved: &DisplayConfig,
    current: &DisplayConfig,
    key: fn(&MonitorInfo) -> Option<&str>,
) -> Option<DisplayConfig> {
    // (saved adapter, saved target id) -> (current adapter, current target id)
    let mut target_map: HashMap<(Luid, u32), (Luid, u32)> = HashMap::new();
    let mut adapter_map: HashMap<Luid, Luid> = HashMap::new();

    for saved_monitor in &saved.monitors {
        let Some(saved_key) = key(saved_monitor).filter(|k| !k.trim().is_empty()) else {
            continue;
        };

        if key_occurrences(&saved.monitors, key, saved_key) != 1
            || key_occurrences(&current.monitors, key, saved_key) != 1
        {
            tracing::debug!(
                key = saved_key,
                "ambiguous monitor identity; not matching on it"
            );
            continue;
        }

        let hit = current
            .monitors
            .iter()
            .find(|c| key(c).is_some_and(|k| k == saved_key));
        let Some(hit) = hit else { continue };

        target_map.insert(
            (saved_monitor.adapter_id, saved_monitor.id),
            (hit.adapter_id, hit.id),
        );
        adapter_map
            .entry(saved_monitor.adapter_id)
            .or_insert(hit.adapter_id);
    }

    if target_map.is_empty() {
        return None;
    }

    let mut out = saved.clone();

    // Rewrite target ids first, while the saved adapter LUIDs still identify
    // which monitor each entry refers to.
    for p in &mut out.paths {
        if let Some(&(_, new_id)) = target_map.get(&(p.target.adapter_id, p.target.id)) {
            p.target.id = new_id;
        }
    }
    for m in &mut out.modes {
        if m.is_target() {
            if let Some(&(_, new_id)) = target_map.get(&(m.adapter_id, m.id)) {
                m.id = new_id;
            }
        }
    }
    for mon in &mut out.monitors {
        if let Some(&(_, new_id)) = target_map.get(&(mon.adapter_id, mon.id)) {
            mon.id = new_id;
        }
    }

    remap_adapters(&mut out, &adapter_map);
    Some(out)
}

/// On a single-GPU machine, every LUID in the saved profile must be the one
/// adapter that exists now, whatever it happens to be called this boot.
fn remap_onto_single_adapter(
    saved: &DisplayConfig,
    current: &DisplayConfig,
) -> Option<DisplayConfig> {
    let current_adapters = current.adapter_ids();
    if current_adapters.len() != 1 {
        return None;
    }
    let only = current_adapters[0];

    let saved_adapters = saved.adapter_ids();
    if saved_adapters.iter().all(|&a| a == only) {
        return None;
    }

    let map: HashMap<Luid, Luid> = saved_adapters.into_iter().map(|a| (a, only)).collect();
    let mut out = saved.clone();
    remap_adapters(&mut out, &map);
    Some(out)
}

/// Build every candidate worth trying, cheapest and most faithful first.
fn candidates(saved: &DisplayConfig, current: &DisplayConfig) -> Vec<Candidate> {
    let mut out = vec![Candidate {
        strategy: Strategy::Verbatim,
        config: saved.clone(),
    }];

    let derived = [
        (Strategy::PathIds, remap_by_path_ids(saved, current)),
        (
            Strategy::DevicePath,
            remap_by_monitor_identity(saved, current, |m| m.device_path.as_deref()),
        ),
        (
            Strategy::FriendlyName,
            remap_by_monitor_identity(saved, current, |m| m.friendly_name.as_deref()),
        ),
        (
            Strategy::SingleAdapter,
            remap_onto_single_adapter(saved, current),
        ),
    ];

    for (strategy, config) in derived {
        let Some(config) = config else { continue };
        // Skip anything that produced a configuration we are already going to try.
        if out.iter().any(|c| c.config == config) {
            continue;
        }
        out.push(Candidate { strategy, config });
    }

    out
}

/// Which strategies would apply cleanly, without applying any of them.
///
/// This is a genuine dry run: `SDC_VALIDATE` asks Windows to check the
/// configuration and report, changing nothing on screen.
pub fn preflight(saved: &DisplayConfig) -> Result<Vec<(Strategy, bool)>> {
    let current = ccd::query(ccd::QueryScope::AllPaths)?;
    let mut out = Vec::new();
    for candidate in candidates(saved, &current) {
        if ccd::validates(&candidate.config, false) {
            out.push((candidate.strategy, false));
        } else if ccd::validates(&candidate.config, true) {
            out.push((candidate.strategy, true));
        }
    }
    Ok(out)
}

/// Apply a saved configuration to the current machine.
///
/// Every candidate is validated before anything is applied, so a strategy that
/// Windows would reject never reaches the screen. Strict candidates are
/// preferred over lenient ones across the board: a later strategy that
/// reproduces the saved modes exactly beats an earlier one that only fits
/// after Windows adjusts it.
pub fn apply(saved: &DisplayConfig) -> Result<ApplyOutcome> {
    let current = ccd::query(ccd::QueryScope::AllPaths)?;
    let candidates = candidates(saved, &current);

    tracing::debug!(
        candidates = candidates.len(),
        saved_paths = saved.paths.len(),
        saved_active = saved.active_path_count(),
        "applying display configuration"
    );

    let mut lenient_fallback: Option<&Candidate> = None;

    for candidate in &candidates {
        if ccd::validates(&candidate.config, false) {
            let status = ccd::set_display_config(&candidate.config, ccd::APPLY_FLAGS);
            if ccd::is_success(status) {
                tracing::info!(strategy = ?candidate.strategy, "applied");
                return Ok(ApplyOutcome {
                    strategy: candidate.strategy,
                    lenient: false,
                });
            }
            // Validation passed but the apply did not. Rare, but the machine
            // can change underneath us between the two calls.
            tracing::warn!(strategy = ?candidate.strategy, status, "validated but failed to apply");
        } else if lenient_fallback.is_none() && ccd::validates(&candidate.config, true) {
            lenient_fallback = Some(candidate);
        }
    }

    if let Some(candidate) = lenient_fallback {
        let status = ccd::set_display_config(&candidate.config, ccd::APPLY_FLAGS_LENIENT);
        if ccd::is_success(status) {
            tracing::info!(
                strategy = ?candidate.strategy,
                "applied with SDC_ALLOW_CHANGES; modes may differ from the saved profile"
            );
            return Ok(ApplyOutcome {
                strategy: candidate.strategy,
                lenient: true,
            });
        }
    }

    // Nothing validated. Windows sometimes rejects a configuration at validate
    // time that it will nonetheless accept, so make a genuine attempt at each
    // candidate before giving up.
    tracing::warn!("no candidate validated; attempting each one directly");
    let mut last_status = 0;
    for candidate in &candidates {
        for flags in [ccd::APPLY_FLAGS, ccd::APPLY_FLAGS_LENIENT] {
            let status = ccd::set_display_config(&candidate.config, flags);
            if ccd::is_success(status) {
                let lenient = flags == ccd::APPLY_FLAGS_LENIENT;
                tracing::info!(strategy = ?candidate.strategy, lenient, "applied on direct attempt");
                return Ok(ApplyOutcome {
                    strategy: candidate.strategy,
                    lenient,
                });
            }
            last_status = status;
        }
    }

    Err(Error::ApplyFailed { last_status })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luid(low: u32) -> Luid {
        Luid { low, high: 0 }
    }

    fn monitor(adapter: u32, id: u32, path: &str, name: &str) -> MonitorInfo {
        MonitorInfo {
            adapter_id: luid(adapter),
            id,
            friendly_name: Some(name.to_string()),
            device_path: Some(path.to_string()),
            output_technology: 5,
            edid_manufacture_id: 1,
            edid_product_code_id: 2,
            connector_instance: 0,
        }
    }

    fn path(adapter: u32, source_id: u32, target_id: u32, active: bool) -> PathInfo {
        PathInfo {
            source: PathSourceInfo {
                adapter_id: luid(adapter),
                id: source_id,
                mode_info_idx: 0,
                status_flags: 0,
            },
            target: PathTargetInfo {
                adapter_id: luid(adapter),
                id: target_id,
                mode_info_idx: 1,
                output_technology: 5,
                rotation: 1,
                scaling: 1,
                refresh_rate: Rational {
                    numerator: 60,
                    denominator: 1,
                },
                scanline_ordering: 1,
                target_available: true,
                status_flags: 0,
            },
            flags: if active { PATH_ACTIVE } else { 0 },
        }
    }

    fn source_mode(adapter: u32, id: u32) -> ModeInfo {
        ModeInfo {
            id,
            adapter_id: luid(adapter),
            mode: ModeKind::Source(SourceMode {
                width: 1920,
                height: 1080,
                pixel_format: 4,
                position: Point { x: 0, y: 0 },
            }),
        }
    }

    fn target_mode(adapter: u32, id: u32) -> ModeInfo {
        ModeInfo {
            id,
            adapter_id: luid(adapter),
            mode: ModeKind::Target(TargetMode {
                video_signal_info: VideoSignalInfo {
                    pixel_rate: 148_500_000,
                    h_sync_freq: Rational {
                        numerator: 67_500,
                        denominator: 1,
                    },
                    v_sync_freq: Rational {
                        numerator: 60,
                        denominator: 1,
                    },
                    active_size: Region2D { cx: 1920, cy: 1080 },
                    total_size: Region2D { cx: 2200, cy: 1125 },
                    video_standard: 0,
                    scanline_ordering: 1,
                },
            }),
        }
    }

    /// A two-monitor profile on one adapter.
    fn saved_config(adapter: u32) -> DisplayConfig {
        DisplayConfig {
            paths: vec![path(adapter, 0, 100, true), path(adapter, 1, 200, true)],
            modes: vec![
                source_mode(adapter, 0),
                target_mode(adapter, 100),
                source_mode(adapter, 1),
                target_mode(adapter, 200),
            ],
            monitors: vec![
                monitor(adapter, 100, "\\\\?\\DISPLAY#DEL4231#LEFT", "DELL U2720Q"),
                monitor(adapter, 200, "\\\\?\\DISPLAY#DEL4232#RIGHT", "DELL U2719D"),
            ],
        }
    }

    #[test]
    fn path_id_match_adopts_current_adapter() {
        let saved = saved_config(0xaaa);
        let current = saved_config(0xbbb);

        let out = remap_by_path_ids(&saved, &current).expect("should remap");

        assert_eq!(out.adapter_ids(), vec![luid(0xbbb)]);
        // Everything other than the LUID is untouched.
        assert_eq!(out.paths[0].source.id, 0);
        assert_eq!(out.paths[1].target.id, 200);
        assert_eq!(out.modes.len(), saved.modes.len());
    }

    #[test]
    fn path_id_match_declines_when_adapter_is_unchanged() {
        let saved = saved_config(0xaaa);
        assert!(remap_by_path_ids(&saved, &saved).is_none());
    }

    #[test]
    fn path_id_match_declines_when_no_path_matches() {
        let saved = saved_config(0xaaa);
        let mut current = saved_config(0xbbb);
        current.paths[0].target.id = 999;
        current.paths[1].target.id = 998;

        assert!(remap_by_path_ids(&saved, &current).is_none());
    }

    #[test]
    fn device_path_match_follows_a_monitor_to_a_new_port() {
        let saved = saved_config(0xaaa);

        // Same two monitors, new adapter, and the first one moved to a port
        // that reports a different target id.
        let mut current = saved_config(0xbbb);
        current.paths[0].target.id = 777;
        current.modes[1].id = 777;
        current.monitors[0].id = 777;

        let out = remap_by_monitor_identity(&saved, &current, |m| m.device_path.as_deref())
            .expect("should remap");

        assert_eq!(out.adapter_ids(), vec![luid(0xbbb)]);
        assert_eq!(
            out.paths[0].target.id, 777,
            "target id should follow the monitor"
        );
        assert_eq!(out.paths[1].target.id, 200, "unmoved monitor keeps its id");

        let target_ids: Vec<u32> = out
            .modes
            .iter()
            .filter(|m| m.is_target())
            .map(|m| m.id)
            .collect();
        assert_eq!(target_ids, vec![777, 200], "target modes follow too");

        let source_ids: Vec<u32> = out
            .modes
            .iter()
            .filter(|m| m.is_source())
            .map(|m| m.id)
            .collect();
        assert_eq!(
            source_ids,
            vec![0, 1],
            "source modes are not target ids and must not move"
        );
    }

    #[test]
    fn identity_match_declines_when_nothing_is_recognizable() {
        let saved = saved_config(0xaaa);
        let mut current = saved_config(0xbbb);
        current.monitors[0].device_path = Some("\\\\?\\DISPLAY#OTHER#1".into());
        current.monitors[1].device_path = Some("\\\\?\\DISPLAY#OTHER#2".into());

        assert!(
            remap_by_monitor_identity(&saved, &current, |m| m.device_path.as_deref()).is_none()
        );
    }

    /// A matched pair of identical monitors plus one other, which is a very
    /// ordinary desk and the layout this was first caught on.
    fn config_with_identical_pair(adapter: u32) -> DisplayConfig {
        let mut config = saved_config(adapter);
        config.paths.push(path(adapter, 2, 300, true));
        config.modes.push(source_mode(adapter, 2));
        config.modes.push(target_mode(adapter, 300));

        // Two monitors of the same model report the same friendly name but
        // have distinct device paths.
        config.monitors = vec![
            monitor(adapter, 100, "\\\\?\\DISPLAY#DEL42D3#UID100", "DELL U2724D"),
            monitor(adapter, 200, "\\\\?\\DISPLAY#BNQ78EC#UID200", "BenQ GL2780"),
            monitor(adapter, 300, "\\\\?\\DISPLAY#DEL42D0#UID300", "DELL U2724D"),
        ];
        config
    }

    #[test]
    fn duplicate_monitor_names_are_never_collapsed_onto_one() {
        let saved = config_with_identical_pair(0xaaa);
        let current = config_with_identical_pair(0xbbb);

        // Name matching must decline entirely for the ambiguous Dells. The
        // BenQ is unique, so it alone is matchable.
        let out = remap_by_monitor_identity(&saved, &current, |m| m.friendly_name.as_deref())
            .expect("the unique monitor should still match");

        let target_ids: Vec<u32> = out.paths.iter().map(|p| p.target.id).collect();
        assert_eq!(
            target_ids,
            vec![100, 200, 300],
            "the two identical Dells must keep their own ids"
        );
    }

    #[test]
    fn device_paths_disambiguate_identical_models() {
        let saved = config_with_identical_pair(0xaaa);

        // Same three monitors on a new adapter, with the second Dell moved to
        // a port that reports a different target id.
        let mut current = config_with_identical_pair(0xbbb);
        current.paths[2].target.id = 777;
        current.monitors[2].id = 777;

        let out = remap_by_monitor_identity(&saved, &current, |m| m.device_path.as_deref())
            .expect("device paths are unique and should match");

        let target_ids: Vec<u32> = out.paths.iter().map(|p| p.target.id).collect();
        assert_eq!(
            target_ids,
            vec![100, 200, 777],
            "only the monitor that actually moved should be rewritten"
        );
        assert_eq!(out.adapter_ids(), vec![luid(0xbbb)]);
    }

    #[test]
    fn an_unchanged_machine_with_identical_monitors_needs_no_remapping() {
        // The regression this guards: on a machine where nothing has changed,
        // name matching used to produce a *different* configuration by mapping
        // both Dells onto whichever one it found first.
        let config = config_with_identical_pair(0xaaa);
        let candidates = candidates(&config, &config);

        assert_eq!(
            candidates.len(),
            1,
            "expected only the verbatim candidate, got: {:?}",
            candidates.iter().map(|c| c.strategy).collect::<Vec<_>>()
        );
    }

    #[test]
    fn identity_match_ignores_blank_keys() {
        let mut saved = saved_config(0xaaa);
        saved.monitors[0].friendly_name = Some("   ".into());
        saved.monitors[1].friendly_name = None;

        let current = saved_config(0xbbb);

        assert!(
            remap_by_monitor_identity(&saved, &current, |m| m.friendly_name.as_deref()).is_none(),
            "blank and missing names must not be treated as a match"
        );
    }

    #[test]
    fn single_adapter_collapse_unifies_a_split_profile() {
        // A profile saved when the machine reported two adapters.
        let mut saved = saved_config(0xaaa);
        saved.paths[1].source.adapter_id = luid(0xccc);
        saved.paths[1].target.adapter_id = luid(0xccc);
        saved.modes[2].adapter_id = luid(0xccc);
        saved.modes[3].adapter_id = luid(0xccc);

        let current = saved_config(0xbbb);

        let out = remap_onto_single_adapter(&saved, &current).expect("should collapse");
        assert_eq!(out.adapter_ids(), vec![luid(0xbbb)]);
    }

    #[test]
    fn single_adapter_collapse_declines_with_two_adapters_present() {
        let saved = saved_config(0xaaa);
        let mut current = saved_config(0xbbb);
        current.paths[1].source.adapter_id = luid(0xccc);
        current.paths[1].target.adapter_id = luid(0xccc);

        assert!(remap_onto_single_adapter(&saved, &current).is_none());
    }

    #[test]
    fn remap_does_not_chain_one_substitution_into_another() {
        // The bug this guards against: rewriting A->B and then B->C in
        // sequence would land the first group on C.
        let mut config = saved_config(0xaaa);
        config.paths[1].source.adapter_id = luid(0xbbb);
        config.paths[1].target.adapter_id = luid(0xbbb);
        config.modes[2].adapter_id = luid(0xbbb);
        config.modes[3].adapter_id = luid(0xbbb);

        let map = HashMap::from([(luid(0xaaa), luid(0xbbb)), (luid(0xbbb), luid(0xccc))]);
        remap_adapters(&mut config, &map);

        assert_eq!(config.paths[0].source.adapter_id, luid(0xbbb));
        assert_eq!(config.paths[1].source.adapter_id, luid(0xccc));
    }

    #[test]
    fn candidate_list_starts_verbatim_and_has_no_duplicates() {
        let saved = saved_config(0xaaa);
        let current = saved_config(0xbbb);

        let candidates = candidates(&saved, &current);

        assert_eq!(candidates[0].strategy, Strategy::Verbatim);
        assert_eq!(candidates[0].config, saved);

        for i in 0..candidates.len() {
            for j in (i + 1)..candidates.len() {
                assert_ne!(
                    candidates[i].config, candidates[j].config,
                    "{:?} and {:?} produced identical configurations",
                    candidates[i].strategy, candidates[j].strategy
                );
            }
        }
    }

    #[test]
    fn unchanged_machine_yields_only_the_verbatim_candidate() {
        let saved = saved_config(0xaaa);
        let candidates = candidates(&saved, &saved);
        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].strategy, Strategy::Verbatim);
    }

    #[test]
    fn a_profile_is_recognised_as_active_across_a_reboot() {
        // Same monitors, new adapter LUIDs and new target ids, as after a
        // reboot. The saved profile should still be recognised as the one on
        // screen, because device paths do not change.
        let saved = saved_config(0xaaa);
        let mut current = saved_config(0xbbb);
        current.paths[0].target.id = 777;
        current.monitors[0].id = 777;

        assert!(saved.has_same_active_monitors(&current));
    }

    #[test]
    fn profiles_lighting_different_monitors_are_distinguished() {
        // Play: everything on. Work: only the first monitor on.
        let play = saved_config(0xaaa);
        let mut work = saved_config(0xaaa);
        work.paths[1].flags = 0;

        assert!(!play.has_same_active_monitors(&work));
        assert!(!work.has_same_active_monitors(&play));
        assert!(work.has_same_active_monitors(&work));
    }

    #[test]
    fn a_configuration_with_nothing_active_matches_nothing() {
        let mut dark = saved_config(0xaaa);
        for p in &mut dark.paths {
            p.flags = 0;
        }
        assert!(
            !dark.has_same_active_monitors(&dark),
            "no active monitors is not a meaningful identity to match on"
        );
    }

    #[test]
    fn identical_monitors_get_distinguishable_labels() {
        // Two monitors reporting the same model name, which is the normal case
        // for a matched pair.
        let mut config = saved_config(0xaaa);
        config.monitors[1].friendly_name = Some("DELL U2720Q".into());

        assert_eq!(
            config.active_monitor_labels(),
            vec!["DELL U2720Q #1", "DELL U2720Q #2"]
        );
    }

    #[test]
    fn distinct_monitors_are_labelled_plainly() {
        let config = saved_config(0xaaa);
        assert_eq!(
            config.active_monitor_labels(),
            vec!["DELL U2720Q", "DELL U2719D"],
            "unique names should not pick up a suffix"
        );
    }

    #[test]
    fn disabled_monitors_survive_remapping() {
        // The work profile: three monitors known, only one active.
        let mut saved = saved_config(0xaaa);
        saved.paths.push(path(0xaaa, 2, 300, false));
        saved.paths[1].flags = 0;
        saved.monitors.push(monitor(
            0xaaa,
            300,
            "\\\\?\\DISPLAY#DEL4233#THIRD",
            "DELL U2719D",
        ));

        let current = saved_config(0xbbb);
        let out = remap_by_path_ids(&saved, &current).expect("should remap");

        assert_eq!(out.paths.len(), 3, "inactive paths must be preserved");
        assert_eq!(out.active_path_count(), 1);
        assert_eq!(
            out.paths[2].target.adapter_id,
            luid(0xbbb),
            "an unmatched path still picks up the remapped adapter"
        );
    }
}

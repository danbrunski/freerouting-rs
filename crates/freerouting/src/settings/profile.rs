//! Routing profiles (N01, D10): `legacy` and `enhanced`.
//!
//! A profile is a named set of settings defaults. It is the user's starting
//! point, not a hidden mode: `legacy` gives results identical to Java
//! Freerouting at the pin, `enhanced` (the default) adds the accepted
//! improvements. Below the profile every stage and option stays
//! individually configurable; the profile only decides what each one
//! starts at.
//!
//! # The key and the apply order
//!
//! `router.profile` (`--profile`, `--router.profile=`,
//! `FREEROUTING__ROUTER__PROFILE`, `"router": {"profile": ...}`) names the
//! profile; unset is [`Profile::Enhanced`]. [`resolve`](super::resolve)
//! reads it from the highest-priority source that sets it, then merges the
//! profile's differences as [`Source::Profile`](super::Source::Profile) at
//! priority 5: after Java's defaults (0) and before the settings file (10),
//! the DSN (20), the rules file, the environment, the CLI and the testing
//! sources. So an explicit key from any source beats the profile, and a
//! settings file that sets every key routes the same under both profiles.
//! The board tuning of C03 (`apply_board_specific`) runs after the merge, as
//! for any other source.
//!
//! An unknown profile is a settings error that names the valid values
//! ([`UnknownProfile`]): `prepare_job` fails with it and the CLI's
//! `--profile` refuses it as a usage error. [`resolve`](super::resolve)
//! itself applies no profile then and reports it in
//! [`Resolved::profile`](super::Resolved::profile).
//!
//! # The table
//!
//! `legacy` has no differences: it is Java's defaults. [`ENHANCED`] lists
//! what `enhanced` changes, one [`Difference`] per settings key, each naming
//! the measurement that accepted it (N05 adds them). N06.13 added the 30
//! Java-bug fixes (`java_quirks.<id> = false`) that N06's batches accepted:
//! they were measured under `legacy`, and
//! `docs/roadmap/prereg/N06.13-promotion.json` registers their confirmation
//! under `enhanced`. Its second round (2026-10-20) added five more:
//! `simplex_prev_division_line` (inert, on its tests), N06.9's two optimizer
//! fixes, `shove_via_into_pending_piece` (N06.15) and
//! `any_angle_room_side_index` (N06.16); `N06.13-promotion-2.json` registers
//! that round's confirmation. Its third round (2026-10-20) added N06.14's two
//! simplex cutout fixes, `cutout_hole_crosses_border` and
//! `simplex_octagon_half_corner`, accepted together by
//! `docs/roadmap/prereg/N06.14b.json`, whose candidate arm was this table's
//! `enhanced`. Q17.4 (2026-10-20) added `neck_down.widen_back = true` on top
//! of the necked retry at 300 um, and Q16.5 (decided 2026-10-20) added
//! `cleanup.remove_stubs = true`. Adding one is one entry
//! here plus one line in the "Profile `enhanced`" section of
//! `docs/DIVERGENCES.md`; `tests/settings_profile.rs` checks that every
//! entry is a valid settings key and value, and that `enhanced` sets it.
//!
//! # Parity pins `legacy`
//!
//! Every parity test and gate (A14, connection parity, the probes,
//! `tests/settings_defaults.rs`) sets `router.profile = legacy` and checks
//! it with [`require_legacy`], so a parity run without the pin fails loudly
//! instead of comparing `enhanced` with Java. [`with_enhanced_differences`]
//! lets a test give `enhanced` a dummy difference on the current thread to
//! prove that the pins hold.

use std::cell::RefCell;
use std::fmt;

use super::fields::{RouterSettings, SettingsPatch};

/// The settings key, as the CLI and the logs name it.
pub const SETTINGS_KEY: &str = "router.profile";

/// A routing profile.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Profile {
    /// Java Freerouting's behaviour: the parity baseline.
    Legacy,
    /// The accepted improvements (the default).
    #[default]
    Enhanced,
}

/// One setting a profile changes from Java's default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Difference {
    /// The canonical settings path, without `router.` (`autorouter.sweep`).
    pub path: &'static str,
    /// The value, as the CLI writes it (`--router.<path>=<value>`).
    pub value: &'static str,
    /// The measurement that accepted it (a checklist item and its numbers).
    pub measurement: &'static str,
}

/// What `enhanced` changes from `legacy`, one entry per measurement N05
/// accepted (the promotion procedure is in `docs/development/engine.md`);
/// each entry has a line in DIVERGENCES, "Profile `enhanced`".
pub const ENHANCED: &[Difference] = &[
    Difference {
        path: "power_layer_traces",
        value: "none",
        measurement: "N05.3 (docs/roadmap/prereg/N05-power-layer-traces.json): no trace on a \
                      power layer; KiCad unconnected 256 -> 256 against own_plane, 257 -> 256 \
                      against Java's layer, power-layer traces 6 -> 0",
    },
    Difference {
        path: "neck_width_um",
        value: "300",
        measurement: "N05.3 (docs/roadmap/prereg/N05-neck-down.json): Board Setup widths plus \
                      Java's necked retry against class narrowing; KiCad unconnected 103 -> 106 \
                      (set noise 17.14), copper under the Board Setup width where the narrowing \
                      acted 848.9 -> 310.4 mm",
    },
    Difference {
        path: "via_clearance",
        value: "net_class",
        measurement: "N05.3 (docs/roadmap/prereg/N05-via-clearance-netclass.json): a routed \
                      via takes its net class's clearance (Q14); KiCad unconnected 263 -> 261 \
                      (set noise 42.70), KiCad net-class via clearance errors 2 -> 1, canary-pwr's \
                      plugin path 1 -> 0",
    },
    Difference {
        path: "neck_down.widen_back",
        value: "true",
        measurement: "N05.3 from Q17.4 (docs/roadmap/prereg/Q17.4.json, sha256 b3d4ad49..., \
                      under enhanced): the necked retry's narrow traces widened back to the \
                      class width where they fit; gate PASS, KiCad unconnected 164 -> 168 (set \
                      noise 15.42), narrowed copper on the wide classes 461.8 -> 400.6 mm, no \
                      hard-guard breach",
    },
    Difference {
        path: "board_minimums.from_classes",
        value: "true",
        measurement: "decided 2026-10-20 (Q18.2: routing never breaks the board's minimum \
                      manufacturing rules; with no board minimum supplied, the narrowest class \
                      width of the design is the minimum track width); its cost measured in \
                      Q18.6 (docs/roadmap/prereg/Q18.json)",
    },
    Difference {
        path: "cleanup.remove_stubs",
        value: "true",
        measurement: "N05.3 from Q16.5 (docs/roadmap/prereg/Q16-5.json, amendment 2, sha256 \
                      a17c579f..., under enhanced): the final stub cleanup; canary-pwr dangling \
                      vias 8 -> 0, KiCad unconnected 26 -> 26, no new DRC item, deterministic \
                      (decided 2026-10-20: the gate's one hard-guard breach, sonde-xilinx \
                      clearance = 1, is in both arms and comes from copper text the DSN lacks, \
                      Q22)",
    },
    Difference {
        path: "java_quirks.init_failure_room_leak",
        value: "false",
        measurement: N06_5_MAZE,
    },
    Difference {
        path: "java_quirks.failed_attempt_ripped_carryover",
        value: "false",
        measurement: N06_5_MAZE,
    },
    Difference {
        path: "java_quirks.ortho_zero_width_room",
        value: "false",
        measurement: N06_5_MAZE,
    },
    Difference {
        path: "java_quirks.edge_corner_not_advanced",
        value: "false",
        measurement: N06_5_MAZE,
    },
    Difference {
        path: "java_quirks.connect_to_trace_swapped_width",
        value: "false",
        measurement: N06_5_MAZE,
    },
    Difference {
        path: "java_quirks.fanout_stale_drill_carry",
        value: "false",
        measurement: N06_6_FANOUT,
    },
    Difference {
        path: "java_quirks.drill_destination_room_index",
        value: "false",
        measurement: N06_6_FANOUT,
    },
    Difference {
        path: "java_quirks.fanout_pin_order_names",
        value: "false",
        measurement: N06_6_FANOUT,
    },
    Difference {
        path: "java_quirks.in_front_of_pad_typo",
        value: "false",
        measurement: N06_7_SHOVE,
    },
    Difference {
        path: "java_quirks.store_trace_class_self_compare",
        value: "false",
        measurement: N06_7_SHOVE,
    },
    Difference {
        path: "java_quirks.rational_perp_projection",
        value: "false",
        measurement: N06_7_SHOVE,
    },
    Difference {
        path: "java_quirks.in_front_of_pad_side0_flank",
        value: "false",
        measurement: N06_7_SHOVE,
    },
    Difference {
        path: "java_quirks.smooth_loses_trace",
        value: "false",
        measurement: N06_8_PULL_TIGHT,
    },
    Difference {
        path: "java_quirks.pull_tight_reused_lines_change",
        value: "false",
        measurement: N06_8_PULL_TIGHT,
    },
    Difference {
        path: "java_quirks.dsn_via_alias",
        value: "false",
        measurement: N06_10_INPUTS,
    },
    Difference {
        path: "java_quirks.dsn_class_class_pairs",
        value: "false",
        measurement: N06_10_INPUTS,
    },
    Difference {
        path: "java_quirks.dsn_shape_bbox",
        value: "false",
        measurement: N06_10_INPUTS,
    },
    Difference {
        path: "java_quirks.dsn_unit_ignored",
        value: "false",
        measurement: N06_10_INPUTS,
    },
    Difference {
        path: "java_quirks.ses_reader_scale",
        value: "false",
        measurement: N06_10_INPUTS,
    },
    Difference {
        path: "java_quirks.stats_line_ab_length",
        value: "false",
        measurement: N06_10_INPUTS,
    },
    Difference {
        path: "java_quirks.route_result_int_via_ratio",
        value: "false",
        measurement: N06_10_INPUTS,
    },
    Difference {
        path: "java_quirks.drc_units_10000_per_mm",
        value: "false",
        measurement: N06_10_INPUTS,
    },
    Difference {
        path: "java_quirks.snapshot_drops_compensation",
        value: "false",
        measurement: N06_11_INTERACTIVE,
    },
    Difference {
        path: "java_quirks.rules_matrix_trees_not_rebuilt",
        value: "false",
        measurement: N06_11_INTERACTIVE,
    },
    Difference {
        path: "java_quirks.move_drill_tidy_region",
        value: "false",
        measurement: N06_11_INTERACTIVE,
    },
    Difference {
        path: "java_quirks.route_change_layer_no_via",
        value: "false",
        measurement: N06_11_INTERACTIVE,
    },
    Difference {
        path: "java_quirks.route_connect_inner_break",
        value: "false",
        measurement: N06_11_INTERACTIVE,
    },
    Difference {
        path: "java_quirks.select_extension_ignores_filter",
        value: "false",
        measurement: N06_11_INTERACTIVE,
    },
    Difference {
        path: "java_quirks.rules_unknown_layer_scope",
        value: "false",
        measurement: N06_12_RULES_SCOPE,
    },
    Difference {
        path: "java_quirks.any_angle_skip_length_squared",
        value: "false",
        measurement: N06_12_SKIP_LENGTH,
    },
    // N06.13's second round (2026-10-20): the fixes accepted after the first.
    Difference {
        path: "java_quirks.simplex_prev_division_line",
        value: "false",
        measurement: N06_4_PREV_DIVISION_LINE,
    },
    Difference {
        path: "java_quirks.opt_first_pass_stops",
        value: "false",
        measurement: N06_9_OPTIMIZER,
    },
    Difference {
        path: "java_quirks.opt_equal_key_skip",
        value: "false",
        measurement: N06_9_OPTIMIZER,
    },
    Difference {
        path: "java_quirks.shove_via_into_pending_piece",
        value: "false",
        measurement: N06_15_PENDING_PIECE,
    },
    Difference {
        path: "java_quirks.any_angle_room_side_index",
        value: "false",
        measurement: N06_16_ROOM_SIDES,
    },
    // N06.13's third round (2026-10-20): N06.14's two simplex cutout fixes.
    Difference {
        path: "java_quirks.cutout_hole_crosses_border",
        value: "false",
        measurement: N06_14_SIMPLEX_CUTOUTS,
    },
    Difference {
        path: "java_quirks.simplex_octagon_half_corner",
        value: "false",
        measurement: N06_14_SIMPLEX_CUTOUTS,
    },
];

// The N06 Java-bug fixes promoted by N06.13 (2026-10-19), one measurement per
// batch. They were measured under `legacy`; the confirmation under `enhanced` is
// docs/roadmap/prereg/N06.13-promotion.json.

const N06_5_MAZE: &str = "N06.13 from N06.5 (docs/roadmap/prereg/N06.5.json, batch M, under \
                          legacy): gate PASS, every session identical to Java's on 5 DSN rows \
                          and canary-pwr's plugin path, total 38 -> 38 (set noise \
                          10.95)";
const N06_6_FANOUT: &str = "N06.13 from N06.6 (docs/roadmap/prereg/N06.6.json, batch F, under \
                            legacy, fanout on): gate PASS, every session identical to Java's on \
                            6 DSN rows and canary-pwr's plugin path, total 100 -> 100 (set noise \
                            11.14), 307 fanout retries, 0 succeeded in either arm";
const N06_7_SHOVE: &str = "N06.13 from N06.7 (docs/roadmap/prereg/N06.7.json, batch S with arm \
                           all_flank, under legacy): no registered board reaches the branch \
                           (every flip counter 0, every session identical to Java's on 5 DSN \
                           rows and canary-pwr's plugin path); accepted on its tests";
const N06_8_PULL_TIGHT: &str = "N06.13 from N06.8 (docs/roadmap/prereg/N06.8.json, batch P, \
                                under legacy): gate PASS, both counters 0 and every session \
                                identical to Java's on 6 DSN rows and canary-pwr's plugin path; \
                                accepted on its tests";
const N06_10_INPUTS: &str = "N06.13 from N06.10 (docs/roadmap/prereg/N06.10.json, batch I, \
                             under legacy): 40 runs with every session identical to Java's, the \
                             DSN counters 0 on 203 corpus boards, route_result_zero_test_changes \
                             0, reference_optimize 3 passed, canary-pwr's plugin path one session \
                             for every arm; accepted on its tests";
const N06_11_INTERACTIVE: &str = "N06.13 from N06.11 (docs/roadmap/prereg/N06.11.json, batch U, \
                                  under legacy): 32 runs with every session identical to \
                                  Java's, compensated_snapshot_restores 0, canary-pwr's plugin \
                                  path one session for every arm; interactive code that batch \
                                  runs never call, accepted on its tests";
const N06_12_RULES_SCOPE: &str = "N06.13 from N06.12 (decided 2026-10-18): a rules-file layer \
                                  scope naming an unknown layer is ignored with a warning; \
                                  rules files only (-dr), accepted on its test \
                                  (unknown_layer_scope_switch_both_behaviours)";
const N06_12_SKIP_LENGTH: &str = "N06.13 from N06.12 (docs/roadmap/prereg/N06.12-ripup-pass.json, \
                                  reported arm, under legacy): any_angle_skip_length_flips 0 and \
                                  every session identical to Java's on 6 DSN rows and canary-pwr's \
                                  plugin path; accepted on its test";

// N06.13's second round (2026-10-20). N06.9, N06.15 and N06.16 measured under
// `enhanced` with the first round's 30 fixes; the confirmation of the round is
// docs/roadmap/prereg/N06.13-promotion-2.json.

const N06_4_PREV_DIVISION_LINE: &str = "N06.13 from N06.4 (docs/roadmap/prereg/N06.4.json, batch \
                                        G, and N06.14's run at every Area split, \
                                        docs/roadmap/prereg/N06.14.json): \
                                        simplex_prev_division_line_reached 0 in 40 runs and 0 \
                                        in the corpus inventory; the fix gives Java's pieces \
                                        (the_simplex_switch_changes_no_piece); accepted on its \
                                        tests";
const N06_9_OPTIMIZER: &str = "N06.13 from N06.9 (docs/roadmap/prereg/N06.9.json, batch O, under \
                               enhanced): gate PASS, every row's four arms one session, total \
                               76 -> 76 (set noise 19.95) over 6 rows, canary-pwr's plugin path \
                               among them; neutral";
const N06_15_PENDING_PIECE: &str = "N06.13 from N06.15 (docs/roadmap/prereg/N06.15.json, under \
                                    enhanced and legacy): gate PASS in both pairs; enhanced \
                                    total 312 -> 319 (set noise 69.24), Protein 89 -> 96 \
                                    within its band, no new clearance violation or short on 7 \
                                    DSN rows and canary-pwr's plugin path";
const N06_16_ROOM_SIDES: &str = "N06.13 from N06.16 (docs/roadmap/prereg/N06.16.json, under \
                                 enhanced): J2-any and rpi_splitter-any abort at the 8000000 kB \
                                 cap under Java's indexes and route with 0 unrouted, 0 clearance \
                                 under the fix (the abort rule); the other 6 rows gate PASS, \
                                 0 -> 0, sessions identical";

// N06.13's third round (2026-10-20). N06.14b measured the pair under `enhanced`
// with the first two rounds' 35 fixes; its candidate arm is this table, so that
// run is the round's confirmation.

const N06_14_SIMPLEX_CUTOUTS: &str = "N06.13 from N06.14 (docs/roadmap/prereg/N06.14b.json, \
                                      amendment 1, both switches together under enhanced): \
                                      gate PASS, total 196 -> 208 (set noise 21.40), canary-pwr's \
                                      DSN 122 -> 129 and plugin path 39 -> 44 within their bands \
                                      of 10, no hard-guard or guard breach on 8 rows";

/// `legacy` changes nothing.
const LEGACY: &[Difference] = &[];

thread_local! {
    static ENHANCED_OVERRIDE: RefCell<Option<&'static [Difference]>> =
        const { RefCell::new(None) };
}

/// Runs `f` with `differences` in place of [`ENHANCED`] on this thread.
/// For tests only: it proves that a test pinned to `legacy` does not move
/// when `enhanced` does. The table is restored when `f` returns or panics.
#[doc(hidden)]
pub fn with_enhanced_differences<R>(
    differences: &'static [Difference],
    f: impl FnOnce() -> R,
) -> R {
    struct Restore(Option<&'static [Difference]>);
    impl Drop for Restore {
        fn drop(&mut self) {
            ENHANCED_OVERRIDE.with(|cell| *cell.borrow_mut() = self.0);
        }
    }
    let previous = ENHANCED_OVERRIDE.with(|cell| cell.borrow_mut().replace(differences));
    let _restore = Restore(previous);
    f()
}

impl Profile {
    /// Every profile, in the order the interfaces list them.
    pub const ALL: [Profile; 2] = [Profile::Legacy, Profile::Enhanced];

    /// The settings value (`legacy`, `enhanced`).
    pub fn name(self) -> &'static str {
        match self {
            Profile::Legacy => "legacy",
            Profile::Enhanced => "enhanced",
        }
    }

    /// The name shown in the GUI and the plugin (`Legacy`, `Enhanced`).
    pub fn label(self) -> &'static str {
        match self {
            Profile::Legacy => "Legacy",
            Profile::Enhanced => "Enhanced",
        }
    }

    /// One line on what the profile is for.
    pub fn description(self) -> &'static str {
        match self {
            Profile::Legacy => "Results identical to Java Freerouting.",
            Profile::Enhanced => "The accepted improvements over Java Freerouting; the default.",
        }
    }

    /// Reads a settings value, ignoring ASCII case and surrounding blanks.
    pub fn parse(text: &str) -> Result<Profile, UnknownProfile> {
        let wanted = text.trim();
        Profile::ALL
            .into_iter()
            .find(|p| p.name().eq_ignore_ascii_case(wanted))
            .ok_or_else(|| UnknownProfile {
                given: text.to_string(),
            })
    }

    /// The profile a patch names; unset is the default, `enhanced`.
    pub fn of_patch(patch: &SettingsPatch) -> Result<Profile, UnknownProfile> {
        patch
            .profile
            .as_deref()
            .map_or(Ok(Profile::default()), Profile::parse)
    }

    /// The profile of resolved settings; unset is `enhanced`.
    pub fn of_settings(settings: &RouterSettings) -> Result<Profile, UnknownProfile> {
        settings
            .profile
            .as_deref()
            .map_or(Ok(Profile::default()), Profile::parse)
    }

    /// The settings this profile changes from Java's defaults.
    pub fn differences(self) -> &'static [Difference] {
        match self {
            Profile::Legacy => LEGACY,
            Profile::Enhanced => ENHANCED_OVERRIDE.with(|cell| cell.borrow().unwrap_or(ENHANCED)),
        }
    }

    /// The differences as a settings patch (the source merged at priority
    /// 5). Panics on an entry that is no valid setting, which
    /// `tests/settings_profile.rs` rules out for the built-in table.
    pub fn patch(self) -> SettingsPatch {
        let mut patch = SettingsPatch::default();
        for difference in self.differences() {
            if let Err(e) = patch.set_path(difference.path, difference.value) {
                panic!(
                    "profile {}: {} = {}: {e}",
                    self.name(),
                    difference.path,
                    difference.value
                );
            }
        }
        patch
    }

    /// Java's defaults with this profile's differences on top: the value
    /// each setting starts at under the profile, before any settings file,
    /// design or flag. The interfaces compare with it to mark the values
    /// that differ from the profile, and reset to it.
    pub fn defaults(self, cores: usize) -> SettingsPatch {
        let mut patch = super::fields::default_patch(cores);
        patch.apply_from(&self.patch());
        patch.profile = Some(self.name().to_string());
        patch
    }
}

/// Moves `patch` from the profile it names to `to` (an interface's profile
/// selector): every field that still has the old profile's value takes
/// `to`'s, and every other value (the user's, the design's, the board
/// tuning's) stays. Sets `router.profile` to `to`.
pub fn rebase(patch: &mut SettingsPatch, to: Profile, cores: usize) {
    let from = Profile::of_patch(patch).unwrap_or_default();
    let (old, new) = (from.defaults(cores), to.defaults(cores));
    for field in super::fields::FIELDS.iter().filter(|f| f.path != "profile") {
        let now = (field.get)(patch);
        let target = (field.get)(&new);
        if now == (field.get)(&old) && now != target {
            match target {
                Some(value) => (field.set)(patch, value),
                None => (field.clear)(patch),
            }
        }
    }
    patch.profile = Some(to.name().to_string());
}

impl fmt::Display for Profile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

/// A `router.profile` value that names no profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnknownProfile {
    /// The value given.
    pub given: String,
}

impl fmt::Display for UnknownProfile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let valid: Vec<&str> = Profile::ALL.iter().map(|p| p.name()).collect();
        write!(
            f,
            "{SETTINGS_KEY}: unknown profile '{}' (valid profiles: {})",
            self.given,
            valid.join(", ")
        )
    }
}

impl std::error::Error for UnknownProfile {}

/// The profile the sources choose: the value of the highest-priority source
/// that sets `router.profile` (the last one among equal priorities, as the
/// merge's stable sort keeps them), else the default.
pub fn chosen<'a>(
    sources: impl IntoIterator<Item = &'a (super::Source, SettingsPatch)>,
) -> Result<Profile, UnknownProfile> {
    let mut best: Option<(i32, &str)> = None;
    for (source, patch) in sources {
        if let Some(name) = patch.profile.as_deref()
            && best.is_none_or(|(priority, _)| source.priority() >= priority)
        {
            best = Some((source.priority(), name));
        }
    }
    best.map_or(Ok(Profile::default()), |(_, name)| Profile::parse(name))
}

/// Checks that resolved settings are pinned to `legacy`: the parity tests
/// and gates call this so that a run without the pin fails loudly.
pub fn require_legacy(settings: &RouterSettings) -> Result<(), String> {
    match settings.profile.as_deref().map(Profile::parse) {
        Some(Ok(Profile::Legacy)) => Ok(()),
        other => Err(format!(
            "a parity run must set {SETTINGS_KEY} = legacy explicitly; it has {}",
            match other {
                None => "no profile (the default, enhanced)".to_string(),
                Some(Ok(p)) => format!("'{p}'"),
                Some(Err(e)) => e.to_string(),
            }
        )),
    }
}

/// A settings patch that only pins `legacy`, for the parity tests.
pub fn legacy_pin() -> SettingsPatch {
    SettingsPatch {
        profile: Some(Profile::Legacy.name().to_string()),
        ..SettingsPatch::default()
    }
}

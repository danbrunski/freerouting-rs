//! N06: the `router.java_quirks.*` switches, one per Java bug the port kept for
//! parity.
//!
//! Each switch is a settings key `router.java_quirks.<id>` (the ids of the N06
//! inventory, `docs/roadmap/tasks/N06-fix-kept-java-bugs.md`). `true` keeps Java's
//! behaviour, `false` selects the fix. Unset is `true`, and `legacy` keeps every
//! switch unset, so `legacy` is Java; `enhanced`'s table (N05,
//! [`ENHANCED`](super::profile::ENHANCED)) turns a switch off only once its batch is
//! accepted. N06.13 turned off the 30 that N06's batches accepted (2026-10-19) and,
//! in its second round (2026-10-20), `simplex_prev_division_line` (N06.4), the
//! optimizer's two (N06.9), `shove_via_into_pending_piece` (N06.15) and
//! `any_angle_room_side_index` (N06.16). The rejected `octagon_box_cutout_branch3`
//! and `ripup_pass_no_unset`, N06.14's two cutout switches (no verdict yet) and the
//! (c) rows left as Java stay `true` in both profiles. The keys are `opt` table entries without a default, as every
//! key that is not Java's, so Java's settings dump stays as it was.
//!
//! [`JavaQuirks`] is the resolved value: built once from the settings at job start,
//! with no global state, and carried where each bug lives:
//! - [`BoardRouterSettings`](super::board::BoardRouterSettings), copied into
//!   [`AutorouteControl::quirks`](crate::autoroute::control::AutorouteControl::quirks):
//!   the autorouter, the fanout and the batch optimizer;
//! - [`BasicBoard::java_quirks`](crate::board::BasicBoard::java_quirks): shove,
//!   pull-tight, the DRC, the statistics, the rules and session readers and the
//!   interactive code, which see only the board. A
//!   [`BoardSnapshot`](crate::board::BoardSnapshot) keeps it, so deep copies and
//!   restores (the optimizer's candidates) carry it;
//! - the DSN reader's `ReadContext`
//!   ([`build_board_with`](crate::io::specctra::dsn_reader::build_board_with));
//! - the geometry crate, as an explicit argument
//!   ([`JavaQuirks::split`], `rational_perp_projection`).
//!
//! Every read site carries the marker `// PARITY(quirk, java_quirks.<id>):` beside
//! Java's path. The seven inert switches (`simplex_prev_division_line` and the six
//! (c) rows N06.12 left as Java, whose `false` changes nothing) carry it at the Java
//! code they name; `tests/settings_java_quirks.rs` checks every id has one.

use freerouting_geometry::SplitQuirks;

use super::fields::{JavaQuirksPatch, JavaQuirksSettings, RouterSettings};

/// The settings key prefix of the switches.
pub const SETTINGS_PREFIX: &str = "router.java_quirks.";

macro_rules! java_quirks {
    ($( $id:ident : $item:literal ),* $(,)?) => {
        /// The switches of the Java bugs kept for parity (N06): `true` is Java's
        /// behaviour. See the module docs.
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub struct JavaQuirks {
            $( #[doc = concat!("`router.java_quirks.", stringify!($id), "`; the fix is ", $item, ".")]
               pub $id: bool, )*
        }

        impl JavaQuirks {
            /// Java's behaviour: every switch `true`.
            pub const JAVA: JavaQuirks = JavaQuirks { $( $id: true, )* };

            /// Every switch `false`: each fix that has landed is on.
            pub const ALL_FIXED: JavaQuirks = JavaQuirks { $( $id: false, )* };

            /// The switch ids, in the order of the N06 inventory.
            pub const IDS: &'static [&'static str] = &[ $( stringify!($id), )* ];

            /// The switches a settings patch sets; unset is `true`.
            pub fn from_patch(patch: &JavaQuirksPatch) -> JavaQuirks {
                JavaQuirks { $( $id: patch.$id.unwrap_or(true), )* }
            }

            /// The switches of the resolved section; unset is `true`.
            pub fn from_section(section: &JavaQuirksSettings) -> JavaQuirks {
                JavaQuirks { $( $id: section.$id.unwrap_or(true), )* }
            }

            /// The value of switch `id`; `None` for an unknown id.
            pub fn get(&self, id: &str) -> Option<bool> {
                match id {
                    $( stringify!($id) => Some(self.$id), )*
                    _ => None,
                }
            }

            /// This value with switch `id` set to `value`; `None` for an unknown id.
            pub fn with(self, id: &str, value: bool) -> Option<JavaQuirks> {
                let mut next = self;
                match id {
                    $( stringify!($id) => next.$id = value, )*
                    _ => return None,
                }
                Some(next)
            }

            /// The checklist item that owns the fix of switch `id`.
            pub fn fix_item(id: &str) -> Option<&'static str> {
                match id {
                    $( stringify!($id) => Some($item), )*
                    _ => None,
                }
            }
        }
    };
}

java_quirks! {
    // (b) rows, by batch.
    simplex_prev_division_line: "N06.4",
    octagon_box_cutout_branch3: "N06.4",
    cutout_hole_crosses_border: "N06.14",
    simplex_octagon_half_corner: "N06.14",
    init_failure_room_leak: "N06.5",
    failed_attempt_ripped_carryover: "N06.5",
    ortho_zero_width_room: "N06.5",
    edge_corner_not_advanced: "N06.5",
    connect_to_trace_swapped_width: "N06.5",
    fanout_stale_drill_carry: "N06.6",
    drill_destination_room_index: "N06.6",
    fanout_pin_order_names: "N06.6",
    in_front_of_pad_typo: "N06.7",
    store_trace_class_self_compare: "N06.7",
    rational_perp_projection: "N06.7",
    smooth_loses_trace: "N06.8",
    pull_tight_reused_lines_change: "N06.8",
    opt_first_pass_stops: "N06.9",
    opt_equal_key_skip: "N06.9",
    dsn_via_alias: "N06.10",
    dsn_class_class_pairs: "N06.10",
    dsn_shape_bbox: "N06.10",
    dsn_unit_ignored: "N06.10",
    ses_reader_scale: "N06.10",
    stats_line_ab_length: "N06.10",
    route_result_int_via_ratio: "N06.10",
    drc_units_10000_per_mm: "N06.10",
    snapshot_drops_compensation: "N06.11",
    rules_matrix_trees_not_rebuilt: "N06.11",
    move_drill_tidy_region: "N06.11",
    route_change_layer_no_via: "N06.11",
    route_connect_inner_break: "N06.11",
    select_extension_ignores_filter: "N06.11",
    // (b) rows found after the inventory (N03.2, N06.16).
    shove_via_into_pending_piece: "N06.15",
    any_angle_room_side_index: "N06.16",
    // (c) rows.
    ripup_pass_no_unset: "N06.12",
    insert_stale_trace_object: "N06.12",
    in_front_of_pad_side0_flank: "N06.12",
    opt_threshold_fraction: "N06.12",
    rules_unknown_layer_scope: "N06.12",
    rules_autoroute_settings_reenable: "N06.12",
    direction_compare_zero: "N06.12",
    route_stubs_layer_test: "N06.12",
    any_angle_skip_length_squared: "N06.12",
    polyline_array_reversal: "N06.12",
    avoid_acid_traps_disabled: "N06.12",
}

impl Default for JavaQuirks {
    fn default() -> Self {
        JavaQuirks::JAVA
    }
}

impl JavaQuirks {
    /// The switches of resolved settings.
    pub fn from_settings(settings: &RouterSettings) -> JavaQuirks {
        JavaQuirks::from_section(&settings.java_quirks)
    }

    /// The switches of the geometry's convex split (batch G and N06.14).
    pub fn split(&self) -> SplitQuirks {
        SplitQuirks {
            simplex_prev_division_line: self.simplex_prev_division_line,
            octagon_box_cutout_branch3: self.octagon_box_cutout_branch3,
            cutout_hole_crosses_border: self.cutout_hole_crosses_border,
            simplex_octagon_half_corner: self.simplex_octagon_half_corner,
        }
    }

    /// The ids of the switches that are `false`, in inventory order.
    pub fn fixed_ids(&self) -> Vec<&'static str> {
        Self::IDS
            .iter()
            .copied()
            .filter(|id| self.get(id) == Some(false))
            .collect()
    }
}

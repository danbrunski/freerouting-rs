//! The five board overrides Java applies when it loads a board for routing
//! (C03.3-C03.5).
//!
//! Port of `HeadlessBoardManager.applyCopperToEdgeClearanceOverride`,
//! `applyHoleClearanceOverride` with `assignHoleKeepoutClearanceClass`,
//! `applyPlaneNetsOverride`, `applyPlaneAsObstacleOverride` and
//! `applyClearanceToleranceOverride` (Java at `cda540f`). The overrides run
//! only on the routing path: the DRC loads boards without them
//! (`drc::runner::load_design`), because Java's `DrcRunner` loads with empty
//! router settings.
//!
//! - **Board units.** `(int) Math.round(Unit.scale(um * max(1, resolution),
//!   UM, unit))`, computed in Java's order ([`um_to_board_units`]).
//! - **Quirks kept.** The edge-clearance default is detected by value
//!   (`|v - 250.0| < 1e-9`), so an explicit 250 is capped too; an explicit
//!   outline class from the DSN wins over the default value.
//! - **Unset values.** The resolved [`RouterSettings`] has no unset members
//!   (a missing value is its default), so Java's "null is skipped" cannot
//!   occur here.

use super::{LogLevel, LogLine};
use crate::autoroute::batch::report::describe_item;
use crate::board::items::ItemId;
use crate::board::{BasicBoard, RoutingBoard};
use crate::rules::ItemClass;
use crate::settings::RouterSettings;
use crate::settings::text::java_double_to_string;
use crate::util::equals_ignore_case;
use freerouting_geometry::jmath::{d2i, round_to_i32};
use freerouting_geometry::{Area, Shape};

/// `HeadlessBoardManager.BOARD_EDGE_CLEARANCE_CLASS_NAME`.
pub const BOARD_EDGE_CLEARANCE_CLASS_NAME: &str = "board_edge";
/// `HeadlessBoardManager.HOLE_EDGE_CLEARANCE_CLASS_NAME`.
pub const HOLE_EDGE_CLEARANCE_CLASS_NAME: &str = "hole_edge";
/// `DefaultSettings.DEFAULT_COPPER_TO_EDGE_CLEARANCE_UM`.
pub const DEFAULT_COPPER_TO_EDGE_CLEARANCE_UM: f64 = 250.0;

/// What the overrides remember between calls (Java's
/// `edgeClearanceAppliedByOverride`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverrideState {
    /// The copper-to-edge override changed the outline's class. Once true,
    /// the default value is applied again even if the outline class is not
    /// the fallback one.
    pub edge_applied: bool,
}

/// `(int) Math.round(Unit.scale(um * max(1, resolution), Unit.UM, unit))`.
pub fn um_to_board_units(board: &BasicBoard, um: f64) -> i32 {
    let resolution = board.communication.resolution.max(1);
    let scaled = crate::board::structure::Unit::scale(
        um * f64::from(resolution),
        crate::board::structure::Unit::Um,
        board.communication.unit,
    );
    round_to_i32(scaled)
}

/// The pin-to-outline gap of a board and where it comes from (K15.7).
#[derive(Clone, Debug, PartialEq)]
pub struct PinGap {
    /// The smallest gap in board units, as [`BasicBoard::minimum_pin_gap`].
    pub gap: f64,
    /// The pin that sets it (the first one on a tie); `None` without pins.
    pub pin: Option<ItemId>,
    /// True if that pin lies wholly outside the outline.
    pub pin_outside: bool,
    /// The pins left out of the gap because they lie wholly outside the
    /// outline (only when asked), in item order.
    pub left_out: Vec<ItemId>,
}

impl BasicBoard {
    /// `BoardOutline.minimumPinGap()`: the smallest gap in board units
    /// between the copper of any pin, on any layer it reaches, and the
    /// outline lines. 0 if a pin touches or crosses the outline, `+∞` if the
    /// board has no pins (or no outline).
    pub fn minimum_pin_gap(&self) -> f64 {
        self.pin_gap(false).gap
    }

    /// [`Self::minimum_pin_gap`] with the pin that sets the gap. With
    /// `leave_out_off_board`, a pin whose pad shapes all lie wholly outside
    /// the outline is not counted and goes to [`PinGap::left_out`]; without,
    /// the gap is Java's exactly (such a pin gives 0).
    pub fn pin_gap(&self, leave_out_off_board: bool) -> PinGap {
        let Some(outline) = self.outline().and_then(|item| item.as_board_outline()) else {
            return PinGap {
                gap: f64::INFINITY,
                pin: None,
                pin_outside: false,
                left_out: Vec::new(),
            };
        };
        let mut left_out = Vec::new();
        let mut shapes: Vec<(ItemId, &Shape)> = Vec::new();
        for item in self.items().filter(|item| item.is_pin()) {
            let drill = item.drill().expect("a pin has drill data");
            let layers = (drill.last_layer - drill.first_layer + 1).max(0) as usize;
            let pin_shapes: Vec<&Shape> = (0..layers).filter_map(|i| drill.shape(i)).collect();
            if leave_out_off_board
                && !pin_shapes.is_empty()
                && pin_shapes.iter().all(|s| outline.is_wholly_outside(s))
            {
                left_out.push(item.id());
            } else {
                shapes.extend(pin_shapes.into_iter().map(|s| (item.id(), s)));
            }
        }
        let (gap, pin) = outline.minimum_pin_gap_with(shapes.iter().map(|(id, s)| (*id, *s)));
        let pin_outside = pin.is_some_and(|id| {
            self.item(id)
                .and_then(|item| item.drill())
                .is_some_and(|drill| {
                    let layers = (drill.last_layer - drill.first_layer + 1).max(0) as usize;
                    (0..layers)
                        .filter_map(|i| drill.shape(i))
                        .all(|s| outline.is_wholly_outside(s))
                })
        });
        PinGap {
            gap,
            pin,
            pin_outside,
            left_out,
        }
    }
}

fn log(lines: &mut Vec<LogLine>, level: LogLevel, text: String) {
    lines.push(LogLine { level, text });
}

/// `applyCopperToEdgeClearanceOverride`: the class `board_edge` gets the
/// configured clearance to every other class, and the outline moves into
/// it. The default value is capped at the minimum pin gap, because a pin
/// closer to the outline than the default would block every connection.
pub fn apply_edge_clearance(
    board: &mut BasicBoard,
    settings: &RouterSettings,
    state: &mut OverrideState,
) -> Vec<LogLine> {
    let mut lines = Vec::new();
    let um = settings.copper_to_edge_clearance_um;
    if um < 0.0 {
        log(
            &mut lines,
            LogLevel::Warn,
            format!(
                "Ignoring router.copper_to_edge_clearance_um because it is negative: {}",
                java_double_to_string(um)
            ),
        );
        return lines;
    }
    let Some(outline_id) = board.outline().map(|item| item.id()) else {
        log(
            &mut lines,
            LogLevel::Warn,
            "Ignoring router.copper_to_edge_clearance_um because the board outline is unavailable."
                .into(),
        );
        return lines;
    };
    let default_class = board.rules.default_net_class();
    let default_area_class = board
        .rules
        .net_classes
        .get(default_class)
        .default_item_clearance_classes
        .get(ItemClass::Area);
    let outline_class = board.item_ref(outline_id).clearance_class();
    let uses_fallback_outline_class = outline_class == default_area_class;
    let uses_default = (um - DEFAULT_COPPER_TO_EDGE_CLEARANCE_UM).abs() < 1e-9;
    // An explicit DSN outline class wins over the global default.
    if uses_default && !uses_fallback_outline_class && !state.edge_applied {
        return lines;
    }

    let mut units = um_to_board_units(board, um);
    if uses_default {
        // The default is only a guess: never demand more edge clearance than
        // the input already has.
        let skip = settings.edge_cap_skips_off_board_pins.unwrap_or(false);
        let pins = board.pin_gap(skip);
        for pin in &pins.left_out {
            log(
                &mut lines,
                LogLevel::Warn,
                format!(
                    "Copper-to-edge clearance cap: pin {} lies wholly outside the board outline \
                     and is left out.",
                    describe_item(board, *pin)
                ),
            );
        }
        let minimum_pin_gap = pins.gap;
        if minimum_pin_gap < f64::from(units) {
            let capped = d2i(0.0_f64.max(minimum_pin_gap.floor()));
            let pin = pins.pin.map_or_else(
                || "a pin".to_string(),
                |id| format!("pin {}", describe_item(board, id)),
            );
            let gap_um = crate::board::structure::Unit::scale(
                minimum_pin_gap,
                board.communication.unit,
                crate::board::structure::Unit::Um,
            ) / f64::from(board.communication.resolution.max(1));
            let outside = if pins.pin_outside {
                " and lies wholly outside the board outline (router.edge_cap_skips_off_board_pins \
                 leaves such a pin out)"
            } else {
                ""
            };
            log(
                &mut lines,
                LogLevel::Info,
                format!(
                    "Copper-to-edge clearance capped from {units} to {capped} board units \
                     because {pin} is {gap_um:.1} um from the board outline{outside}."
                ),
            );
            units = capped;
        }
    }

    let matrix = &mut board.rules.clearance_matrix;
    let mut edge_class = matrix.get_no(BOARD_EDGE_CLEARANCE_CLASS_NAME);
    if edge_class < 0 {
        matrix.append_class(BOARD_EDGE_CLEARANCE_CLASS_NAME);
        edge_class = matrix.get_no(BOARD_EDGE_CLEARANCE_CLASS_NAME);
    }
    if edge_class < 0 {
        log(
            &mut lines,
            LogLevel::Warn,
            "Unable to create/find the board_edge clearance class for copper-to-edge override."
                .into(),
        );
        return lines;
    }
    let edge = edge_class as usize;
    for layer in 0..matrix.layer_count() {
        for class in 1..matrix.class_count() {
            matrix.set_value_on_layer(edge, class, layer, units);
            matrix.set_value_on_layer(class, edge, layer, units);
        }
    }

    board.remove_from_trees(outline_id);
    if let Some(item) = board.items.get_mut(&outline_id) {
        item.clearance_class = edge_class;
    }
    board.insert_into_trees(outline_id);
    state.edge_applied = true;
    log(
        &mut lines,
        LogLevel::Debug,
        format!(
            "Applied copper-to-edge clearance override: {} um ({units} board units).",
            java_double_to_string(um)
        ),
    );
    lines
}

/// `applyHoleClearanceOverride`: sets the board's hole clearance and, for a
/// value above 0, moves KiCad's circular package keepouts (its non-plated
/// holes) into the class `hole_edge`, which keeps other-net copper at least
/// that far from the hole. Returns the log lines and the number of keepouts
/// reclassified.
pub fn apply_hole_clearance(
    board: &mut BasicBoard,
    settings: &RouterSettings,
) -> (Vec<LogLine>, usize) {
    let mut lines = Vec::new();
    let um = settings.hole_clearance_um;
    if um < 0.0 {
        log(
            &mut lines,
            LogLevel::Warn,
            format!(
                "Ignoring router.hole_clearance_um because it is negative: {}",
                java_double_to_string(um)
            ),
        );
        return (lines, 0);
    }
    let units = um_to_board_units(board, um);
    let changed = units != board.rules.hole_clearance();
    board.rules.set_hole_clearance(units);
    let mut reclassified = 0;
    if units > 0 {
        reclassified = assign_hole_keepout_clearance_class(board, units, &mut lines);
    }
    if changed || reclassified > 0 {
        // Tree shapes are computed at insert time; the items loaded before
        // the override must be reinserted so their obstacle shapes include
        // the drill-hole inflation.
        board.reinsert_tree_items();
    }
    if units > 0 {
        let detail = if reclassified > 0 {
            format!(", {reclassified} hole keepouts reclassified.")
        } else {
            ".".to_string()
        };
        log(
            &mut lines,
            LogLevel::Info,
            format!(
                "Applied drill-hole clearance override: {} um ({units} board units){detail}",
                java_double_to_string(um)
            ),
        );
    }
    (lines, reclassified)
}

/// `assignHoleKeepoutClearanceClass`. Returns the number of keepouts moved
/// into `hole_edge`.
fn assign_hole_keepout_clearance_class(
    board: &mut BasicBoard,
    hole_clearance: i32,
    lines: &mut Vec<LogLine>,
) -> usize {
    // Obstacle areas of exactly that type, belonging to a component, with a
    // circle as their area: a drilled hole in a footprint.
    let keepouts: Vec<_> = board
        .items()
        .filter(|item| {
            item.component_id() > 0
                && item.as_area().is_some_and(|a| {
                    a.kind == crate::board::items::AreaKind::Obstacle
                        && matches!(a.area(), Area::Shape(Shape::Circle(_)))
                })
        })
        .map(|item| item.id())
        .collect();
    if keepouts.is_empty() {
        return 0;
    }
    let default_class = board.rules.default_net_class();
    let default_area_class = board
        .rules
        .net_classes
        .get(default_class)
        .default_item_clearance_classes
        .get(ItemClass::Area);
    let matrix = &mut board.rules.clearance_matrix;
    let mut hole_edge = matrix.get_no(HOLE_EDGE_CLEARANCE_CLASS_NAME);
    if hole_edge < 0 {
        matrix.append_class(HOLE_EDGE_CLEARANCE_CLASS_NAME);
        hole_edge = matrix.get_no(HOLE_EDGE_CLEARANCE_CLASS_NAME);
    }
    if hole_edge < 0 {
        log(
            lines,
            LogLevel::Warn,
            "Unable to create/find the hole_edge clearance class for the hole clearance override."
                .into(),
        );
        return 0;
    }
    for layer in 0..matrix.layer_count() {
        for class in 1..matrix.class_count() {
            // Never reduce an existing requirement: the hole clearance is a
            // floor on top of the copper clearance the keepout would get.
            let value = hole_clearance.max(matrix.get_value(
                default_area_class,
                class as i32,
                layer as i32,
                false,
            ));
            matrix.set_value_on_layer(hole_edge as usize, class, layer, value);
            matrix.set_value_on_layer(class, hole_edge as usize, layer, value);
        }
    }
    let mut reclassified = 0;
    for id in keepouts {
        if let Some(item) = board.items.get_mut(&id)
            && item.clearance_class != hole_edge
        {
            item.clearance_class = hole_edge;
            reclassified += 1;
        }
    }
    reclassified
}

/// `applyPlaneNetsOverride`: every net whose name equals (ignoring case) a
/// trimmed, non-blank `plane_nets` entry gets `contains_plane`.
pub fn apply_plane_nets(board: &mut BasicBoard, settings: &RouterSettings) -> Vec<LogLine> {
    let mut lines = Vec::new();
    for name in &settings.plane_nets {
        if name.trim().is_empty() {
            continue;
        }
        let wanted = name.trim();
        let numbers: Vec<i32> = board
            .rules
            .nets
            .iter()
            .filter(|net| equals_ignore_case(&net.name, wanted))
            .map(|net| net.net_number)
            .collect();
        for number in numbers {
            if let Some(net) = board.rules.nets.get_mut(number)
                && !net.contains_plane()
            {
                net.set_contains_plane(true);
                log(
                    &mut lines,
                    LogLevel::Info,
                    format!(
                        "Configured net '{}' as a power plane net via router.plane_nets.",
                        net.name
                    ),
                );
            }
        }
    }
    lines
}

/// `applyPlaneAsObstacleOverride`.
pub fn apply_plane_as_obstacle(
    board: &mut RoutingBoard,
    settings: &RouterSettings,
) -> Vec<LogLine> {
    let value = settings.plane_as_obstacle;
    board.change_plane_as_obstacle(value);
    vec![LogLine {
        level: LogLevel::Debug,
        text: format!("Applied plane_as_obstacle override: {value}"),
    }]
}

/// `applyClearanceToleranceOverride`: a finite value of at least 0 becomes
/// the board's clearance tolerance; anything else is a warning and ignored.
pub fn apply_clearance_tolerance(
    board: &mut BasicBoard,
    settings: &RouterSettings,
) -> Vec<LogLine> {
    let tolerance = settings.clearance_tolerance_um;
    if !tolerance.is_finite() || tolerance < 0.0 {
        return vec![LogLine {
            level: LogLevel::Warn,
            text: format!(
                "Ignoring router.clearance_tolerance_um because it is invalid (must be finite and >= 0): {}",
                java_double_to_string(tolerance)
            ),
        }];
    }
    board.rules.clearance_tolerance_um = tolerance;
    vec![LogLine {
        level: LogLevel::Debug,
        text: format!(
            "Applied clearance tolerance: {} um.",
            java_double_to_string(tolerance)
        ),
    }]
}

/// The five overrides in Java's order (`applyRouterSettingsForLoadedBoard`):
/// copper-to-edge clearance, hole clearance, plane nets, plane as obstacle,
/// clearance tolerance.
pub fn apply_all(
    board: &mut RoutingBoard,
    settings: &RouterSettings,
    state: &mut OverrideState,
) -> Vec<LogLine> {
    // Q22.10: not Java's; nothing without `router.board_minimums.clearance_um`.
    // First, so the edge and hole classes the next two make keep their own rules.
    let mut lines = super::clearance_floor::apply_clearance_floor(board, settings);
    // Q22.11: not Java's; nothing without `router.board_minimums.hole_to_hole_um`.
    lines.extend(super::hole_to_hole::apply_hole_to_hole(board, settings));
    lines.extend(apply_edge_clearance(board, settings, state));
    lines.extend(apply_hole_clearance(board, settings).0);
    lines.extend(apply_plane_nets(board, settings));
    lines.extend(apply_plane_as_obstacle(board, settings));
    lines.extend(apply_clearance_tolerance(board, settings));
    // Q18: not Java's; nothing without a board via minimum.
    lines.extend(super::via_minimums::apply_via_minimums(board, settings));
    // Q18.7: not Java's; nothing without `router.solder_mask_expansions`.
    lines.extend(super::solder_mask::apply_solder_mask_expansions(
        board, settings,
    ));
    lines
}

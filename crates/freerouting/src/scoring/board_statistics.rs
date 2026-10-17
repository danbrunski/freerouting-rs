//! Board statistics.
//!
//! Port of `core.scoring.BoardStatistics` and its parts
//! (`BoardStatisticsBoard`, `BoardStatisticsItems`, ...), with the router and
//! optimizer scores: the V2 formulas here, the legacy one in
//! [`super::legacy`], dispatched on the [`RouterSettings`] as Java's
//! `getRouterScore(RouterSettings)` and `getOptimizerScore(RouterSettings)`
//! do.

use crate::util::HashMap;

use freerouting_geometry::FloatPoint;
use freerouting_geometry::jmath::{double_stream_sum, max_f64};

use super::bounds_calculator::{BoardStatisticsBounds, calculate_bounds};
use super::legacy;
use crate::board::BasicBoard;
use crate::board::items::{ItemId, ItemKind};
use crate::board::structure::{FixedState, Unit};
use crate::drc::{ClearanceViolation, DesignRulesChecker};
use crate::rules::BoardRules;
use crate::settings::fields::ScoringSettings;
use crate::settings::{
    OptimizerScoringVersion, RouterScoringVersion, RouterSettings, default_patch,
};

/// A rectangle as Java's `Rectangle2D.Float`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Port of `BoardStatisticsBoard`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardStatisticsBoard {
    /// Java fills this with `(ur.x, ur.y, ll.x, ll.y)` of the bounding box,
    /// converted to the statistics unit without the board resolution.
    pub bounding_box: Rect,
    pub size: Rect,
    pub area_cm2: f32,
}

/// Port of `BoardStatisticsLayers`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsLayers {
    pub total_count: usize,
    pub signal_count: usize,
}

/// Port of `BoardStatisticsItems`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsItems {
    pub total_count: usize,
    pub trace_count: usize,
    pub via_count: usize,
    pub conduction_area_count: usize,
    pub drill_item_count: usize,
    pub pin_count: usize,
    /// Component outlines (Java's `component_count`).
    pub component_outline_count: usize,
    pub other_count: usize,
}

/// Port of `BoardStatisticsComponents`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsComponents {
    pub total_count: usize,
}

/// Port of `BoardStatisticsPads`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsPads {
    pub total_count: usize,
}

/// Port of `BoardStatisticsNets`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsNets {
    pub total_count: i32,
    pub class_count: usize,
}

/// Port of `BoardStatisticsTraces`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardStatisticsTraces {
    pub total_count: usize,
    pub total_segment_count: usize,
    pub total_length: f32,
    pub total_length_mm: f32,
    pub total_weighted_length: f32,
    pub average_length: f32,
    pub total_vertical_length: f32,
    pub total_horizontal_length: f32,
    pub total_angled_length: f32,
}

/// Port of `BoardStatisticsBends`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsBends {
    pub total_count: usize,
    pub ninety_degree_count: usize,
    pub forty_five_degree_count: usize,
    pub other_angle_count: usize,
}

/// Port of `BoardStatisticsVias`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsVias {
    pub total_count: usize,
    pub through_hole_count: usize,
    pub blind_count: usize,
    pub buried_count: usize,
}

/// Port of `BoardStatisticsDifficulty`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardStatisticsDifficulty {
    pub pin_count: usize,
    pub signal_layer_count: usize,
    pub complexity_c: usize,
    pub difficulty_d: f32,
    pub board_area_cm2: f32,
}

/// Port of `BoardStatistics.BoardStatisticsFanout`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsFanout {
    /// SMD pins with a net.
    pub total_smd_pins: usize,
    /// SMD pins with a net minus those whose unconnected set
    /// (`Item.getUnconnectedSet`) is empty.
    pub pins_to_escape: usize,
    /// SMD pins with a net that are escaped (Java `isPinEscaped`).
    pub escaped_count: usize,
}

/// Port of `BoardStatisticsConnections`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsConnections {
    /// Per net the pins and conduction areas minus one.
    pub maximum_count: usize,
    /// The airlines of all nets.
    pub incomplete_count: usize,
}

/// Port of `BoardStatisticsClearanceViolations`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BoardStatisticsClearanceViolations {
    pub total_count: usize,
    /// The violations the board had when it was loaded
    /// ([`BasicBoard::pre_existing_clearance_violations_count`]).
    pub pre_existing_count: usize,
    /// Violations between items that are not routable.
    pub unfixable_count: usize,
    /// `max(0, total - pre_existing)`.
    pub router_introduced_count: usize,
    /// The summed clearance shortfalls in micrometers.
    pub total_violation_um: f64,
    pub min_violation_um: f64,
    pub max_violation_um: f64,
    pub avg_violation_um: f64,
}

/// The settings of the V2 (continuous) router score.
///
/// Port of `settings.RouterScoreSettings` with the defaults of
/// `DefaultSettings`; the weights are `float` in Java.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RouterScoreSettings {
    pub unrouted_free_fraction: f32,
    pub unrouted_first_half_weight: f32,
    pub unrouted_second_half_weight: f32,
    pub clearance_violation_count_weight: f32,
    pub clearance_violation_depth_weight: f32,
    pub clearance_violation_depth_scale: f32,
}

impl Default for RouterScoreSettings {
    fn default() -> Self {
        RouterScoreSettings {
            unrouted_free_fraction: 0.5,
            unrouted_first_half_weight: 1000.0 / 3.0,
            unrouted_second_half_weight: 2000.0 / 3.0,
            clearance_violation_count_weight: 25.0,
            clearance_violation_depth_weight: 300.0,
            clearance_violation_depth_scale: 1000.0,
        }
    }
}

/// The settings of the V2 (lower bound) optimizer score.
///
/// Port of `settings.OptimizerScoreSettings` with the defaults of
/// `DefaultSettings`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OptimizerScoreSettings {
    pub excess_wire_length_weight: f32,
    pub excess_via_weight: f32,
    pub excess_bend_weight: f32,
    pub length_floor: f32,
    pub difficulty_scale_floor: f32,
}

impl Default for OptimizerScoreSettings {
    fn default() -> Self {
        OptimizerScoreSettings {
            excess_wire_length_weight: 1000.0,
            excess_via_weight: 2000.0,
            excess_bend_weight: 500.0,
            length_floor: 1.0,
            difficulty_scale_floor: 1.0,
        }
    }
}

/// Statistics of a board.
///
/// Port of `core.scoring.BoardStatistics`, with the V2 and the legacy
/// scores and their dispatch on the [`RouterSettings`]
/// ([`router_score`](Self::router_score),
/// [`optimizer_score`](Self::optimizer_score),
/// [`optimizer_ranking_score`](Self::optimizer_ranking_score)).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardStatistics {
    /// `<host cad>,<host version>` from the DSN parser scope.
    pub host: String,
    /// The unit of the length values (always `mm`).
    pub unit: String,
    pub board: BoardStatisticsBoard,
    pub layers: BoardStatisticsLayers,
    pub items: BoardStatisticsItems,
    pub components: BoardStatisticsComponents,
    pub pads: BoardStatisticsPads,
    pub nets: BoardStatisticsNets,
    pub connections: BoardStatisticsConnections,
    pub traces: BoardStatisticsTraces,
    pub bends: BoardStatisticsBends,
    pub vias: BoardStatisticsVias,
    pub clearance_violations: BoardStatisticsClearanceViolations,
    pub difficulty: BoardStatisticsDifficulty,
    pub bounds: BoardStatisticsBounds,
    pub fanout: BoardStatisticsFanout,
}

/// Java `TextManager.unescapeUnicode`: replaces `\uXXXX` escapes.
fn unescape_unicode(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut result = String::with_capacity(s.len());
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '\\' && chars.get(i + 1) == Some(&'u') && i + 6 <= chars.len() {
            let hex: String = chars[i + 2..i + 6].iter().collect();
            if let Ok(code) = u32::from_str_radix(&hex, 16)
                && let Some(c) = char::from_u32(code)
            {
                result.push(c);
                i += 6;
                continue;
            }
        }
        result.push(chars[i]);
        i += 1;
    }
    result
}

impl BoardStatistics {
    /// The statistics of `board`, with lengths in millimeters, including
    /// the connection counts and the clearance violations.
    pub fn new(board: &BasicBoard) -> Self {
        Self::with_unit(board, Unit::Mm)
    }

    /// The statistics of `board`, with lengths converted to `unit`,
    /// including the connection counts and the clearance violations.
    pub fn with_unit(board: &BasicBoard, unit: Unit) -> Self {
        Self::with_options(board, unit, true, true)
    }

    /// The statistics of `board`; the clearance violations and the
    /// connection counts (both run the design rules checker) only if asked
    /// for, zero otherwise.
    pub fn with_options(
        board: &BasicBoard,
        unit: Unit,
        include_clearance_violations: bool,
        include_connections: bool,
    ) -> Self {
        let mut s = Self::base(board, unit);
        if include_connections {
            // Java reads the connection counts from the board's routing
            // ledger (`BoardStatistics.java:297-299`).
            s.connections = BoardStatisticsConnections {
                maximum_count: board.ledger_maximum_connections(),
                incomplete_count: board.ledger_incomplete_count(),
            };
        }
        if include_clearance_violations {
            let checker = DesignRulesChecker::new(board);
            s.set_clearance_violations(board, &checker.all_clearance_violations());
        }
        s
    }

    /// The statistics of `board` with clearance violations and connection
    /// counts already computed by a [`DesignRulesChecker`].
    pub fn with_drc_results(
        board: &BasicBoard,
        unit: Unit,
        violations: &[ClearanceViolation],
        maximum_connections: usize,
        incomplete_connections: usize,
    ) -> Self {
        let mut s = Self::base(board, unit);
        s.connections = BoardStatisticsConnections {
            maximum_count: maximum_connections,
            incomplete_count: incomplete_connections,
        };
        s.set_clearance_violations(board, violations);
        s
    }

    fn set_clearance_violations(&mut self, board: &BasicBoard, violations: &[ClearanceViolation]) {
        let c = &mut self.clearance_violations;
        c.total_count = violations.len();
        if !violations.is_empty() {
            let factor = board.board_unit_to_um_factor();
            let mut min = f64::MAX;
            let mut max = 0.0f64;
            let mut sum = 0.0;
            for v in violations {
                let shortfall_um = v.shortfall().max(0.0) * factor;
                min = min.min(shortfall_um);
                max = max.max(shortfall_um);
                sum += shortfall_um;
            }
            c.total_violation_um = sum;
            c.min_violation_um = min;
            c.max_violation_um = max;
            c.avg_violation_um = sum / violations.len() as f64;
        }
        c.pre_existing_count = board.pre_existing_clearance_violations_count;
        c.unfixable_count = violations.iter().filter(|v| v.is_unfixable(board)).count();
        c.router_introduced_count = c.total_count.saturating_sub(c.pre_existing_count);
    }

    /// Java `getRouterScore` with the V2 (continuous) formula: 1000 minus
    /// the penalties for unrouted connections and clearance violations.
    pub fn router_score_v2(&self, settings: &RouterScoreSettings) -> f32 {
        let difficulty = f64::from(self.difficulty.difficulty_d).max(1.0);
        let connections = self.connections.maximum_count as f64;
        let incomplete = self.connections.incomplete_count as f64;
        let violation_count = self.clearance_violations.total_count as f64;
        let violation_depth = self.clearance_violations.total_violation_um.max(0.0);
        let split = f64::from(settings.unrouted_free_fraction).clamp(0.0, 1.0);
        let first_half_weight = f64::from(settings.unrouted_first_half_weight);
        let second_half_weight = f64::from(settings.unrouted_second_half_weight);
        let open_fraction = if connections > 0.0 {
            incomplete / connections
        } else {
            0.0
        };
        let (first_half_open, second_half_open) = if connections <= 0.0 {
            (0.0, 0.0)
        } else if split <= 0.0 {
            (0.0, open_fraction)
        } else if split >= 1.0 {
            (open_fraction, 0.0)
        } else {
            (
                ((open_fraction - split) / (1.0 - split)).clamp(0.0, 1.0),
                (open_fraction / split).min(1.0),
            )
        };
        let unrouted_penalty =
            first_half_weight * first_half_open + second_half_weight * second_half_open;
        let mut drc_penalty =
            f64::from(settings.clearance_violation_count_weight) * violation_count / difficulty;
        let depth_scale = f64::from(settings.clearance_violation_depth_scale).max(1.0);
        drc_penalty += f64::from(settings.clearance_violation_depth_weight) * violation_depth
            / depth_scale
            / difficulty;
        max_f64(0.0, 1000.0 - unrouted_penalty - drc_penalty) as f32
    }

    /// [`router_score_v2`](Self::router_score_v2) with Java's default
    /// settings.
    pub fn router_score_v2_default(&self) -> f32 {
        self.router_score_v2(&RouterScoreSettings::default())
    }

    /// Java `getOptimizerScore` with the V2 (lower bound) formula: 1000
    /// minus the penalties for trace length, vias and bends above the lower
    /// bounds of the board.
    pub fn optimizer_score_v2(&self, settings: &OptimizerScoreSettings) -> f32 {
        max_f64(0.0, self.optimizer_score_v2_unclamped(settings)) as f32
    }

    /// The V2 optimizer score without the clamp at zero (Java
    /// `getV2OptimizerScoreUnclamped`, `a773a70ec`): 1000 minus the excess
    /// length, via and bend penalties, in `double`.
    pub fn optimizer_score_v2_unclamped(&self, settings: &OptimizerScoreSettings) -> f64 {
        let difficulty = f64::from(self.difficulty.difficulty_d).max(1.0);
        let min_trace_length = f64::from(self.bounds.min_trace_length_mm).max(0.0);
        let min_via_count = f64::from(self.bounds.min_via_count.max(0));
        let min_bend_count = f64::from(self.bounds.min_bend_count.max(0));
        let actual_trace_length = f64::from(self.traces.total_length_mm).max(0.0);
        let actual_via_count = self.vias.total_count as f64;
        let actual_bend_count = self.bends.total_count as f64;
        let length_floor = f64::from(settings.length_floor).max(0.0);
        let difficulty_floor = f64::from(settings.difficulty_scale_floor).max(1.0);
        let length_penalty = f64::from(settings.excess_wire_length_weight)
            * (actual_trace_length - min_trace_length).max(0.0)
            / min_trace_length.max(length_floor);
        let via_penalty = f64::from(settings.excess_via_weight)
            * (actual_via_count - min_via_count).max(0.0)
            / difficulty.max(difficulty_floor);
        let bend_penalty = f64::from(settings.excess_bend_weight)
            * (actual_bend_count - min_bend_count).max(0.0)
            / difficulty.max(difficulty_floor);
        1000.0 - length_penalty - via_penalty - bend_penalty
    }

    /// [`optimizer_score_v2`](Self::optimizer_score_v2) with Java's default
    /// settings.
    pub fn optimizer_score_v2_default(&self) -> f32 {
        self.optimizer_score_v2(&OptimizerScoreSettings::default())
    }

    /// Java `getRouterScore(RouterSettings)`: the V2 score when the router
    /// scoring version is `V2_CONTINUOUS`, else the legacy normalized score
    /// with the settings' `scoring` costs. `None` is Java's `null`: the
    /// legacy score with the default costs.
    pub fn router_score(&self, settings: Option<&RouterSettings>) -> f32 {
        match settings {
            Some(s) if s.router_scoring.version == RouterScoringVersion::V2Continuous => {
                self.router_score_v2(&s.router_scoring.score_weights())
            }
            _ => self.legacy_normalized_score(&legacy_scoring_or_default(settings)),
        }
    }

    /// Java `getOptimizerScore(RouterSettings)`: the V2 score when the
    /// optimizer scoring version is `V2_LOWER_BOUND`, else the legacy
    /// normalized score, as in [`router_score`](Self::router_score).
    pub fn optimizer_score(&self, settings: Option<&RouterSettings>) -> f32 {
        match settings {
            Some(s) if s.optimizer_scoring.version == OptimizerScoringVersion::V2LowerBound => {
                self.optimizer_score_v2(&s.optimizer_scoring.score_weights())
            }
            _ => self.legacy_normalized_score(&legacy_scoring_or_default(settings)),
        }
    }

    /// Java `getOptimizerRankingScore(RouterSettings)` (`a773a70ec`), which
    /// the optimizer ranks passes by: the V2 score without the clamp at
    /// zero, cast to `float`, else [`optimizer_score`](Self::optimizer_score).
    /// On real boards the V2 penalties often pass 1000, and the clamped score
    /// is then 0 for better and worse boards alike.
    pub fn optimizer_ranking_score(&self, settings: Option<&RouterSettings>) -> f32 {
        match settings {
            Some(s) if s.optimizer_scoring.version == OptimizerScoringVersion::V2LowerBound => {
                self.optimizer_score_v2_unclamped(&s.optimizer_scoring.score_weights()) as f32
            }
            _ => self.optimizer_score(settings),
        }
    }

    /// Java `calculateScore(RoutingCostSettings)`: the legacy score (see
    /// [`super::legacy`]).
    pub fn legacy_score(&self, costs: &ScoringSettings) -> f32 {
        legacy::legacy_score(self, costs)
    }

    /// Java `getMaximumScore(RoutingCostSettings)`.
    pub fn legacy_maximum_score(&self, costs: &ScoringSettings) -> f32 {
        legacy::legacy_maximum_score(self, costs)
    }

    /// Java `getLegacyNormalizedScore`, which is also
    /// `getRouterScore(RoutingCostSettings)` and
    /// `getOptimizerScore(RoutingCostSettings)`.
    pub fn legacy_normalized_score(&self, costs: &ScoringSettings) -> f32 {
        legacy::legacy_normalized_score(self, costs)
    }

    /// The statistics without the parts that need the design rules
    /// checker.
    fn base(board: &BasicBoard, unit: Unit) -> Self {
        let mut s = BoardStatistics::default();
        let bb = board.bounding_box();
        let info = board.communication.specctra_parser_info.as_ref();
        let host_cad = info.and_then(|i| i.host_cad.as_deref()).unwrap_or("null");
        let host_version = info
            .and_then(|i| i.host_version.as_deref())
            .unwrap_or("null");
        s.host = unescape_unicode(&format!("{host_cad},{host_version}"));
        let board_unit = board.communication.unit;
        s.unit = board_unit.name().to_string();
        s.board.bounding_box = Rect {
            x: bb.ur.x as f32,
            y: bb.ur.y as f32,
            width: bb.ll.x as f32,
            height: bb.ll.y as f32,
        };
        s.board.size = Rect {
            x: 0.0,
            y: 0.0,
            width: (bb.ll.x as f32 - bb.ur.x as f32).abs(),
            height: (bb.ll.y as f32 - bb.ur.y as f32).abs(),
        };
        s.layers.total_count = board.layer_count();
        s.layers.signal_count = board.layer_structure.signal_layer_count();

        for item in board.items() {
            s.items.total_count += 1;
            match &item.kind {
                ItemKind::Trace(_) => s.items.trace_count += 1,
                ItemKind::Via(_) => s.items.via_count += 1,
                ItemKind::Area(area) if area.is_conduction_area() => {
                    s.items.conduction_area_count += 1
                }
                ItemKind::Pin(_) => s.items.pin_count += 1,
                ItemKind::ComponentOutline(_) => s.items.component_outline_count += 1,
                _ => s.items.other_count += 1,
            }
        }
        s.components.total_count = board.components.count();
        s.pads.total_count = board.pins().len();
        s.nets.total_count = board.rules.nets.max_net_number();
        s.nets.class_count = board.rules.net_classes.count();

        let traces: Vec<_> = board
            .traces()
            .into_iter()
            .filter_map(|id| board.item(id))
            .collect();
        s.traces.total_count = traces.len();
        // Java `mapToDouble(Trace::getLength).sum()`: compensated, and +0.0
        // without traces.
        s.traces.total_length = double_stream_sum(
            traces
                .iter()
                .filter_map(|t| t.as_trace())
                .map(|t| t.length()),
        ) as f32;
        let resolution = f64::from(if board.communication.resolution > 0 {
            board.communication.resolution
        } else {
            1
        });
        let to_mm = Unit::scale(1.0, board_unit, Unit::Mm) / resolution;
        s.board.area_cm2 = (f64::from(s.board.size.width)
            * to_mm
            * f64::from(s.board.size.height)
            * to_mm
            / 100.0) as f32;
        s.difficulty.pin_count = s.items.pin_count;
        s.difficulty.signal_layer_count = s.layers.signal_count;
        s.difficulty.complexity_c =
            (s.difficulty.pin_count * s.difficulty.signal_layer_count).max(1);
        s.difficulty.board_area_cm2 = s.board.area_cm2;
        s.difficulty.difficulty_d = s.difficulty.complexity_c as f32;
        s.bounds = calculate_bounds(board);
        s.traces.total_length_mm = (f64::from(s.traces.total_length) * to_mm) as f32;
        s.traces.average_length = if s.traces.total_count > 0 {
            s.traces.total_length / s.traces.total_count as f32
        } else {
            0.0
        };
        for trace in traces.iter().filter_map(|t| t.as_trace()) {
            let polyline = &trace.polyline;
            let corner_count = polyline.corner_count();
            if corner_count > 1 {
                s.traces.total_segment_count += corner_count - 1;
            }
            for line in polyline.lines() {
                let (a, b) = (line.a(), line.b());
                let (ax, ay, bx, by) = (
                    f64::from(a.x),
                    f64::from(a.y),
                    f64::from(b.x),
                    f64::from(b.y),
                );
                let length = ((ax - bx).powi(2) + (ay - by).powi(2)).sqrt() as f32;
                if ax == bx {
                    s.traces.total_vertical_length += length;
                } else if ay == by {
                    s.traces.total_horizontal_length += length;
                } else {
                    s.traces.total_angled_length += length;
                }
            }
        }
        let default_clearance_class = BoardRules::default_clearance_class();
        for item in &traces {
            let Some(trace) = item.as_trace() else {
                continue;
            };
            let fixed = item.fixed_state();
            if fixed == FixedState::Unfixed || fixed == FixedState::ShoveFixed {
                let mut weighted = trace.length()
                    * f64::from(
                        trace.half_width
                            + board.clearance_value(
                                item.clearance_class(),
                                default_clearance_class,
                                trace.layer,
                            ),
                    );
                if fixed == FixedState::ShoveFixed {
                    weighted /= 2.0;
                }
                s.traces.total_weighted_length += weighted as f32;
            }
        }
        for trace in traces.iter().filter_map(|t| t.as_trace()) {
            let polyline = &trace.polyline;
            let corner_count = polyline.corner_count();
            if corner_count < 3 {
                continue;
            }
            s.bends.total_count += corner_count - 2;
            let corner = |i: usize| -> FloatPoint { polyline.corner(i).to_float() };
            for i in 1..corner_count - 1 {
                let (prev, current, next) = (corner(i - 1), corner(i), corner(i + 1));
                let (dx1, dy1) = (current.x - prev.x, current.y - prev.y);
                let (dx2, dy2) = (next.x - current.x, next.y - current.y);
                let mut angle = (dy2.atan2(dx2) - dy1.atan2(dx1)).to_degrees().abs();
                angle = angle.min(360.0 - angle);
                if angle > 180.0 {
                    angle = 360.0 - angle;
                }
                if (angle - 90.0).abs() < 1.0 {
                    s.bends.ninety_degree_count += 1;
                } else if (angle - 45.0).abs() < 1.0 || (angle - 135.0).abs() < 1.0 {
                    s.bends.forty_five_degree_count += 1;
                } else {
                    s.bends.other_angle_count += 1;
                }
            }
        }
        let last_layer = s.layers.total_count as i32 - 1;
        for id in board.vias() {
            let Some(via) = board.item(id) else {
                continue;
            };
            s.vias.total_count += 1;
            let (first, last) = (via.first_layer(), via.last_layer());
            if first == 0 && last == last_layer {
                s.vias.through_hole_count += 1;
            } else if first == 0 || last == last_layer {
                s.vias.blind_count += 1;
            } else {
                s.vias.buried_count += 1;
            }
        }

        if unit != board_unit {
            let scale = |v: f32| Unit::scale(f64::from(v), board_unit, unit) as f32;
            s.unit = unit.name().to_string();
            let b = s.board.bounding_box;
            s.board.bounding_box = Rect {
                x: scale(b.x),
                y: scale(b.y),
                width: scale(b.width),
                height: scale(b.height),
            };
            s.board.size = Rect {
                x: 0.0,
                y: 0.0,
                width: scale(s.board.size.width),
                height: scale(s.board.size.height),
            };
            let t = &mut s.traces;
            t.total_length = scale(t.total_length);
            t.total_weighted_length = scale(t.total_weighted_length);
            t.average_length = scale(t.average_length);
            t.total_horizontal_length = scale(t.total_horizontal_length);
            t.total_vertical_length = scale(t.total_vertical_length);
            t.total_angled_length = scale(t.total_angled_length);
        }

        // The board does not change while counting, so the contacts of the
        // items are computed once for all pins.
        let mut contacts = HashMap::default();
        for id in board.smd_pins() {
            let Some(pin) = board.item(id) else {
                continue;
            };
            if pin.net_count() > 0 {
                s.fanout.total_smd_pins += 1;
                // Java counts the pins with an empty unconnected set as
                // already connected and reports the rest.
                if !board
                    .unconnected_set_cached(id, pin.net_number(0), &mut contacts)
                    .is_empty()
                {
                    s.fanout.pins_to_escape += 1;
                }
                if is_pin_escaped(board, id) {
                    s.fanout.escaped_count += 1;
                }
            }
        }
        s
    }
}

/// Java `legacyScoringOrDefault`: the settings' `scoring` section, else
/// that of `new DefaultSettings().getSettings()`.
fn legacy_scoring_or_default(settings: Option<&RouterSettings>) -> ScoringSettings {
    match settings {
        Some(s) => s.scoring.clone(),
        // The `scoring` defaults don't depend on the core count.
        None => RouterSettings::from_patch(&default_patch(1), 1).scoring,
    }
}

/// Java `BoardStatistics.isPinEscaped`: the pin has a contact trace
/// without clearance violations, a contact via without clearance
/// violations that continues in a trace or conduction area, or a contact
/// conduction area.
pub fn is_pin_escaped(board: &BasicBoard, pin: ItemId) -> bool {
    for contact in board.normal_contacts(pin) {
        let Some(item) = board.item(contact) else {
            continue;
        };
        match &item.kind {
            ItemKind::Trace(_) => {
                if board.clearance_violations(contact).is_empty() {
                    return true;
                }
            }
            ItemKind::Via(_) => {
                if board.clearance_violations(contact).is_empty()
                    && board.normal_contacts(contact).iter().any(|c| {
                        board
                            .item(*c)
                            .is_some_and(|c| c.is_trace() || c.is_conduction_area())
                    })
                {
                    return true;
                }
            }
            _ if item.is_conduction_area() => return true,
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_unicode_escapes() {
        assert_eq!(unescape_unicode("KiCad\\u0027s"), "KiCad's");
        assert_eq!(unescape_unicode("plain\\u00"), "plain\\u00");
    }
}

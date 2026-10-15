//! Board statistics.
//!
//! Port of `core.scoring.BoardStatistics` and its parts
//! (`BoardStatisticsBoard`, `BoardStatisticsItems`, ...), as far as the board
//! model supports them. The connection counts and clearance violations need
//! the design rule checker and are not part of the board model.

use std::collections::HashMap;

use freerouting_geometry::FloatPoint;

use super::bounds_calculator::{BoardStatisticsBounds, calculate_bounds};
use crate::board::BasicBoard;
use crate::board::items::ItemKind;
use crate::board::structure::{FixedState, Unit};
use crate::rules::BoardRules;

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

/// Port of `BoardStatistics.BoardStatisticsFanout` without `escaped_count`,
/// which needs the clearance checker.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BoardStatisticsFanout {
    /// SMD pins with a net.
    pub total_smd_pins: usize,
    /// SMD pins with a net minus those whose unconnected set
    /// (`Item.getUnconnectedSet`) is empty.
    pub pins_to_escape: usize,
}

/// Statistics of a board.
///
/// Port of `core.scoring.BoardStatistics`, without the connection counts
/// and the clearance violations (they belong to the design rule checker).
// TODO(router): Java's `getRouterScore` and `getOptimizerScore` take the
// router settings (`RouterSettings`), which come with the router stage.
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
    pub traces: BoardStatisticsTraces,
    pub bends: BoardStatisticsBends,
    pub vias: BoardStatisticsVias,
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
    /// The statistics of `board`, with lengths in millimeters.
    pub fn new(board: &BasicBoard) -> Self {
        Self::with_unit(board, Unit::Mm)
    }

    /// The statistics of `board`, with lengths converted to `unit`.
    pub fn with_unit(board: &BasicBoard, unit: Unit) -> Self {
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
        s.traces.total_length = traces
            .iter()
            .filter_map(|t| t.as_trace())
            .map(|t| t.length())
            .sum::<f64>() as f32;
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
        let mut contacts = HashMap::new();
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
            }
        }
        s
    }
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

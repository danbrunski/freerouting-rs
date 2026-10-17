//! Q02.1: the width each wide net actually got (a report; it changes
//! nothing).
//!
//! For each net whose class width on some signal layer is above the board's
//! narrowest class width, in ascending net number: the requested (class)
//! width per signal layer, the total trace length, the length under the
//! requested width on its layer and its share, the minimum width, and the
//! length per distinct width (ascending width). Every trace narrower than
//! its layer's class width counts as under-width, so the steps of a taper
//! count as narrow. The idea is KiCadRoutingTools'
//! `routing_common.power_width_report` (at `0486e799`: `requested_mm`,
//! `length_mm`, `under_mm`, `under_share`, `min_mm`, `by_width_mm`); no code
//! is ported.
//!
//! **The narrowest class width** is the smallest class trace width, over
//! the classes of the board's nets and its signal layers.
//!
//! **Delivered copper** is every trace but the system-fixed ones (DSN
//! `(type fix)` wires, such as pours exported as wires): the router can't
//! change them and the SES writer leaves them out, so they are input, not
//! delivered width. User-fixed (`protect`) and routed traces count.
//!
//! **Lengths** are [`PolylineTrace::length`] (the float corners), summed per
//! net in ascending item id, so the sums do not depend on the item order.
//! `tools/quality/ses_widths.py` computes the same figures from a SES with
//! an independent parser (`tests/quality_widths.rs` compares them).

use std::collections::BTreeMap;

use crate::board::BasicBoard;
use crate::board::items::{ItemKind, PolylineTrace};
use crate::board::structure::FixedState;
use crate::drc::json::JsonValue;
use crate::quality::feasibility::units_per_mm;

/// The schema of [`WidthReport::to_json`].
pub const SCHEMA: &str = "freerouting-widths/1";

/// The delivered widths of one wide net.
#[derive(Clone, Debug, PartialEq)]
pub struct NetWidthReport {
    pub net: i32,
    pub net_name: String,
    pub net_class: String,
    /// The class width per signal layer, as `(layer index, layer name,
    /// width in board units)`, ascending layer index.
    pub requested: Vec<(i32, String, i32)>,
    /// The total length of the net's traces, board units.
    pub length: f64,
    /// The length of the traces narrower than the class width on their
    /// layer, board units.
    pub under: f64,
    /// The smallest trace width, board units (`None` without traces).
    pub min_width: Option<i32>,
    /// The length per distinct width, ascending width.
    pub by_width: Vec<(i32, f64)>,
}

impl NetWidthReport {
    /// `under / length` (0 without traces).
    pub fn under_share(&self) -> f64 {
        if self.length > 0.0 {
            self.under / self.length
        } else {
            0.0
        }
    }

    /// The class width on `layer`, board units.
    pub fn requested_on(&self, layer: i32) -> Option<i32> {
        self.requested
            .iter()
            .find(|(l, _, _)| *l == layer)
            .map(|(_, _, w)| *w)
    }

    fn to_json(&self, upm: f64) -> JsonValue {
        let requested = self
            .requested
            .iter()
            .map(|(_, name, w)| {
                obj(vec![
                    ("layer", JsonValue::string(name)),
                    ("width_mm", mm4(f64::from(*w) / upm)),
                ])
            })
            .collect();
        let by_width = self
            .by_width
            .iter()
            .map(|(w, len)| {
                obj(vec![
                    ("width_mm", mm4(f64::from(*w) / upm)),
                    ("length_mm", mm4(len / upm)),
                ])
            })
            .collect();
        obj(vec![
            ("net", JsonValue::int(i64::from(self.net))),
            ("net_name", JsonValue::string(&self.net_name)),
            ("net_class", JsonValue::string(&self.net_class)),
            ("requested_mm", JsonValue::Array(requested)),
            ("length_mm", mm4(self.length / upm)),
            ("under_mm", mm4(self.under / upm)),
            ("under_share", mm4(self.under_share())),
            (
                "min_mm",
                self.min_width
                    .map_or(JsonValue::Null, |w| mm4(f64::from(w) / upm)),
            ),
            ("by_width_mm", JsonValue::Array(by_width)),
        ])
    }
}

/// The delivered-width report of a board.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct WidthReport {
    /// One entry per wide net, ascending net number.
    pub nets: Vec<NetWidthReport>,
    /// The board's narrowest class width, board units.
    pub narrowest_class_width: i32,
    /// The board's units per mm.
    pub units_per_mm: f64,
}

impl WidthReport {
    /// The under-width length over all reported nets, board units.
    pub fn under_total(&self) -> f64 {
        self.nets.iter().map(|n| n.under).sum()
    }

    /// True if some reported net has copper under its requested width.
    pub fn has_under_width(&self) -> bool {
        self.nets.iter().any(|n| n.under > 0.0)
    }

    /// The entry of net `name`, if it is reported.
    pub fn net(&self, name: &str) -> Option<&NetWidthReport> {
        self.nets.iter().find(|n| n.net_name == name)
    }

    /// The report's members as `freerouting-widths/1` JSON (the caller adds
    /// the ledger, [`super::NeckdownReport::to_json`]).
    pub fn json_members(&self) -> Vec<(String, JsonValue)> {
        let upm = self.units_per_mm;
        let under_nets = self.nets.iter().filter(|n| n.under > 0.0).count();
        vec![
            ("schema".into(), JsonValue::string(SCHEMA)),
            (
                "narrowest_class_width_mm".into(),
                mm4(f64::from(self.narrowest_class_width) / upm),
            ),
            ("count".into(), JsonValue::int(self.nets.len() as i64)),
            ("under_width_nets".into(), JsonValue::int(under_nets as i64)),
            ("under_mm".into(), mm4(self.under_total() / upm)),
            (
                "nets".into(),
                JsonValue::Array(self.nets.iter().map(|n| n.to_json(upm)).collect()),
            ),
        ]
    }

    /// The report alone as JSON.
    pub fn to_json(&self) -> JsonValue {
        JsonValue::Object(self.json_members())
    }
}

/// The class width of net `net` on `layer`, board units.
pub fn class_width(board: &BasicBoard, net: i32, layer: i32) -> i32 {
    board.rules.trace_half_width(net, layer).saturating_mul(2)
}

/// The signal layers of `board`, as `(index, name)`.
fn signal_layers(board: &BasicBoard) -> Vec<(i32, String)> {
    board
        .layer_structure
        .layers
        .iter()
        .enumerate()
        .filter(|(_, l)| l.is_signal)
        .map(|(i, l)| (i as i32, l.name.clone()))
        .collect()
}

/// The nets of `board` that have a net record, ascending.
fn net_numbers(board: &BasicBoard) -> Vec<i32> {
    (1..=board.rules.nets.max_net_number())
        .filter(|n| board.rules.nets.get(*n).is_some())
        .collect()
}

/// The board's narrowest class width over the classes of its nets and its
/// signal layers, board units (0 without nets or signal layers).
pub fn narrowest_class_width(board: &BasicBoard) -> i32 {
    let layers = signal_layers(board);
    net_numbers(board)
        .into_iter()
        .flat_map(|net| layers.iter().map(move |(l, _)| (net, *l)))
        .map(|(net, layer)| class_width(board, net, layer))
        .min()
        .unwrap_or(0)
}

/// The delivered traces of `board` with their item ids, ascending id:
/// every trace but the system-fixed ones.
pub fn traces_by_id(board: &BasicBoard) -> Vec<(i32, &[i32], &PolylineTrace)> {
    let mut traces: Vec<(i32, &[i32], &PolylineTrace)> = board
        .items()
        .filter(|item| item.fixed_state() != FixedState::SystemFixed)
        .filter_map(|item| match &item.kind {
            ItemKind::Trace(t) => Some((item.id().0, item.net_numbers(), t)),
            _ => None,
        })
        .collect();
    traces.sort_by_key(|(id, _, _)| *id);
    traces
}

/// The delivered-width report of `board`.
pub fn report(board: &BasicBoard) -> WidthReport {
    let layers = signal_layers(board);
    let narrowest = narrowest_class_width(board);
    let mut nets: BTreeMap<i32, NetWidthReport> = BTreeMap::new();
    for net in net_numbers(board) {
        let requested: Vec<(i32, String, i32)> = layers
            .iter()
            .map(|(l, name)| (*l, name.clone(), class_width(board, net, *l)))
            .collect();
        if !requested.iter().any(|(_, _, w)| *w > narrowest) {
            continue;
        }
        let record = board.rules.nets.get(net);
        nets.insert(
            net,
            NetWidthReport {
                net,
                net_name: record.map_or_else(|| format!("net {net}"), |n| n.name.clone()),
                net_class: board
                    .rules
                    .net_class_of(net)
                    .map_or_else(String::new, |c| c.name().to_string()),
                requested,
                length: 0.0,
                under: 0.0,
                min_width: None,
                by_width: Vec::new(),
            },
        );
    }
    let mut by_width: BTreeMap<i32, BTreeMap<i32, f64>> = BTreeMap::new();
    for (_, net_numbers, trace) in traces_by_id(board) {
        let width = trace.half_width.saturating_mul(2);
        let length = trace.length();
        for net in net_numbers {
            let Some(entry) = nets.get_mut(net) else {
                continue;
            };
            entry.length += length;
            if width < class_width(board, *net, trace.layer) {
                entry.under += length;
            }
            entry.min_width = Some(entry.min_width.map_or(width, |m| m.min(width)));
            *by_width
                .entry(*net)
                .or_default()
                .entry(width)
                .or_insert(0.0) += length;
        }
    }
    for (net, widths) in by_width {
        if let Some(entry) = nets.get_mut(&net) {
            entry.by_width = widths.into_iter().collect();
        }
    }
    WidthReport {
        nets: nets.into_values().collect(),
        narrowest_class_width: narrowest,
        units_per_mm: units_per_mm(board),
    }
}

pub(super) fn mm4(v: f64) -> JsonValue {
    if v.is_finite() {
        JsonValue::Number(format!("{v:.4}"))
    } else {
        JsonValue::Null
    }
}

pub(super) fn obj(members: Vec<(&str, JsonValue)>) -> JsonValue {
    JsonValue::Object(
        members
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

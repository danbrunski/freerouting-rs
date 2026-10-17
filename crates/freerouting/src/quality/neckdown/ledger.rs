//! Q02.2: the narrowing ledger, and the strict-sizes rule.
//!
//! **The ledger** lists every trace of the final board that is narrower than
//! its net's class width on its layer, in insertion order (ascending item
//! id: the board's id generator only counts up), with the net, the item id,
//! the requested and delivered widths, the length and the stage that made
//! it. The idea is KiCadRoutingTools' `fab_tiers.note_narrowing` (at
//! `0486e799`); no code is ported.
//!
//! **Stages** are found from item ids, not from hooks in the stages: the
//! `widths` diagnostic records the largest item id of the input board and,
//! for every connection-router call, the first id the call could use
//! ([`ConnectionOutcome::first_new_id`](crate::autoroute::stages::ConnectionOutcome))
//! and the largest id after it. An item is then
//! - `input`: on the board as loaded;
//! - the fanout stage id: made before the first connection-router call;
//! - the neck-down policy id: made by a call, with the half width of
//!   Java's neck-down retry (`router.neck_width_um` > 0, the retry width
//!   [`neck_half_width`]);
//! - the connection-router id: made by a call otherwise (Java's room-local
//!   neck-down, `automatic_neckdown`, and the fanout's micro neck-downs of
//!   that call), or between two calls (the pass's own clean-up);
//! - `optimizer`: made after the last call (the optimizer rewrites traces
//!   and keeps their widths).
//!
//! Without any call (the router off) every item after the input is the
//! fanout's. Narrowings that a later rip removed are not listed: the ledger
//! is about shipped copper.
//!
//! **Strict sizes** (`router.neck_down.strict`, unset by default: off): the
//! neck-down stages refuse to narrow (the board settings turn
//! `automatic_neckdown` off and `neck_width_um` to 0,
//! [`BoardRouterSettings::from_settings`](crate::settings::board::BoardRouterSettings::from_settings)),
//! the `widths` diagnostic runs, and the CLI exits with 3 when any reported
//! net has copper under its requested width (KRT's `--strict-sizes`). The
//! fanout's micro neck-downs and narrow input copper are not refused; the
//! exit status reports them.
//!
//! **No width ratchet** (Q00 rule 7): nothing here writes the board's rules;
//! `tests/quality_no_ratchet.rs` checks that routing, writing DSN and SES,
//! reading them back and routing again leaves every class width as loaded.

use crate::autoroute::batch::connection::neck_half_width;
use crate::board::RoutingBoard;
use crate::drc::json::JsonValue;
use crate::settings::RouterSettings;

use super::width_report::{WidthReport, class_width, mm4, obj, traces_by_id};

/// The `widths` diagnostic id that strict mode adds.
pub const STRICT_DIAGNOSTIC: &str = super::DIAGNOSTIC_ID;

/// The CLI exit status of a strict run that shipped under-width copper.
pub const STRICT_EXIT_CODE: u8 = 3;

/// One shipped narrowing.
#[derive(Clone, Debug, PartialEq)]
pub struct Narrowing {
    pub net: i32,
    pub net_name: String,
    /// The trace's item id.
    pub item: i32,
    pub layer: String,
    /// The class width on the trace's layer, board units.
    pub requested: i32,
    /// The trace's width, board units.
    pub delivered: i32,
    /// The trace's length, board units.
    pub length: f64,
    /// The stage that made the trace (see the module docs).
    pub stage: String,
}

/// Every shipped narrowing, in insertion order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NarrowingLedger {
    pub entries: Vec<Narrowing>,
}

impl NarrowingLedger {
    /// The ledger as a JSON array.
    pub fn to_json(&self, upm: f64) -> JsonValue {
        JsonValue::Array(
            self.entries
                .iter()
                .map(|n| {
                    obj(vec![
                        ("net", JsonValue::int(i64::from(n.net))),
                        ("net_name", JsonValue::string(&n.net_name)),
                        ("item", JsonValue::int(i64::from(n.item))),
                        ("layer", JsonValue::string(&n.layer)),
                        ("requested_mm", mm4(f64::from(n.requested) / upm)),
                        ("delivered_mm", mm4(f64::from(n.delivered) / upm)),
                        ("length_mm", mm4(n.length / upm)),
                        ("stage", JsonValue::string(&n.stage)),
                    ])
                })
                .collect(),
        )
    }
}

/// One connection-router call, as the ledger sees it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CallMark {
    /// The first id the call could use.
    pub first_new_id: i32,
    /// The largest item id on the board after the call.
    pub max_after: i32,
}

/// The id marks the ledger attributes stages by, and the stage ids.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StageMarks {
    /// The largest item id of the input board (`None` before `on_input`).
    pub input_max: Option<i32>,
    /// The connection-router calls, in call order.
    pub calls: Vec<CallMark>,
    pub fanout_id: String,
    pub router_id: String,
    pub neck_down_id: String,
    /// The neck-down retry's width in µm (Java's `neck_width_um`; ≤ 0 off).
    pub neck_width_um: f64,
    /// The `router.min_trace_width_um` floor as a half width, board units.
    pub min_trace_half_width: i32,
}

impl StageMarks {
    /// The marks of a job with `settings` (stage ids from its selection).
    pub fn from_settings(settings: &RouterSettings) -> StageMarks {
        let selection = crate::autoroute::stages::StageSelection::from_settings(settings);
        StageMarks {
            input_max: None,
            calls: Vec::new(),
            fanout_id: selection.fanout.id,
            router_id: selection.connection_router.id,
            neck_down_id: selection.neck_down.id,
            neck_width_um: 0.0,
            min_trace_half_width: 0,
        }
    }

    /// The stage of the item with id `id` and half width `half_width`.
    pub fn stage_of(&self, board: &RoutingBoard, id: i32, half_width: i32) -> String {
        if self.input_max.is_some_and(|max| id <= max) {
            return "input".into();
        }
        let call = self.calls.iter().rev().find(|c| c.first_new_id <= id);
        let Some(call) = call else {
            return self.fanout_id.clone();
        };
        let last_max = self.calls.iter().map(|c| c.max_after).max().unwrap_or(0);
        if id > last_max {
            return "optimizer".into();
        }
        if id <= call.max_after
            && self.neck_width_um > 0.0
            && half_width == neck_half_width(board, self.neck_width_um, self.min_trace_half_width)
        {
            return self.neck_down_id.clone();
        }
        self.router_id.clone()
    }
}

/// The ledger of the final `board`: its under-width traces of the nets in
/// `report`, ascending item id, attributed by `marks`.
pub fn ledger(board: &RoutingBoard, report: &WidthReport, marks: &StageMarks) -> NarrowingLedger {
    let layer_name = |layer: i32| {
        board
            .layer_structure
            .layers
            .get(layer as usize)
            .map_or_else(|| format!("layer {layer}"), |l| l.name.clone())
    };
    let mut entries = Vec::new();
    for (id, nets, trace) in traces_by_id(board) {
        let delivered = trace.half_width.saturating_mul(2);
        for net in nets {
            let Some(entry) = report.nets.iter().find(|n| n.net == *net) else {
                continue;
            };
            let requested = class_width(board, *net, trace.layer);
            if delivered >= requested {
                continue;
            }
            entries.push(Narrowing {
                net: *net,
                net_name: entry.net_name.clone(),
                item: id,
                layer: layer_name(trace.layer),
                requested,
                delivered,
                length: trace.length(),
                stage: marks.stage_of(board, id, trace.half_width),
            });
        }
    }
    NarrowingLedger { entries }
}

/// True if `settings` asks for strict sizes.
pub fn is_strict(settings: &RouterSettings) -> bool {
    settings.neck_down.strict == Some(true)
}

/// `settings` with the `widths` diagnostic added when strict sizes are on
/// and it isn't listed (the strict exit status needs the report).
pub fn with_strict_report(settings: RouterSettings) -> RouterSettings {
    if !is_strict(&settings) {
        return settings;
    }
    let listed = settings
        .special
        .diagnostics
        .as_ref()
        .is_some_and(|d| d.iter().any(|id| id == STRICT_DIAGNOSTIC));
    if listed {
        return settings;
    }
    let mut settings = settings;
    settings
        .special
        .diagnostics
        .get_or_insert_with(Vec::new)
        .push(STRICT_DIAGNOSTIC.to_string());
    settings
}

/// True if a strict run with `report` must exit with [`STRICT_EXIT_CODE`].
pub fn strict_violation(settings: &RouterSettings, report: Option<&WidthReport>) -> bool {
    is_strict(settings) && report.is_some_and(WidthReport::has_under_width)
}

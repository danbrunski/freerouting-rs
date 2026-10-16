//! Pull-tightens a whole routed board with trace costs, so the via optimizer
//! runs too (O05.6), and writes the canonical item dump, as probe
//! `opt-vias`'s `board-optvias` section does: loads the design, imports the
//! session, unfixes every trace and via (the import makes them user-fixed),
//! marks the whole board and runs `opt_changed_area` with accuracy 500, no
//! clip, no limit and the trace costs.
//!
//! ```sh
//! cargo run --release -p freerouting --example opt_vias_dump -- \
//!     board.dsn board.ses out.jsonl [--trace-costs <json>|default]
//! ```
//!
//! `<json>` is a JSON array with one entry per layer, either `[horizontal,
//! vertical]` or `{"horizontal": h, "vertical": v}`, given inline (starting
//! with `[`) or as a file path. `default` (also without the option) takes the
//! board-specific costs of the default settings, as the batch autorouter
//! builds them. The run time goes to stderr; the output is deterministic
//! (`tools/determinism-check.sh`).

#[path = "../tests/board_probe/mod.rs"]
mod board_probe;
#[path = "../tests/support/item_dump.rs"]
mod item_dump;

use std::process::ExitCode;
use std::time::Instant;

use freerouting::autoroute::control::ExpansionCostFactor;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::drc::json::JsonValue;
use freerouting::drc::load_design;
use freerouting::io::read_session;
use freerouting::optimize::PullTightParams;
use freerouting::settings::board::apply_board_specific;
use freerouting::settings::{RouterSettings, default_patch};

const USAGE: &str =
    "usage: opt_vias_dump <dsn> <ses> <out> [--trace-costs <json-array|file>|default]";

fn number(value: &JsonValue) -> Result<f64, String> {
    match value {
        JsonValue::Number(n) => n.parse().map_err(|e| format!("{n}: {e}")),
        other => Err(format!("not a number: {other:?}")),
    }
}

/// One layer's costs: `[h, v]` or `{"horizontal": h, "vertical": v}`.
fn cost_factor(value: &JsonValue) -> Result<ExpansionCostFactor, String> {
    match value {
        JsonValue::Array(pair) if pair.len() == 2 => Ok(ExpansionCostFactor {
            horizontal: number(&pair[0])?,
            vertical: number(&pair[1])?,
        }),
        JsonValue::Object(members) => {
            let field = |key: &str| {
                members
                    .iter()
                    .find(|(k, _)| k == key)
                    .ok_or_else(|| format!("missing \"{key}\""))
                    .and_then(|(_, v)| number(v))
            };
            Ok(ExpansionCostFactor {
                horizontal: field("horizontal")?,
                vertical: field("vertical")?,
            })
        }
        other => Err(format!(
            "a layer's costs must be [h, v] or an object: {other:?}"
        )),
    }
}

fn parse_costs(spec: &str) -> Result<Vec<ExpansionCostFactor>, String> {
    let text = if spec.trim_start().starts_with('[') {
        spec.to_string()
    } else {
        std::fs::read_to_string(spec).map_err(|e| format!("{spec}: {e}"))?
    };
    match JsonValue::parse(&text).map_err(|e| format!("--trace-costs: {e:?}"))? {
        JsonValue::Array(layers) => layers.iter().map(cost_factor).collect(),
        _ => Err("--trace-costs takes a JSON array".into()),
    }
}

/// The batch autorouter's costs with default settings.
fn default_costs(board: &RoutingBoard) -> Vec<ExpansionCostFactor> {
    let mut patch = default_patch(1);
    apply_board_specific(&mut patch, board);
    RouterSettings::from_patch(&patch, 1).trace_costs()
}

fn run(args: &[String]) -> Result<(), String> {
    let (mut positional, mut costs_spec) = (Vec::new(), None);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--trace-costs" {
            costs_spec = Some(iter.next().ok_or(USAGE)?.clone());
        } else {
            positional.push(arg.clone());
        }
    }
    let [dsn, ses, out] = positional.as_slice() else {
        return Err(USAGE.into());
    };
    let bytes = std::fs::read(dsn).map_err(|e| format!("{dsn}: {e}"))?;
    let mut board = load_design(&bytes).map_err(|e| format!("{dsn}: {e}"))?;
    let session = std::fs::read(ses).map_err(|e| format!("{ses}: {e}"))?;
    read_session(&mut board, &freerouting::dsn::decode(&session))
        .map_err(|e| format!("{ses}: {e}"))?;
    let costs = match costs_spec.as_deref() {
        None | Some("default") => default_costs(&board),
        Some(spec) => parse_costs(spec)?,
    };
    if costs.len() < board.layer_count() {
        return Err(format!(
            "--trace-costs has {} layers, the board {}",
            costs.len(),
            board.layer_count()
        ));
    }
    let ids: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_trace() || i.as_via().is_some())
        .map(|i| i.id())
        .collect();
    for id in ids {
        board.unfix(id);
    }
    let start = Instant::now();
    board.start_marking_changed_area();
    board.mark_all_changed_area();
    let mut params = PullTightParams::new(500);
    params.trace_costs = Some(costs.into());
    board.opt_changed_area(&params);
    eprintln!("opt_vias_dump: opt_changed_area {:?}", start.elapsed());
    let mut lines = vec![board_probe::header("opt-vias", dsn)];
    lines.extend(item_dump::dump(&board));
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(out, text).map_err(|e| format!("{out}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("opt_vias_dump: {e}");
            ExitCode::FAILURE
        }
    }
}

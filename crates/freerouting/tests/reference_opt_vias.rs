//! The via optimizer (O05) against Java at the pin: probe `opt-vias`
//! (`tools/java-reference/probes/board/OptViasProbe.java`, which documents
//! its sections) writes `fixtures/reference/probe/opt-vias/<board>.jsonl`.
//!
//! - `costs`: the batch autorouter's trace costs Java built for the board
//!   (default settings after `applyBoardSpecificOptimizations`). The Rust
//!   side reads them from Java's file, so the comparison doesn't depend on
//!   the Rust settings port (C03).
//! - `vias-each`: `opt_via_location(via, costs, 500, 10)` for every via,
//!   highest id first, with the result, the centre and `maxGeneratedId`
//!   after each, then the canonical item dump (O05.1, O05.2).
//! - `board-optvias`: `opt_changed_area` of the whole marked board with the
//!   trace costs, then the dump (O05.3).
//!
//! Every board is the routed session with its traces and vias unfixed (the
//! import makes them user-fixed), 45°, on the 41 boards of probe
//! `pull-tight`. [`EXPECTED_MISMATCHES`] is empty: every board matches.

mod board_probe;
#[path = "support/item_dump.rs"]
mod item_dump;

use std::collections::BTreeMap;

use board_probe::{Line, Row, fixture, repo_root};
use freerouting::autoroute::control::ExpansionCostFactor;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::geometry::Point;
use freerouting::optimize::{OPT_VIA_RECURSION_DEPTH, PullTightParams};

const KIND: &str = "opt-vias";
const ACCURACY: i32 = 500;

/// Boards whose Java output may differ, each with the first differing via
/// move and its cause. None.
const EXPECTED_MISMATCHES: [(&str, &str); 0] = [];

/// The board set of probe `pull-tight` (`OptViasProbe.BOARD_SET`).
const BOARD_SET: [&str; 41] = [
    "empty_board",
    "scoring-perfect-two-pin",
    "Issue756-minimal-ok",
    "Issue757-minimal-soe-ok",
    "Issue954-circle",
    "Issue756-minimal-hang",
    "Issue757-minimal-soe",
    "scoring-empty-net",
    "scoring-zero-length",
    "scoring-mixed-layer",
    "Issue508-SMD-routing-issue-demo",
    "Issue269-min_fr_test/min_fr_test_no_quotes",
    "Issue269-min_fr_test/min_fr_test",
    "Issue368-CorneyIslandWireless_input_design",
    "Issue143-rpi_splitter",
    "Issue270-non-ansi_bracket",
    "Issue143-rpi_splitter_mod",
    "PCBench-retroreflectors-CONGFLOCK",
    "Issue954-invalid-polygon64",
    "Issue954-polygon64",
    "Issue368-CorneyIslandWireless/corney_island_wireless",
    "Issue069-TestSensel/TestSensel-KiCad6",
    "Issue508-DAC2020/DAC2020_bm08/DAC2020_bm08.unrouted",
    "Issue508-DAC2020/DAC2020_bm08/FRv1.9.0/DAC2020_bm08.unrouted",
    "Issue508-DAC2020/DAC2020_bm08/FRv2.1.0/DAC2020_bm08.unrouted",
    "Issue508-DAC2020_bm08",
    "Issue069-TestSensel/TestSensel",
    "Issue110-testProjectFromFreeroutingBugTest",
    "Issue026-J2_reference",
    "Issue433-my-board",
    "Issue413-test",
    "Issue110-testProjectFromFreeroutingBugTest01",
    "Issue508-DAC2020/DAC2020_bm07/DAC2020_bm07.unrouted",
    "Issue508-DAC2020/DAC2020_bm07/FRv1.9.0/DAC2020_bm07.unrouted",
    "Issue508-DAC2020/DAC2020_bm07/FRv2.1.0/DAC2020_bm07.unrouted",
    "Issue508-DAC2020_bm07",
    "Issue957-smoke-ldo-led",
    "Issue508-DAC2020/DAC2020_bm05/DAC2020_bm05.unrouted",
    "Issue508-DAC2020/DAC2020_bm05/FRv1.9.0/DAC2020_bm05.unrouted",
    "Issue508-DAC2020/DAC2020_bm05/FRv2.1.0/DAC2020_bm05.unrouted",
    "Issue508-DAC2020_bm05",
];

/// The routed board with every trace and via unfixed, 45°.
fn unfixed(row: &Row) -> RoutingBoard {
    let mut board = board_probe::load(row);
    board_probe::import_session(&mut board, row);
    let ids: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_trace() || i.as_via().is_some())
        .map(|i| i.id())
        .collect();
    for id in ids {
        board.unfix(id);
    }
    board
}

fn max_generated_id(board: &RoutingBoard) -> i64 {
    use freerouting::datastructures::IdGenerator;
    i64::from(board.communication.id_generator.max_generated_id())
}

/// The value of `"key":"0x…"` in a probe line, as a double.
fn bits_field(line: &str, key: &str) -> f64 {
    let start = line.find(&format!("\"{key}\":\"0x")).expect(key) + key.len() + 6;
    let end = start + line[start..].find('"').expect("closing quote");
    f64::from_bits(u64::from_str_radix(&line[start..end], 16).expect("hex bits"))
}

/// Java's trace costs for `row`, from the `costs` section of its output.
fn java_costs(row: &Row) -> (Vec<String>, Vec<ExpansionCostFactor>) {
    let path = repo_root()
        .join("fixtures/reference/probe")
        .join(KIND)
        .join(format!("{}.jsonl", row.board));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|_| {
        panic!(
            "{KIND}/{}: no Java output; generate it with {}",
            row.board,
            board_probe::probe_command(KIND, row)
        )
    });
    let mut lines = Vec::new();
    let mut costs = Vec::new();
    let mut in_costs = false;
    for line in text.lines().skip(1) {
        if line.starts_with("{\"stage\":") {
            in_costs = line.starts_with("{\"stage\":\"costs\"");
            if !in_costs && !lines.is_empty() {
                break;
            }
        }
        if !in_costs {
            continue;
        }
        if line.starts_with("{\"layer\":") {
            costs.push(ExpansionCostFactor {
                horizontal: bits_field(line, "horizontal"),
                vertical: bits_field(line, "vertical"),
            });
        }
        lines.push(line.to_string());
    }
    (lines, costs)
}

fn costs_lines(costs: &[ExpansionCostFactor]) -> Vec<String> {
    let mut lines = vec![
        Line::new()
            .str("stage", "costs")
            .num("layers", costs.len() as i64)
            .end(),
    ];
    for (i, c) in costs.iter().enumerate() {
        lines.push(
            Line::new()
                .num("layer", i as i64)
                .dbl("horizontal", c.horizontal)
                .dbl("vertical", c.vertical)
                .end(),
        );
    }
    lines
}

/// Section `vias-each`.
fn vias_each(row: &Row, costs: &[ExpansionCostFactor]) -> Vec<String> {
    let mut board = unfixed(row);
    let mut lines = vec![Line::new().str("stage", "vias-each").end()];
    let vias: Vec<ItemId> = board
        .items()
        .filter(|i| i.as_via().is_some())
        .map(|i| i.id())
        .collect();
    board.start_marking_changed_area();
    for via in vias {
        if !board.item(via).is_some_and(|i| i.is_on_the_board()) {
            continue;
        }
        let result = board.opt_via_location(via, Some(costs), ACCURACY, OPT_VIA_RECURSION_DEPTH);
        let center = Point::Int(
            board
                .item(via)
                .expect("an item")
                .drill()
                .expect("a via")
                .center,
        );
        lines.push(
            Line::new()
                .num("via", i64::from(via.0))
                .bool("result", result)
                .str("center", &item_dump::point_text(&center))
                .num("maxGeneratedId", max_generated_id(&board))
                .end(),
        );
    }
    lines.extend(item_dump::dump(&board));
    lines
}

/// Section `board-optvias`, and the board after it.
fn board_optvias(row: &Row, costs: &[ExpansionCostFactor]) -> (Vec<String>, RoutingBoard) {
    let mut board = unfixed(row);
    board.start_marking_changed_area();
    board.mark_all_changed_area();
    let mut params = PullTightParams::new(ACCURACY);
    params.trace_costs = Some(costs.into());
    board.opt_changed_area(&params);
    let mut lines = vec![Line::new().str("stage", "board-optvias").end()];
    lines.extend(item_dump::dump(&board));
    (lines, board)
}

/// The clearance violations of `board` by (layer, the two items' kinds and
/// nets), which doesn't depend on item ids.
fn violation_counts(board: &RoutingBoard) -> BTreeMap<String, usize> {
    let describe = |id: ItemId| {
        let item = board.item(id).expect("an item");
        let kind = if item.is_trace() {
            "trace"
        } else if item.is_via() {
            "via"
        } else if item.is_pin() {
            "pin"
        } else {
            "other"
        };
        format!("{kind}{:?}", item.net_numbers())
    };
    let mut counts = BTreeMap::new();
    for v in board.all_clearance_violations() {
        let mut pair = [describe(v.first_item), describe(v.second_item)];
        pair.sort();
        *counts
            .entry(format!("layer {} {} {}", v.layer, pair[0], pair[1]))
            .or_insert(0) += 1;
    }
    counts
}

/// The vias overlapping a pin of another net (D8).
fn vias_in_foreign_pads(board: &RoutingBoard) -> usize {
    board
        .all_clearance_violations()
        .into_iter()
        .filter(|v| {
            let (a, b) = (
                board.item(v.first_item).expect("an item"),
                board.item(v.second_item).expect("an item"),
            );
            let via_pin = (a.is_via() && b.is_pin()) || (a.is_pin() && b.is_via());
            via_pin && !a.shares_net(b) && v.actual_clearance <= 0.0
        })
        .count()
}

/// The DRC acceptance of O05.5: no clearance violation the input board
/// didn't have, no via inside a pad of another net that wasn't there.
fn drc_regressions(row: &Row, after: &RoutingBoard) -> Result<(), String> {
    let before = unfixed(row);
    let (old, new) = (violation_counts(&before), violation_counts(after));
    let added: Vec<String> = new
        .iter()
        .filter(|(key, n)| old.get(*key).is_none_or(|o| o < n))
        .map(|(key, n)| format!("{key}: {} -> {n}", old.get(key).copied().unwrap_or(0)))
        .collect();
    let (pads_before, pads_after) = (vias_in_foreign_pads(&before), vias_in_foreign_pads(after));
    if added.is_empty() && pads_after <= pads_before {
        return Ok(());
    }
    Err(format!(
        "{}: new clearance violations {added:?}, vias in foreign pads {pads_before} -> {pads_after}",
        row.board
    ))
}

/// O05.1-O05.3, O05.5: both sections equal Java's on every board, ids
/// included, and the optimized board has no new clearance violation.
#[test]
fn opt_vias_match_java() {
    let mut failures = Vec::new();
    let mut moved = 0usize;
    for board in BOARD_SET {
        if EXPECTED_MISMATCHES.iter().any(|(b, _)| *b == board) {
            continue;
        }
        let row = fixture(board);
        let (java_cost_lines, costs) = java_costs(&row);
        assert_eq!(
            costs_lines(&costs),
            java_cost_lines,
            "{board}: costs section"
        );
        let mut lines = vec![board_probe::header(KIND, &row.board)];
        lines.extend(costs_lines(&costs));
        let each = vias_each(&row, &costs);
        moved += each
            .iter()
            .filter(|l| l.contains("\"result\":true"))
            .count();
        lines.extend(each);
        let (section, optimized) = board_optvias(&row, &costs);
        lines.extend(section);
        if let Err(e) = board_probe::compare(KIND, &row, &lines) {
            failures.push(e);
        }
        if let Err(e) = drc_regressions(&row, &optimized) {
            failures.push(e);
        }
    }
    assert!(
        failures.is_empty(),
        "{} failures on {} boards:\n{}",
        failures.len(),
        BOARD_SET.len(),
        failures.join("\n")
    );
    eprintln!("opt-vias: {moved} vias moved in vias-each");
}

/// O05.3: without trace costs the via branch is off: no via moves, and the
/// result equals Java's `board-45` of probe `pull-tight` (O01).
#[test]
fn opt_changed_area_without_costs_skips_vias() {
    for board in [
        "Issue026-J2_reference",
        "Issue508-DAC2020_bm08",
        "Issue508-DAC2020_bm05",
        "Issue143-rpi_splitter",
    ] {
        let row = fixture(board);
        let mut board_state = unfixed(&row);
        let centres = |b: &RoutingBoard| -> Vec<(ItemId, Point)> {
            b.items()
                .filter_map(|i| Some((i.id(), Point::Int(i.drill()?.center))))
                .filter(|(id, _)| b.item(*id).expect("an item").is_via())
                .collect()
        };
        let before = centres(&board_state);
        board_state.start_marking_changed_area();
        board_state.mark_all_changed_area();
        board_state.opt_changed_area(&PullTightParams::new(ACCURACY));
        assert_eq!(centres(&board_state), before, "{board}: a via moved");
        let path = repo_root()
            .join("fixtures/reference/probe/pull-tight")
            .join(format!("{}.jsonl", row.board));
        let text = std::fs::read_to_string(&path).expect("pull-tight output");
        let java: Vec<&str> = text
            .lines()
            .skip_while(|l| !l.starts_with("{\"stage\":\"board-45\""))
            .skip(1)
            .take_while(|l| !l.starts_with("{\"stage\":"))
            .collect();
        let rust = item_dump::dump(&board_state);
        item_dump::compare_dumps(&java, &rust).unwrap_or_else(|e| panic!("{board}: {e}"));
    }
}

/// A cost array without an entry for a contact trace's layer leaves a via
/// between two traces unchanged (Java throws
/// `ArrayIndexOutOfBoundsException`; DIVERGENCES "Pull-tight"). On J2, Java
/// moves 5 vias with its costs; with an empty array no via between two
/// traces moves and no id is used.
#[test]
fn short_trace_costs_leave_a_via_between_traces() {
    let row = fixture("Issue026-J2_reference");
    let mut board = unfixed(&row);
    let vias: Vec<ItemId> = board
        .items()
        .filter(|i| i.as_via().is_some())
        .map(|i| i.id())
        .collect();
    let two_traces = |b: &RoutingBoard, via: ItemId| {
        let contacts = b.normal_contacts(via);
        contacts.len() == 2
            && contacts
                .iter()
                .all(|c| b.item(*c).is_some_and(|i| i.is_trace()))
    };
    let mut checked = 0;
    board.start_marking_changed_area();
    for via in vias {
        if !two_traces(&board, via) {
            continue;
        }
        let max_id = max_generated_id(&board);
        assert!(!board.opt_via_location(via, Some(&[]), ACCURACY, OPT_VIA_RECURSION_DEPTH));
        assert_eq!(max_generated_id(&board), max_id);
        checked += 1;
    }
    assert!(checked > 0, "J2 has vias between two traces");
}

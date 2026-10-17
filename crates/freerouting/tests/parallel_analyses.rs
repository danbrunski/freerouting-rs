//! P03.1: the analyses on the analysis threads (`freerouting::parallel`)
//! give the same answers on every thread budget, and the statistics'
//! fanout counts equal their per-pin definition (Java's loop over
//! `getSmdPins` with `getUnconnectedSet` and `isPinEscaped`).
//!
//! The boards are routed: each fixture with its Java route session from
//! `fixtures/reference/route`, so the connected sets, contacts and airlines
//! are those of the later passes. The bounds and the unrouted fanout counts
//! are compared with Java directly by `reference_load`.

use std::path::{Path, PathBuf};

use freerouting::board::routing_board::RoutingBoard;
use freerouting::drc::{DesignRulesChecker, load_design};
use freerouting::io::read_session;
use freerouting::parallel::ThreadBudget;
use freerouting::scoring::{BoardStatistics, is_pin_escaped};

/// Small and medium boards with SMD pins, planes and several layers.
const BOARDS: &[&str] = &[
    "Issue143-rpi_splitter",
    "Issue508-DAC2020_bm07",
    "Issue093-interf_u",
    "Issue555-CNH_Functional_Tester_1",
];

const BUDGETS: &[usize] = &[1, 2, 3, 8];

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn loaded(name: &str) -> RoutingBoard {
    let dsn = std::fs::read(fixtures().join(format!("{name}.dsn"))).expect("fixture");
    load_design(&dsn).expect("loads")
}

fn routed(name: &str) -> RoutingBoard {
    let mut board = loaded(name);
    let ses = fixtures().join(format!("reference/route/{name}.ses"));
    let session = std::fs::read_to_string(&ses).expect("route reference session");
    read_session(&mut board, &session).expect("session imports");
    board
}

#[test]
fn statistics_do_not_depend_on_the_thread_budget() {
    for name in BOARDS {
        let board = routed(name);
        let reference = {
            let _budget = ThreadBudget::set(1);
            BoardStatistics::new(&board)
        };
        assert!(reference.traces.total_count > 0, "{name}: not routed");
        for &threads in BUDGETS {
            let _budget = ThreadBudget::set(threads);
            assert_eq!(
                BoardStatistics::new(&board),
                reference,
                "{name}: {threads} threads"
            );
        }
    }
}

#[test]
fn fanout_counts_equal_the_per_pin_definition() {
    let (mut some_to_escape, mut some_connected) = (false, false);
    for (name, board) in BOARDS
        .iter()
        .flat_map(|name| [(name, loaded(name)), (name, routed(name))])
    {
        let (mut total, mut to_escape, mut escaped) = (0, 0, 0);
        for id in board.smd_pins() {
            let pin = board.item(id).expect("pin");
            if pin.net_count() == 0 {
                continue;
            }
            total += 1;
            if !board.unconnected_set(id, pin.net_number(0)).is_empty() {
                to_escape += 1;
            }
            if is_pin_escaped(&board, id) {
                escaped += 1;
            }
        }
        assert!(total > 0, "{name}: no SMD pin with a net");
        some_to_escape |= to_escape > 0;
        some_connected |= to_escape < total;
        for &threads in BUDGETS {
            let _budget = ThreadBudget::set(threads);
            let fanout = BoardStatistics::new(&board).fanout;
            assert_eq!(
                (
                    fanout.total_smd_pins,
                    fanout.pins_to_escape,
                    fanout.escaped_count
                ),
                (total, to_escape, escaped),
                "{name}: {threads} threads"
            );
        }
    }
    assert!(some_to_escape && some_connected, "both answers are tested");
}

#[test]
fn drc_findings_do_not_depend_on_the_thread_budget() {
    for name in BOARDS {
        let board = routed(name);
        let check = |threads: usize| {
            let _budget = ThreadBudget::set(threads);
            let mut drc = DesignRulesChecker::new(&board);
            let findings = drc.check();
            let airlines = drc.all_airlines();
            let counts = (drc.incomplete_count(), drc.max_connections());
            (findings, airlines, counts)
        };
        let (reference, airlines, counts) = check(1);
        for &threads in &BUDGETS[1..] {
            let (findings, other_airlines, other_counts) = check(threads);
            assert_eq!(
                format!("{findings:?}"),
                format!("{reference:?}"),
                "{name}: {threads} threads"
            );
            assert_eq!(other_airlines, airlines, "{name}: {threads} threads");
            assert_eq!(other_counts, counts, "{name}: {threads} threads");
        }
    }
}

/// P03.2: the optimizer's candidates read only the via count and the
/// weighted trace length, now computed alone; they equal those of the full
/// cheap statistics bit for bit, in every unit.
#[test]
fn lean_optimizer_figures_equal_the_cheap_statistics() {
    use freerouting::board::structure::Unit;
    for name in BOARDS {
        for board in [loaded(name), routed(name)] {
            for unit in [Unit::Mm, Unit::Inch, Unit::Um, board.communication.unit] {
                let stats = BoardStatistics::with_options(&board, unit, false, false);
                let (vias, length) = BoardStatistics::via_count_and_weighted_length(&board, unit);
                assert_eq!(vias, stats.items.via_count, "{name} {unit:?}");
                assert_eq!(
                    length.to_bits(),
                    stats.traces.total_weighted_length.to_bits(),
                    "{name} {unit:?}"
                );
            }
        }
    }
}

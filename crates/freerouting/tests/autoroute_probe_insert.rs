//! The `insert` probe (A06.7): connection parity, rung 2. One connection
//! routed on the loaded board gives the same items as Java, line for line
//! against `tools/java-reference/probes/autoroute/AutorouteProbe.java`.
//!
//! Per sampled connection, on a fresh load of the board: the control as the
//! batch router builds it in pass 1 (ripup on, the start ripup costs), then
//! [`route_connection`] with the router's maze-search work limit (100,000
//! nominal ms at ripup pass 1, `AutorouteConnectionRouter.java:83-89`), as
//! Java's `AutorouteEngine.autorouteConnection` on a new engine. Printed:
//! the state and details, the ripped ids and costs, every item changed
//! (same id, other geometry), added and removed in id order,
//! `maxGeneratedId`, the `BoardStatistics` deltas (incomplete connections,
//! vias, the total trace length before and after) and the work meter units
//! the call used.
//!
//! - `insert_matches_java`: the fixture ladder, one fresh load per
//!   connection;
//! - `insert_routed_matches_java`: `insert-routed`, the walk of
//!   `maze-routed` through pre-routed traces (traces and vias unfixed, the
//!   nets cut one by one and routed on the same board), so the rip of
//!   `ripConnections` is compared too.

mod autoroute_common;
#[path = "support/insert_lines.rs"]
mod insert_lines;

use autoroute_common::{board_name, compare_probe, kind_fixtures};
use insert_lines::{insert_lines, routed_lines};

/// Boards of the ladder whose `insert` output differs from Java's, with the
/// reason. Empty: every board is required.
const EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

/// Boards of the `insert-routed` row whose output differs from Java's, with
/// the reason. Empty: every board is required.
const ROUTED_EXPECTED_MISMATCHES: &[(&str, &str)] = &[];

fn compare_kind(kind: &str, expected: &[(&str, &str)], lines: impl Fn(&str) -> Vec<String>) {
    let mut failures = Vec::new();
    let mut boards = 0;
    // `INSERT_PROBE_BOARD=<board name>` runs one board (for debugging).
    let only = std::env::var("INSERT_PROBE_BOARD").ok();
    for dsn in kind_fixtures(kind) {
        let board = board_name(&dsn);
        if only.as_ref().is_some_and(|b| *b != board) {
            continue;
        }
        let result = compare_probe(kind, &dsn, &lines(&dsn));
        boards += 1;
        let listed = expected.iter().any(|(b, _)| *b == board);
        match result {
            Err(report) if !listed => failures.push(report),
            Ok(()) if listed => failures.push(format!(
                "{board}: listed as an expected mismatch but matches Java; remove it"
            )),
            _ => {}
        }
    }
    assert!(boards > 0 || only.is_some(), "no {kind} fixture");
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn insert_matches_java() {
    compare_kind("insert", EXPECTED_MISMATCHES, insert_lines);
}

#[test]
fn insert_routed_matches_java() {
    compare_kind("insert-routed", ROUTED_EXPECTED_MISMATCHES, routed_lines);
}

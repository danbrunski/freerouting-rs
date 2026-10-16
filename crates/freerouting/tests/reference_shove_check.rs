//! The shove checks against Java (O03.4, O03.6-O03.8).
//!
//! Probe `shove-check` (`tools/java-reference/probes/board/ShoveCheckProbe.java`), stages in
//! the probe's order on one board:
//! - `trace-check`: on the shove sampler (`tests/support/shove_sampler.rs`), the instance
//!   `TraceShover.check` with depths (20, 8, 5) and (0, 0, 0), the static OK length for both
//!   shove sides (via depth 8 and 0) and `checkForcedTracePolyline` with both depth sets;
//! - `via-grid`: the board's first via on a 20 x 20 grid, `ForcedViaInserter.check` and
//!   `checkLayer` per padstack layer in seven kinds of rooms;
//! - `via-pins`: the same at the centres of the first 100 pins with a net, with the pin's nets;
//! - `limit`: 50 deep checks under a work limit that fires a quarter, half or three quarters
//!   through (Java's `WorkLimit`, search-tree nodes visited);
//! - `trace-check-90`, `via-grid-90`, `via-pins-90`: the first three again in forced 90 degree
//!   mode.
//!
//! Each query is followed by the board's shove-failing obstacle and layer and
//! `maxGeneratedId`. Every line is compared; OK lengths by raw bits. A mismatch is a bug to fix,
//! never an expected mismatch, because the maze search depends on each answer.

mod board_probe;
#[path = "support/shove_check_stages.rs"]
mod shove_check_stages;
#[path = "support/shove_sampler.rs"]
mod shove_sampler;

use board_probe::header;
use shove_check_stages::shove_check_lines;

const KIND: &str = "shove-check";

/// Runs every stage on `board_name` (with its fixtures.tsv session if
/// `with_session`) and compares with the probe's output.
fn check_board(board_name: &str, with_session: bool) {
    let row = board_probe::fixture(board_name);
    let mut board = board_probe::load(&row);
    if with_session {
        board_probe::import_session(&mut board, &row);
    }
    let mut lines = vec![header(KIND, &row.board)];
    shove_check_lines(&mut board, &mut lines);
    if let Err(e) = board_probe::compare(KIND, &row, &lines) {
        panic!("{e}");
    }
}

#[test]
fn shove_checks_match_java_issue413() {
    check_board("Issue413-test", false);
}

#[test]
fn shove_checks_match_java_issue676() {
    check_board("Issue676-ch32v-tx118s", false);
}

#[test]
fn shove_checks_match_java_issue957() {
    check_board("Issue957-smoke-ldo-led", false);
}

#[test]
fn shove_checks_match_java_j2() {
    check_board("Issue026-J2_reference", true);
}

#[test]
fn shove_checks_match_java_bm08() {
    check_board("Issue508-DAC2020_bm08", true);
}

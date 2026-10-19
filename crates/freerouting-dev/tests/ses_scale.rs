//! A measurement reads a session at the scale the writer used, whatever the
//! route's `router.java_quirks.ses_reader_scale` was.
//!
//! `Issue676-ch32v-tx118s` has a DSN-to-board factor of 0.1. Java's reader and
//! writer both divide by it, so the default reader puts the router's wiring
//! 100 times off the board: `freerouting-dev` once read 12 nets unrouted of a
//! route whose own result says 0. The session here is Java's route at the
//! pin (`fixtures/reference/route/`), whose result JSON holds the router's
//! own counts.

use std::path::{Path, PathBuf};

use freerouting::board::RoutingBoard;
use freerouting::io::specctra::import_session;
use freerouting::settings::JavaQuirks;
use freerouting_dev::bench::measure;
use freerouting_dev::json_read::Json;
use freerouting_dev::ses_diff::{SessionStatistics, import_routed_session, parse_session};
use freerouting_dev::{load_board, read_text};

const BOARD: &str = "Issue676-ch32v-tx118s";

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures")
}

fn router_count(json: &Json, field: &str) -> usize {
    json.path(&format!("statistics.{field}"))
        .and_then(Json::as_f64)
        .unwrap_or_else(|| panic!("{field} missing")) as usize
}

fn board_with_quirks(java_scale: bool) -> RoutingBoard {
    let mut board = load_board(&fixtures().join(format!("{BOARD}.dsn"))).unwrap();
    board.set_java_quirks(
        JavaQuirks::JAVA
            .with("ses_reader_scale", java_scale)
            .unwrap(),
    );
    board
}

#[test]
fn issue676_measures_what_the_router_reports() {
    let ses = fixtures().join(format!("reference/route/{BOARD}.ses"));
    let json =
        Json::parse(&read_text(&fixtures().join(format!("reference/route/{BOARD}.json"))).unwrap())
            .unwrap();
    let want_unrouted = router_count(&json, "connections.incomplete_count");
    let want_vias = router_count(&json, "vias.total_count");
    let want_clearance = router_count(&json, "clearance_violations.total_count");
    assert_eq!(want_unrouted, 0, "the reference route is complete");

    // Whatever the board's switches were, the measurement agrees with the router.
    for java_scale in [true, false] {
        let board = board_with_quirks(java_scale);
        let measured = measure(&board, &ses).unwrap();
        assert_eq!(
            measured.incomplete_connections, want_unrouted,
            "unrouted (board switch ses_reader_scale = {java_scale})"
        );
        assert_eq!(measured.vias, want_vias, "vias (switch {java_scale})");
        assert_eq!(
            measured.clearance_violations, want_clearance,
            "clearance violations (switch {java_scale})"
        );
        // The importer restores the board's own switches.
        let mut copy = board.clone();
        import_routed_session(
            &mut copy,
            &parse_session(&read_text(&ses).unwrap()).unwrap(),
        );
        assert_eq!(copy.java_quirks().ses_reader_scale, java_scale);
    }
}

/// The test above would pass if Java's scale did no harm; it does: the same
/// session imported with the Java switch misplaces the wiring.
#[test]
fn java_scale_misplaces_issue676() {
    let ses = fixtures().join(format!("reference/route/{BOARD}.ses"));
    let session = parse_session(&read_text(&ses).unwrap()).unwrap();
    let mut board = board_with_quirks(true);
    board.delete_all_traces_and_vias();
    import_session(&mut board, &session);
    let java_read = SessionStatistics::of(&board);
    assert!(
        java_read.incomplete_connections > 0,
        "Java's scale leaves nets unrouted: {java_read:?}"
    );
}

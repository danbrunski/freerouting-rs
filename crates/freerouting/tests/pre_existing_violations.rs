//! The clearance violations a board has before routing are its
//! pre-existing ones (Java `BasicBoard.preExistingClearanceViolationsCount`,
//! counted by `HeadlessBoardManager`'s post-load DRC), so the statistics
//! count only the violations routing adds as `router_introduced`.
//!
//! The board: two fixed wires of nets A and B on F.Cu 100 um apart with a
//! 200 um clearance (one violation), and a fixed via of net A inside a
//! structure-level `wire_keepout` (one violation under Java's reading of
//! `wire_keepout` as a full keepout, none under `router.wire_keepout =
//! traces_only`). The count is taken after every setting that changes the
//! board ([`prepare_job`], step 8), so the via counts only under the
//! default.

use freerouting::board::BasicBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::drc::DesignRulesChecker;
use freerouting::job::prepare::{
    JobInput, RulesChoice, prepare_board_for_routing, prepare_job, record_pre_existing_violations,
};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::{ResolveContext, SettingsPatch, Source};
use freerouting_geometry::{IntPoint, Point, Polyline};

const DSN: &str = r#"
(pcb pre-existing.dsn (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10) (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  0 0  20000 0  20000 -10000  0 -10000  0 0))
    (wire_keepout "" (rect F.Cu 14000 -6000 18000 -2000))
    (via VIA)
    (rule (width 200) (clearance 200)))
  (placement
    (component TP
      (place P1 2000 -8000 front 0)
      (place P2 18000 -8000 front 0)))
  (library
    (image TP
      (pin THT 1 0 0))
    (padstack THT (shape (circle F.Cu 1000)) (shape (circle B.Cu 1000)) (attach off))
    (padstack VIA (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off)))
  (network
    (net A (pins P1-1))
    (net B (pins P2-1)))
  (wiring
    (wire (path F.Cu 200  3000 -5000  9000 -5000) (net A) (type fix))
    (wire (path F.Cu 200  3000 -5300  9000 -5300) (net B) (type fix))
    (via VIA 16000 -4000 (net A) (type fix))))
"#;

fn ctx() -> ResolveContext {
    ResolveContext { cores: 1 }
}

fn input(settings: &[(&str, &str)]) -> JobInput {
    let mut input = JobInput::new(DSN.as_bytes().to_vec(), "pre-existing");
    input.rules = RulesChoice::None;
    if !settings.is_empty() {
        let mut patch = SettingsPatch::default();
        for (key, value) in settings {
            patch
                .set_path(key, value)
                .unwrap_or_else(|e| panic!("{key}: {e:?}"));
        }
        input.extra_sources.push((Source::Testing, patch));
    }
    input
}

fn violations(board: &BasicBoard) -> usize {
    DesignRulesChecker::new(board)
        .all_clearance_violations()
        .len()
}

/// Inserts an unfixed trace of the net of P2 (net B) across the A wire.
fn add_crossing_trace(board: &mut BasicBoard) -> ItemId {
    let net = board.rules.nets.get_by_name("B", 1).unwrap().net_number;
    let class = board.rules.net_class_of(net).unwrap();
    let (half_width, clearance_class) = (class.trace_half_width(0), class.trace_clearance_class());
    let point = |x: i32, y: i32| Point::Int(IntPoint::new(10 * x, 10 * y));
    board
        .insert_trace_without_cleaning(
            Polyline::from_corners(&point(6000, -7000), &point(6000, -3000)),
            0,
            half_width,
            &[net],
            clearance_class,
            FixedState::Unfixed,
        )
        .unwrap()
}

#[test]
fn prepare_job_records_the_loaded_violations_as_pre_existing() {
    let prepared = prepare_job(&input(&[]), &ctx()).unwrap();
    let board = &prepared.board;
    let total = violations(board);
    // The wire pair and the via in the wire keepout, on F.Cu.
    assert!(total >= 2, "{total}");
    assert_eq!(board.pre_existing_clearance_violations_count, total);
    let stats = BoardStatistics::new(board).clearance_violations;
    assert_eq!(stats.total_count, total);
    assert_eq!(stats.pre_existing_count, total);
    assert_eq!(stats.router_introduced_count, 0);
}

#[test]
fn violations_added_after_preparation_count_as_router_introduced() {
    let mut prepared = prepare_job(&input(&[]), &ctx()).unwrap();
    let before = prepared.board.pre_existing_clearance_violations_count;
    add_crossing_trace(&mut prepared.board);
    let after = violations(&prepared.board);
    assert!(after > before, "{after} > {before}");
    let stats = BoardStatistics::new(&prepared.board).clearance_violations;
    assert_eq!(stats.pre_existing_count, before);
    assert_eq!(stats.router_introduced_count, after - before);
}

#[test]
fn the_count_follows_the_wire_keepout_setting() {
    let full = prepare_job(&input(&[]), &ctx()).unwrap();
    let traces_only = prepare_job(&input(&[("wire_keepout", "traces_only")]), &ctx()).unwrap();
    let (full_count, traces_only_count) = (
        full.board.pre_existing_clearance_violations_count,
        traces_only.board.pre_existing_clearance_violations_count,
    );
    // The via in the keepout violates only while the keepout blocks vias.
    assert!(
        traces_only_count < full_count,
        "{traces_only_count} < {full_count}"
    );
    assert_eq!(traces_only_count, violations(&traces_only.board));
}

#[test]
fn the_gui_preparation_records_them_too() {
    let mut board = prepare_job(&input(&[]), &ctx()).unwrap().board;
    board.pre_existing_clearance_violations_count = 0;
    let _ready = prepare_board_for_routing(&mut board, &SettingsPatch::default(), &ctx());
    assert_eq!(
        board.pre_existing_clearance_violations_count,
        violations(&board)
    );
    board.pre_existing_clearance_violations_count = 0;
    record_pre_existing_violations(&mut board);
    assert_eq!(
        board.pre_existing_clearance_violations_count,
        violations(&board)
    );
}

#[test]
fn a_clean_board_has_none() {
    let clean = DSN
        .replace("(net B) (type fix)", "(net A) (type fix)")
        .replace("(via VIA 16000 -4000 (net A) (type fix))", "");
    let mut input = JobInput::new(clean.into_bytes(), "clean");
    input.rules = RulesChoice::None;
    let prepared = prepare_job(&input, &ctx()).unwrap();
    assert_eq!(violations(&prepared.board), 0);
    assert_eq!(prepared.board.pre_existing_clearance_violations_count, 0);
}

//! Q22.11: the board's minimum hole-to-hole distance (KiCad Board Setup
//! `m_HoleToHoleMin`, `.kicad_pro` `min_hole_to_hole`) between any two
//! drills, of any net (`router.board_minimums.hole_to_hole_um`,
//! `job::hole_to_hole`, `board::hole_spacing`).
//!
//! Fixture `tests/data/quality/q22-same-net-vias.dsn`: net A has an F.Cu
//! and a B.Cu pad on each side of a B.Cu keepout wall (x 0.99-1.01 mm), so
//! it needs a via on each side; the copper clearance keeps nothing apart,
//! both vias are net A's.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::drc::board_rules::{BoardRule, BoardRuleLimits, InputCopper, check_board_rules};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const VIAS: &str = "crates/freerouting/tests/data/quality/q22-same-net-vias.dsn";

fn prepare(hole_to_hole_um: Option<f64>, profile: &str) -> PreparedJob {
    let path = repo_root().join(VIAS);
    let bytes = std::fs::read(&path).unwrap();
    let mut input = JobInput::new(bytes, JobInput::design_name_of(VIAS));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    let mut patch = SettingsPatch::default();
    patch.board_minimums.hole_to_hole_um = hole_to_hole_um;
    patch.profile = Some(profile.to_string());
    patch.autorouter.max_passes = Some(3);
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

fn route(prepared: PreparedJob) -> RoutingBoard {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    outcome.board.take().expect("no listener took the board")
}

/// The via centres, in board units (0.1 µm).
fn vias(board: &RoutingBoard) -> Vec<(f64, f64)> {
    board
        .items()
        .filter(|i| i.is_via())
        .map(|i| {
            let c = i.drill().unwrap().center;
            (f64::from(c.x), f64::from(c.y))
        })
        .collect()
}

/// The smallest gap between two via holes (0.3 mm drills), edge to edge.
fn smallest_hole_gap(board: &RoutingBoard) -> f64 {
    let v = vias(board);
    let mut gap = f64::INFINITY;
    for (i, a) in v.iter().enumerate() {
        for b in &v[i + 1..] {
            gap = gap.min((a.0 - b.0).hypot(a.1 - b.1) - 3000.0);
        }
    }
    gap
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

fn log_text(prepared: &PreparedJob) -> Vec<String> {
    prepared.log.iter().map(|l| l.text.clone()).collect()
}

#[test]
fn without_a_minimum_the_vias_are_javas() {
    let plain = prepare(None, "legacy");
    assert_eq!(plain.board.rules.hole_to_hole(), 0);
    assert!(
        !log_text(&plain).iter().any(|l| l.contains("hole to hole")),
        "{:?}",
        log_text(&plain)
    );
    let board = route(plain);
    assert_eq!(incomplete(&board), 0);
    // one via either side of the wall, 1.0 mm apart hole edge to hole edge
    assert_eq!(
        vias(&board),
        vec![(93_500.0, 98_287.0), (106_500.0, 98_287.0)]
    );
    assert_eq!(smallest_hole_gap(&board), 10_000.0);
}

#[test]
fn vias_of_one_net_keep_the_board_hole_to_hole_distance() {
    for profile in ["legacy", "enhanced"] {
        let prepared = prepare(Some(1500.0), profile);
        assert_eq!(prepared.board.rules.hole_to_hole(), 15_000, "{profile}");
        assert!(
            log_text(&prepared)
                .iter()
                .any(|l| l.contains("Board minimum hole to hole 1500.0 um (15000 board units)")),
            "{profile}: {:?}",
            log_text(&prepared)
        );
        let board = route(prepared);
        assert_eq!(incomplete(&board), 0, "{profile}");
        assert_eq!(vias(&board).len(), 2, "{profile}");
        let gap = smallest_hole_gap(&board);
        assert!(gap >= 15_000.0, "{profile}: gap {gap}");
    }
}

#[test]
fn the_output_check_reports_holes_closer_than_the_minimum() {
    let mut board = route(prepare(None, "legacy"));
    let prepared = prepare(None, "legacy");
    let limits = BoardRuleLimits::of(&board, &prepared.board_settings);
    let hole_to_hole = |board: &RoutingBoard| {
        check_board_rules(board, &limits, &InputCopper::none())
            .into_iter()
            .filter(|v| v.rule == BoardRule::HoleToHole)
            .collect::<Vec<_>>()
    };
    assert!(hole_to_hole(&board).is_empty());
    // the two routed vias, 1.0 mm apart, against a 1.5 mm minimum
    board.rules.set_hole_to_hole(15_000);
    let found = hole_to_hole(&board);
    assert_eq!(found.len(), 1, "{found:?}");
    let v = &found[0];
    assert_eq!(BoardRule::HoleToHole.name(), "hole_to_hole");
    assert_eq!(v.items.len(), 2);
    assert!(
        v.items
            .iter()
            .all(|id| board.item(*id).is_some_and(|i| i.is_via()))
    );
    assert_eq!(v.routed_items.len(), 2);
    assert_eq!(v.required, 15_000.0);
    assert!((v.actual - 10_000.0).abs() < 1e-6, "{v:?}");
    // at the minimum, no breach
    board.rules.set_hole_to_hole(10_000);
    assert!(hole_to_hole(&board).is_empty());
}

#[test]
fn a_kicad_pcb_input_passes_its_hole_to_hole_minimum() {
    use freerouting::io::kicad::reader::project::{BoardMinimums, Project};
    use freerouting::job::prepare::board_minimums_patch;
    let rules = |r: &str| {
        Project::parse(&format!(
            r#"{{"board": {{"design_settings": {{"rules": {{{r}}}}}}}}}"#
        ))
        .unwrap()
        .minimums
    };
    // KiCad's default (`DEFAULT_HOLETOHOLEMIN`) is 0.25 mm
    assert_eq!(rules("").hole_to_hole, Some(250_000));
    assert_eq!(BoardMinimums::default().hole_to_hole, Some(250_000));
    assert_eq!(Project::default().minimums.hole_to_hole, Some(250_000));
    assert_eq!(rules(r#""min_hole_to_hole": 0.0"#).hole_to_hole, None);
    let m = rules(r#""min_hole_to_hole": 0.3"#);
    assert_eq!(m.hole_to_hole, Some(300_000));
    let patch = |m: &BoardMinimums| {
        board_minimums_patch(m)
            .unwrap()
            .board_minimums
            .hole_to_hole_um
    };
    assert_eq!(patch(&m), Some(300.0));
    assert_eq!(patch(&BoardMinimums::default()), Some(250.0));
}

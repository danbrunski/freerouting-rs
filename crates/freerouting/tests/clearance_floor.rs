//! Q22.10: the board's minimum clearance (KiCad Board Setup `m_MinClearance`,
//! `.kicad_pro` `min_clearance`) as a floor under every copper clearance
//! (`router.board_minimums.clearance_um`, `job::clearance_floor`).
//!
//! Fixture `tests/data/quality/q18-mask-bridge.dsn` (Q18.7's): net SIG runs
//! straight from P1 to P2 on F.Cu, past the 0.8 mm test point TP1 of net CLK.
//! The DSN's clearance is 0.2 mm, so Java routes 0.2 mm from TP1's copper.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const BRIDGE: &str = "crates/freerouting/tests/data/quality/q18-mask-bridge.dsn";
/// TP1's radius in board units (0.1 µm).
const TP_RADIUS: f64 = 4_000.0;

fn prepare(clearance_um: Option<f64>, profile: &str) -> PreparedJob {
    let path = repo_root().join(BRIDGE);
    let bytes = std::fs::read(&path).unwrap();
    let mut input = JobInput::new(bytes, JobInput::design_name_of(BRIDGE));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    let mut patch = SettingsPatch::default();
    patch.board_minimums.clearance_um = clearance_um;
    patch.profile = Some(profile.to_string());
    patch.optimizer.enabled = Some(false);
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

/// Every clearance between two non-null classes on every layer, but the
/// classes of the copper-to-edge and copper-to-hole rules (`board_edge`,
/// `hole_edge`), which keep their own values.
fn matrix_values(board: &RoutingBoard) -> Vec<(usize, usize, usize, i32)> {
    let m = &board.rules.clearance_matrix;
    let classes: Vec<usize> = (1..m.class_count())
        .filter(|&c| !matches!(m.get_name(c as i32), Some("board_edge" | "hole_edge")))
        .collect();
    let mut out = Vec::new();
    for &i in &classes {
        for &j in &classes {
            for layer in 0..m.layer_count() {
                out.push((
                    i,
                    j,
                    layer,
                    m.get_value(i as i32, j as i32, layer as i32, false),
                ));
            }
        }
    }
    out
}

fn segment_distance(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len2 = dx * dx + dy * dy;
    let t = if len2 == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len2).clamp(0.0, 1.0)
    };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

/// The smallest copper gap between net SIG's F.Cu traces and TP1, in board
/// units.
fn gap_to_tp(board: &RoutingBoard) -> f64 {
    let sig = board.rules.nets.get_by_name("SIG", 1).unwrap().net_number;
    let clk = board.rules.nets.get_by_name("CLK", 1).unwrap().net_number;
    let tp = board
        .items()
        .find(|i| i.is_pin() && i.contains_net(clk))
        .and_then(|i| i.drill())
        .map(|d| (f64::from(d.center.x), f64::from(d.center.y)))
        .expect("TP1");
    let mut gap = f64::INFINITY;
    for item in board.items().filter(|i| i.contains_net(sig)) {
        let Some(t) = item.as_trace() else { continue };
        if t.layer != 0 {
            continue;
        }
        let pts: Vec<(f64, f64)> = t
            .polyline
            .corner_approx_arr()
            .iter()
            .map(|p| (p.x, p.y))
            .collect();
        for w in pts.windows(2) {
            gap = gap.min(segment_distance(tp, w[0], w[1]) - TP_RADIUS - f64::from(t.half_width));
        }
    }
    gap
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

#[test]
fn without_a_minimum_clearance_the_matrix_is_the_dsns() {
    let plain = prepare(None, "legacy");
    // 0.2 mm in 0.1 µm units between every pair of non-null classes
    assert!(
        matrix_values(&plain.board)
            .iter()
            .all(|&(.., v)| v == 2000 || v == 500),
        "{:?}",
        matrix_values(&plain.board)
    );
    let log: Vec<String> = plain.log.iter().map(|l| l.text.clone()).collect();
    assert!(
        !log.iter().any(|l| l.contains("minimum clearance")),
        "{log:?}"
    );
    // a minimum at or under every clearance changes nothing either
    let under = prepare(Some(50.0), "legacy");
    assert_eq!(matrix_values(&plain.board), matrix_values(&under.board));
}

#[test]
fn the_board_minimum_clearance_raises_every_clearance_below_it() {
    let plain = prepare(None, "legacy");
    let floored = prepare(Some(350.0), "legacy");
    let before = matrix_values(&plain.board);
    let after = matrix_values(&floored.board);
    assert_eq!(before.len(), after.len());
    for (b, a) in before.iter().zip(&after) {
        assert_eq!(a.3, b.3.max(3500), "{b:?} -> {a:?}");
    }
    // the copper-to-edge class keeps the edge clearance (250 um by default)
    let m = &floored.board.rules.clearance_matrix;
    let edge = m.get_no("board_edge");
    assert!(edge > 0);
    assert_eq!(m.get_value(1, edge, 0, false), 2500);
    let log: Vec<String> = floored.log.iter().map(|l| l.text.clone()).collect();
    assert!(
        log.iter()
            .any(|l| l.contains("Board minimum clearance 350.0 um")),
        "{log:?}"
    );
}

#[test]
fn routed_copper_keeps_the_board_minimum_clearance() {
    let plain = route(prepare(None, "legacy"));
    assert_eq!(incomplete(&plain), 0);
    let gap = gap_to_tp(&plain);
    assert!((1999.0..3000.0).contains(&gap), "gap {gap}");

    for profile in ["legacy", "enhanced"] {
        let floored = route(prepare(Some(400.0), profile));
        assert_eq!(incomplete(&floored), 0, "{profile}");
        let gap = gap_to_tp(&floored);
        assert!(gap >= 3999.0, "{profile}: gap {gap}");
    }
}

#[test]
fn a_kicad_pcb_input_passes_its_minimum_clearance() {
    use freerouting::io::kicad::reader::project::{BoardMinimums, Project};
    use freerouting::job::prepare::board_minimums_patch;
    let rules = |r: &str| {
        Project::parse(&format!(
            r#"{{"board": {{"design_settings": {{"rules": {{{r}}}}}}}}}"#
        ))
        .unwrap()
        .minimums
    };
    // KiCad's default (`DEFAULT_MINCLEARANCE`) is 0: no minimum
    assert_eq!(rules("").clearance, None);
    assert_eq!(BoardMinimums::default().clearance, None);
    assert_eq!(rules(r#""min_clearance": 0.0"#).clearance, None);
    let m = rules(r#""min_clearance": 0.127"#);
    assert_eq!(m.clearance, Some(127_000));
    let patch = board_minimums_patch(&m).unwrap();
    assert_eq!(patch.board_minimums.clearance_um, Some(127.0));
    assert_eq!(
        board_minimums_patch(&BoardMinimums::default())
            .unwrap()
            .board_minimums
            .clearance_um,
        None
    );
}

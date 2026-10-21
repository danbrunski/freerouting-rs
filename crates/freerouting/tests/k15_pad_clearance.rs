//! K15.9: a pad's own clearance is kept on a `.kicad_pcb` input.
//!
//! Fixture `crates/freerouting-kicad/tests/data/k15-fiducial.kicad_pcb`: FID1 of
//! the power-control unit, verbatim (one F.Cu pad, 1 mm round, `(clearance
//! 0.6)`), at (52, 33) mm, with two GND pads at (46, 33) and (58, 33) mm
//! across it. KiCad 10.0.7 gives the pad a local clearance of 0.6 mm. The
//! straight connection runs through the fiducial.

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::io::kicad::{KicadInput, ReadOptions, read_kicad_input};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_kicad::pad_clearance::KEEPOUT_NAME;

const PCB: &str = "crates/freerouting-kicad/tests/data/k15-fiducial.kicad_pcb";
/// The fiducial's pad: centre and radius, board units (0.1 µm, Y up).
const FID: (f64, f64, f64) = (520_000.0, -330_000.0, 5_000.0);
/// Its clearance, board units.
const CLEARANCE: f64 = 6_000.0;

fn read(keep_circle: bool) -> KicadInput {
    let mut read = read_kicad_input(&repo_root().join(PCB), ReadOptions::default()).unwrap();
    if !keep_circle {
        let s = read.pcb.structure.as_mut().unwrap();
        s.keepouts
            .retain(|k| k.area.name.as_deref() != Some(KEEPOUT_NAME));
    }
    read
}

fn route(keep_circle: bool, profile: &str) -> RoutingBoard {
    let mut input = JobInput::new(Vec::new(), "k15-fiducial");
    input.set_kicad(read(keep_circle));
    let mut patch = SettingsPatch {
        profile: Some(profile.to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    input.extra_sources = vec![(Source::Testing, patch)];
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
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

/// The smallest gap between GND's traces on F.Cu and the fiducial pad's
/// edge, board units; infinite without traces there.
fn gap(board: &RoutingBoard) -> f64 {
    let gnd = board.rules.nets.get_by_name("GND", 1).unwrap().net_number;
    let mut gap = f64::INFINITY;
    for item in board.items().filter(|i| i.contains_net(gnd)) {
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
            for k in 0..=400 {
                let s = f64::from(k) / 400.0;
                let p = (
                    w[0].0 + s * (w[1].0 - w[0].0),
                    w[0].1 + s * (w[1].1 - w[0].1),
                );
                let d = (p.0 - FID.0).hypot(p.1 - FID.1) - FID.2 - f64::from(t.half_width);
                gap = gap.min(d);
            }
        }
    }
    gap
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

/// The reader writes the keepout: a circle of the pad's size plus twice the
/// clearance above the default 0.2 mm (1 + 2 * 0.4 = 1.8 mm) on F.Cu.
#[test]
fn the_fiducials_keepout_is_there() {
    let read = read(true);
    let circles: Vec<_> = read
        .pcb
        .structure
        .as_ref()
        .unwrap()
        .keepouts
        .iter()
        .filter(|k| k.area.name.as_deref() == Some(KEEPOUT_NAME))
        .collect();
    assert_eq!(circles.len(), 1, "{circles:?}");
    match &circles[0].area.shape {
        freerouting::dsn::ast::Shape::Circle {
            diameter, center, ..
        } => {
            assert_eq!(*diameter, 1800.0);
            assert_eq!((center.x, center.y), (52_000.0, -33_000.0));
        }
        other => panic!("{other:?}"),
    }
}

/// Without the circle the route passes the fiducial at the default 0.2 mm;
/// with it, 0.6 mm from the pad on both profiles.
#[test]
fn a_route_keeps_the_pads_own_clearance() {
    for profile in ["legacy", "enhanced"] {
        let without = route(false, profile);
        assert_eq!(incomplete(&without), 0, "{profile}");
        let near = gap(&without);
        assert!(
            near < CLEARANCE - 500.0,
            "{profile}: without the keepout the gap is {near}"
        );
        let board = route(true, profile);
        assert_eq!(incomplete(&board), 0, "{profile}");
        let kept = gap(&board);
        println!("{profile}: gap without the keepout {near}, with it {kept} (board units)");
        // 1 unit of rounding
        assert!(kept >= CLEARANCE - 1.0, "{profile}: gap {kept}");
    }
}

//! Q22.4: copper shapes and copper text are obstacles the router keeps its
//! clearance from (`freerouting_kicad::copper_graphics`).
//!
//! Fixture `tests/data/kicad/q22-copper-graphics.kicad_pcb`: J1's two pads
//! numbered 2 (net N2) at (12, 10) and (18, 10) mm, a 0.5 mm B.Cu `fp_line`
//! of no net between them, and a `gr_text "GND"` on F.Cu over the same gap.
//! The straight connection on either layer crosses one of them; KiCad's DSN
//! export carries neither.

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::io::kicad::{ReadOptions, read_kicad_input};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_kicad::copper_graphics::{KEEPOUT_NAME, TEXT_KEEPOUT_NAME};

const PCB: &str = "crates/freerouting/tests/data/kicad/q22-copper-graphics.kicad_pcb";
/// The B.Cu line's copper and the reader's text box, board units (0.1 µm,
/// Y up): x0, y0, x1, y1.
const LINE: [f64; 4] = [129_500.0, -102_500.0, 170_500.0, -97_500.0];
const TEXT: [f64; 4] = [120_375.0, -115_000.0, 179_625.0, -85_000.0];

fn route(keep_obstacles: bool, profile: &str) -> RoutingBoard {
    let mut read = read_kicad_input(&repo_root().join(PCB), ReadOptions::default()).unwrap();
    if !keep_obstacles {
        let s = read.pcb.structure.as_mut().unwrap();
        s.keepouts.retain(|k| {
            !matches!(
                k.area.name.as_deref(),
                Some(KEEPOUT_NAME | TEXT_KEEPOUT_NAME)
            )
        });
    }
    let mut input = JobInput::new(Vec::new(), "q22-copper-graphics");
    input.set_kicad(read);
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

/// The smallest gap between N2's traces on `layer` and the box, board units
/// (negative: they overlap).
fn gap(board: &RoutingBoard, layer: i32, b: [f64; 4]) -> f64 {
    let n2 = board.rules.nets.get_by_name("N2", 1).unwrap().net_number;
    let mut gap = f64::INFINITY;
    for item in board.items().filter(|i| i.contains_net(n2)) {
        let Some(t) = item.as_trace() else { continue };
        if t.layer != layer {
            continue;
        }
        let pts: Vec<(f64, f64)> = t
            .polyline
            .corner_approx_arr()
            .iter()
            .map(|p| (p.x, p.y))
            .collect();
        for w in pts.windows(2) {
            // sample the segment; the distance from a point to the box
            for k in 0..=200 {
                let s = f64::from(k) / 200.0;
                let p = (
                    w[0].0 + s * (w[1].0 - w[0].0),
                    w[0].1 + s * (w[1].1 - w[0].1),
                );
                let dx = (b[0] - p.0).max(p.0 - b[2]).max(0.0);
                let dy = (b[1] - p.1).max(p.1 - b[3]).max(0.0);
                let inside = dx == 0.0 && dy == 0.0;
                let d = if inside { -1.0 } else { dx.hypot(dy) };
                gap = gap.min(d - f64::from(t.half_width));
            }
        }
    }
    gap
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

#[test]
fn without_the_obstacles_the_route_crosses_the_copper_graphics() {
    let board = route(false, "legacy");
    assert_eq!(incomplete(&board), 0);
    let crosses = gap(&board, 0, TEXT) < 0.0 || gap(&board, 1, LINE) < 0.0;
    assert!(
        crosses,
        "F.Cu {} B.Cu {}",
        gap(&board, 0, TEXT),
        gap(&board, 1, LINE)
    );
}

#[test]
fn the_route_goes_round_the_copper_line_and_the_copper_text() {
    for profile in ["legacy", "enhanced"] {
        let board = route(true, profile);
        // routed round them, at the 0.2 mm clearance (1 unit of rounding)
        assert_eq!(incomplete(&board), 0, "{profile}");
        let (front, back) = (gap(&board, 0, TEXT), gap(&board, 1, LINE));
        assert!(front >= 1999.0, "{profile}: F.Cu gap {front}");
        assert!(back >= 1999.0, "{profile}: B.Cu gap {back}");
    }
}

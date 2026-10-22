//! K15.19: no routed track crosses the board edge or an NPTH's hole clearance.
//!
//! Both fixtures are cut from wisweep's driver board (`slimcdk/wisweep`
//! `f22ec65c9b`, `electronics/driver-board.kicad_pcb`, MIT; corpus row
//! `wisweep-driver-board`), with the board's own `.kicad_pro`
//! (`min_copper_edge_clearance` 0, `min_hole_clearance` 0.25 mm, Default class
//! 0.2 mm clearance and 0.25 mm track):
//!
//! - `wisweep-j1-edge`: the outline, J1, JP3, U5, C24, R6, R8 and J2 verbatim,
//!   no tracks or zones. J1's pads (4 x 1.4 mm, centres at x = 65.7956) lie
//!   across the outline's notch line x = 64.5, their centres outside the
//!   board. K15.14's run (`pcb-off`) laid a /TMC_AIN track from J1-2 across
//!   that line; Java lets a trace of a net with a pin on or outside the outline
//!   cross the outline anywhere (`BoardOutline.blocksNets`), and the cut
//!   repeats it under `legacy`: /TMC_AIN and GND cross the line.
//! - `wisweep-j10-slot`: the outline, J10's oval NPTH (a 0.95 x 0.65 mm slot,
//!   no copper, at (45.4516, 63.6636)) and two test points on net SIG at
//!   x = 44.6316, where the straight track's edge is 0.22 mm from the slot,
//!   over the 0.2 mm copper clearance and under the 0.25 mm hole clearance (as
//!   K15.14's `pcb-on` Net-(U5-VCP) track, 0.2247 mm). KiCad's DSN export
//!   makes the slot a pin whose pad is the slot, and the padstack's drill
//!   radius was the estimate 0.45 x 0.325 mm, so the hole clearance added
//!   nothing.

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemKind;
use freerouting::drc::{DrcReport, DrcRunOptions, run_drc};
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{FloatPoint, PolylineShapeOps};

const DIR: &str = "crates/freerouting/tests/data/kicad/k1519";

/// J10's slot: centre (mm, y down), half length along x, half width.
const SLOT: ((f64, f64), f64, f64) = ((45.4516, 63.6636), 0.15, 0.325);

/// The board's `min_hole_clearance`, mm.
const HOLE_CLEARANCE_MM: f64 = 0.25;

fn path(name: &str) -> std::path::PathBuf {
    repo_root().join(DIR).join(format!("{name}.kicad_pcb"))
}

/// Routes fixture `name` under `profile` (two passes), with `patch` on top.
fn route(name: &str, profile: &str, edit: impl FnOnce(&mut SettingsPatch)) -> RoutingBoard {
    let path = path(name);
    let bytes = std::fs::read(&path).unwrap();
    let read = read_kicad_bytes(&path, &bytes, ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    let mut patch = SettingsPatch {
        profile: Some(profile.to_string()),
        ..SettingsPatch::default()
    };
    patch.autorouter.max_passes = Some(2);
    edit(&mut patch);
    // After the board's own sources (its Board Setup minimums among them).
    input.extra_sources.push((Source::Testing, patch));
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

/// Board units (0.1 µm, y up) to mm (y down).
fn mm(p: FloatPoint) -> (f64, f64) {
    (p.x / 10_000.0, -p.y / 10_000.0)
}

fn point_segment(p: (f64, f64), a: (f64, f64), b: (f64, f64)) -> f64 {
    let (dx, dy) = (b.0 - a.0, b.1 - a.1);
    let len = dx * dx + dy * dy;
    let t = if len == 0.0 {
        0.0
    } else {
        (((p.0 - a.0) * dx + (p.1 - a.1) * dy) / len).clamp(0.0, 1.0)
    };
    (p.0 - a.0 - t * dx).hypot(p.1 - a.1 - t * dy)
}

fn crosses(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> bool {
    let side = |p: (f64, f64), q: (f64, f64), r: (f64, f64)| {
        (q.0 - p.0) * (r.1 - p.1) - (q.1 - p.1) * (r.0 - p.0)
    };
    side(a, b, c) * side(a, b, d) < 0.0 && side(c, d, a) * side(c, d, b) < 0.0
}

/// The distance between two segments, 0 where they cross.
fn segment_distance(a: (f64, f64), b: (f64, f64), c: (f64, f64), d: (f64, f64)) -> f64 {
    if crosses(a, b, c, d) {
        return 0.0;
    }
    [
        point_segment(a, c, d),
        point_segment(b, c, d),
        point_segment(c, a, b),
        point_segment(d, a, b),
    ]
    .into_iter()
    .fold(f64::MAX, f64::min)
}

/// A trace segment: its ends and its half width, in mm.
type Segment = ((f64, f64), (f64, f64), f64);

/// Every trace segment of `net`.
fn segments(board: &RoutingBoard, net: &str) -> Vec<Segment> {
    let n = board.rules.nets.get_by_name(net, 1).unwrap().net_number;
    board
        .items()
        .filter(|item| item.contains_net(n))
        .filter_map(|item| match &item.kind {
            ItemKind::Trace(trace) => Some(trace),
            _ => None,
        })
        .flat_map(|trace| {
            let corners: Vec<_> = trace
                .polyline
                .corner_approx_arr()
                .into_iter()
                .map(mm)
                .collect();
            let half = f64::from(trace.half_width) / 10_000.0;
            corners
                .windows(2)
                .map(|w| (w[0], w[1], half))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The routed trace segments of every net whose copper reaches the board
/// outline (a gap under 0.5 µm, or a crossing), as `(net, gap in mm)`.
fn edge_breaches(board: &RoutingBoard) -> Vec<(String, f64)> {
    let outline = board
        .outline()
        .and_then(|item| item.as_board_outline())
        .expect("an outline");
    let edges: Vec<((f64, f64), (f64, f64))> = outline
        .shapes()
        .iter()
        .flat_map(|shape| {
            let corners: Vec<_> = shape.corner_approx_arr().into_iter().map(mm).collect();
            (0..corners.len())
                .map(|i| (corners[i], corners[(i + 1) % corners.len()]))
                .collect::<Vec<_>>()
        })
        .collect();
    let mut found = Vec::new();
    for net in board.rules.nets.iter() {
        for (a, b, half) in segments(board, &net.name) {
            let gap = edges
                .iter()
                .map(|(c, d)| segment_distance(a, b, *c, *d))
                .fold(f64::MAX, f64::min)
                - half;
            if gap < 0.0005 {
                found.push((net.name.clone(), gap));
            }
        }
    }
    found
}

/// The smallest gap, mm, from net SIG's routed copper to J10's slot.
fn slot_gap(board: &RoutingBoard) -> f64 {
    let ((x, y), half_length, radius) = SLOT;
    let (start, end) = ((x - half_length, y), (x + half_length, y));
    segments(board, "SIG")
        .into_iter()
        .map(|(a, b, half)| segment_distance(a, b, start, end) - half - radius)
        .fold(f64::MAX, f64::min)
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

#[test]
fn no_routed_trace_crosses_the_outline_beside_a_pin_on_the_edge() {
    // Java's exemption (`legacy`): /TMC_AIN and GND reach J1's pad centres
    // across the outline.
    let legacy = route("wisweep-j1-edge", "legacy", |_| {});
    let mut nets: Vec<String> = edge_breaches(&legacy).into_iter().map(|b| b.0).collect();
    nets.sort();
    nets.dedup();
    assert_eq!(nets, ["/TMC_AIN", "GND"], "{:?}", edge_breaches(&legacy));
    assert_eq!(incomplete(&legacy), 0);

    // `enhanced` (`java_quirks.edge_pin_net_crosses_outline=false`): no trace
    // reaches the outline; J1-1 and J1-2, whose centres lie outside the board,
    // stay open.
    let enhanced = route("wisweep-j1-edge", "enhanced", |_| {});
    assert_eq!(edge_breaches(&enhanced), []);
    assert_eq!(incomplete(&enhanced), 2);

    // The switch alone: `enhanced` with Java's exemption crosses again.
    let java = route("wisweep-j1-edge", "enhanced", |patch| {
        patch.java_quirks.edge_pin_net_crosses_outline = Some(true);
    });
    assert!(!edge_breaches(&java).is_empty());
}

#[test]
fn no_routed_track_comes_nearer_than_the_hole_clearance_to_an_npth() {
    for profile in ["legacy", "enhanced"] {
        let board = route("wisweep-j10-slot", profile, |_| {});
        assert_eq!(board.rules.hole_clearance(), 2500, "{profile}");
        assert_eq!(incomplete(&board), 0, "{profile}");
        let gap = slot_gap(&board);
        assert!(
            gap >= HOLE_CLEARANCE_MM - 1e-4,
            "{profile}: SIG is {gap:.4} mm from J10's slot"
        );
    }
}

/// `-drc` on the unrouted slot fixture: the slot's own pad is no copper
/// against its own hole (it was 4 `hole_clearance` findings of 0 mm, one per
/// layer), and nothing else is near it.
#[test]
fn an_npth_slots_own_pad_is_no_hole_clearance() {
    let run = run_drc(&DrcRunOptions::new(path("wisweep-j10-slot"))).expect("loads");
    let report: &DrcReport = &run.report;
    let holes: Vec<&str> = report
        .violations
        .iter()
        .filter(|v| v.violation_type == "hole_clearance")
        .map(|v| v.description.as_str())
        .collect();
    assert_eq!(holes, Vec::<&str>::new());
}

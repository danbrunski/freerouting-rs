//! K15.22: a via keeps the hole-to-hole distance from a pin's real hole.
//!
//! Fixture `tests/data/kicad/k1519/wisweep-j10-via.kicad_pcb`, cut from
//! wisweep's driver board (`slimcdk/wisweep` `f22ec65c9b`,
//! `electronics/driver-board.kicad_pcb`, MIT; corpus row
//! `wisweep-driver-board`) as K15.19's `wisweep-j10-slot`: the outline and
//! J10's oval NPTH (a 0.95 x 0.65 mm slot, no copper, at (45.4516, 63.6636)),
//! with the board's own `.kicad_pro` (Default class 0.8 / 0.4 mm vias,
//! `min_hole_clearance` and `min_hole_to_hole` 0.25 mm). Hand-written: net
//! SIG has a 1 x 1 mm F.Cu test point (TP1) and a B.Cu one (TP2), both
//! centred 1.5 mm right of the slot's centre (1.025 mm from its wall), so the
//! route needs one via beside the slot.
//!
//! With the board's 0.25 mm the hole clearance (0.25 mm from the via's
//! 0.4 mm copper radius to the slot) already keeps the drills further apart
//! than the hole-to-hole distance needs, so the tests raise the minimum to
//! 1.4 mm (`router.board_minimums.hole_to_hole_um=1400`). The router lays
//! the via at (46.9516, 62.4910) mm: 1.2632 mm from the slot's wall, 1.442 mm
//! from the padstack's estimate of the slot (`Padstack::get_drill_radius`,
//! 0.45 x 0.325 = 0.146 mm round its centre), which let it stand (RED before
//! K15.22: "legacy: the via at (469516, -624910) is 1.2632 mm from J10's
//! slot", and the check found nothing). Measured from the real slot, the via
//! moves to (46.9516, 62.2712) mm, 1.4144 mm from the wall, in both
//! profiles.

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::drc::board_rules::{BoardRule, BoardRuleLimits, InputCopper, check_board_rules};
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, PreparedJob, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const FIXTURE: &str = "crates/freerouting/tests/data/kicad/k1519/wisweep-j10-via.kicad_pcb";

/// J10's slot in board units (0.1 µm, y up): centre, half length along x,
/// half width.
const SLOT: ((f64, f64), f64, f64) = ((454_516.0, -636_636.0), 1_500.0, 3_250.0);

/// The via drill's radius (0.4 mm), board units.
const VIA_DRILL_RADIUS: f64 = 2_000.0;

/// The tests' hole-to-hole minimum, µm and board units.
const MIN_UM: f64 = 1_400.0;
const MIN_UNITS: f64 = 14_000.0;

fn prepare(profile: &str, hole_to_hole_um: Option<f64>) -> PreparedJob {
    let path = repo_root().join(FIXTURE);
    let bytes = std::fs::read(&path).unwrap();
    let read = read_kicad_bytes(&path, &bytes, ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), "wisweep-j10-via");
    input.set_kicad(read);
    let mut patch = SettingsPatch {
        profile: Some(profile.to_string()),
        ..SettingsPatch::default()
    };
    patch.autorouter.max_passes = Some(2);
    patch.board_minimums.hole_to_hole_um = hole_to_hole_um;
    // After the board's own sources (its Board Setup minimums among them).
    input.extra_sources.push((Source::Testing, patch));
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

fn route(prepared: &PreparedJob) -> RoutingBoard {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(prepared.board.clone(), &job).unwrap_or_else(|f| panic!("{}", f.error));
    outcome.board.take().expect("no listener took the board")
}

/// The gap, board units, from a via drill at `(x, y)` to the slot's wall.
fn slot_gap(x: f64, y: f64) -> f64 {
    let ((cx, cy), half_length, radius) = SLOT;
    let dx = (x - cx).abs() - half_length;
    dx.max(0.0).hypot(y - cy) - radius - VIA_DRILL_RADIUS
}

/// Every via's drill centre, board units.
fn vias(board: &RoutingBoard) -> Vec<(f64, f64)> {
    board
        .items()
        .filter(|item| item.is_via())
        .map(|item| {
            let c = item.drill().unwrap().center;
            (f64::from(c.x), f64::from(c.y))
        })
        .collect()
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

#[test]
fn a_via_keeps_the_hole_to_hole_distance_from_an_npth_slot() {
    for profile in ["legacy", "enhanced"] {
        let prepared = prepare(profile, Some(MIN_UM));
        assert_eq!(prepared.board.rules.hole_to_hole(), 14_000, "{profile}");
        let board = route(&prepared);
        assert_eq!(incomplete(&board), 0, "{profile}");
        let vias = vias(&board);
        assert!(!vias.is_empty(), "{profile}: no via");
        for (x, y) in vias {
            let gap = slot_gap(x, y);
            assert!(
                gap >= MIN_UNITS,
                "{profile}: the via at ({x}, {y}) is {:.4} mm from J10's slot",
                gap / 10_000.0
            );
        }
    }
}

#[test]
fn the_output_check_finds_a_via_nearer_than_the_hole_to_hole_distance_to_a_slot() {
    // The board's own 0.25 mm: the via lands nearer the slot than 1.0 mm.
    let prepared = prepare("legacy", None);
    let mut board = route(&prepared);
    assert_eq!(board.rules.hole_to_hole(), 2_500);
    let limits = BoardRuleLimits::of(&board, &prepared.board_settings);
    let hole_to_hole = |board: &RoutingBoard| {
        check_board_rules(board, &limits, &InputCopper::none())
            .into_iter()
            .filter(|v| v.rule == BoardRule::HoleToHole)
            .collect::<Vec<_>>()
    };
    assert!(hole_to_hole(&board).is_empty());
    let near: Vec<f64> = vias(&board)
        .into_iter()
        .map(|(x, y)| slot_gap(x, y))
        .filter(|gap| *gap < MIN_UNITS)
        .collect();
    assert_eq!(near.len(), 1, "{:?}", vias(&board));
    let gap = near[0];

    board.rules.set_hole_to_hole(MIN_UNITS as i32);
    let found = hole_to_hole(&board);
    assert_eq!(found.len(), 1, "{found:?}");
    let v = &found[0];
    assert_eq!(v.required, MIN_UNITS);
    // Measured from the slot's wall, not from the padstack's estimate.
    assert!((v.actual - gap).abs() < 1.0, "{v:?}, the slot gap {gap}");
    assert!(
        v.items
            .iter()
            .any(|id| board.item(*id).is_some_and(|i| i.is_pin())),
        "{v:?}"
    );
}

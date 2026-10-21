//! Q22.20: routed copper of another net leaves a zone's thermal spokes room,
//! and the pad stays reachable by its own net.
//!
//! Fixtures in `tests/data/quality/` (mm, KiCad's frame; the DSN negates Y):
//! `q22-thermal.kicad_pcb` has a GND zone on F.Cu and B.Cu over (10..30,
//! 5..25) with thermal relief (gap 0.5, spoke 0.5, clearance 0.3); J1 at
//! (20, 15), a 1.7 mm through-hole GND pad; U2 (14, 9), a turned 2 x 1 mm
//! F.Cu GND pad; U3 (26, 21), a through-hole GND pad; U4 (14, 21), a solid
//! GND pad (no spokes); U5 (24, 9), a 1 mm through-hole GND pad 1.27 mm from
//! U5-2 of net A, which its diagonal spoke ways overlap; P2 (35, 15), a GND
//! pad outside the zone; net A from PA1 (16.5, 15) to PA2 (23.5, 15), whose
//! straight way runs through J1, and U5-2.
//! `q22-thermal-base.dsn` is KiCad 10.0.7's export; `q22-thermal.dsn` is what
//! the KiCad plugin makes of it (`kicad-plugin/tests/test_thermal_spokes.py`):
//! 28 `thermal_spoke` keepouts, 4 per pad and layer, 1.1 mm wide and 2.25 mm
//! long for J1 and U3 (`io::specctra::parser::thermal_keepouts`).

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::items::{ItemId, THERMAL_SPOKE_KEEPOUT_NAME};
use freerouting::drc::{BoardRule, BoardRuleLimits, InputCopper, check_board_rules};
use freerouting::job::prepare::{JobInput, PreparedJob, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{IntBox, Shape, TileShape};

const DIR: &str = "crates/freerouting/tests/data/quality";

fn text(name: &str) -> String {
    std::fs::read_to_string(repo_root().join(DIR).join(name)).unwrap()
}

fn patch(profile: &str) -> SettingsPatch {
    let mut patch = SettingsPatch {
        profile: Some(profile.to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(5);
    patch
}

fn prepare_dsn(dsn: &str, profile: &str) -> PreparedJob {
    let mut input = JobInput::new(dsn.as_bytes().to_vec(), "q22-thermal");
    input.extra_sources = vec![(Source::Testing, patch(profile))];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

fn prepare_pcb(profile: &str) -> PreparedJob {
    use freerouting::io::kicad::{ReadOptions, read_kicad_input};
    let read = read_kicad_input(
        &repo_root().join(DIR).join("q22-thermal.kicad_pcb"),
        ReadOptions::default(),
    )
    .unwrap();
    let mut input = JobInput::new(Vec::new(), "q22-thermal");
    input.set_kicad(read);
    input.extra_sources.push((Source::Testing, patch(profile)));
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

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

fn net(board: &RoutingBoard, name: &str) -> i32 {
    board.rules.nets.get_by_name(name, 1).unwrap().net_number
}

fn everything(board: &RoutingBoard, layer: i32) -> Vec<ItemId> {
    let all = TileShape::IntBox(IntBox::new(0, -300_000, 400_000, 0));
    board.overlapping_objects(&all, layer).into_iter().collect()
}

/// The thermal-spoke keepouts: each one's layer and convex pieces.
fn corridors(board: &RoutingBoard) -> Vec<(ItemId, i32, Vec<Shape>)> {
    let mut ids: Vec<ItemId> = (0..2).flat_map(|l| everything(board, l)).collect();
    ids.sort();
    ids.dedup();
    ids.into_iter()
        .filter_map(|id| {
            let item = board.item(id)?;
            let area = item.as_area()?;
            (area.name.as_deref() == Some(THERMAL_SPOKE_KEEPOUT_NAME)).then(|| {
                let shapes = (0..board.tile_shape_count(item))
                    .filter_map(|i| board.tile_shape(item, i))
                    .map(Shape::from)
                    .collect();
                (id, area.layer(), shapes)
            })
        })
        .collect()
}

/// The corridors (of `keepouts`, any board's) that a trace or via of
/// `net` meets on `board`.
fn crossed(board: &RoutingBoard, keepouts: &[(ItemId, i32, Vec<Shape>)], net: i32) -> usize {
    keepouts
        .iter()
        .filter(|(_, layer, shapes)| {
            everything(board, *layer)
                .iter()
                .filter_map(|id| board.item(*id))
                .filter(|i| (i.is_trace() || i.is_via()) && i.net_numbers() == [net])
                .any(|i| {
                    freerouting::quality::stub_contacts::pieces(board, i)
                        .iter()
                        .any(|(l, s)| l == layer && shapes.iter().any(|c| c.intersects(s)))
                })
        })
        .count()
}

/// Each keepout carries GND, the net of the pad at its start, U5-1's too,
/// though they also overlap U5-2 of net A; and they are no obstacle for that
/// pin.
#[test]
fn the_keepouts_carry_the_pads_net() {
    for prepared in [
        prepare_dsn(&text("q22-thermal.dsn"), "legacy"),
        prepare_pcb("legacy"),
    ] {
        let board = &prepared.board;
        let found = corridors(board);
        assert_eq!(found.len(), 28);
        let gnd = net(board, "GND");
        let a = net(board, "A");
        let pins: Vec<_> = everything(board, 0)
            .iter()
            .filter_map(|id| board.item(*id))
            .filter(|i| i.is_pin() && i.net_numbers() == [a])
            .collect();
        assert_eq!(pins.len(), 3);
        for (id, ..) in &found {
            let item = board.item(*id).unwrap();
            assert!(item.as_area().unwrap().is_pad_keepout());
            assert_eq!(item.net_numbers(), [gnd]);
            for pin in &pins {
                assert!(!board.is_obstacle(item, pin) && !board.is_obstacle(pin, item));
            }
        }
    }
}

/// Without the keepouts (KiCad's export) A's route crosses J1's spoke ways;
/// with them, on both inputs and both profiles, it leaves every corridor free
/// (at least `min_resolved_spokes` = 2 per pad and layer) and everything
/// connects.
#[test]
fn another_nets_trace_leaves_the_spokes_free() {
    let reference = prepare_dsn(&text("q22-thermal.dsn"), "legacy").board;
    let keepouts = corridors(&reference);
    let a = net(&reference, "A");
    let base = route(prepare_dsn(&text("q22-thermal-base.dsn"), "legacy"));
    assert_eq!(incomplete(&base), 0);
    assert!(
        crossed(&base, &keepouts, a) >= 1,
        "the control crosses a spoke way"
    );
    for board in [
        route(prepare_dsn(&text("q22-thermal.dsn"), "legacy")),
        route(prepare_dsn(&text("q22-thermal.dsn"), "enhanced")),
        route(prepare_pcb("legacy")),
        route(prepare_pcb("enhanced")),
    ] {
        assert_eq!(incomplete(&board), 0, "A and GND connect");
        assert_eq!(crossed(&board, &keepouts, a), 0);
    }
}

/// The pad stays reachable: without the zones' planes GND must reach J1, U2,
/// U3 and U4 by traces, through or between the spoke ways, and does.
#[test]
fn the_pad_is_still_reachable_by_its_own_net() {
    let dsn: String = text("q22-thermal.dsn")
        .lines()
        .filter(|l| !l.trim_start().starts_with("(plane GND"))
        .map(|l| format!("{l}\n"))
        .collect();
    assert!(!dsn.contains("(plane"));
    let board = route(prepare_dsn(&dsn, "legacy"));
    assert_eq!(incomplete(&board), 0);
    assert_eq!(corridors(&board).len(), 28);
}

/// The output check: a GND trace through J1's spoke way is fine, an A trace
/// through it is a keepout breach.
#[test]
fn the_output_check_reports_only_another_nets_trace_in_a_spoke_way() {
    let wiring = "(wiring\n    (wire (path F.Cu 200  20000 -15000  21500 -13500) (net GND))\n    \
                  (wire (path F.Cu 200  18000 -12000  18000 -18000) (net A))\n  )";
    let base = text("q22-thermal.dsn");
    let dsn = base
        .replace("(wiring\n  )", wiring)
        .replace("(wiring)", wiring);
    assert_ne!(dsn, base);
    let prepared = prepare_dsn(&dsn, "legacy");
    let limits = BoardRuleLimits::of(&prepared.board, &prepared.board_settings);
    let found = check_board_rules(&prepared.board, &limits, &InputCopper::none());
    let nets: std::collections::BTreeSet<String> = found
        .iter()
        .filter(|v| v.rule == BoardRule::Keepout)
        .flat_map(|v| v.items.iter())
        .filter_map(|id| prepared.board.item(*id))
        .filter(|item| item.is_trace())
        .flat_map(|item| item.net_numbers().to_vec())
        .filter_map(|n| prepared.board.rules.nets.get(n).map(|net| net.name.clone()))
        .collect();
    assert_eq!(
        nets.into_iter().collect::<Vec<_>>(),
        vec!["A".to_owned()],
        "{found:?}"
    );
}

/// The DSN writer keeps the name, so the keepouts read back the same.
#[test]
fn the_dsn_writer_keeps_the_keepouts() {
    let prepared = prepare_dsn(&text("q22-thermal.dsn"), "legacy");
    let written = freerouting::io::specctra::dsn_writer::write_dsn(&prepared.board, "q22");
    assert_eq!(
        written.matches(THERMAL_SPOKE_KEEPOUT_NAME).count(),
        28,
        "{written}"
    );
    let again = prepare_dsn(&written, "legacy");
    assert_eq!(corridors(&again.board).len(), 28);
}

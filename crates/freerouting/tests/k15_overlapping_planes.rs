//! K15.15: a plane-layer zone under a higher-priority zone is not counted there.
//!
//! Fixture `crates/freerouting-kicad/tests/data/k15-overlapping-planes.kicad_pcb`,
//! cut from the power-control unit (`../power-control-unit` `76163c7`): its two
//! In2.Cu zones verbatim without fills, V_BUS_24V (priority 41) over V_BUS_12V
//! (priority 1); J1, a V_BUS_12V pad on F.Cu with a track to a V_BUS_12V via at
//! (165.71, 44.32) mm inside the overlap (one of the three vias KiCad found
//! dangling on K15.13's pcu runs); J2, a V_BUS_12V through-hole pad at
//! (200, 60) mm in V_BUS_12V's own fill. KiCad 10.0.7 after `--refill-zones`:
//! 1 unconnected item (J1's track and J2's pad) and the via dangling.

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::items::{ItemId, ItemKind};
use freerouting::drc::DesignRulesChecker;
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const PCB: &str = "crates/freerouting-kicad/tests/data/k15-overlapping-planes.kicad_pcb";

/// V_BUS_24V's outline on In2.Cu, mm (the fixture's `polygon`).
const HIGH_OUTLINE: [(f64, f64); 8] = [
    (43.1, 31.9),
    (177.525299, 31.9),
    (177.574581, 52.45),
    (177.599872, 52.45),
    (177.600118, 84.701495),
    (129.006319, 84.798495),
    (128.895885, 151.959932),
    (42.9, 151.8),
];

/// The fixture, or without J1's track and via (J1's pad alone in the overlap).
fn input(profile: &str, max_passes: Option<i32>, with_via: bool) -> JobInput {
    let path = repo_root().join(PCB);
    let text = std::fs::read_to_string(&path).unwrap();
    let text: String = if with_via {
        text
    } else {
        text.lines()
            .filter(|l| {
                !l.trim_start().starts_with("(segment") && !l.trim_start().starts_with("(via")
            })
            .map(|l| format!("{l}\n"))
            .collect()
    };
    let read = read_kicad_bytes(&path, text.as_bytes(), ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), "k15-overlapping-planes");
    input.set_kicad(read);
    let mut patch = SettingsPatch {
        profile: Some(profile.to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = max_passes;
    input.extra_sources = vec![(Source::Testing, patch)];
    input
}

fn loaded(profile: &str) -> RoutingBoard {
    prepare_job(&input(profile, None, true), &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

fn route(profile: &str, with_via: bool) -> RoutingBoard {
    let prepared = prepare_job(
        &input(profile, Some(3), with_via),
        &ResolveContext { cores: 1 },
    )
    .unwrap();
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
fn mm(x: i32, y: i32) -> (f64, f64) {
    (f64::from(x) / 10_000.0, -f64::from(y) / 10_000.0)
}

fn in_high_outline(p: (f64, f64)) -> bool {
    let poly = &HIGH_OUTLINE;
    let mut c = false;
    for i in 0..poly.len() {
        let (a, b) = (poly[i], poly[(i + 1) % poly.len()]);
        if (a.1 > p.1) != (b.1 > p.1) && p.0 < (b.0 - a.0) * (p.1 - a.1) / (b.1 - a.1) + a.0 {
            c = !c;
        }
    }
    c
}

fn plane_of(board: &RoutingBoard, net: &str) -> ItemId {
    let n = board.rules.nets.get_by_name(net, 1).unwrap().net_number;
    let planes: Vec<ItemId> = board
        .items()
        .filter(|i| i.as_conduction_area().is_some() && i.contains_net(n))
        .map(|i| i.id())
        .collect();
    assert_eq!(planes.len(), 1, "{net}");
    planes[0]
}

/// The V_BUS_12V vias, mm, and whether each touches V_BUS_12V's plane.
fn vias_on_plane(board: &RoutingBoard) -> Vec<((f64, f64), bool)> {
    let plane = plane_of(board, "V_BUS_12V");
    let n = board
        .rules
        .nets
        .get_by_name("V_BUS_12V", 1)
        .unwrap()
        .net_number;
    board
        .items()
        .filter(|i| i.contains_net(n))
        .filter_map(|i| match &i.kind {
            ItemKind::Via(v) => Some((
                mm(v.drill.center.x, v.drill.center.y),
                board.normal_contacts(i.id()).contains(&plane),
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn the_via_in_the_overlap_is_not_on_the_lower_plane() {
    for profile in ["legacy", "enhanced"] {
        let board = loaded(profile);
        let vias = vias_on_plane(&board);
        assert_eq!(vias.len(), 1, "{profile}");
        let (at, on_plane) = vias[0];
        assert!((at.0 - 165.71).abs() < 1e-6 && (at.1 - 44.32).abs() < 1e-6);
        assert!(!on_plane, "{profile}: the via counts on V_BUS_12V's plane");
        // J2's pad is on it, so J1's track, pad and via are one connection short,
        // as KiCad counts it
        let plane = plane_of(&board, "V_BUS_12V");
        assert!(
            board.items().any(|i| matches!(i.kind, ItemKind::Pin(_))
                && board.normal_contacts(i.id()).contains(&plane)),
            "{profile}"
        );
        assert_eq!(
            DesignRulesChecker::new(&board).incomplete_count(),
            1,
            "{profile}"
        );
    }
}

/// Every V_BUS_12V via that counts on the plane lies outside V_BUS_24V's outline.
fn no_via_on_the_plane_under_the_higher_zone(board: &RoutingBoard, profile: &str) {
    for (at, on_plane) in vias_on_plane(board) {
        assert!(
            !on_plane || !in_high_outline(at),
            "{profile}: a V_BUS_12V via at {at:?} counts on the plane inside V_BUS_24V"
        );
    }
}

/// The ends on In2.Cu of V_BUS_12V's traces, mm.
fn plane_layer_trace_ends(board: &RoutingBoard) -> Vec<(f64, f64)> {
    let n = board
        .rules
        .nets
        .get_by_name("V_BUS_12V", 1)
        .unwrap()
        .net_number;
    let in2 = board
        .layer_structure
        .layers
        .iter()
        .position(|l| l.name == "V_BUS")
        .unwrap() as i32;
    board
        .items()
        .filter(|i| i.contains_net(n))
        .filter_map(|i| i.as_trace())
        .filter(|t| t.layer == in2)
        .flat_map(|t| {
            [t.first_corner(), t.last_corner()].map(|p| {
                let f = p.to_float();
                (f.x / 10_000.0, -f.y / 10_000.0)
            })
        })
        .collect()
}

#[test]
fn the_via_in_the_overlap_stays_open() {
    // J1's track and via start inside V_BUS_12V's window. Neither profile routes a
    // connection that starts inside a plane's window, as neither routes one that starts
    // in a zone's own hole (the fixture with V_BUS_24V's part drawn as a hole of
    // V_BUS_12V's zone routes the same): it stays open, which is what KiCad counts.
    for profile in ["legacy", "enhanced"] {
        let board = route(profile, true);
        assert_eq!(
            DesignRulesChecker::new(&board).incomplete_count(),
            1,
            "{profile}"
        );
        no_via_on_the_plane_under_the_higher_zone(&board, profile);
    }
}

#[test]
fn the_router_reaches_the_lower_plane_where_it_is_filled() {
    // J1's pad alone: `legacy` reaches V_BUS_12V's own part of the plane with a trace on
    // In2.Cu (Java's power-layer rule) that ends past V_BUS_24V's outline grown by the
    // zones' 0.8 mm clearance, where KiCad fills V_BUS_12V. `enhanced` lays no trace on a
    // power layer (`router.power_layer_traces=none`); its fan-out via lands in the window
    // and the connection stays open.
    for (profile, open) in [("legacy", 0), ("enhanced", 1)] {
        let board = route(profile, false);
        assert_eq!(
            DesignRulesChecker::new(&board).incomplete_count(),
            open,
            "{profile}"
        );
        no_via_on_the_plane_under_the_higher_zone(&board, profile);
        let ends = plane_layer_trace_ends(&board);
        if open == 0 {
            assert!(
                // V_BUS_24V's right edge runs from x 177.525 (y 31.9) to 177.575 (y 52.45)
                ends.iter()
                    .any(|&(x, y)| y < 52.45 && x >= 177.575 + 0.8 - 0.05),
                "{profile}: {ends:?}"
            );
        }
    }
}

//! Q22.4: copper shapes and copper text are obstacles the router keeps its
//! clearance from (`freerouting_kicad::copper_graphics`), and a copper shape
//! of a net is copper of that net that other nets keep clear of
//! (`(attr obstacle)`, `BasicBoard::insert_fixed_copper_area`).
//!
//! Fixture `tests/data/kicad/q22-copper-graphics.kicad_pcb`: J1's two pads
//! numbered 2 (net N2) at (12, 10) and (18, 10) mm, a 0.5 mm B.Cu `fp_line`
//! of no net between them, and a `gr_text "GND"` on F.Cu over the same gap;
//! a filled F.Cu `gr_poly` of net FG, (3, 3)-(7, 5), under FG's only pads P1
//! at (4, 4) and P2 at (6, 4); a 1 mm F.Cu `gr_line` of net FG from (20, 14)
//! to (28, 14) across the straight path between N3's F.Cu pads at (24, 11.5)
//! and (24, 16.5). The straight connection of N2 on either layer crosses the
//! line or the text, and N3's crosses the FG line; KiCad's DSN export carries
//! none of the four shapes.

mod autoroute_common;

use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::structure::FixedState;
use freerouting::drc::{BoardRule, BoardRuleLimits, InputCopper, Origin, check_board_rules};
use freerouting::dsn::ast::{WIRE_ATTR_OBSTACLE, WiringItem};
use freerouting::io::kicad::{
    KicadInput, ReadOptions, final_copper, read_kicad_input, render_kicad_board,
};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{IntPoint, Point, Polyline};
use freerouting_kicad::copper_graphics::{KEEPOUT_NAME, TEXT_KEEPOUT_NAME};

const PCB: &str = "crates/freerouting/tests/data/kicad/q22-copper-graphics.kicad_pcb";
/// The B.Cu line's copper and the reader's text box, board units (0.1 µm,
/// Y up): x0, y0, x1, y1.
const LINE: [f64; 4] = [129_500.0, -102_500.0, 170_500.0, -97_500.0];
const TEXT: [f64; 4] = [120_375.0, -115_000.0, 179_625.0, -85_000.0];
/// The FG line's copper (0.5 mm on each side and past each end).
const FG_LINE: [f64; 4] = [195_000.0, -145_000.0, 285_000.0, -135_000.0];

/// What the reader gives, without its Q22.4 obstacles and areas unless
/// `keep_obstacles`.
fn read(keep_obstacles: bool) -> KicadInput {
    let mut read = read_kicad_input(&repo_root().join(PCB), ReadOptions::default()).unwrap();
    if !keep_obstacles {
        let s = read.pcb.structure.as_mut().unwrap();
        s.keepouts.retain(|k| {
            !matches!(
                k.area.name.as_deref(),
                Some(KEEPOUT_NAME | TEXT_KEEPOUT_NAME)
            )
        });
        read.pcb.wiring.as_mut().unwrap().items.retain(
            |i| !matches!(i, WiringItem::Wire(w) if w.attr.as_deref() == Some(WIRE_ATTR_OBSTACLE)),
        );
    }
    read
}

fn prepare(read: KicadInput, profile: &str) -> PreparedJob {
    let mut input = JobInput::new(Vec::new(), "q22-copper-graphics");
    input.set_kicad(read);
    let mut patch = SettingsPatch {
        profile: Some(profile.to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    input.extra_sources.push((Source::Testing, patch));
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

fn route(keep_obstacles: bool, profile: &str) -> RoutingBoard {
    let prepared = prepare(read(keep_obstacles), profile);
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

fn net(board: &RoutingBoard, name: &str) -> i32 {
    board.rules.nets.get_by_name(name, 1).unwrap().net_number
}

/// The smallest gap between `name`'s traces on `layer` and the box, board
/// units (negative: they overlap); infinite without traces there.
fn gap(board: &RoutingBoard, name: &str, layer: i32, b: [f64; 4]) -> f64 {
    let n = net(board, name);
    let mut gap = f64::INFINITY;
    for item in board.items().filter(|i| i.contains_net(n)) {
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

fn traces_of(board: &RoutingBoard, name: &str) -> usize {
    let n = net(board, name);
    board
        .items()
        .filter(|i| i.is_trace() && i.contains_net(n))
        .count()
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

/// The FG areas of the board: (conduction area, obstacle, fixed obstacle).
fn fg_areas(board: &RoutingBoard) -> Vec<(bool, bool, bool)> {
    let fg = net(board, "FG");
    board
        .items()
        .filter(|i| i.contains_net(fg))
        .filter_map(|i| i.as_area())
        .map(|a| {
            (
                a.is_conduction_area(),
                a.conduction_is_obstacle(),
                a.is_fixed_obstacle(),
            )
        })
        .collect()
}

#[test]
fn without_the_obstacles_the_route_crosses_the_copper_graphics() {
    let board = route(false, "legacy");
    assert_eq!(incomplete(&board), 0);
    let crosses = gap(&board, "N2", 0, TEXT) < 0.0 || gap(&board, "N2", 1, LINE) < 0.0;
    assert!(
        crosses,
        "F.Cu {} B.Cu {}",
        gap(&board, "N2", 0, TEXT),
        gap(&board, "N2", 1, LINE)
    );
    // N3 goes straight over the FG line, and FG needs a trace of its own
    assert!(gap(&board, "N3", 0, FG_LINE) < 0.0);
    assert!(traces_of(&board, "FG") > 0);
}

#[test]
fn the_route_goes_round_the_copper_line_and_the_copper_text() {
    for profile in ["legacy", "enhanced"] {
        let board = route(true, profile);
        // routed round them, at the 0.2 mm clearance (1 unit of rounding)
        assert_eq!(incomplete(&board), 0, "{profile}");
        let (front, back) = (gap(&board, "N2", 0, TEXT), gap(&board, "N2", 1, LINE));
        assert!(front >= 1999.0, "{profile}: F.Cu gap {front}");
        assert!(back >= 1999.0, "{profile}: B.Cu gap {back}");
    }
}

/// The FG items P1's and P2's pads touch, before routing.
fn fg_pad_contacts(board: &RoutingBoard) -> Vec<Vec<(bool, bool)>> {
    let fg = net(board, "FG");
    board
        .items()
        .filter(|i| i.is_pin() && i.contains_net(fg))
        .map(|pin| {
            board
                .normal_contacts(pin.id())
                .iter()
                .filter_map(|id| board.item(*id))
                .map(|c| {
                    (
                        c.is_pin(),
                        c.as_area().is_some_and(|a| a.is_fixed_obstacle()),
                    )
                })
                .collect()
        })
        .collect()
}

#[test]
fn other_nets_keep_clear_of_a_net_s_copper_and_the_net_connects_through_it() {
    for profile in ["legacy", "enhanced"] {
        // P1 and P2 touch the FG polygon and so each other through it;
        // without the areas they touch nothing
        let loaded = prepare(read(true), profile).board;
        assert_eq!(fg_pad_contacts(&loaded), [[(false, true)]; 2], "{profile}");
        let control = prepare(read(false), profile).board;
        assert!(fg_pad_contacts(&control).iter().all(Vec::is_empty));
        let board = route(true, profile);
        // two fixed FG areas, obstacles after `router.plane_as_obstacle`
        // (false in both profiles) was applied
        assert_eq!(fg_areas(&board), [(true, true, true); 2], "{profile}");
        // everything connects: the FG line is joined to the polygon's group
        assert_eq!(incomplete(&board), 0, "{profile}");
        // N3 goes round the FG line (or under it on B.Cu)
        let front = gap(&board, "N3", 0, FG_LINE);
        assert!(front >= 1999.0, "{profile}: N3's F.Cu gap {front}");
    }
}

#[test]
fn the_session_and_the_kicad_output_leave_the_net_shapes_out() {
    let board = route(true, "enhanced");
    // the session: no conduction area (they are fixed input copper)
    let ses = write_ses(&board, "q22-copper-graphics", SesStyle::default());
    freerouting::dsn::Session::parse(&ses).unwrap();
    assert!(!ses.contains("polygon"), "{ses}");
    assert!(ses.contains("(wire"), "{ses}");
    // the `.kicad_pcb` output: the traces and vias only, and no shape or
    // zone added
    let input = read(true);
    let items = final_copper(&board, &input.copper_index).unwrap();
    let copper = board.items().filter(|i| i.is_trace() || i.is_via()).count();
    assert_eq!(items.len(), copper);
    let path = repo_root().join(PCB);
    let (text, _) = render_kicad_board(&path, &board, None).unwrap();
    let before = std::fs::read_to_string(&path).unwrap();
    for word in ["(gr_", "(zone", "(fp_poly", "(fp_line"] {
        assert_eq!(
            text.matches(word).count(),
            before.matches(word).count(),
            "{word}"
        );
    }
}

#[test]
fn the_output_check_counts_a_crossing_as_routed_and_keeps_the_area() {
    let prepared = prepare(read(true), "enhanced");
    let mut board = prepared.board;
    let limits = BoardRuleLimits::of(&board, &prepared.board_settings);
    let input = InputCopper::of(&board);
    let n3 = net(&board, "N3");
    let class = board
        .items()
        .find(|i| i.contains_net(n3))
        .unwrap()
        .clearance_class();
    // N3 straight over the FG line on F.Cu, as a router would lay it
    let (a, b) = (
        Point::Int(IntPoint::new(240_000, -115_000)),
        Point::Int(IntPoint::new(240_000, -165_000)),
    );
    board.insert_trace(
        Polyline::from_corners(&a, &b),
        0,
        1_000,
        &[n3],
        class,
        FixedState::Unfixed,
    );
    let areas_before = fg_areas(&board).len();
    let found = check_board_rules(&board, &limits, &input);
    let fg = net(&board, "FG");
    let crossing: Vec<_> = found
        .iter()
        .filter(|v| {
            v.rule == BoardRule::Clearance
                && v.items.iter().any(|id| {
                    board
                        .item(*id)
                        .is_some_and(|i| i.as_area().is_some() && i.contains_net(fg))
                })
        })
        .collect();
    assert!(!crossing.is_empty(), "{found:?}");
    assert!(
        crossing.iter().all(|v| v.origin == Origin::Routed),
        "{crossing:?}"
    );
    // only the routed trace may go; the area is input copper
    for v in &crossing {
        assert!(
            v.routed_items
                .iter()
                .all(|id| board.item(*id).is_some_and(|i| i.is_trace())),
            "{v:?}"
        );
    }
    assert_eq!(fg_areas(&board).len(), areas_before);
}

#[test]
fn plane_as_obstacle_never_clears_a_net_shape() {
    let mut board = prepare(read(true), "legacy").board;
    board.change_plane_as_obstacle(true);
    board.change_plane_as_obstacle(false);
    assert_eq!(fg_areas(&board), [(true, true, true); 2]);
}

#[test]
fn the_dsn_text_gives_the_same_areas() {
    // the plugin's path: the same wires as DSN text
    let text = read(true).pcb.to_dsn();
    assert_eq!(text.matches("(attr obstacle)").count(), 2);
    let mut input = JobInput::new(text.into_bytes(), "q22-copper-graphics");
    let mut patch = SettingsPatch {
        profile: Some("legacy".to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    input.extra_sources = vec![(Source::Testing, patch)];
    let board = prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board;
    assert_eq!(fg_areas(&board), [(true, true, true); 2]);
    assert_eq!(fg_pad_contacts(&board), [[(false, true)]; 2]);
}

/// A 0.8 mm F.Cu `gr_arc` of net A from (5, 10) through (10, 5) to (15, 10),
/// cut into chords, with A's two pads centred on its ends.
const ARC_BOARD: &str = r#"(kicad_pcb (version 20240108) (generator "pcbnew") (generator_version "8.0")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "A")
  (gr_rect (start 0 0) (end 20 20) (stroke (width 0.05) (type solid)) (fill none) (layer "Edge.Cuts"))
  (footprint "l:pad" (layer "F.Cu") (at 5 10)
    (property "Reference" "P1") (property "Value" "x")
    (pad "1" smd rect (at 0 0) (size 0.6 0.6) (layers "F.Cu") (net 1 "A")))
  (footprint "l:pad" (layer "F.Cu") (at 15 10)
    (property "Reference" "P2") (property "Value" "x")
    (pad "1" smd rect (at 0 0) (size 0.6 0.6) (layers "F.Cu") (net 1 "A")))
  (gr_arc (start 5 10) (mid 10 5) (end 15 10) (stroke (width 0.8) (type solid)) (layer "F.Cu") (net 1))
)"#;

/// The number of net-shape areas of `board_text` and its board.
fn net_shape_board(board_text: &str) -> (usize, RoutingBoard) {
    let path = repo_root().join("target/no-such-dir/arc.kicad_pcb");
    let read = freerouting::io::kicad::read_kicad_bytes(
        &path,
        board_text.as_bytes(),
        ReadOptions::default(),
    )
    .unwrap();
    let pieces = read
        .pcb
        .wiring
        .as_ref()
        .unwrap()
        .items
        .iter()
        .filter(|i| matches!(i, WiringItem::Wire(w) if w.attr.is_some()))
        .count();
    (pieces, prepare(read, "legacy").board)
}

#[test]
fn the_pieces_of_a_stroked_shape_are_one_copper() {
    // the arc stays one area per chord (decided 2026-10-20, Q22.17: no
    // merge); the chords' areas overlap, so P1, the arc and P2 are one group (Q22.4's
    // contact rule)
    let (pieces, board) = net_shape_board(ARC_BOARD);
    assert!(pieces > 2, "{pieces}");
    assert_eq!(incomplete(&board), 0);
    // a Bezier curve too
    let curve = ARC_BOARD.replace(
        "(gr_arc (start 5 10) (mid 10 5) (end 15 10)",
        "(gr_curve (pts (xy 5 10) (xy 5 4) (xy 15 4) (xy 15 10))",
    );
    let (pieces, board) = net_shape_board(&curve);
    assert!(pieces > 2, "{pieces}");
    assert_eq!(incomplete(&board), 0);
}

/// Q22.17: a 0.4 mm F.Cu `gr_circle` of net A, radius 3 mm about (10, 10),
/// stays one area per chord (decided 2026-10-20: no merge): A's pads P1 at
/// (7, 10) and P2 at (13, 10), on the ring, are one group through the
/// overlapping pieces; P3 at the centre, inside the ring, is not.
#[test]
fn a_stroked_circle_s_pieces_are_one_copper_and_its_inside_is_not() {
    let pad = |name: &str, x: f64| {
        format!(
            r#"  (footprint "l:pad" (layer "F.Cu") (at {x} 10)
    (property "Reference" "{name}") (property "Value" "x")
    (pad "1" smd rect (at 0 0) (size 0.3 0.3) (layers "F.Cu") (net 1 "A")))
"#
        )
    };
    let ring = |pads: &str| {
        format!(
            r#"(kicad_pcb (version 20240108) (generator "pcbnew") (generator_version "8.0")
  (layers (0 "F.Cu" signal) (31 "B.Cu" signal) (44 "Edge.Cuts" user))
  (net 0 "")
  (net 1 "A")
  (gr_rect (start 0 0) (end 20 20) (stroke (width 0.05) (type solid)) (fill none) (layer "Edge.Cuts"))
{pads}  (gr_circle (center 10 10) (end 13 10) (stroke (width 0.4) (type solid)) (fill none) (layer "F.Cu") (net 1))
)"#
        )
    };
    let two = pad("P1", 7.0) + &pad("P2", 13.0);
    let (pieces, board) = net_shape_board(&ring(&two));
    assert!(pieces > 2, "{pieces}");
    assert_eq!(incomplete(&board), 0);
    let three = two + &pad("P3", 10.0);
    let (_, board) = net_shape_board(&ring(&three));
    assert_eq!(incomplete(&board), 1);
}

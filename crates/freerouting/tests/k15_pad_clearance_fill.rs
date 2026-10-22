//! K15.40: the cleanup's fill model takes a pad-clearance keepout as KiCad's
//! zone filler takes the pad it stands for, and keeps the board edge's
//! clearance as the filler keeps it, on a DSN and on a `.kicad_pcb` input.
//!
//! The boards are `tests/data/kicad/cm5-k1540/`, written by `make_boards.py`
//! with every coordinate taken from CM5_MINIMA_3 (K13.6's board), whose route
//! K15.40 measured on main `7bbbc09`: CM5's Edge.Cuts round its mounting
//! hole at (127.25, 77.6) mm and its GND zone (F.Cu, B.Cu), each board as
//! KiCad 10.0.7's `pcb drc --refill-zones --save-board` saved it and as the
//! KiCad plugin's export of that (`.dsn`: KiCad's Specctra export plus the
//! plugin's fix-ups, among them the pad-clearance circles and edge strips):
//!
//! - `a-netless-npth`: Module301's and Module302's NPTHs, verbatim (2.7 mm
//!   with `(clearance 1.5)` and 3.0 mm with `(clearance 1.7)`, no net). KiCad
//!   keeps the GND fill 3.2005 mm from the centre (the hole's radius plus 1.7
//!   mm plus 0.5 µm); the plugin's and the reader's circles (6.14 mm) stand
//!   for the clearance less the 0.13 mm default. Main's DSN model cut them at
//!   their outline (3.07 mm), inside KiCad's clearance. KiCad keeps the fill
//!   0.3725 mm from the straight edges (Board Setup's 0.372 mm plus 0.5 µm);
//!   main's DSN model reached the plugin's edge strips (0.242 mm), its
//!   `.kicad_pcb` model stopped 0.3836 mm off (the router's 0.372 mm, Java's
//!   1.6 µm safety margin and the outline lines' 10 µm half width);
//! - `c-edge-rule`: `a-netless-npth` with the pcu board's outer-layer
//!   `edge_clearance` rule (0.254 mm, below Board Setup's 0.372 mm) in its
//!   `.kicad_dru`; KiCad keeps the fill 0.2545 mm from the straight edges;
//! - `d-inner-layers` (K15.45): `a-netless-npth` on CM5's six copper layers
//!   with the GND zone on its five GND layers (F.Cu, In1.Cu, In2.Cu, In4.Cu,
//!   B.Cu). The circles are written on F.Cu and B.Cu only; KiCad keeps the
//!   fill on every layer the same arc as on `a-netless-npth`'s F.Cu
//!   ([`KICAD_NPTH_ARC`], each vertex on each layer). K15.40's model kept
//!   only the NPTH's hole clearance on the inner layers (1.6989 mm from the
//!   centre on CM5_MINIMA_3's route).
//! - `e-pad-override` (K15.45): Module302's pads 102-108 under CM5's GND
//!   zone on B.Cu, verbatim, with the footprint's `(clearance 0.127)`. It
//!   overrides the 90ohm class's 0.2 mm and the zone's 0.15 mm: KiCad keeps
//!   the fill 0.1275 mm from the signal pads (its edge at x 91.7325 mm on each
//!   pad's centre line); K15.40's `.kicad_pcb` model kept the classes' 0.2 mm
//!   (0.12 mm2 at (91.70, 46.70) mm on CM5_MINIMA_3's route).
//! - `b-gnd-pad`: CM5 has no mounting-hole pad of a pour's net, so Module302's
//!   hole is a GND plated pad (3.6 mm, `(clearance 1.7)`) at the same place.
//!   KiCad joins it to the GND zone by its thermal relief (the ring 1.9316 mm
//!   from the centre); main's model cut the circle (6.74 mm) out of the pour.

mod autoroute_common;

use autoroute_common::repo_root;
use freerouting::board::RoutingBoard;
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::quality::pour_fill::PourFills;
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{FloatPoint, Shape};

const DIR: &str = "crates/freerouting/tests/data/kicad/cm5-k1540";
const NPTH: &str = "a-netless-npth";
const PAD: &str = "b-gnd-pad";
const RULE: &str = "c-edge-rule";
const INNER: &str = "d-inner-layers";
const OVERRIDE: &str = "e-pad-override";
/// `e-pad-override`'s signal pads' centre lines (y, mm) and their left ends
/// (x, mm).
const OVERRIDE_PADS_Y: [f64; 3] = [46.3, 46.7, 47.1];
const OVERRIDE_PAD_END_X: f64 = 91.86;
/// KiCad's fill edge on those lines: the pads' ends less 0.1275 mm.
const KICAD_OVERRIDE_EDGE_X: f64 = 91.7325;
/// `d-inner-layers`' GND layers (board layer numbers: F.Cu, In1.Cu, In2.Cu,
/// In4.Cu, B.Cu).
const GND_LAYERS: [i32; 5] = [0, 1, 2, 4, 5];

/// The mounting hole's centre (mm, KiCad's y down).
const HOLE: (f64, f64) = (127.25, 77.6);

/// KiCad's fill of GND F.Cu round the NPTHs on `a-netless-npth.kicad_pcb`:
/// the vertices of its arc above the centre, 3.2005-3.2055 mm from it.
const KICAD_NPTH_ARC: [(f64, f64); 30] = [
    (124.0495, 77.42026),
    (124.08975, 77.06305),
    (124.16974, 76.7126),
    (124.28846, 76.37329),
    (124.44443, 76.04942),
    (124.63568, 75.74504),
    (124.85981, 75.46399),
    (125.11399, 75.20982),
    (125.39504, 74.98569),
    (125.69942, 74.79443),
    (126.0233, 74.63846),
    (126.36258, 74.51974),
    (126.71305, 74.43975),
    (126.89166, 74.41962),
    (127.07026, 74.3995),
    (127.42974, 74.3995),
    (127.60834, 74.41962),
    (127.78695, 74.43975),
    (128.1374, 74.51973),
    (128.4767, 74.63846),
    (128.80058, 74.79443),
    (129.10496, 74.98569),
    (129.386, 75.20981),
    (129.64019, 75.464),
    (129.86432, 75.74504),
    (130.05557, 76.04942),
    (130.21154, 76.37329),
    (130.33026, 76.71259),
    (130.41025, 77.06305),
    (130.4505, 77.42026),
];

/// KiCad's fill of GND F.Cu round the GND pad on `b-gnd-pad.kicad_pcb`: the
/// vertices of its thermal ring above the centre, 1.9316-1.9341 mm from it.
const KICAD_PAD_RING: [(f64, f64); 28] = [
    (125.32, 77.4735),
    (125.35302, 77.22267),
    (125.4185, 76.9783),
    (125.51532, 76.74456),
    (125.64182, 76.52545),
    (125.79584, 76.32473),
    (125.82978, 76.29079),
    (125.94079, 76.17978),
    (125.97473, 76.14584),
    (126.17545, 75.99182),
    (126.39455, 75.86532),
    (126.62829, 75.76851),
    (126.87268, 75.70302),
    (127.1235, 75.67),
    (127.3765, 75.67),
    (127.62732, 75.70302),
    (127.8717, 75.7685),
    (128.10544, 75.86532),
    (128.32455, 75.99182),
    (128.52527, 76.14584),
    (128.55921, 76.17978),
    (128.67022, 76.29079),
    (128.70416, 76.32473),
    (128.85818, 76.52545),
    (128.98468, 76.74455),
    (129.08149, 76.97829),
    (129.14698, 77.22267),
    (129.18, 77.4735),
];

/// KiCad's fill along the straight edges of `a-netless-npth.kicad_pcb`: y
/// 80.6975 mm (the bottom edge y 81.07) from x 120.4465 to 125.98141, and x
/// 132.5372 mm (the right edge x 132.909) from y 72.44649 to 74.69426.
const KICAD_BOTTOM: (f64, (f64, f64)) = (80.6975, (120.4465, 125.98141));
const KICAD_RIGHT: (f64, (f64, f64)) = (132.5372, (72.44649, 74.69426));
/// The same on `c-edge-rule.kicad_pcb` (the `.kicad_dru`'s 0.254 mm): y 80.8155
/// from x 120.3285 (to the arc at 126.424817), x 132.6552 from y 72.32849.
const RULE_BOTTOM: (f64, (f64, f64)) = (80.8155, (120.3285, 126.3));
const RULE_RIGHT: (f64, (f64, f64)) = (132.6552, (72.32849, 74.69884));
/// The edges themselves (mm).
const EDGE_BOTTOM_Y: f64 = 81.07;
const EDGE_RIGHT_X: f64 = 132.909;

/// The DSN board `name`, told what the KiCad plugin passes with it (its
/// `args.json`): the zone's 150 µm thickness and clearance, island removal
/// with GND's islands kept, the 372 µm edge clearance and 200 µm hole
/// clearance; with `dru`, board `dru`'s `.kicad_dru` (`--kicad-dru`).
fn dsn_board(name: &str, dru: Option<&str>) -> RoutingBoard {
    let text = std::fs::read(repo_root().join(DIR).join(format!("{name}.dsn"))).unwrap();
    let mut input = JobInput::new(text, name);
    if let Some(dru) = dru {
        let path = repo_root().join(DIR).join(format!("{dru}.kicad_dru"));
        let rules = std::fs::read_to_string(&path).unwrap();
        input.kicad_dru = Some((path, rules));
    }
    let mut patch = SettingsPatch {
        profile: Some("enhanced".to_string()),
        ..SettingsPatch::default()
    };
    patch.cleanup.pour_min_thickness_um = Some(150.0);
    patch.cleanup.pour_clearance_um = Some(150.0);
    patch.cleanup.pour_island_removal = Some(true);
    patch.cleanup.pour_islands_kept_nets = Some(vec!["GND".to_string()]);
    patch.copper_to_edge_clearance_um = Some(372.0);
    patch.hole_clearance_um = Some(200.0);
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

/// The `.kicad_pcb` board `name` (its `.kicad_pro` beside it gives the
/// edge clearance, its `.kicad_dru` the rules).
fn kicad_board(name: &str) -> RoutingBoard {
    let path = repo_root().join(DIR).join(format!("{name}.kicad_pcb"));
    let text = std::fs::read(&path).unwrap();
    // As the CLI's route reads it (K13.1): the `.kicad_dru` applied.
    let options = ReadOptions {
        apply_design_rules: true,
        ..ReadOptions::default()
    };
    let read = read_kicad_bytes(&path, &text, options).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    // After the reader's own sources (the `.kicad_pro`'s Board Setup).
    input.extra_sources.push((
        Source::Testing,
        SettingsPatch {
            profile: Some("enhanced".to_string()),
            ..SettingsPatch::default()
        },
    ));
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

/// The pieces of GND's F.Cu pour in the cleanup's fill model.
fn model(board: &RoutingBoard) -> Vec<Shape> {
    model_on(board, 0)
}

/// The pieces of GND's pour on board layer `layer` in the cleanup's fill
/// model.
fn model_on(board: &RoutingBoard, layer: i32) -> Vec<Shape> {
    let net = board.rules.nets.get_by_name("GND", 1).unwrap().net_number;
    let pour = board
        .items()
        .find(|i| i.is_conduction_area() && i.contains_net(net) && i.first_layer() == layer)
        .unwrap();
    let pieces = PourFills::new().pieces(board, pour).unwrap();
    pieces.iter().map(|(_, shape)| shape.clone()).collect()
}

/// The point (`x`, `y`) mm, KiCad's y down, in board units.
fn at(x: f64, y: f64) -> FloatPoint {
    FloatPoint::new(x * 10_000.0, -y * 10_000.0)
}

fn holds(pieces: &[Shape], x: f64, y: f64) -> bool {
    pieces.iter().any(|s| s.contains_float(at(x, y)))
}

/// The point at `r` mm from the hole's centre towards (`x`, `y`).
fn towards(x: f64, y: f64, r: f64) -> (f64, f64) {
    let (cx, cy) = HOLE;
    let d = (x - cx).hypot(y - cy);
    (cx + (x - cx) / d * r, cy + (y - cy) / d * r)
}

/// Round the netless NPTHs the model holds every vertex of KiCad's arc (10 µm
/// outwards) and nothing 3.19 mm from the centre, inside the 1.7 mm clearance
/// KiCad keeps.
fn keeps_the_npths_clearance(board: &RoutingBoard) {
    keeps_the_npths_clearance_on(board, 0);
}

/// [`keeps_the_npths_clearance`] on board layer `layer`.
fn keeps_the_npths_clearance_on(board: &RoutingBoard, layer: i32) {
    let pieces = model_on(board, layer);
    for (x, y) in KICAD_NPTH_ARC {
        let r = (x - HOLE.0).hypot(y - HOLE.1);
        let (px, py) = towards(x, y, r + 0.01);
        assert!(
            holds(&pieces, px, py),
            "layer {layer}: KiCad's fill at ({px:.4}, {py:.4}) mm is outside the model"
        );
        let (qx, qy) = towards(x, y, 3.19);
        assert!(
            !holds(&pieces, qx, qy),
            "layer {layer}: ({qx:.4}, {qy:.4}) mm, inside the NPTH's clearance, is in the model"
        );
    }
}

/// Round the GND pad the model holds every vertex of KiCad's thermal ring (10
/// µm outwards).
fn joins_the_own_net_pad(board: &RoutingBoard) {
    let pieces = model(board);
    for (x, y) in KICAD_PAD_RING {
        let r = (x - HOLE.0).hypot(y - HOLE.1);
        let (px, py) = towards(x, y, r + 0.01);
        assert!(
            holds(&pieces, px, py),
            "KiCad's fill at ({px:.4}, {py:.4}) mm is outside the model"
        );
    }
}

/// Along the straight edges the model holds KiCad's fill (`bottom` and
/// `right`, 10 µm inwards, every 0.25 mm) and nothing `inside` mm from the
/// edge, within the clearance KiCad keeps.
fn keeps_the_edge_clearance(
    board: &RoutingBoard,
    bottom: (f64, (f64, f64)),
    right: (f64, (f64, f64)),
    inside: f64,
) {
    let pieces = model(board);
    let (y, (x0, x1)) = bottom;
    let mut x = x0 + 0.05;
    while x < x1 {
        assert!(
            holds(&pieces, x, y - 0.01),
            "KiCad's fill at ({x:.4}, {:.4}) mm is outside the model",
            y - 0.01
        );
        let q = EDGE_BOTTOM_Y - inside;
        assert!(
            !holds(&pieces, x, q),
            "({x:.4}, {q:.4}) mm, inside the edge clearance, is in the model"
        );
        x += 0.25;
    }
    let (x, (y0, y1)) = right;
    let mut y = y0 + 0.05;
    while y < y1 {
        assert!(
            holds(&pieces, x - 0.01, y),
            "KiCad's fill at ({:.4}, {y:.4}) mm is outside the model",
            x - 0.01
        );
        let q = EDGE_RIGHT_X - inside;
        assert!(
            !holds(&pieces, q, y),
            "({q:.4}, {y:.4}) mm, inside the edge clearance, is in the model"
        );
        y += 0.25;
    }
}

/// RED before K15.40: the DSN model cut the plugin's circles at their outline.
#[test]
fn the_dsn_model_keeps_a_netless_npths_own_clearance() {
    keeps_the_npths_clearance(&dsn_board(NPTH, None));
}

#[test]
fn the_kicad_pcb_model_keeps_a_netless_npths_own_clearance() {
    keeps_the_npths_clearance(&kicad_board(NPTH));
}

/// RED before K15.40: the circle cut the pad out of its own net's pour.
#[test]
fn the_dsn_model_joins_a_pad_of_the_pours_net() {
    joins_the_own_net_pad(&dsn_board(PAD, None));
}

/// RED before K15.40, as on the DSN.
#[test]
fn the_kicad_pcb_model_joins_a_pad_of_the_pours_net() {
    joins_the_own_net_pad(&kicad_board(PAD));
}

/// RED before K15.40: the model reached the plugin's edge strips.
#[test]
fn the_dsn_model_keeps_the_edge_clearance() {
    keeps_the_edge_clearance(&dsn_board(NPTH, None), KICAD_BOTTOM, KICAD_RIGHT, 0.36);
}

/// RED before K15.40: the model stopped 11 µm short of KiCad's fill.
#[test]
fn the_kicad_pcb_model_keeps_the_edge_clearance() {
    keeps_the_edge_clearance(&kicad_board(NPTH), KICAD_BOTTOM, KICAD_RIGHT, 0.36);
}

/// RED before K15.40: the model kept the router's 0.372 mm (Board Setup's)
/// where the `.kicad_dru`'s rule gives KiCad's fill 0.254 mm.
#[test]
fn the_kicad_pcb_model_takes_an_edge_rule_below_board_setup() {
    keeps_the_edge_clearance(&kicad_board(RULE), RULE_BOTTOM, RULE_RIGHT, 0.23);
}

/// The same on the DSN (the export is `a-netless-npth`'s; the plugin passes
/// the board's `.kicad_dru`). Not RED before K15.40: the model reached the
/// plugin's edge strips (0.242 mm), wider than KiCad's fill; the outline's
/// own 0.372 mm alone would be narrower.
#[test]
fn the_dsn_model_takes_an_edge_rule_below_board_setup() {
    keeps_the_edge_clearance(&dsn_board(NPTH, Some(RULE)), RULE_BOTTOM, RULE_RIGHT, 0.23);
}

/// K15.45, RED before it: on the inner layers, where neither the plugin nor
/// the reader writes a circle, the DSN model kept only the NPTH's hole
/// clearance (the export's 3.4 mm keepout).
#[test]
fn the_dsn_model_keeps_a_netless_npths_own_clearance_on_every_layer() {
    let board = dsn_board(INNER, None);
    for layer in GND_LAYERS {
        keeps_the_npths_clearance_on(&board, layer);
    }
}

/// K15.45, RED before it, as on the DSN (the reader's hole keepout).
#[test]
fn the_kicad_pcb_model_keeps_a_netless_npths_own_clearance_on_every_layer() {
    let board = kicad_board(INNER);
    for layer in GND_LAYERS {
        keeps_the_npths_clearance_on(&board, layer);
    }
}

/// K15.45, RED before it: a footprint's own clearance below the net classes'
/// (Module302's 0.127 mm against the 90ohm class's 0.2 mm) overrides them in
/// KiCad's filler, so the `.kicad_pcb` model reaches KiCad's fill edge (10 µm
/// short of it) and holds nothing 0.12 mm from a signal pad.
#[test]
fn the_kicad_pcb_model_takes_a_footprints_clearance_below_the_classes() {
    let board = kicad_board(OVERRIDE);
    let pieces = model_on(&board, 1);
    for y in OVERRIDE_PADS_Y {
        let x = KICAD_OVERRIDE_EDGE_X - 0.01;
        assert!(
            holds(&pieces, x, y),
            "KiCad's fill at ({x:.4}, {y:.4}) mm is outside the model"
        );
        let q = OVERRIDE_PAD_END_X - 0.12;
        assert!(
            !holds(&pieces, q, y),
            "({q:.4}, {y:.4}) mm, inside the pad's clearance, is in the model"
        );
    }
}

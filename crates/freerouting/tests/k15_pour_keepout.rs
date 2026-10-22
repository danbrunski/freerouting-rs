//! K15.35: the cleanup's fill model of a DSN input's pour takes the board's
//! keepouts as KiCad's zone filler does.
//!
//! The boards are `tests/data/kicad/pcu-k1535/`, written by `make_boards.py`
//! with every coordinate taken from the pcu board's DSN route (K15.34, main
//! `af0c0de`, session `6a90062c...`), where the fill model lay 0.127 mm
//! short of KiCad's refilled fill along five footprint rule areas and an
//! NPTH. Each board is there as KiCad 10.0.7's `pcb drc --refill-zones
//! --save-board` saved it (with the pcu board's `.kicad_dru`) and as KiCad's
//! Specctra export of that (`.dsn`):
//!
//! - `a-l1-keepout`: zone BUCK_SW (`Net-(U1-SW)`) and L1 with its own rule
//!   area (tracks, vias and copper pour not allowed). KiCad fills BUCK_SW up
//!   to the rule area's outline, x 200.6472 mm, with no clearance
//!   (`ZONE_FILLER::buildCopperItemClearances`: "Keepouts use outline with no
//!   clearance"); the model kept the 0.127 mm floor from the export's
//!   `keepout`;
//! - `b-f1-npth`: zone V_PD_IN (`/V_PD_IN`) round F1's NPTH (2.794 mm at
//!   (97.0, 124.1) mm). KiCad fills up to 1.6566 mm from its centre (the
//!   `.kicad_dru`'s 0.254 mm hole clearance); the export writes the NPTH as a
//!   keepout circle already grown by the board's 0.25 mm hole clearance
//!   (3.294 mm), and the model kept the floor from that too (1.774 mm).

mod autoroute_common;

use autoroute_common::repo_root;
use freerouting::board::RoutingBoard;
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::quality::pour_fill::PourFills;
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{FloatPoint, Shape};

const DIR: &str = "crates/freerouting/tests/data/kicad/pcu-k1535";
const L1: &str = "a-l1-keepout";
const F1: &str = "b-f1-npth";
const SW: &str = "Net-(U1-SW)";
const PD: &str = "/V_PD_IN";

/// KiCad's fill of BUCK_SW along L1's rule area on `a-l1-keepout.kicad_pcb`:
/// the edge from (200.6472, 113.9) to (200.6472, 126.6) mm.
const RULE_AREA_X: f64 = 200.6472;
const RULE_AREA_Y: (f64, f64) = (113.9, 126.6);

/// F1's NPTH centre (mm, KiCad's y down).
const NPTH: (f64, f64) = (97.0, 124.1);

/// KiCad's fill of V_PD_IN round F1's NPTH on `b-f1-npth.kicad_pcb`: the
/// vertices of the arc 1.6566 mm from the NPTH's centre (mm, y down).
const KICAD_ARC: [(f64, f64); 32] = [
    (95.92412, 122.8403),
    (95.7403, 123.02412),
    (95.58751, 123.23442),
    (95.46949, 123.46604),
    (95.38916, 123.71328),
    (95.3485, 123.97001),
    (95.3485, 124.22997),
    (95.38916, 124.48672),
    (95.46949, 124.73395),
    (95.58751, 124.96558),
    (95.7403, 125.17587),
    (95.92412, 125.35969),
    (96.13442, 125.51249),
    (96.36604, 125.6305),
    (96.61327, 125.71084),
    (96.87002, 125.7515),
    (97.12997, 125.7515),
    (97.38673, 125.71084),
    (97.63396, 125.6305),
    (97.86557, 125.51249),
    (98.07588, 125.35969),
    (98.25969, 125.17588),
    (98.41249, 124.96558),
    (98.5305, 124.73396),
    (98.61083, 124.48673),
    (98.6515, 124.22998),
    (98.6515, 123.97002),
    (98.61083, 123.71327),
    (98.5305, 123.46604),
    (98.41249, 123.23443),
    (98.2597, 123.02412),
    (98.07588, 122.84031),
];

/// The DSN board `name`, told what the KiCad plugin passes on the pcu board:
/// the zones' 500 µm thickness, the `.kicad_dru`'s 127 µm clearance, island
/// removal on, BUCK_SW's net keeping its islands.
fn dsn_board(name: &str) -> RoutingBoard {
    let text = std::fs::read(repo_root().join(DIR).join(format!("{name}.dsn"))).unwrap();
    let mut input = JobInput::new(text, name);
    let mut patch = SettingsPatch {
        profile: Some("enhanced".to_string()),
        ..SettingsPatch::default()
    };
    patch.cleanup.pour_min_thickness_um = Some(500.0);
    patch.cleanup.pour_clearance_um = Some(127.0);
    patch.cleanup.pour_island_removal = Some(true);
    patch.cleanup.pour_islands_kept_nets = Some(vec![SW.to_string()]);
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

/// Net `net`'s F.Cu pour on `board`.
fn pour<'b>(board: &'b RoutingBoard, net: &str) -> &'b freerouting::board::items::Item {
    let net = board.rules.nets.get_by_name(net, 1).unwrap().net_number;
    board
        .items()
        .find(|i| i.is_conduction_area() && i.contains_net(net))
        .unwrap()
}

/// The pieces of net `net`'s pour in the cleanup's fill model.
fn model(board: &RoutingBoard, net: &str) -> Vec<Shape> {
    let pieces = PourFills::new().pieces(board, pour(board, net)).unwrap();
    pieces.iter().map(|(_, shape)| shape.clone()).collect()
}

/// The point (`x`, `y`) mm, KiCad's y down, in board units.
fn at(x: f64, y: f64) -> FloatPoint {
    FloatPoint::new(x * 10_000.0, -y * 10_000.0)
}

fn holds(pieces: &[Shape], x: f64, y: f64) -> bool {
    pieces.iter().any(|s| s.contains_float(at(x, y)))
}

/// RED before K15.35: the model kept 0.127 mm from the rule area. Now it
/// covers KiCad's fill along the rule area's outline (10 µm inwards, every
/// 0.5 mm) and stops at the outline (10 µm into the rule area is no copper).
#[test]
fn the_model_reaches_a_rule_area_as_kicads_fill_does() {
    let board = dsn_board(L1);
    let pieces = model(&board, SW);
    let (y0, y1) = RULE_AREA_Y;
    let mut y = y0 + 0.1;
    while y < y1 - 0.1 {
        assert!(
            holds(&pieces, RULE_AREA_X - 0.01, y),
            "KiCad's fill at ({:.4}, {y:.4}) mm is outside the model",
            RULE_AREA_X - 0.01
        );
        assert!(
            !holds(&pieces, RULE_AREA_X + 0.01, y),
            "inside the rule area at y {y:.4}"
        );
        y += 0.5;
    }
}

/// RED before K15.35: the model kept 0.127 mm from the export's NPTH
/// keepout, 0.117 mm short of KiCad's fill. Now it covers every vertex of
/// KiCad's arc (10 µm outwards from the NPTH), and nothing within the hole's
/// clearance (1.5 mm from the centre).
#[test]
fn the_model_reaches_an_npth_as_kicads_fill_does() {
    let board = dsn_board(F1);
    let pieces = model(&board, PD);
    let (cx, cy) = NPTH;
    for (x, y) in KICAD_ARC {
        let d = (x - cx).hypot(y - cy);
        assert!(
            (d - 1.6566).abs() < 0.001,
            "({x}, {y}) is {d} mm from the NPTH"
        );
        let (px, py) = (x + (x - cx) / d * 0.01, y + (y - cy) / d * 0.01);
        assert!(
            holds(&pieces, px, py),
            "KiCad's fill at ({px:.4}, {py:.4}) mm is outside the model"
        );
        let (qx, qy) = (cx + (x - cx) / d * 1.5, cy + (y - cy) / d * 1.5);
        assert!(
            !holds(&pieces, qx, qy),
            "({qx:.4}, {qy:.4}) mm is in the model"
        );
    }
}

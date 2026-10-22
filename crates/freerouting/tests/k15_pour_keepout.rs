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
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, KicadSources, prepare_job};
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

/// K15.38: the `.kicad_pcb` board `name` (each zone takes its own
/// min_thickness and the router's clearances; the `.kicad_dru` beside it).
fn kicad_board(name: &str) -> RoutingBoard {
    prepare_job(&kicad_input(name), &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

/// The job input of the `.kicad_pcb` board `name`: the reader's sources
/// (`JobInput::set_kicad`) and the `enhanced` profile.
fn kicad_input(name: &str) -> JobInput {
    let path = repo_root().join(DIR).join(format!("{name}.kicad_pcb"));
    let text = std::fs::read(&path).unwrap();
    let read = read_kicad_bytes(&path, &text, ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    input.extra_sources.push((
        Source::Testing,
        SettingsPatch {
            profile: Some("enhanced".to_string()),
            ..SettingsPatch::default()
        },
    ));
    input
}

/// K15.45: the `.kicad_pcb` boards keep the sources the reader adds
/// (`KicadSources::from_read`: the pads' solder-mask apertures, Board Setup's
/// minimums), with the test's profile after them.
#[test]
fn the_kicad_pcb_boards_keep_the_readers_sources() {
    for name in [L1, F1] {
        let path = repo_root().join(DIR).join(format!("{name}.kicad_pcb"));
        let text = std::fs::read(&path).unwrap();
        let read = read_kicad_bytes(&path, &text, ReadOptions::default()).unwrap();
        let reader: Vec<Source> = KicadSources::from_read(&read)
            .extra_sources
            .into_iter()
            .map(|(source, _)| source)
            .collect();
        assert!(!reader.is_empty(), "{name}: the reader adds no source");
        let kept: Vec<Source> = kicad_input(name)
            .extra_sources
            .into_iter()
            .map(|(source, _)| source)
            .collect();
        let mut expected = reader;
        expected.push(Source::Testing);
        assert_eq!(kept, expected, "{name}");
    }
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

/// The model of BUCK_SW on `board` covers KiCad's fill along L1's rule
/// area's outline (10 µm inwards, every 0.5 mm) and stops at the outline (10
/// µm into the rule area is no copper).
fn reaches_the_rule_area(board: &RoutingBoard) {
    let pieces = model(board, SW);
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

/// The model of V_PD_IN on `board` covers every vertex of KiCad's arc (10 µm
/// outwards from the NPTH), and nothing within the hole's clearance (1.5 mm
/// from the centre).
fn reaches_the_npth(board: &RoutingBoard) {
    let pieces = model(board, PD);
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

/// RED before K15.35: the model kept 0.127 mm from the rule area.
#[test]
fn the_model_reaches_a_rule_area_as_kicads_fill_does() {
    reaches_the_rule_area(&dsn_board(L1));
}

/// RED before K15.35: the model kept 0.127 mm from the export's NPTH
/// keepout, 0.117 mm short of KiCad's fill.
#[test]
fn the_model_reaches_an_npth_as_kicads_fill_does() {
    reaches_the_npth(&dsn_board(F1));
}

/// K15.38, RED before: the `.kicad_pcb` model kept the router's clearance
/// from the reader's keepout of the rule area.
#[test]
fn the_kicad_pcb_model_reaches_a_rule_area_as_kicads_fill_does() {
    reaches_the_rule_area(&kicad_board(L1));
}

/// K15.38, RED before: the `.kicad_pcb` model kept the router's clearance
/// from the reader's NPTH keepout (the hole grown by the hole clearance).
#[test]
fn the_kicad_pcb_model_reaches_an_npth_as_kicads_fill_does() {
    reaches_the_npth(&kicad_board(F1));
}

/// K15.38: `tests/data/kicad/k1538-pour-allowed/` (`make_boards.py`; refilled
/// by KiCad 10.0.7): zone POUR on F.Cu and three rule areas, R1 (the
/// board's, tracks and vias banned, pour allowed), R2 (footprint U1's,
/// rotated 90 degrees, tracks banned, vias and pour allowed) and R3 (the
/// board's, tracks, vias and pour banned).
const POUR_DIR: &str = "crates/freerouting/tests/data/kicad/k1538-pour-allowed";

/// True if KiCad's fill of `fill` holds the point (`x`, `y`) mm, KiCad's y
/// down (even-odd over each island's polygon, which KiCad fractures into one
/// outline).
fn kicad_fill_holds(fill: &freerouting_kicad::zone_fills::ZoneFill, x: f64, y: f64) -> bool {
    let (px, py) = (x * 1000.0, -y * 1000.0);
    fill.islands.iter().any(|ring| {
        let mut inside = false;
        for (i, a) in ring.iter().enumerate() {
            let b = &ring[(i + 1) % ring.len()];
            if (a.y > py) != (b.y > py) && px < a.x + (py - a.y) / (b.y - a.y) * (b.x - a.x) {
                inside = !inside;
            }
        }
        inside
    })
}

/// RED before K15.38: the model kept the router's clearance from R1's and
/// R2's keepouts, which KiCad's fill passes through. Now it holds every
/// point of KiCad's fill inside them (10 µm in from each side, and a grid
/// across), and R3, which bans the pour, cuts it at the outline as KiCad
/// does (10 µm in: no copper in either; 10 µm out: copper in both).
#[test]
fn a_rule_area_that_allows_the_pour_does_not_cut_the_kicad_pcb_model() {
    let path = repo_root().join(POUR_DIR).join("c-pour-allowed.kicad_pcb");
    let text = std::fs::read(&path).unwrap();
    let read = read_kicad_bytes(&path, &text, ReadOptions::default()).unwrap();
    let fill = read.extras.zone_fills[0].clone();
    assert_eq!(fill.net, "POUR");
    let flags: Vec<bool> = read
        .extras
        .rule_areas
        .iter()
        .map(|a| a.bans_copper_pour)
        .collect();
    assert_eq!(flags, [false, false, true], "U1's R2, then R1 and R3");
    let boxes: Vec<[f64; 4]> = read.extras.rule_areas.iter().map(|a| a.bbox).collect();
    assert_eq!(
        boxes,
        [
            [17000.0, -15000.0, 23000.0, -5000.0],
            [5000.0, -15000.0, 11000.0, -5000.0],
            [14000.0, -16000.0, 16000.0, -4000.0],
        ],
        "DSN µm, y up"
    );
    let mut input = JobInput::new(Vec::new(), "c-pour-allowed");
    input.set_kicad(read);
    input.extra_sources.push((
        Source::Testing,
        SettingsPatch {
            profile: Some("enhanced".to_string()),
            ..SettingsPatch::default()
        },
    ));
    let board = prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board;
    let pieces = model(&board, "POUR");
    let mut points = Vec::new();
    for (x0, x1) in [(5.0, 11.0), (17.0, 23.0)] {
        for x in [x0 + 0.01, x0 + 1.0, (x0 + x1) / 2.0, x1 - 1.0, x1 - 0.01] {
            for y in [5.01, 7.0, 10.0, 13.0, 14.99] {
                points.push((x, y, true));
            }
        }
    }
    for y in [4.5, 7.0, 10.0, 13.0, 15.5] {
        points.extend([
            (13.99, y, true),
            (14.01, y, false),
            (15.0, y, false),
            (15.99, y, false),
            (16.01, y, true),
        ]);
    }
    for (x, y, copper) in points {
        assert_eq!(
            kicad_fill_holds(&fill, x, y),
            copper,
            "KiCad's fill at ({x}, {y}) mm"
        );
        assert_eq!(holds(&pieces, x, y), copper, "the model at ({x}, {y}) mm");
    }
}

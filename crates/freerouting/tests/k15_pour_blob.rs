//! K15.33: a pour's fill model drops the blobs KiCad's filler deletes.
//!
//! The boards are `tests/data/kicad/pcu-k1533/`, written by `make_boards.py`
//! with every coordinate of the zone and the copper taken from the pcu
//! board's DSN route (K15.32, `target/orchestrator/k1532/runs/pcu-dsn-off-dflt`,
//! session `aac8151f...`), where KiCad reports `via_dangling` for `/CH5_NO`'s
//! via at (112.9114, 92.1902) mm: zone EFUSE_NO_5 (F.Cu, min_thickness 0.5
//! mm), R11's pad, the via and its B.Cu track, and `SCL`'s and `VCC_3V3`'s
//! routed F.Cu tracks. Each board is there as KiCad 10.0.7's `pcb drc
//! --refill-zones --save-board` saved it (with the pcu board's `.kicad_dru`,
//! whose 0.127 mm clearance KiCad's filler applies), and as KiCad's Specctra
//! export of that (`.dsn`; the export drops the zone's min_thickness, so the
//! two DSNs are the same board).
//!
//! `SCL`'s track cuts off the zone's corner with the via, a triangle of 0.65
//! mm2. Opened by 0.249 mm it leaves a blob 0.29 mm across, and KiCad's
//! filler deletes a deflated polygon smaller than min_thickness on both axes
//! (`ZONE_FILLER::fillCopperZone`):
//!
//! - `a-corner-cut-off` (min_thickness 0.5 mm): KiCad fills nothing at the
//!   via, `via_dangling` "Via [/CH5_NO] on F.Cu - B.Cu";
//! - `b-thin-zone` (min_thickness 0.4 mm): the blob is 0.47 mm across, KiCad
//!   keeps the triangle (0.538 mm2) and the via is connected.

mod autoroute_common;

use std::collections::BTreeSet;

use autoroute_common::repo_root;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::quality::pour_fill::PourFills;
use freerouting::quality::stub_contacts::{via_contact_layers, via_contact_layers_with};
use freerouting::quality::stubs::remove_stubs;
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{FloatPoint, IntPoint};

const DIR: &str = "crates/freerouting/tests/data/kicad/pcu-k1533";
const NO: &str = "/CH5_NO";
const CUT: &str = "a-corner-cut-off";
const THIN: &str = "b-thin-zone";

/// KiCad's fill of EFUSE_NO_5 on `b-thin-zone` around the via: the
/// triangle's 19 vertices (mm, KiCad's y down) as `b-thin-zone.kicad_pcb`
/// holds them.
const KICAD_TRIANGLE: [(f64, f64); 19] = [
    (113.0148, 91.6646),
    (113.0774, 91.7272),
    (113.1067, 91.8108),
    (113.1078, 91.8393),
    (113.0932, 92.3032),
    (113.0708, 92.3889),
    (113.0134, 92.4563),
    (112.9324, 92.4922),
    (112.8946, 92.4959),
    (112.4446, 92.4967),
    (112.3582, 92.4772),
    (112.2889, 92.4221),
    (112.2503, 92.3424),
    (112.2501, 92.2538),
    (112.2884, 92.1739),
    (112.3035, 92.157),
    (112.7682, 91.6923),
    (112.8432, 91.6452),
    (112.9312, 91.6353),
];

fn settings() -> SettingsPatch {
    SettingsPatch {
        profile: Some("enhanced".to_string()),
        ..SettingsPatch::default()
    }
}

fn prepared(input: JobInput) -> RoutingBoard {
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

/// The `.kicad_pcb` board `name`: each zone takes its own min_thickness.
fn kicad_board(name: &str) -> RoutingBoard {
    let path = repo_root().join(DIR).join(format!("{name}.kicad_pcb"));
    let text = std::fs::read(&path).unwrap();
    let read = read_kicad_bytes(&path, &text, ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    input.extra_sources = vec![(Source::Testing, settings())];
    prepared(input)
}

/// The DSN board `name`, told what the KiCad plugin passes: the zones' min
/// thickness `thickness_um`, the `.kicad_dru`'s 127 µm clearance, island
/// removal on.
fn dsn_board(name: &str, thickness_um: f64) -> RoutingBoard {
    dsn_board_with(name, |patch| {
        patch.cleanup.pour_min_thickness_um = Some(thickness_um);
    })
}

fn dsn_board_with(name: &str, tell: impl FnOnce(&mut SettingsPatch)) -> RoutingBoard {
    let text = std::fs::read(repo_root().join(DIR).join(format!("{name}.dsn"))).unwrap();
    let mut input = JobInput::new(text, name);
    let mut patch = settings();
    patch.cleanup.pour_clearance_um = Some(127.0);
    patch.cleanup.pour_island_removal = Some(true);
    tell(&mut patch);
    input.extra_sources = vec![(Source::Testing, patch)];
    prepared(input)
}

/// `/CH5_NO`'s via at (112.9114, 92.1902) mm (KiCad's DSN export rounds x
/// to 112.911).
fn the_via(board: &RoutingBoard) -> ItemId {
    let net = board.rules.nets.get_by_name(NO, 1).unwrap().net_number;
    let vias: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_via() && i.contains_net(net))
        .map(|i| i.id())
        .collect();
    assert_eq!(vias.len(), 1);
    let centre = board.item(vias[0]).unwrap().drill().unwrap().center;
    let expected = IntPoint::new(1_129_114, -921_902);
    assert!(
        (centre.x - expected.x).abs() <= 4 && (centre.y - expected.y).abs() <= 2,
        "{centre:?}"
    );
    vias[0]
}

/// The via's contact layers by the pour's outline (Q16.4) and by its fill
/// model as the cleanup takes it.
fn contact_layers(board: &RoutingBoard) -> (BTreeSet<i32>, BTreeSet<i32>) {
    let via = board.item(the_via(board)).unwrap();
    (
        via_contact_layers(board, via),
        via_contact_layers_with(board, via, Some(&PourFills::new())),
    )
}

/// F.Cu and B.Cu.
fn outer(board: &RoutingBoard) -> BTreeSet<i32> {
    let last = board.layer_structure.layers.len() as i32 - 1;
    BTreeSet::from([0, last])
}

/// The outline's contacts (both layers), and the fill model's: B.Cu only
/// when `reached` is false.
fn expected(board: &RoutingBoard, reached: bool) -> (BTreeSet<i32>, BTreeSet<i32>) {
    let both = outer(board);
    let fills = if reached {
        both.clone()
    } else {
        BTreeSet::from([*both.last().unwrap()])
    };
    (both, fills)
}

/// The cleanup on `board`, judging only the via (as the router's copper):
/// the vias it removes.
fn clean(mut board: RoutingBoard) -> usize {
    let via = the_via(&board);
    board.unfix(via);
    let keep: BTreeSet<ItemId> = board
        .items()
        .map(|i| i.id())
        .filter(|id| *id != via)
        .collect();
    let summary = remove_stubs(&mut board, &keep);
    assert_eq!(summary.traces, 0);
    assert_eq!(board.item(via).is_none(), summary.vias == 1);
    summary.vias
}

/// The pieces of `/CH5_NO`'s F.Cu pour in the fill model.
fn pour_pieces(board: &RoutingBoard) -> Vec<freerouting_geometry::Shape> {
    let net = board.rules.nets.get_by_name(NO, 1).unwrap().net_number;
    let area = board
        .items()
        .find(|i| i.is_conduction_area() && i.contains_net(net))
        .unwrap();
    let pieces = PourFills::new().pieces(board, area).unwrap();
    pieces.iter().map(|(_, shape)| shape.clone()).collect()
}

/// RED before K15.33: the fill model kept the opened triangle, so the via
/// touched it. On the DSN told the zone's 0.5 mm, the via is no contact on
/// F.Cu and the cleanup removes it, as KiCad reports it dangling.
#[test]
fn the_via_in_the_cut_off_corner_is_no_contact_on_the_dsn() {
    let board = dsn_board(CUT, 500.0);
    assert_eq!(contact_layers(&board), expected(&board, false));
    assert_eq!(clean(board), 1);
}

/// On the `.kicad_pcb` the zone's own min_thickness decides it: 0.5 mm drops
/// the blob, 0.4 mm keeps the triangle, as KiCad's fill of each.
#[test]
fn on_the_kicad_pcb_the_zones_own_thickness_decides() {
    let cut = kicad_board(CUT);
    assert_eq!(contact_layers(&cut), expected(&cut, false));
    assert_eq!(clean(cut), 1);

    let thin = kicad_board(THIN);
    assert_eq!(contact_layers(&thin), expected(&thin, true));
    assert_eq!(clean(thin), 0);
}

/// The near case: told 0.4 mm (the zone of `b-thin-zone`; the DSN is the
/// same board), the triangle stays a contact, and the model covers every
/// vertex of KiCad's triangle (moved 10 µm inwards): no narrower than
/// KiCad's fill there.
#[test]
fn told_400_um_the_triangle_stays_and_covers_kicads() {
    let strip = |name: &str| {
        let text =
            std::fs::read_to_string(repo_root().join(DIR).join(format!("{name}.dsn"))).unwrap();
        text.split_once('\n').unwrap().1.to_string()
    };
    assert_eq!(strip(CUT), strip(THIN));

    let board = dsn_board(THIN, 400.0);
    assert_eq!(contact_layers(&board), expected(&board, true));
    let pieces = pour_pieces(&board);
    let n = KICAD_TRIANGLE.len() as f64;
    let (cx, cy) = KICAD_TRIANGLE
        .iter()
        .fold((0.0, 0.0), |(x, y), (a, b)| (x + a / n, y + b / n));
    for (x, y) in KICAD_TRIANGLE {
        let d = (x - cx).hypot(y - cy);
        let (px, py) = (x - (x - cx) / d * 0.01, y - (y - cy) / d * 0.01);
        let p = FloatPoint::new(px * 10_000.0, -py * 10_000.0);
        assert!(
            pieces.iter().any(|s| s.contains_float(p)),
            "KiCad's fill at ({px:.4}, {py:.4}) mm is outside the model"
        );
    }
    assert_eq!(clean(board), 0);
}

/// The KiCad plugin passes the board's smallest thickness and each net
/// whose zones are thicker (`pour_min_thickness_nets`): the net's own value
/// decides, as each zone's own does on the `.kicad_pcb`.
#[test]
fn a_nets_own_thickness_wins_over_the_boards_smallest() {
    let own = |default_um: f64, entry: &str| {
        dsn_board_with(CUT, |patch| {
            patch.cleanup.pour_min_thickness_um = Some(default_um);
            patch.cleanup.pour_min_thickness_nets = Some(vec![entry.to_string()]);
        })
    };
    let thick = own(400.0, "500:/CH5_NO");
    assert_eq!(contact_layers(&thick), expected(&thick, false));
    assert_eq!(clean(thick), 1);

    let thin = own(500.0, "400:/CH5_NO");
    assert_eq!(contact_layers(&thin), expected(&thin, true));
    assert_eq!(clean(thin), 0);

    // Another net's entry leaves the default.
    let other = own(400.0, "500:SCL");
    assert_eq!(contact_layers(&other), expected(&other, true));
}

/// An entry the cleanup cannot read is left out with a warning, and its net
/// keeps the board's smallest thickness.
#[test]
fn an_unreadable_entry_is_left_out_with_a_warning() {
    let text = std::fs::read(repo_root().join(DIR).join(format!("{CUT}.dsn"))).unwrap();
    let mut input = JobInput::new(text, CUT);
    let mut patch = settings();
    patch.cleanup.pour_min_thickness_um = Some(400.0);
    patch.cleanup.pour_clearance_um = Some(127.0);
    patch.cleanup.pour_min_thickness_nets = Some(vec![
        "/CH5_NO".to_string(),
        "0:/CH5_NO".to_string(),
        "500:NO_SUCH_NET".to_string(),
    ]);
    input.extra_sources = vec![(Source::Testing, patch)];
    let job = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
    let warnings: Vec<&str> = job
        .log
        .iter()
        .map(|line| line.text.as_str())
        .filter(|text| text.contains("pour_min_thickness_nets"))
        .collect();
    assert_eq!(warnings.len(), 3, "{warnings:?}");
    let board = job.board;
    assert_eq!(contact_layers(&board), expected(&board, true));
}

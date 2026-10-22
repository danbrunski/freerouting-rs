//! K15.31: the router counts a `.kicad_pcb` signal pour's contacts where its
//! opened fill reaches them, each island of the fill on its own
//! (`router.pour_fill_contacts`).
//!
//! The boards are `tests/data/kicad/pcu-k1531/`, written by `make_boards.py`
//! with every coordinate of the zone and the copper taken from the pcu
//! board's `.kicad_pcb` route with `router.pour_fill_target=true` (K15.25,
//! `target/orchestrator/k1525/runs/b/final-true`, session `a1118e6c...`),
//! where KiCad reports `/CH2_NO`'s U4 pad 10 and pad 8 unconnected and zone
//! EFUSE_NO_2 unconnected to itself. Each board is there as KiCad 10.0.7's
//! `pcb drc --refill-zones --save-board` saved it:
//!
//! - `a-pads-in-three-islands`: `VCC_3V3`'s tracks cut EFUSE_NO_2's fill in
//!   three. KiCad: 2 unconnected items of `/CH2_NO` ("Zone 'EFUSE_NO_2' |
//!   Zone 'EFUSE_NO_2'" and "Track [/CH2_NO] ... (139.7159, 97.6731) |
//!   Track [/CH2_NO] ... (137.3428, 95.5735)", the tracks of U4 pads 10 and
//!   8);
//! - `b-no-vcc-cut`: without those tracks the fill is one piece. KiCad: no
//!   unconnected item of `/CH2_NO`.
//!
//! Java's contacts (`ConductionArea.getNormalContacts`, the outline) join
//! every `/CH2_NO` item on both boards.

mod autoroute_common;

use std::collections::BTreeSet;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::drc::NetIncompletes;
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{IntPoint, Point};

const DIR: &str = "crates/freerouting/tests/data/kicad/pcu-k1531";
const NO: &str = "/CH2_NO";
const CUT: &str = "a-pads-in-three-islands";
const WHOLE: &str = "b-no-vcc-cut";

/// A pad centre in board units (0.1 µm, y up) from KiCad's mm (y down).
const fn at(x: i32, y: i32) -> IntPoint {
    IntPoint { x, y: -y }
}

/// The `/CH2_NO` pads, by the island of EFUSE_NO_2's fill they are in on
/// `a-pads-in-three-islands` (U4's pads 8 and 10 lie outside the outline;
/// their tracks end in the top left and the right island).
const U4_8: IntPoint = at(1_373_428, 963_000);
const U4_10: IntPoint = at(1_383_428, 963_000);
const U17_18: IntPoint = at(1_332_250, 971_028);
const U17_19: IntPoint = at(1_338_750, 971_028);
const U17_20: IntPoint = at(1_345_250, 971_028);
const R85_2: IntPoint = at(1_331_553, 998_993);
const C54_1: IntPoint = at(1_336_678, 1_025_993);
const R8_1: IntPoint = at(1_363_303, 1_020_493);

/// Board `name` read and prepared under `enhanced`, with
/// `router.pour_fill_contacts` and `router.pour_fill_target` as given, and
/// without `/CH2_NO`'s tracks unless `with_tracks`.
fn input(name: &str, contacts: Option<bool>, target: Option<bool>, with_tracks: bool) -> JobInput {
    let path = repo_root().join(DIR).join(format!("{name}.kicad_pcb"));
    let text = std::fs::read_to_string(&path).unwrap();
    let text = if with_tracks {
        text
    } else {
        without_tracks_of(&text, NO)
    };
    let read = read_kicad_bytes(&path, text.as_bytes(), ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    let mut patch = SettingsPatch {
        profile: Some("enhanced".to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    patch.pour_fill_contacts = contacts;
    patch.pour_fill_target = target;
    input.extra_sources = vec![(Source::Testing, patch)];
    input
}

/// `text` without its top-level `(segment ...)` lists of net `net`.
fn without_tracks_of(text: &str, net: &str) -> String {
    let net = format!("(net \"{net}\")");
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("\n\t(segment") {
        let end = at + rest[at..].find("\n\t)").map(|e| e + 3).unwrap();
        let end = if rest[at..end].contains("\n\t\t") {
            end
        } else {
            // A one-line segment (this script's format).
            at + 1 + rest[at + 1..].find('\n').unwrap()
        };
        out.push_str(&rest[..at]);
        if !rest[at..end].contains(&net) {
            out.push_str(&rest[at..end]);
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

fn loaded(name: &str, contacts: Option<bool>) -> RoutingBoard {
    prepare_job(
        &input(name, contacts, None, true),
        &ResolveContext { cores: 1 },
    )
    .unwrap()
    .board
}

fn routed(name: &str, contacts: Option<bool>, target: Option<bool>) -> RoutingBoard {
    let prepared = prepare_job(
        &input(name, contacts, target, false),
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

fn net(board: &RoutingBoard) -> i32 {
    board.rules.nets.get_by_name(NO, 1).unwrap().net_number
}

/// The `/CH2_NO` pin centred at `p`.
fn pin(board: &RoutingBoard, p: IntPoint) -> ItemId {
    let n = net(board);
    board
        .items()
        .find(|i| i.is_pin() && i.contains_net(n) && i.drill().is_some_and(|d| d.center == p))
        .map(|i| i.id())
        .unwrap_or_else(|| panic!("no /CH2_NO pin at {p:?}"))
}

/// The centres of the `/CH2_NO` pins connected to the pin at `p`.
fn joined(board: &RoutingBoard, p: IntPoint) -> BTreeSet<(i32, i32)> {
    let n = net(board);
    board
        .connected_set(pin(board, p), n, false)
        .into_iter()
        .filter_map(|id| board.item(id).filter(|i| i.is_pin()))
        .filter_map(|i| i.drill().map(|d| (d.center.x, d.center.y)))
        .collect()
}

fn centres(points: &[IntPoint]) -> BTreeSet<(i32, i32)> {
    points.iter().map(|p| (p.x, p.y)).collect()
}

/// `/CH2_NO`'s incomplete connections, recounted from scratch and from the
/// routing ledger.
fn incompletes(board: &RoutingBoard) -> (usize, usize) {
    let n = net(board);
    let items = board.connectable_items(n);
    let counted = NetIncompletes::new(board, n, &items).count();
    let ledger = board.routing_ledger().net_incomplete_count(board, n);
    (counted, ledger)
}

const ALL: [IntPoint; 8] = [U4_8, U4_10, U17_18, U17_19, U17_20, R85_2, C54_1, R8_1];

/// Java's contacts join all of `/CH2_NO` through the outline on both boards;
/// with the setting the cut board has three groups (KiCad's 2 unconnected
/// items) and the whole one is one.
#[test]
fn a_pad_the_fill_does_not_reach_is_not_joined_through_the_pour() {
    for name in [CUT, WHOLE] {
        for contacts in [None, Some(false)] {
            let board = loaded(name, contacts);
            assert_eq!(incompletes(&board), (0, 0), "{name} {contacts:?}");
            assert_eq!(joined(&board, U4_8), centres(&ALL), "{name} {contacts:?}");
        }
    }
    let board = loaded(WHOLE, Some(true));
    assert_eq!(incompletes(&board), (0, 0));
    assert_eq!(joined(&board, U4_8), centres(&ALL));

    let board = loaded(CUT, Some(true));
    assert_eq!(incompletes(&board), (2, 2));
    let top_left = centres(&[U4_8, U17_18, U17_19, U17_20, R85_2]);
    assert_eq!(joined(&board, U4_8), top_left);
    assert_eq!(joined(&board, R85_2), top_left);
    assert_eq!(joined(&board, C54_1), centres(&[C54_1]));
    assert_eq!(joined(&board, U4_10), centres(&[U4_10, R8_1]));
}

/// The `/CH2_NO` traces and vias of `board`, as (layer, first, last) and
/// via centres.
fn copper(board: &RoutingBoard) -> (Vec<(i32, Point, Point)>, Vec<IntPoint>) {
    let n = net(board);
    let traces = board
        .items()
        .filter(|i| i.contains_net(n))
        .filter_map(|i| i.as_trace())
        .map(|t| (t.layer, t.first_corner(), t.last_corner()))
        .collect();
    let vias = board
        .items()
        .filter(|i| i.is_via() && i.contains_net(n))
        .filter_map(|i| i.drill().map(|d| d.center))
        .collect();
    (traces, vias)
}

/// `a-pads-in-three-islands` without `/CH2_NO`'s tracks, routed under
/// `enhanced` (3 passes). Unset or `false`, the router joins U4's pads 8 and
/// 10 to the outline and counts the net done (KiCad, refilling the routed
/// board: 2 unconnected `/CH2_NO` items, as on the pcu board). With
/// `pour_fill_contacts` alone it routes the same copper and counts the 2
/// open: a search between two islands starts or ends in the area's outline,
/// which holds both. With `pour_fill_target` too the search reaches the
/// area in its main island's core, and the router joins the three islands
/// and U4's pads (0 incomplete; KiCad on the routed board: no `/CH2_NO`
/// item, K15.31's note).
#[test]
fn with_the_setting_the_router_routes_the_islands_together_or_counts_them_open() {
    let java = routed(CUT, None, None);
    assert_eq!(incompletes(&java), (0, 0));
    assert_eq!(copper(&routed(CUT, Some(false), None)), copper(&java));

    let alone = routed(CUT, Some(true), None);
    assert_eq!(incompletes(&alone), (2, 2));
    assert_eq!(copper(&alone), copper(&java));

    let both = routed(CUT, Some(true), Some(true));
    assert_eq!(incompletes(&both), (0, 0));
    assert_eq!(joined(&both, U4_8), centres(&ALL));
    let (traces, vias) = copper(&both);
    assert!(traces.len() > copper(&java).0.len(), "{traces:?}");
    assert!(!vias.is_empty());
}

/// The islands follow the board: inserting `VCC_3V3`'s tracks inside the zone
/// (`make_boards.py`'s `CUT`, as routed copper) into `b-no-vcc-cut` cuts the
/// fill in three, and the routing ledger, built before, recounts `/CH2_NO`
/// to 2, as on `a-pads-in-three-islands`.
#[test]
fn the_islands_are_recomputed_when_the_board_changes() {
    let mut board = loaded(WHOLE, Some(true));
    assert_eq!(incompletes(&board), (0, 0));
    let n = net(&board);
    let area = board
        .items()
        .find(|i| i.is_conduction_area() && i.contains_net(n))
        .map(|i| i.id())
        .unwrap();
    assert_eq!(board.pour_islands(area).unwrap().island_count(), 1);
    let vcc = board
        .rules
        .nets
        .get_by_name("VCC_3V3", 1)
        .unwrap()
        .net_number;
    let mm = |x: f64, y: f64| {
        Point::Int(IntPoint::new(
            (x * 1e4).round() as i32,
            (-y * 1e4).round() as i32,
        ))
    };
    for corners in [
        vec![
            mm(136.9928, 98.6493),
            mm(134.7427, 100.8994),
            mm(134.7427, 102.9618),
            mm(135.8134, 104.0325),
            mm(142.7738, 104.0325),
        ],
        vec![mm(129.4945, 101.4152), mm(134.7427, 101.4152)],
    ] {
        let polyline = freerouting_geometry::Polyline::from_points(&corners);
        board
            .insert_trace_without_cleaning(polyline, 0, 1000, &[vcc], 1, FixedState::Unfixed)
            .expect("the trace");
    }
    assert_eq!(board.pour_islands(area).unwrap().island_count(), 3);
    assert_eq!(incompletes(&board), (2, 2));
}

/// `make_boards.py`'s `CUT`: `VCC_3V3`'s tracks inside zone EFUSE_NO_2 of
/// the pcu route, in mm (KiCad, y down).
const VCC_CUT: [&[(f64, f64)]; 2] = [
    &[
        (136.9928, 98.6493),
        (134.7427, 100.8994),
        (134.7427, 102.9618),
        (135.8134, 104.0325),
        (142.7738, 104.0325),
    ],
    &[(129.4945, 101.4152), (134.7427, 101.4152)],
];

/// Inserts `VCC_3V3` track `k` of [`VCC_CUT`] into `board` as routed copper.
fn insert_vcc(board: &mut RoutingBoard, k: usize) -> ItemId {
    let vcc = board
        .rules
        .nets
        .get_by_name("VCC_3V3", 1)
        .unwrap()
        .net_number;
    let corners: Vec<Point> = VCC_CUT[k]
        .iter()
        .map(|(x, y)| {
            Point::Int(IntPoint::new(
                (x * 1e4).round() as i32,
                (-y * 1e4).round() as i32,
            ))
        })
        .collect();
    let polyline = freerouting_geometry::Polyline::from_points(&corners);
    board
        .insert_trace_without_cleaning(polyline, 0, 1000, &[vcc], 1, FixedState::Unfixed)
        .expect("the trace")
}

/// Asserts that `board`'s pour contacts, with what they kept from the
/// board's earlier states, give what they give computed from scratch on a
/// copy: every connected set of every net with a judged pour, the islands
/// of each pour, and each such net's incomplete count, recounted and from
/// the routing ledger.
fn assert_same_as_from_scratch(board: &RoutingBoard, step: &str) {
    let mut fresh = board.clone();
    fresh.set_pour_fill_contacts(false);
    fresh.set_pour_fill_contacts(true);
    let areas: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_conduction_area())
        .map(|i| i.id())
        .collect();
    // The zone's net is checked also when the zone is gone.
    let mut nets = BTreeSet::from([net(board)]);
    for area in &areas {
        let held = board.pour_islands(*area).map(|i| i.island_count());
        let now = fresh.pour_islands(*area).map(|i| i.island_count());
        assert_eq!(held, now, "{step}: islands of area {area:?}");
        if held.is_some() {
            nets.extend(board.item(*area).unwrap().net_numbers().iter().copied());
        }
    }
    for net in nets {
        for item in board.connectable_items(net) {
            assert_eq!(
                board.connected_set(item, net, false),
                fresh.connected_set(item, net, false),
                "{step}: net {net} item {item:?}"
            );
        }
        let items = board.connectable_items(net);
        let counted = NetIncompletes::new(&fresh, net, &items).count();
        let ledger = board.routing_ledger().net_incomplete_count(board, net);
        assert_eq!(ledger, counted, "{step}: net {net} incomplete");
    }
}

/// K15.41: the pour contacts keep each pour's last fill with its inputs,
/// its fill model's grid cells and its pieces and their shape tests, and
/// take what an item change left as it was. After each change of a
/// sequence on `b-no-vcc-cut` (the `VCC_3V3` track that cuts EFUSE_NO_2's
/// fill inserted; another inserted and removed with no query in between, so
/// the fill's inputs are as before; that one inserted; the zone removed
/// and the removal undone) they give the connected sets of a fill computed
/// from scratch, and the fills reuse work ([`RoutingBoard::pour_fill_work`]).
#[test]
fn the_kept_fills_give_the_connected_sets_of_fills_computed_from_scratch() {
    let mut board = loaded(WHOLE, Some(true));
    assert_same_as_from_scratch(&board, "loaded");
    insert_vcc(&mut board, 0);
    assert_same_as_from_scratch(&board, "first VCC track");
    let before = board.pour_fill_work();
    let second = insert_vcc(&mut board, 1);
    assert!(board.remove_item(second));
    assert_same_as_from_scratch(&board, "second track inserted and removed");
    let after = board.pour_fill_work();
    assert!(
        after.fills_reused > before.fills_reused,
        "{before:?} {after:?}"
    );
    insert_vcc(&mut board, 1);
    assert_same_as_from_scratch(&board, "second VCC track");
    let n = net(&board);
    let area = board
        .items()
        .find(|i| i.is_conduction_area() && i.contains_net(n))
        .map(|i| i.id())
        .unwrap();
    assert_eq!(board.pour_islands(area).unwrap().island_count(), 3);
    // A track's change re-fills the zone from its kept cells and pieces; a
    // zone's change forgets every fill.
    let work = board.pour_fill_work();
    assert!(work.pieces_reused > 0 && work.tests_reused > 0, "{work:?}");
    assert!(work.cells_reused > 0, "{work:?}");
    board.generate_snapshot();
    board.set_fixed_state(area, FixedState::Unfixed);
    assert!(board.remove_item(area));
    assert_same_as_from_scratch(&board, "zone removed");
    assert!(board.undo(None));
    assert_same_as_from_scratch(&board, "zone removal undone");
}

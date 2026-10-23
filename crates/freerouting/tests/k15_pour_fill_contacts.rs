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
//!
//! K15.46: a pad, a via or a track joins every island its copper meets, as
//! KiCad's connectivity does (`tests/data/kicad/k1546-shape-contacts/` and
//! moteus's U2 pad 2, `tests/data/kicad/moteus-k1542/h-pad-*`).

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
    input.extra_sources.push((Source::Testing, patch));
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

/// `router.pour_fill_target` is `false` (Java's), as it was unset before K15.51
/// put it in `enhanced`.
fn loaded(name: &str, contacts: Option<bool>) -> RoutingBoard {
    prepare_job(
        &input(name, contacts, Some(false), true),
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
/// items) and the whole one is one. Unset is `enhanced`'s `true` (K15.51).
#[test]
fn a_pad_the_fill_does_not_reach_is_not_joined_through_the_pour() {
    assert_eq!(incompletes(&loaded(CUT, None)), (2, 2));
    for name in [CUT, WHOLE] {
        let board = loaded(name, Some(false));
        assert_eq!(incompletes(&board), (0, 0), "{name}");
        assert_eq!(joined(&board, U4_8), centres(&ALL), "{name}");
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
/// `enhanced` (3 passes; both settings given, since `enhanced` sets both
/// `true`, K15.51). `false`, the router joins U4's pads 8 and
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
    let java = routed(CUT, Some(false), Some(false));
    assert_eq!(incompletes(&java), (0, 0));

    let alone = routed(CUT, Some(true), Some(false));
    assert_eq!(incompletes(&alone), (2, 2));
    assert_eq!(copper(&alone), copper(&java));

    let both = routed(CUT, Some(true), Some(true));
    assert_eq!(incompletes(&both), (0, 0));
    assert_eq!(joined(&both, U4_8), centres(&ALL));
    let (traces, vias) = copper(&both);
    assert!(traces.len() > copper(&java).0.len(), "{traces:?}");
    assert!(!vias.is_empty());
    // Unset, `enhanced` routes with both.
    assert_eq!(copper(&routed(CUT, None, None)), (traces, vias));
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
    let pins: Vec<Point> = board
        .items()
        .filter_map(|i| i.drill().map(|d| Point::Int(d.center)))
        .collect();
    for area in &areas {
        let held = board.pour_islands(*area);
        let now = fresh.pour_islands(*area);
        let count = |i: &Option<Arc<freerouting::board::pour_contacts::AreaIslands>>| {
            i.as_ref().map(|i| i.island_count())
        };
        assert_eq!(
            count(&held),
            count(&now),
            "{step}: islands of area {area:?}"
        );
        // K15.47: the kept fill's pieces and their islands are the fresh
        // fill's, and the grid's point queries are the scan's.
        if let (Some(held), Some(now)) = (&held, &now) {
            let pieces = |i: &freerouting::board::pour_contacts::AreaIslands| -> Vec<String> {
                i.pieces().iter().map(|p| format!("{p:?}")).collect()
            };
            assert_eq!(pieces(held), pieces(now), "{step}: pieces of area {area:?}");
            assert_same_point_queries(held, &pins, step);
        }
        if held.is_some() {
            nets.extend(board.item(*area).unwrap().net_numbers().iter().copied());
        }
        // K15.47: the cached Java contacts of the area are those computed now.
        assert_eq!(
            board.normal_contacts(*area),
            board.area_normal_contacts_uncached(*area),
            "{step}: contacts of area {area:?}"
        );
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

/// K15.47: [`AreaIslands::island_at`] through the grid of the pieces' boxes
/// gives the scan over every piece at `points`, at each piece's box corners
/// and centre, and just outside its box.
fn assert_same_point_queries(
    islands: &freerouting::board::pour_contacts::AreaIslands,
    points: &[Point],
    step: &str,
) {
    let mut at: Vec<Point> = points.to_vec();
    for (_, b, _) in islands.pieces() {
        let (x0, y0, x1, y1) = (b.ll.x, b.ll.y, b.ur.x, b.ur.y);
        for (x, y) in [
            (x0, y0),
            (x1, y1),
            (x0, y1),
            (x1, y0),
            ((x0 + x1) / 2, (y0 + y1) / 2),
        ] {
            at.push(Point::Int(IntPoint::new(x, y)));
        }
        at.push(Point::Int(IntPoint::new(x1 + 1, y0)));
        at.push(Point::Int(IntPoint::new(x0 - 1, (y0 + y1) / 2)));
    }
    let mut found = 0;
    for p in &at {
        let grid = islands.island_at(p);
        assert_eq!(
            grid,
            islands.island_at_by_scan(p),
            "{step}: island at {p:?}"
        );
        found += usize::from(grid.is_some());
    }
    assert!(found > 0, "{step}: no point in the fill");
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

/// K15.47: the board keeps each conduction area's Java contacts
/// (`ConductionArea.getNormalContacts`) while its trees and items are as
/// they were: a cached answer is the one computed now and adds to the work
/// meter what computing it adds (`board::area_contacts`); a track of the
/// zone's net that ends in the zone joins its contacts, and leaves them
/// when it is removed.
#[test]
fn the_cached_area_contacts_are_computed_ones_and_add_the_same_work() {
    let mut board = loaded(CUT, Some(false));
    let n = net(&board);
    let area = board
        .items()
        .find(|i| i.is_conduction_area() && i.contains_net(n))
        .map(|i| i.id())
        .unwrap();
    let units = |board: &RoutingBoard| board.search_trees().work_meter().units();
    let m0 = units(&board);
    let first = board.normal_contacts(area);
    let m1 = units(&board);
    let again = board.normal_contacts(area);
    let m2 = units(&board);
    let now = board.area_normal_contacts_uncached(area);
    let m3 = units(&board);
    assert!(!first.is_empty());
    assert_eq!(first, again);
    assert_eq!(first, now);
    assert!(m1 > m0, "the contacts query the trees");
    assert_eq!(
        m2 - m1,
        m1 - m0,
        "a cached answer adds the computation's work"
    );
    assert_eq!(m3 - m2, m1 - m0);
    let copy = board.clone();
    assert_eq!(copy.normal_contacts(area), first);
    // A track of the net from R85_2 to C54_1, both in the zone.
    let layer = board.item(area).unwrap().first_layer();
    let polyline = freerouting_geometry::Polyline::from_points(&[
        Point::Int(R85_2),
        Point::Int(IntPoint::new(R85_2.x, C54_1.y)),
        Point::Int(C54_1),
    ]);
    let track = board
        .insert_trace_without_cleaning(polyline, layer, 1000, &[n], 1, FixedState::Unfixed)
        .expect("the trace");
    let with = board.normal_contacts(area);
    assert_eq!(with, board.area_normal_contacts_uncached(area));
    assert!(with.contains(&track), "{with:?}");
    assert!(board.remove_item(track));
    let without = board.normal_contacts(area);
    assert_eq!(without, board.area_normal_contacts_uncached(area));
    assert_eq!(without, first);
    assert_eq!(copy.normal_contacts(area), first, "the copy kept its own");
}

// K15.46: KiCad joins a pad, a via or a track to every fill island its
// copper meets, not only to the island holding Java's contact point (a pin
// or via centre, a trace end, in the outline).

const SHAPES: &str = "crates/freerouting/tests/data/kicad/k1546-shape-contacts";
const MOTEUS: &str = "crates/freerouting/tests/data/kicad/moteus-k1542";

/// Board `dir/name` read and prepared under `enhanced`, with
/// `router.pour_fill_contacts` as given and `router.pour_fill_target` `false`
/// (unset before K15.51).
fn loaded_from(dir: &str, name: &str, contacts: Option<bool>) -> RoutingBoard {
    let path = repo_root().join(dir).join(format!("{name}.kicad_pcb"));
    let text = std::fs::read(&path).unwrap();
    let read = read_kicad_bytes(&path, &text, ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    let patch = SettingsPatch {
        profile: Some("enhanced".to_string()),
        pour_fill_contacts: contacts,
        pour_fill_target: Some(false),
        ..SettingsPatch::default()
    };
    input.extra_sources.push((Source::Testing, patch));
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

/// For each net `N0`, `N1`, ... of `board`: true if the connected set of
/// its witness pad (the pin at the zone's centre) holds every item of the
/// net.
fn witness_joins_all(board: &RoutingBoard) -> Vec<bool> {
    (0..)
        .map_while(|k| board.rules.nets.get_by_name(&format!("N{k}"), 1))
        .map(|net| {
            let n = net.net_number;
            let items: BTreeSet<ItemId> = board
                .items()
                .filter(|i| i.contains_net(n) && (i.is_pin() || i.is_via() || i.is_trace()))
                .map(|i| i.id())
                .collect();
            let area = board
                .items()
                .find(|i| i.is_conduction_area() && i.contains_net(n))
                .unwrap();
            let b = area.bounding_box();
            let centre = IntPoint::new((b.ll.x + b.ur.x) / 2, (b.ll.y + b.ur.y) / 2);
            let witness = board
                .items()
                .find(|i| i.is_pin() && i.contains_net(n) && i.drill().unwrap().center == centre)
                .unwrap()
                .id();
            let set = board.connected_set(witness, n, false);
            items.iter().all(|i| set.contains(i))
        })
        .collect()
}

/// KiCad 10.0.7 (`pcb drc --refill-zones`, and pcbnew's connectivity after
/// a refill) joins every item of `meets` to its zone and no item of `gaps`
/// (`make_boards.py` names each case). Under the setting the router joins
/// the same. Java's outline joins the items whose contact point lies in the
/// outline (`meets`' N5-N8 and `gaps`' N4); K15.31's rule, the island at the
/// contact point, joined only N8.
#[test]
fn a_pad_a_via_or_a_track_joins_the_fill_its_copper_meets() {
    let meets = loaded_from(SHAPES, "meets", Some(true));
    assert_eq!(witness_joins_all(&meets), vec![true; 9]);
    let gaps = loaded_from(SHAPES, "gaps", Some(true));
    assert_eq!(witness_joins_all(&gaps), vec![false; 5]);
    let java = loaded_from(SHAPES, "meets", Some(false));
    let mut expected = vec![false; 5];
    expected.extend([true; 4]);
    assert_eq!(witness_joins_all(&java), expected);
    let java = loaded_from(SHAPES, "gaps", Some(false));
    assert_eq!(
        witness_joins_all(&java),
        vec![false, false, false, false, true]
    );
}

/// moteus's GND B.Cu zone of priority 46 and U2 pad 2, whose centre lies
/// 0.0625 mm left of the zone's outline and whose copper reaches 0.6 mm into
/// it, with a GND via in the zone (`moteus-k1542/make_boards.py`). KiCad
/// 10.0.7 joins the pad, the zone and the via on `h-pad-shape` (no
/// unconnected item) and reports the pad unconnected and the zone
/// `isolated_copper` on `h-pad-gap`, whose pad is clear of the outline.
#[test]
fn moteus_u2_pad_2_joins_the_zone_its_copper_meets() {
    let joined = |name: &str, contacts: Option<bool>| -> bool {
        let board = loaded_from(MOTEUS, name, contacts);
        let n = board.rules.nets.get_by_name("GND", 1).unwrap().net_number;
        let pad = board
            .items()
            .find(|i| i.is_pin() && i.contains_net(n))
            .unwrap()
            .id();
        let via = board
            .items()
            .find(|i| i.is_via() && i.contains_net(n))
            .unwrap()
            .id();
        board.connected_set(pad, n, false).contains(&via)
    };
    assert!(joined("h-pad-shape", Some(true)));
    assert!(!joined("h-pad-gap", Some(true)));
    // Java's contact is the pad's centre in the outline.
    assert!(!joined("h-pad-shape", Some(false)));
    assert!(!joined("h-pad-gap", Some(false)));
}

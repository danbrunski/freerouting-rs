//! K15.23: a trace end inside a pour's outline but outside its fill is not a
//! contact of the stub cleanup.
//!
//! The boards are in `tests/data/kicad/pcu-k1523/`, written by
//! `make_boards.py` there and cut from the pcu board's K15.18 `.kicad_pcb`
//! route (`target/orchestrator/k1518/runs/`): zone EFUSE_NO_6 (`/CH6_NO`,
//! F.Cu, priority 43, clearance 0.5 mm, `min_thickness` 0.5 mm) verbatim,
//! `/CH6_NO`'s routed stub (F.Cu from (124.59, 108.0953) mm to a via at
//! (125.0442, 107.7107), B.Cu on to J2) and `Net-(U15-PGTH)`'s routed F.Cu
//! track at x 124.0539 through the zone, with the pcu board's `.kicad_dru`.
//! Each board is there as written (`-unfilled`) and as `kicad-cli pcb drc
//! --severity-all --format json --refill-zones --save-board` (KiCad 10.0.7)
//! saved it:
//!
//! - `a-stub-beside-a-crossing`: KiCad reports the stub `track_dangling`
//!   ("Track [/CH6_NO] on F.Cu, length 0.5439 mm") and 1 unconnected item
//!   (the track and the zone): between the PGTH track's clearance and the
//!   zone's edge at x 124.6279 the fill is 0.347 mm wide, under the zone's
//!   minimum thickness, and the filler removes it;
//! - `b-no-crossing`: the same without the PGTH track. KiCad reports no
//!   `/CH6_NO` item (its 1 unconnected item is PGTH's J3 and J4, which have
//!   no track here).
//!
//! The router's contacts (Java's) join the stub to the pour on both boards:
//! the end is inside the outline.

mod autoroute_common;

use std::collections::BTreeSet;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::FixedState;
use freerouting::drc::{DesignRulesChecker, eroded_fill_islands, fill_islands};
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::quality::pour_fill::{PourFills, opened_fill_core};
use freerouting::quality::stub_contacts::{end_has_contact, end_has_contact_with};
use freerouting::quality::stubs::{is_removable_stub, remove_stubs};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{IntPoint, Point, PolylineShape, PolylineShapeOps, Shape, TileShapeOps};

const DIR: &str = "crates/freerouting/tests/data/kicad/pcu-k1523";
const CH6: &str = "/CH6_NO";

/// The stub's end in the zone, board units (0.1 µm, y up).
const END: IntPoint = IntPoint {
    x: 1_245_900,
    y: -1_080_953,
};

/// Board `name`, with `/CH6_NO`'s tracks and via (or without them), read and
/// prepared as a routing job under `profile`.
fn input(name: &str, profile: &str, with_stub: bool) -> JobInput {
    let path = repo_root().join(DIR).join(format!("{name}.kicad_pcb"));
    let text = std::fs::read_to_string(&path).unwrap();
    let text = if with_stub {
        text
    } else {
        without_copper_of(&text, CH6)
    };
    let read = read_kicad_bytes(&path, text.as_bytes(), ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    let mut patch = SettingsPatch {
        profile: Some(profile.to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    input.extra_sources.push((Source::Testing, patch));
    input
}

/// `text` without its top-level `(segment ...)` and `(via ...)` lists of net
/// `net` (KiCad saves each over several lines).
fn without_copper_of(text: &str, net: &str) -> String {
    let net = format!("(net \"{net}\")");
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let (mut at, mut depth, mut copied) = (0, 0, 0);
    while at < bytes.len() {
        match bytes[at] {
            b'(' if depth == 1
                && (text[at..].starts_with("(segment") || text[at..].starts_with("(via")) =>
            {
                let mut end = at;
                let mut d = 0;
                loop {
                    match bytes[end] {
                        b'(' => d += 1,
                        b')' => {
                            d -= 1;
                            if d == 0 {
                                break;
                            }
                        }
                        _ => {}
                    }
                    end += 1;
                }
                if text[at..=end].contains(&net) {
                    out.push_str(&text[copied..at]);
                    copied = end + 1;
                }
                at = end + 1;
                continue;
            }
            b'(' => depth += 1,
            b')' => depth -= 1,
            _ => {}
        }
        at += 1;
    }
    out.push_str(&text[copied..]);
    out
}

fn loaded(name: &str) -> RoutingBoard {
    prepare_job(&input(name, "enhanced", true), &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

/// Board `name` without `/CH6_NO`'s copper, routed under `profile`, with
/// `router.cleanup.remove_stubs` set to `cleanup` if given.
fn route(name: &str, profile: &str, cleanup: Option<bool>) -> RoutingBoard {
    route_with(name, profile, |patch| {
        if cleanup.is_some() {
            patch.cleanup.remove_stubs = cleanup;
        }
    })
}

/// Board `name` without `/CH6_NO`'s copper, routed under `profile` with the
/// settings `patch` changes.
fn route_with(name: &str, profile: &str, patch: impl FnOnce(&mut SettingsPatch)) -> RoutingBoard {
    let mut input = input(name, profile, false);
    patch(&mut input.extra_sources[0].1);
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).unwrap();
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

/// The routable traces and vias of net `name`, in id order.
fn copper_of(board: &RoutingBoard, name: &str) -> Vec<ItemId> {
    let n = net(board, name);
    board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.contains_net(n))
        .map(|i| i.id())
        .collect()
}

/// `/CH6_NO`'s trace that ends at [`END`].
fn stub(board: &RoutingBoard) -> ItemId {
    copper_of(board, CH6)
        .into_iter()
        .find(|id| {
            board.item(*id).and_then(|i| i.as_trace()).is_some_and(|t| {
                t.first_corner() == Point::Int(END) || t.last_corner() == Point::Int(END)
            })
        })
        .expect("the stub's trace")
}

fn pour(board: &RoutingBoard) -> ItemId {
    let n = net(board, CH6);
    board
        .items()
        .find(|i| i.is_conduction_area() && i.contains_net(n))
        .map(|i| i.id())
        .expect("EFUSE_NO_6's conduction area")
}

const BOARDS: [&str; 4] = [
    "a-stub-beside-a-crossing",
    "a-stub-beside-a-crossing-unfilled",
    "b-no-crossing",
    "b-no-crossing-unfilled",
];

/// The end touches the pour's outline on both boards (Java's contact and
/// Q16.4's), and the opened fill only where no track crosses the pour, as
/// KiCad's refill gives it. The file's fill (or none) changes nothing.
#[test]
fn the_end_in_the_outline_touches_the_opened_fill_only_without_the_crossing() {
    for name in BOARDS {
        let board = loaded(name);
        let stub = stub(&board);
        let item = board.item(stub).unwrap();
        let end = Point::Int(END);
        let pour = pour(&board);
        assert!(
            board
                .trace_normal_contacts(stub, &end, false)
                .contains(&pour),
            "{name}: the router's contact is the outline"
        );
        assert!(end_has_contact(&board, item, &end), "{name}: Q16.4's");
        let fills = PourFills::new();
        let crossing = name.starts_with("a-");
        assert_eq!(
            end_has_contact_with(&board, item, &end, Some(&fills)),
            !crossing,
            "{name}: the opened fill"
        );
        assert!(fills.pieces(&board, board.item(pour).unwrap()).is_some());
    }
}

/// The cleanup removes the stub beside the crossing and the chain it leaves
/// (the F.Cu trace, the via and the B.Cu trace; the reader joins the two F.Cu
/// segments into one trace), and keeps it without the crossing.
#[test]
fn the_cleanup_removes_the_stub_beside_the_crossing_and_keeps_it_without() {
    for name in BOARDS {
        let mut board = loaded(name);
        let ch6 = copper_of(&board, CH6);
        assert_eq!(ch6.len(), 3, "{name}: 2 traces and a via");
        // the file's tracks are user-fixed; the cleanup judges the router's
        for id in &ch6 {
            board.unfix(*id);
        }
        // everything but `/CH6_NO`'s copper is kept, as input copper is
        let keep: BTreeSet<ItemId> = board
            .items()
            .map(|i| i.id())
            .filter(|id| !ch6.contains(id))
            .collect();
        let crossing = name.starts_with("a-");
        assert_eq!(
            is_removable_stub(&board, stub(&board), &keep),
            crossing,
            "{name}"
        );
        let summary = remove_stubs(&mut board, &keep);
        if crossing {
            assert_eq!((summary.traces, summary.vias), (2, 1), "{name}");
            assert!(copper_of(&board, CH6).is_empty(), "{name}");
        } else {
            assert_eq!(summary.removed(), 0, "{name}");
            assert_eq!(copper_of(&board, CH6), ch6, "{name}");
        }
    }
}

/// With no erosion the eroded fill is the fill model itself.
#[test]
fn the_fill_eroded_by_nothing_is_the_fill_model() {
    for name in BOARDS {
        let board = loaded(name);
        let pour = board.item(pour(&board)).unwrap();
        let model = fill_islands(&board, pour);
        assert!(!model.is_empty());
        assert_eq!(eroded_fill_islands(&board, pour, 0.0), model, "{name}");
    }
}

/// Routed again from J2 without the stub. Under Java's rule (`legacy`, and
/// `enhanced`, which leaves `router.pour_fill_target` unset) the router lays
/// one F.Cu trace from J2 to (124.5279, 109.2002) mm, in the outline right of
/// the PGTH track where KiCad's fill does not reach: the search's target is
/// the pour's outline. `legacy` keeps it (0 incomplete; KiCad would count it
/// dangling); `enhanced` keeps it without the cleanup and removes it with
/// the cleanup, which leaves the connection open (1 incomplete).
///
/// K15.25: with `pour_fill_target = true` (opt-in, on top of `enhanced`) the
/// search reaches the pour only where its opened fill reaches, so the route
/// ends in the fill and the connection is complete (0 incomplete), with the
/// cleanup and the output check and without them (the search alone does it).
#[test]
fn the_route_from_j2_leaves_no_stub_with_the_fill_target() {
    let tip = Point::Int(IntPoint::new(1_245_279, -1_092_002));
    let outline_target = |patch: &mut SettingsPatch| {
        patch.cleanup.remove_stubs = Some(false);
    };
    for (profile, board) in [
        ("legacy", route("a-stub-beside-a-crossing", "legacy", None)),
        (
            "enhanced",
            route_with("a-stub-beside-a-crossing", "enhanced", outline_target),
        ),
    ] {
        let ch6 = copper_of(&board, CH6);
        assert_eq!(ch6.len(), 1, "{profile}");
        let item = board.item(ch6[0]).unwrap();
        let trace = item.as_trace().expect("a trace");
        assert_eq!(trace.last_corner(), tip, "{profile}");
        assert!(end_has_contact(&board, item, &tip), "{profile}");
        assert!(!end_has_contact_with(
            &board,
            item,
            &tip,
            Some(&PourFills::new())
        ));
        assert_eq!(
            DesignRulesChecker::new(&board).incomplete_count(),
            0,
            "{profile}"
        );
    }
    let board = route("a-stub-beside-a-crossing", "enhanced", None);
    assert!(copper_of(&board, CH6).is_empty());
    assert_eq!(DesignRulesChecker::new(&board).incomplete_count(), 1);
    // K15.25: the fill target, with the cleanup and the output check and
    // without them.
    let fill_target = |patch: &mut SettingsPatch| {
        patch.pour_fill_target = Some(true);
    };
    let alone = |patch: &mut SettingsPatch| {
        patch.pour_fill_target = Some(true);
        patch.cleanup.remove_stubs = Some(false);
        patch.output_check = Some("off".to_string());
    };
    for (arm, board) in [
        (
            "enhanced, fill target",
            route_with("a-stub-beside-a-crossing", "enhanced", fill_target),
        ),
        (
            "enhanced, fill target, search alone",
            route_with("a-stub-beside-a-crossing", "enhanced", alone),
        ),
    ] {
        assert_eq!(
            DesignRulesChecker::new(&board).incomplete_count(),
            0,
            "{arm}"
        );
        let ends = pour_ends(&board);
        // One F.Cu trace from J2 to (124.3789, 110.5231) mm, below the strip
        // where the fill is wide (KiCad 10.0.7's refill: no `track_dangling`,
        // 0 unconnected; K15.25's note).
        let end = Point::Int(IntPoint::new(1_243_789, -1_105_231));
        assert_eq!(
            ends.iter().map(|(_, e)| e.clone()).collect::<Vec<_>>(),
            vec![end],
            "{arm}"
        );
        let fills = PourFills::new();
        for (id, end) in ends {
            let item = board.item(id).unwrap();
            assert_ne!(end, tip, "{arm}");
            assert!(
                end_has_contact_with(&board, item, &end, Some(&fills)),
                "{arm}: {end:?}"
            );
            assert!(
                inside_fill(&board, &fills, &end),
                "{arm}: {end:?} in a piece of the fill"
            );
            let pour = board.item(pour(&board)).unwrap();
            let core = opened_fill_core(&board, pour).expect("the pour's core");
            assert!(core.holds(&end), "{arm}: {end:?} in the fill's core");
        }
    }
}

/// The end corners of `/CH6_NO`'s traces that the router's contacts join to
/// EFUSE_NO_6 (its outline).
fn pour_ends(board: &RoutingBoard) -> Vec<(ItemId, Point)> {
    let pour = pour(board);
    let mut ends = Vec::new();
    for id in copper_of(board, CH6) {
        let Some(trace) = board.item(id).and_then(|i| i.as_trace()) else {
            continue;
        };
        for end in [trace.first_corner(), trace.last_corner()] {
            if board.trace_normal_contacts(id, &end, false).contains(&pour) {
                ends.push((id, end));
            }
        }
    }
    ends
}

/// True if `point` lies in a piece of EFUSE_NO_6's opened fill.
fn inside_fill(board: &RoutingBoard, fills: &PourFills, point: &Point) -> bool {
    let pour = board.item(pour(board)).unwrap();
    let pieces = fills
        .pieces(board, pour)
        .expect("the pour has an opened fill");
    pieces.iter().any(|(_, shape)| match shape {
        Shape::Polyline(PolylineShape::Tile(tile)) => tile.contains(point),
        _ => false,
    })
}

/// Board `a-stub-beside-a-crossing` without `/CH6_NO`'s copper, prepared
/// under `profile` with `router.pour_fill_target` set to `fill_target`.
fn prepared(profile: &str, fill_target: Option<bool>) -> RoutingBoard {
    let mut input = input("a-stub-beside-a-crossing", profile, false);
    input.extra_sources[0].1.pour_fill_target = fill_target;
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

/// Inserts a `/CH6_NO` F.Cu trace from `from` to `to`, both inside
/// EFUSE_NO_6's outline, and returns it and whether the board takes it for
/// a cycle through the pour (Java's `Trace.isOverlap` and `isCycle`).
fn insert_through_the_pour(board: &mut RoutingBoard, from: &Point, to: &Point) -> (ItemId, bool) {
    let n = net(board, CH6);
    let polyline = freerouting_geometry::Polyline::from_points(&[from.clone(), to.clone()]);
    let id = board
        .insert_trace_without_cleaning(polyline, 0, 1000, &[n], 1, FixedState::Unfixed)
        .expect("the trace");
    let pour = pour(board);
    assert!(board.trace_start_contacts(id).contains(&pour));
    assert!(board.trace_end_contacts(id).contains(&pour));
    let overlap = board.trace_is_overlap(id);
    assert_eq!(board.trace_is_cycle(id), overlap);
    (id, overlap)
}

/// [`insert_through_the_pour`]'s cycle flag.
fn is_cycle_through_the_pour(board: &mut RoutingBoard, from: &Point, to: &Point) -> bool {
    insert_through_the_pour(board, from, to).1
}

/// K15.25's cycle rule: a `/CH6_NO` trace from (124.3789, 110.5231) mm, in
/// EFUSE_NO_6's opened fill, to the stub's end in the strip has both ends in
/// the pour's outline. Java's rule (`legacy`, `enhanced`, and the board
/// without `router.pour_fill_target`) makes it an overlap with the pour, a
/// cycle the normalization removes; with `pour_fill_target = true` the pour
/// holds only the first end in the core of its fill, so the trace is a bridge
/// into the fill and no cycle.
#[test]
fn a_trace_from_the_fill_to_the_strip_is_a_cycle_only_under_javas_rule() {
    let fill_end = Point::Int(IntPoint::new(1_243_789, -1_105_231));
    for (arm, profile, fill_target, cycle) in [
        ("legacy", "legacy", None, true),
        ("enhanced", "enhanced", None, true),
        ("enhanced, fill target", "enhanced", Some(true), false),
    ] {
        let mut board = prepared(profile, fill_target);
        assert_eq!(board.pour_fill_target(), !cycle, "{arm}");
        let (id, overlap) = insert_through_the_pour(&mut board, &fill_end, &Point::Int(END));
        assert_eq!(overlap, cycle, "{arm}");
        board.set_pour_fill_target(false);
        assert!(board.trace_is_overlap(id), "{arm}: Java's rule");
    }
}

/// K15.25's cycle rule keeps only a bridge into the fill. With
/// `pour_fill_target = true` a trace with both ends in EFUSE_NO_6's outline
/// is still Java's cycle when neither end is in the fill's core (two points
/// of the 0.347 mm strip: the trace joins nothing through the fill), and
/// when its end out of the core is at a `/CH6_NO` via that the opened fill
/// reaches (the fill connects the via, so the trace is redundant copper).
/// A first version kept both (on the pcu board, a `Net-(U24-SW2)` trace of
/// the first kind made `/V_PD_IN` fail; K15.25's note).
#[test]
fn the_cycle_rule_keeps_only_a_bridge_into_the_fill() {
    let strip_tip = Point::Int(IntPoint::new(1_245_279, -1_092_002));
    let fill_end = Point::Int(IntPoint::new(1_243_789, -1_105_231));
    let mut board = prepared("enhanced", Some(true));
    let core = opened_fill_core(&board, board.item(pour(&board)).unwrap()).expect("the core");
    assert!(core.holds(&fill_end));
    for end in [&strip_tip, &Point::Int(END)] {
        assert!(!core.reaches(end), "{end:?} is outside the opened fill");
    }
    assert!(is_cycle_through_the_pour(
        &mut board,
        &Point::Int(END),
        &strip_tip
    ));
    // A point of the opened fill outside the core: half the radius past a
    // core tile's edge, the first such in tile order.
    let d = core.radius / 2.0;
    let at_via = core
        .tiles
        .iter()
        .flat_map(|tile| {
            let b = tile.bounding_box();
            let (cx, cy) = ((b.ll.x + b.ur.x) / 2, (b.ll.y + b.ur.y) / 2);
            let d = d.round() as i32;
            [
                IntPoint::new(b.ur.x + d, cy),
                IntPoint::new(b.ll.x - d, cy),
                IntPoint::new(cx, b.ur.y + d),
                IntPoint::new(cx, b.ll.y - d),
            ]
        })
        .map(Point::Int)
        .find(|p| !core.holds(p) && core.reaches(p))
        .expect("a point of the opened fill outside its core");
    let n = net(&board, CH6);
    let padstack = board
        .rules
        .via_infos
        .get(board.rules.via_infos.nth(0).expect("a via rule"))
        .padstack;
    board.insert_via(
        padstack,
        at_via.expect_int(),
        &[n],
        1,
        FixedState::Unfixed,
        false,
    );
    let core_end = core
        .tiles
        .iter()
        .map(|tile| Point::Int(tile.centre_of_gravity().round()))
        .find(|p| core.holds(p))
        .expect("a core point");
    assert!(is_cycle_through_the_pour(&mut board, &core_end, &at_via));
}

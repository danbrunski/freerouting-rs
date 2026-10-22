//! K15.36: a trace stub at a junction of its own net's traces is dangling
//! for the stub cleanup as KiCad counts it.
//!
//! The boards are in `tests/data/kicad/canary-k1536/`, written by
//! `make_boards.py` there and cut from canary-pwr's (`95882f00`) `.kicad_pcb`
//! route with `router.pour_fill_contacts` and `router.pour_fill_target`
//! (K15.31's `target/orchestrator/k1531/runs/rebased-canary/contacts-target`,
//! session `cce88912...`): `/Supervisor/CAN_EN`'s B.Cu copper from U7 pad 6
//! to the via at (151.7716, 51.5288) mm, with the route's 0.0725 mm stub from
//! the junction (144.777, 79.0511) to (144.7257, 79.1024), on the line of the
//! via's wire, at the end of the 0.15 mm neckdown from U7's wire. KiCad
//! 10.0.7 (`kicad-cli pcb drc --severity-all`):
//!
//! - `a-stub-at-a-junction`: the stub is `track_dangling`. Each of the three
//!   traces at the junction meets both ends of the stub (the far end is
//!   0.0725 mm from the junction, inside every one's copper widened by the
//!   stub's half width), and KiCad's `TestTrackEndpointDangling` counts such
//!   a track for the end nearer to its own ends only: the junction;
//! - `b-stub-end-on-a-track`: a track of the net also ends at the stub's far
//!   end; it is nearer that end, so KiCad counts it there: no
//!   `track_dangling`.
//!
//! On the route the stub is the piece pull-tight's `split_traces` left
//! (`smoothen_end_corners_at_trace`, after a pass's `remove_tails`) when it
//! moved the neckdown's end from the far end, where the fanout had put the
//! junction, to the via's wire. Java's contacts (the board's rule there,
//! `center`) see no contact at the far end; Q16.4's end-cap test (an end
//! whose cap meets other copper of the net) sees the via's wire there, so the
//! cleanup kept the stub.

mod autoroute_common;

use std::collections::BTreeSet;

use autoroute_common::repo_root;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::quality::stub_contacts::{end_has_contact, trace_dangling};
use freerouting::quality::stubs::{input_copper, is_removable_stub, remove_stubs};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{IntPoint, Point};

const DIR: &str = "crates/freerouting/tests/data/kicad/canary-k1536";
const NET: &str = "/Supervisor/CAN_EN";

/// The junction (144.777, 79.0511) mm, board units (0.1 µm, y up).
const JUNCTION: IntPoint = IntPoint {
    x: 1_447_770,
    y: -790_511,
};
/// The stub's far end (144.7257, 79.1024) mm.
const STUB_END: IntPoint = IntPoint {
    x: 1_447_257,
    y: -791_024,
};

fn loaded(name: &str) -> RoutingBoard {
    let path = repo_root().join(DIR).join(format!("{name}.kicad_pcb"));
    let text = std::fs::read(&path).unwrap();
    let read = read_kicad_bytes(&path, &text, ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    let patch = SettingsPatch {
        profile: Some("enhanced".to_string()),
        ..SettingsPatch::default()
    };
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

fn net(board: &RoutingBoard) -> i32 {
    board.rules.nets.get_by_name(NET, 1).unwrap().net_number
}

/// The net's traces and vias, in id order.
fn copper(board: &RoutingBoard) -> Vec<ItemId> {
    let n = net(board);
    board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.contains_net(n))
        .map(|i| i.id())
        .collect()
}

/// The stub: the trace from [`JUNCTION`] to [`STUB_END`].
fn stub(board: &RoutingBoard) -> ItemId {
    copper(board)
        .into_iter()
        .find(|id| {
            board.item(*id).and_then(|i| i.as_trace()).is_some_and(|t| {
                t.polyline.corners() == [Point::Int(JUNCTION), Point::Int(STUB_END)]
            })
        })
        .expect("the stub's trace")
}

/// The board with the stub unfixed (the reader fixes the file's copper) and
/// the cleanup's `keep`: every other trace and via, so that the cleanup
/// judges the stub as router copper beside the rest of the route.
fn with_router_stub(name: &str) -> (RoutingBoard, ItemId, BTreeSet<ItemId>) {
    let mut board = loaded(name);
    let stub = stub(&board);
    board.unfix(stub);
    let mut keep = input_copper(&board);
    keep.remove(&stub);
    (board, stub, keep)
}

/// U7's pin of the net.
fn pin(board: &RoutingBoard) -> ItemId {
    let n = net(board);
    board
        .items()
        .find(|i| i.is_pin() && i.contains_net(n))
        .map(|i| i.id())
        .expect("U7 pad 6")
}

/// The reader makes the route's four traces (the via's wire, the neckdown,
/// the stub, U7's wire) and the via; Java's contacts see nothing at the
/// stub's far end, Q16.4's end cap the via's wire; KiCad's rule calls it
/// dangling.
#[test]
fn the_stub_s_far_end_is_dangling_as_kicad_counts_it() {
    let board = loaded("a-stub-at-a-junction");
    let copper = copper(&board);
    assert_eq!(copper.len(), 5, "4 traces and a via: {copper:?}");
    let stub = stub(&board);
    let item = board.item(stub).unwrap();
    let far = Point::Int(STUB_END);
    assert!(
        board.trace_normal_contacts(stub, &far, false).is_empty(),
        "the router's contacts at the far end"
    );
    assert!(
        !board
            .trace_normal_contacts(stub, &Point::Int(JUNCTION), false)
            .is_empty()
    );
    assert!(end_has_contact(&board, item, &far), "Q16.4's end cap");
    assert!(trace_dangling(&board, item), "KiCad's rule");
    for id in copper.iter().filter(|id| **id != stub) {
        let other = board.item(*id).unwrap();
        if other.is_trace() {
            assert!(!trace_dangling(&board, other), "{id:?}");
        }
    }
}

/// The cleanup removes the stub, and the junction's connections stay: every
/// other trace and the via, all in U7's pin's connected set.
#[test]
fn the_cleanup_removes_the_stub_and_keeps_the_junction() {
    let (mut board, stub, keep) = with_router_stub("a-stub-at-a-junction");
    let pin = pin(&board);
    let before = board.connected_set(pin, net(&board), false);
    assert!(before.contains(&stub));
    assert!(is_removable_stub(&board, stub, &keep));
    let summary = remove_stubs(&mut board, &keep);
    assert_eq!((summary.traces, summary.vias), (1, 0));
    assert!(board.item(stub).is_none(), "the stub is gone");
    let after = board.connected_set(pin, net(&board), false);
    let expected: BTreeSet<ItemId> = before.into_iter().filter(|id| *id != stub).collect();
    assert_eq!(after, expected);
    for id in &keep {
        assert!(after.contains(id), "{id:?} still joins U7's pin");
    }
}

/// A track of the net ending at the stub's far end: KiCad counts it there
/// (its own end is nearer that end than the junction), so the stub is
/// connected at both ends and stays.
#[test]
fn a_stub_whose_far_end_meets_a_track_there_stays() {
    let (mut board, stub, keep) = with_router_stub("b-stub-end-on-a-track");
    assert_eq!(copper(&board).len(), 6, "5 traces and a via");
    let item = board.item(stub).unwrap();
    assert!(!trace_dangling(&board, item));
    assert!(!is_removable_stub(&board, stub, &keep));
    let summary = remove_stubs(&mut board, &keep);
    assert_eq!(summary.removed(), 0);
    assert!(board.item(stub).is_some());
}

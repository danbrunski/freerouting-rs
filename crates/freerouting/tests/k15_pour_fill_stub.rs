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
use freerouting::drc::{DesignRulesChecker, eroded_fill_islands, fill_islands};
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::quality::pour_fill::PourFills;
use freerouting::quality::stub_contacts::{end_has_contact, end_has_contact_with};
use freerouting::quality::stubs::{is_removable_stub, remove_stubs};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{IntPoint, Point};

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
    input.extra_sources = vec![(Source::Testing, patch)];
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
    let mut input = input(name, profile, false);
    if cleanup.is_some() {
        input.extra_sources[0].1.cleanup.remove_stubs = cleanup;
    }
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

/// Routed again from J2 without the stub, the router (both profiles) lays
/// one F.Cu trace from J2 to (124.5279, 109.2002) mm, in the outline right of
/// the PGTH track where KiCad's fill does not reach: its contacts are the
/// outline's. `legacy` keeps it (0 incomplete; KiCad would count it
/// dangling); under `enhanced` the cleanup removes it, so the route leaves no
/// stub and the connection open (1 incomplete).
#[test]
fn the_route_from_j2_leaves_no_stub_under_enhanced() {
    let tip = Point::Int(IntPoint::new(1_245_279, -1_092_002));
    for (profile, cleanup) in [("legacy", None), ("enhanced", Some(false))] {
        let board = route("a-stub-beside-a-crossing", profile, cleanup);
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
}

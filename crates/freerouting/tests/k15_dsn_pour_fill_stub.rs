//! K15.26: on a DSN input the stub cleanup judges a signal-layer pour by the
//! router's fill model, opened by the zones' minimum thickness and kept the
//! zones' clearance from other nets' copper when they are given
//! (`router.cleanup.pour_min_thickness_um`, `pour_clearance_um`).
//!
//! The board is `tests/data/quality/k1526-dsn-pour-crossing.dsn`, hand-written
//! with the pcu board's numbers (K15.24's `dsn-off` cut arm: net class
//! clearance 0.15 mm and track width 0.2 mm in the DSN; KiCad's F.Cu zones
//! have clearance 0.5 mm and `min_thickness` 0.5 mm, which the DSN does not
//! carry). Pour POUR covers x 2..12 mm on F.Cu. CROSS's track at x 11.2 mm
//! crosses it top to bottom and leaves a strip on its right: 0.55 mm wide at
//! the DSN's clearance (x 11.45 to 12), 0.2 mm at the zone's 0.5 mm (x 11.8
//! to 12), under the zone's minimum thickness, so KiCad's filler removes it.
//! POUR's stub runs from P2 (x 16 mm) to (11.65, -6) mm, in that strip: inside
//! the outline, 0.15 mm left of where the zone's clearance lets fill start.

mod autoroute_common;

use std::collections::BTreeSet;

use autoroute_common::repo_root;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::quality::pour_fill::PourFills;
use freerouting::quality::stub_contacts::{end_has_contact, end_has_contact_with};
use freerouting::quality::stubs::{is_removable_stub, remove_stubs};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{IntPoint, Point};

const FIXTURE: &str = "crates/freerouting/tests/data/quality/k1526-dsn-pour-crossing.dsn";
const CROSSING: &str = "    (wire (path F.Cu 200  11200 -1000  11200 -11000) (net CROSS))\n";

/// The stub's end in the strip, board units (0.1 µm, y up).
const END: IntPoint = IntPoint {
    x: 116_500,
    y: -60_000,
};

/// What the DSN is given about its zones.
#[derive(Clone, Copy, Debug)]
enum Zones {
    /// Nothing: no opening, the router's clearances.
    Unknown,
    /// The pcu board's: `min_thickness` 0.5 mm, clearance 0.5 mm.
    Pcu,
}

/// The fixture, with CROSS's track or without it, with F.Cu a signal or a
/// power layer, read and prepared under `enhanced`.
fn board(crossing: bool, zones: Zones, signal: bool) -> RoutingBoard {
    let text = std::fs::read_to_string(repo_root().join(FIXTURE)).unwrap();
    assert!(text.contains(CROSSING));
    let text = if crossing {
        text
    } else {
        text.replace(CROSSING, "")
    };
    let text = if signal {
        text
    } else {
        text.replace("(layer F.Cu (type signal)", "(layer F.Cu (type power)")
    };
    let mut input = JobInput::new(text.into_bytes(), "k1526-dsn-pour-crossing");
    let mut patch = SettingsPatch {
        profile: Some("enhanced".to_string()),
        ..SettingsPatch::default()
    };
    if let Zones::Pcu = zones {
        patch.cleanup.pour_min_thickness_um = Some(500.0);
        patch.cleanup.pour_clearance_um = Some(500.0);
    }
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap()
        .board
}

fn net(board: &RoutingBoard, name: &str) -> i32 {
    board.rules.nets.get_by_name(name, 1).unwrap().net_number
}

/// POUR's traces, in id order.
fn traces_of_pour(board: &RoutingBoard) -> Vec<ItemId> {
    let n = net(board, "POUR");
    board
        .items()
        .filter(|i| i.is_trace() && i.contains_net(n))
        .map(|i| i.id())
        .collect()
}

/// The stub: POUR's trace that ends at [`END`].
fn stub(board: &RoutingBoard) -> ItemId {
    traces_of_pour(board)
        .into_iter()
        .find(|id| {
            board.item(*id).and_then(|i| i.as_trace()).is_some_and(|t| {
                t.first_corner() == Point::Int(END) || t.last_corner() == Point::Int(END)
            })
        })
        .expect("the stub")
}

/// Whether the stub's end in the strip touches the pour: by the outline
/// (Q16.4's contact), and by the pour's fill model as the cleanup takes it.
fn contacts(board: &RoutingBoard) -> (bool, bool) {
    let item = board.item(stub(board)).unwrap();
    let end = Point::Int(END);
    (
        end_has_contact(board, item, &end),
        end_has_contact_with(board, item, &end, Some(&PourFills::new())),
    )
}

/// The cleanup on `board`, judging only the stub (as the router's copper).
fn clean(mut board: RoutingBoard) -> (usize, Vec<ItemId>) {
    let stub = stub(&board);
    board.unfix(stub);
    let keep: BTreeSet<ItemId> = board
        .items()
        .map(|i| i.id())
        .filter(|id| *id != stub)
        .collect();
    let removable = is_removable_stub(&board, stub, &keep);
    let summary = remove_stubs(&mut board, &keep);
    assert_eq!(
        removable,
        summary.traces == 1,
        "the judgement and the removal"
    );
    (summary.removed(), traces_of_pour(&board))
}

/// With the zones' values, the end beside the crossing is no contact and the
/// cleanup removes the stub; without the crossing the end is on the fill and
/// the stub stays. The outline counts it a contact either way.
#[test]
fn the_end_beside_the_crossing_is_no_contact_and_the_cleanup_removes_it() {
    let crossed = board(true, Zones::Pcu, true);
    assert_eq!(contacts(&crossed), (true, false));
    let (removed, left) = clean(crossed);
    assert_eq!((removed, left.len()), (1, 0));

    let whole = board(false, Zones::Pcu, true);
    assert_eq!(contacts(&whole), (true, true));
    let before = traces_of_pour(&whole);
    let (removed, left) = clean(whole);
    assert_eq!((removed, left), (0, before));
}

/// Without the zones' values the fill model keeps the strip (0.55 mm at the
/// DSN's clearance, no opening), so the cleanup keeps the stub KiCad calls
/// dangling: it never takes the pour for smaller than KiCad's fill could be.
#[test]
fn without_the_zones_values_the_strip_stays_a_contact() {
    for crossing in [true, false] {
        let board = board(crossing, Zones::Unknown, true);
        assert_eq!(contacts(&board), (true, true), "crossing {crossing}");
        let (removed, left) = clean(board);
        assert_eq!((removed, left.len()), (0, 1), "crossing {crossing}");
    }
}

/// A pour on a power layer keeps its outline.
#[test]
fn a_pour_on_a_power_layer_keeps_its_outline() {
    let board = board(true, Zones::Pcu, false);
    assert_eq!(contacts(&board), (true, true));
    let (removed, _) = clean(board);
    assert_eq!(removed, 0);
}

//! Q16.4: the stub cleanup counts contacts as KiCad does
//! ([`freerouting::quality::stub_contacts`]).
//!
//! The fixture `tests/data/quality/q16-overlap-stubs.dsn` is a synthetic
//! two-layer board, one net per case, whose wiring stands for copper the
//! router inserted (the tests pass an empty keep set):
//! - `STRIP`: a fixed F.Cu strip from pin A1 (as the plugin exports a fixed
//!   pour) crossed mid-segment by a via, whose B.Cu trace, a second via and
//!   an F.Cu trace reach pin B1: Q16.3's canary-pwr via;
//! - `TEE`: a trace A2 to B2 and a branch from pin C2 that ends on its body,
//!   50 um off its centre line;
//! - `PAD_VIA`: a via 200 um off the centre of pin A3's pad, then B.Cu, a
//!   via and F.Cu to B3;
//! - `OPEN`: the same chain from a via clear of pin A4: a failed start that
//!   touches nothing at its first via;
//! - `TOUCH` and `GAP`: a fixed strip and a via whose pad meets the strip's
//!   copper exactly at its border (425 um from its centre line, the pad's
//!   radius 300 plus the half width 125), and one 0.1 um (one board unit)
//!   further, then the chain to pin B5 / B6;
//! - `HUB` (Q16.5, kit-dev-coldfire-xilinx_5213's `Net-(D101-K)` via of
//!   Q16.4): a via used on F.Cu only, with three contacts there: trace `B`
//!   from its centre to pin B7, a short trace `A` from its centre to a point
//!   290 um off it (inside the pad), and trace `C` from pin A7 ending at that
//!   point. `B` and `C` meet only through `A` (their square caps stay 40 um
//!   apart), so not every two contacts touch, but they are all joined;
//! - `SPLIT` (Q16.5): a via used on F.Cu only whose contacts are two joined
//!   pairs, pins A8 and C8 meeting at a point 290 um left of its centre and
//!   pins B8 and D8 at one 290 um right of it, with no copper between the
//!   pairs but the via.
//!
//! Java's normal contacts see none of `STRIP`'s, `TEE`'s, `PAD_VIA`'s or
//! `TOUCH`'s joints, so Q16.3's test removed them; KiCad connects them all.

mod autoroute_common;

use std::collections::BTreeSet;
use std::path::Path;

use autoroute_common::repo_root;
use freerouting::board::items::ItemId;
use freerouting::board::{BasicBoard, RoutingBoard};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::quality::stub_contacts::{overlap_contacts, touches, via_contact_layers};
use freerouting::quality::stubs::{contacts_are_connected, is_removable_stub, remove_stubs};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::ResolveContext;
use freerouting::settings::Source;
use freerouting::settings::profile::legacy_pin;
use freerouting::settings::profiles::TestingProfile;

const FIXTURE: &str = "crates/freerouting/tests/data/quality/q16-overlap-stubs.dsn";

/// Board units per um (`resolution um 10`).
const UNITS: f64 = 10.0;

fn board() -> RoutingBoard {
    let path = repo_root().join(FIXTURE);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{FIXTURE}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_router_enabled(false);
    profile.set_fanout_enabled(false);
    profile.set_optimizer_enabled(false);
    let mut input = JobInput::new(bytes, JobInput::design_name_of(FIXTURE));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![
        (Source::Testing, legacy_pin()),
        (Source::Testing, profile.patch),
    ];
    prepare_job(&input, &ResolveContext { cores: 1 })
        .unwrap_or_else(|e| panic!("{FIXTURE}: {e}"))
        .board
}

fn net_number(board: &BasicBoard, name: &str) -> i32 {
    (1..=board.rules.nets.max_net_number())
        .find(|n| board.rules.nets.get(*n).is_some_and(|net| net.name == name))
        .unwrap_or_else(|| panic!("no net {name}"))
}

/// The traces and vias of net `name`, by id.
fn copper(board: &BasicBoard, name: &str) -> BTreeSet<ItemId> {
    let net = net_number(board, name);
    board
        .items()
        .filter(|i| (i.is_trace() || i.is_via()) && i.net_numbers() == [net])
        .map(|i| i.id())
        .collect()
}

/// `(vias, traces)` of net `name`.
fn counts(board: &BasicBoard, name: &str) -> (usize, usize) {
    let ids = copper(board, name);
    let vias = ids
        .iter()
        .filter(|id| board.item(**id).is_some_and(|i| i.is_via()))
        .count();
    (vias, ids.len() - vias)
}

/// The via of net `name` whose centre is at (`x`, `y`) um.
fn via_at(board: &BasicBoard, name: &str, x: f64, y: f64) -> ItemId {
    let (x, y) = ((x * UNITS).round() as i32, (y * UNITS).round() as i32);
    copper(board, name)
        .into_iter()
        .find(|id| {
            board.item(*id).is_some_and(|i| {
                i.is_via()
                    && i.drill()
                        .is_some_and(|d| (d.center.x, d.center.y) == (x, y))
            })
        })
        .unwrap_or_else(|| panic!("no via of {name} at ({x}, {y})"))
}

/// The fixed strip of net `name`.
fn strip(board: &BasicBoard, name: &str) -> ItemId {
    copper(board, name)
        .into_iter()
        .find(|id| {
            board
                .item(*id)
                .is_some_and(|i| i.is_trace() && !i.is_routable())
        })
        .unwrap_or_else(|| panic!("no fixed strip of {name}"))
}

fn incomplete(board: &RoutingBoard) -> usize {
    BoardStatistics::new(board).connections.incomplete_count
}

fn cleaned() -> RoutingBoard {
    let mut board = board();
    remove_stubs(&mut board, &BTreeSet::new());
    board
}

#[test]
fn java_sees_stubs_where_kicad_sees_joints() {
    let board = board();
    let none = BTreeSet::new();
    let strip_via = via_at(&board, "STRIP", 12000.0, -4000.0);
    let pad_via = via_at(&board, "PAD_VIA", 5200.0, -18000.0);
    let touch_via = via_at(&board, "TOUCH", 12000.0, -29575.0);
    let branch = copper(&board, "TEE")
        .into_iter()
        .find(|id| board.normal_contacts(*id).len() == 1)
        .expect("the branch: its end on the body is no Java contact");
    for id in [strip_via, pad_via, touch_via, branch] {
        assert!(board.is_tail(id), "{id:?} is a tail for Java's contacts");
        assert!(!is_removable_stub(&board, id, &none), "{id:?} is no stub");
    }
    let board_ref: &BasicBoard = &board;
    let item = |id| board_ref.item(id).expect("an item");
    assert_eq!(via_contact_layers(&board, item(strip_via)).len(), 2);
    assert!(overlap_contacts(&board, strip_via).contains(&strip(&board, "STRIP")));
    assert_eq!(via_contact_layers(&board, item(pad_via)).len(), 2);
    for id in [
        via_at(&board, "OPEN", 8000.0, -24000.0),
        via_at(&board, "GAP", 12000.0, -35574.9),
    ] {
        assert!(
            is_removable_stub(&board, id, &none),
            "{id:?} touches one layer"
        );
    }
}

#[test]
fn the_border_counts_and_one_unit_off_does_not() {
    let board = board();
    let touch_via = via_at(&board, "TOUCH", 12000.0, -29575.0);
    let gap_via = via_at(&board, "GAP", 12000.0, -35574.9);
    let item = |id| board.item(id).expect("an item");
    assert!(touches(
        &board,
        item(touch_via),
        item(strip(&board, "TOUCH"))
    ));
    assert!(!touches(&board, item(gap_via), item(strip(&board, "GAP"))));
    let (board, before) = (cleaned(), board);
    assert_eq!(copper(&board, "TOUCH"), copper(&before, "TOUCH"));
    // GAP's chain goes link by link; only the fixed strip stays.
    assert_eq!(counts(&board, "GAP"), (0, 1));
}

#[test]
fn a_via_crossing_a_fixed_strip_mid_segment_is_kept() {
    let before = board();
    let board = cleaned();
    assert_eq!(counts(&board, "STRIP"), (2, 3));
    assert_eq!(copper(&board, "STRIP"), copper(&before, "STRIP"));
}

#[test]
fn a_trace_ending_on_a_trace_body_is_kept() {
    let before = board();
    let board = cleaned();
    assert_eq!(counts(&board, "TEE"), (0, 2));
    assert_eq!(copper(&board, "TEE"), copper(&before, "TEE"));
}

#[test]
fn a_via_off_a_pad_centre_is_kept() {
    let board = cleaned();
    assert_eq!(counts(&board, "PAD_VIA"), (2, 2));
}

#[test]
fn copper_that_touches_nothing_still_goes() {
    let board = cleaned();
    assert_eq!(counts(&board, "OPEN"), (0, 0));
}

#[test]
fn the_cleanup_completes_nothing_less_and_is_deterministic() {
    let before = board();
    let mut board = board();
    let summary = remove_stubs(&mut board, &BTreeSet::new());
    // OPEN: 2 vias, 2 traces; GAP: 2 vias, 2 traces; HUB: its via (Q16.5).
    assert_eq!((summary.vias, summary.traces), (5, 4), "{summary:?}");
    assert_eq!(incomplete(&board), incomplete(&before));
    let again = cleaned();
    let ids = |b: &RoutingBoard| b.items().map(|i| i.id()).collect::<Vec<_>>();
    assert_eq!(ids(&again), ids(&board));
}

/// The traces of net `name` in id order.
fn traces(board: &BasicBoard, name: &str) -> Vec<ItemId> {
    copper(board, name)
        .into_iter()
        .filter(|id| board.item(*id).is_some_and(|i| i.is_trace()))
        .collect()
}

/// True if net `name` has no incomplete connection.
fn net_complete(board: &BasicBoard, name: &str) -> bool {
    let net = net_number(board, name);
    freerouting::drc::DesignRulesChecker::new(board).incomplete_count_of(net) == 0
}

#[test]
fn a_via_whose_contacts_meet_only_through_one_another_goes() {
    let before = board();
    let hub = via_at(&before, "HUB", 20000.0, -42000.0);
    let item = |id| before.item(id).expect("an item");
    let [c, a, b] = traces(&before, "HUB")[..] else {
        panic!("HUB has three traces");
    };
    assert_eq!(via_contact_layers(&before, item(hub)).len(), 1);
    assert_eq!(overlap_contacts(&before, hub), BTreeSet::from([c, a, b]));
    assert!(touches(&before, item(a), item(b)));
    assert!(touches(&before, item(a), item(c)));
    assert!(
        !touches(&before, item(b), item(c)),
        "B and C meet only through A"
    );
    assert!(contacts_are_connected(&before, &BTreeSet::from([a, b, c])));
    assert!(!contacts_are_connected(&before, &BTreeSet::from([b, c])));
    assert!(contacts_are_connected(&before, &BTreeSet::from([c])));
    assert!(contacts_are_connected(&before, &BTreeSet::new()));
    assert!(is_removable_stub(&before, hub, &BTreeSet::new()));
    let board = cleaned();
    assert_eq!(counts(&board, "HUB"), (0, 3));
    assert_eq!(traces(&board, "HUB"), [c, a, b]);
    assert!(net_complete(&before, "HUB") && net_complete(&board, "HUB"));
}

#[test]
fn a_via_joining_two_groups_of_contacts_is_kept() {
    let before = board();
    let split = via_at(&before, "SPLIT", 30000.0, -50000.0);
    let item = |id| before.item(id).expect("an item");
    let [c1, d1, c2, d2] = traces(&before, "SPLIT")[..] else {
        panic!("SPLIT has four traces");
    };
    assert_eq!(via_contact_layers(&before, item(split)).len(), 1);
    assert_eq!(
        overlap_contacts(&before, split),
        BTreeSet::from([c1, d1, c2, d2])
    );
    // Each contact touches another, but the two pairs meet only at the via.
    assert!(touches(&before, item(c1), item(d1)));
    assert!(touches(&before, item(c2), item(d2)));
    for (x, y) in [(c1, c2), (c1, d2), (d1, c2), (d1, d2)] {
        assert!(!touches(&before, item(x), item(y)), "{x:?} and {y:?}");
    }
    assert!(!contacts_are_connected(
        &before,
        &BTreeSet::from([c1, d1, c2, d2])
    ));
    assert!(!is_removable_stub(&before, split, &BTreeSet::new()));
    let board = cleaned();
    assert_eq!(copper(&board, "SPLIT"), copper(&before, "SPLIT"));
}

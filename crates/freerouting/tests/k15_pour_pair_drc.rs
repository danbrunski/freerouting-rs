//! K15.42: the routing-caused DRC of `router.pour_fill_contacts` with
//! `router.pour_fill_target` (K15.37's run: interf_u 1 -> 7, `via_dangling`
//! 0 -> 2 and `starved_thermal` 1 -> 5).
//!
//! The boards are `tests/data/kicad/interf_u-k1542/`, written by
//! `make_boards.py` with every coordinate taken from interf_u's routes (its
//! docs name the arms and sessions), each as KiCad 10.0.7's `pcb drc
//! --refill-zones --save-board` saved it:
//!
//! - `a-via-chain-cut-off`: K15.37's route. KiCad reports `via_dangling` at
//!   the GND vias (152.4229, 108.9504) and (160.1648, 109.18), which a F.Cu
//!   trace joins: KiCad's fill keeps the zone's 0.5 mm from other nets'
//!   copper, the B.Cu islands under the vias hold no pad, and its filler
//!   removes them. The router's fill model kept the net class's 0.254 mm and
//!   joined the vias to U9 pad L7 through the fill
//!   (`quality::pour_fill::opened_fill_core`, `drc::BoardZone::fill_clearance`);
//! - `b-lone-pad`: the route with that fixed, without its GND copper. U1
//!   pad 10 lies in a B.Cu island of the GND fill that no other GND item
//!   reaches. The route joined it by a F.Cu trace to a via in the main fill,
//!   and KiCad reported `starved_thermal` ("3 spokes connected to isolated
//!   island"): KiCad does not count the spokes of a pad into an island only
//!   that pad connects once it has another spoke, and a track of the pad
//!   whose other end lies over the zone's fill counts as one
//!   (`autoroute::pour_target::lone_pin_layers`).
//!
//! And `tests/data/kicad/moteus-k1542/c-cut-island`, written by its
//! `make_boards.py` with moteus's coordinates (K15.37's `pcb-on` route on
//! main `0564a6d`): GNDPWR's mode-1 zone of priority 33 on In1.Cu with no
//! GNDPWR pad, and the two ends of the `+3V` In1.Cu track that cut it in
//! three (`quality::island_repair`); K15.45: `d-zone-join` and `e-zone-cut`,
//! V+'s two In2.Cu zones from K15.37's `pcb-on` route on main `bd44781`
//! (session `f286d7f6`), which KiCad joins along the higher one's edge, and
//! the `+5V` track that cuts the lower one's fill between them and V+'s pad.

mod autoroute_common;

use std::collections::BTreeSet;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::io::kicad::{ReadOptions, read_kicad_bytes};
use freerouting::job::prepare::{JobInput, prepare_job};
use freerouting::quality::island_repair::{isolated_islands, isolated_islands_with_zone_joins};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};
use freerouting_geometry::{IntBox, IntPoint, Point, PolylineShapeOps, TileShape};

const DIR: &str = "crates/freerouting/tests/data/kicad/interf_u-k1542";
const CHAIN: &str = "a-via-chain-cut-off";
const LONE: &str = "b-lone-pad";
const GND: &str = "GND";

/// A centre in board units (0.1 µm, y up) from KiCad's mm (y down).
const fn at(x: i32, y: i32) -> IntPoint {
    IntPoint { x, y: -y }
}

/// The GND vias KiCad reports `via_dangling` on `a-via-chain-cut-off`.
const VIA_160: IntPoint = at(1_601_648, 1_091_800);
const VIA_152: IntPoint = at(1_524_229, 1_089_504);
/// U9 pad L7 (GND), whose B.Cu island the old model joined to the vias.
const U9_L7: IntPoint = at(1_581_150, 927_100);
/// U1 pad 10 and U8 pad 12 (GND), each alone in a B.Cu island of the
/// fill on `b-lone-pad`, and C6 pad 2 (GND) in its main island.
const U1_10: IntPoint = at(1_746_250, 1_244_600);
const U8_12: IntPoint = at(1_517_650, 1_129_030);
const C6_2: IntPoint = at(1_743_850, 1_117_600);

/// Board `name` read and prepared under `enhanced` (3 passes, the
/// optimizer as the profile has it), with `router.pour_fill_contacts` as
/// given and `router.pour_fill_target` on.
fn prepared(name: &str, contacts: Option<bool>) -> freerouting::job::prepare::PreparedJob {
    prepared_in(DIR, name, contacts)
}

/// [`prepared`] for board `name` of directory `dir`.
fn prepared_in(
    dir: &str,
    name: &str,
    contacts: Option<bool>,
) -> freerouting::job::prepare::PreparedJob {
    let path = repo_root().join(dir).join(format!("{name}.kicad_pcb"));
    let text = std::fs::read(&path).unwrap();
    let read = read_kicad_bytes(&path, &text, ReadOptions::default()).unwrap();
    let mut input = JobInput::new(Vec::new(), name);
    input.set_kicad(read);
    let mut patch = SettingsPatch {
        profile: Some("enhanced".to_string()),
        ..SettingsPatch::default()
    };
    patch.autorouter.max_passes = Some(3);
    patch.pour_fill_contacts = contacts;
    patch.pour_fill_target = Some(true);
    input.extra_sources.push((Source::Testing, patch));
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap()
}

fn routed(name: &str, contacts: Option<bool>) -> RoutingBoard {
    routed_in(DIR, name, contacts)
}

/// [`routed`] for board `name` of directory `dir`.
fn routed_in(dir: &str, name: &str, contacts: Option<bool>) -> RoutingBoard {
    let prepared = prepared_in(dir, name, contacts);
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
    board.rules.nets.get_by_name(GND, 1).unwrap().net_number
}

/// The GND item (pin or via) centred at `p`.
fn centred(board: &RoutingBoard, p: IntPoint) -> ItemId {
    let n = net(board);
    board
        .items()
        .find(|i| i.contains_net(n) && i.drill().is_some_and(|d| d.center == p))
        .map(|i| i.id())
        .unwrap_or_else(|| panic!("no GND pin or via at {p:?}"))
}

/// The centres of the GND pins connected to the item centred at `p`.
fn joined_pins(board: &RoutingBoard, p: IntPoint) -> BTreeSet<(i32, i32)> {
    let n = net(board);
    board
        .connected_set(centred(board, p), n, false)
        .into_iter()
        .filter_map(|id| board.item(id).filter(|i| i.is_pin()))
        .filter_map(|i| i.drill().map(|d| (d.center.x, d.center.y)))
        .collect()
}

/// Java's contacts join the via chain to U9 pad L7 through the zone's
/// outline; with the pair the fill model keeps the zone's own clearance
/// (0.5 mm, above the net class's 0.254 mm) as KiCad's filler does, so no
/// island of it joins the vias to a pin, as in KiCad's fill.
#[test]
fn the_fill_model_keeps_the_zones_clearance_and_the_via_chain_joins_no_pin() {
    for contacts in [None, Some(false)] {
        let board = prepared(CHAIN, contacts).board;
        let pins = joined_pins(&board, VIA_160);
        assert!(pins.contains(&(U9_L7.x, U9_L7.y)), "{contacts:?}: {pins:?}");
    }
    let board = prepared(CHAIN, Some(true)).board;
    assert_eq!(joined_pins(&board, VIA_160), BTreeSet::new());
    assert_eq!(joined_pins(&board, VIA_152), BTreeSet::new());
}

/// The GND traces of `board` with an end at `p`, as (layer, other end).
fn traces_at(board: &RoutingBoard, p: IntPoint) -> Vec<(i32, Point)> {
    let n = net(board);
    let p = Point::Int(p);
    board
        .items()
        .filter(|i| i.contains_net(n))
        .filter_map(|i| i.as_trace())
        .filter_map(|t| {
            if t.first_corner() == p {
                Some((t.layer, t.last_corner()))
            } else if t.last_corner() == p {
                Some((t.layer, t.first_corner()))
            } else {
                None
            }
        })
        .collect()
}

/// With the pair the router joins U1 pad 10 and U8 pad 12, each alone in
/// its B.Cu island of the GND fill, to C6 pad 2 through their islands: no
/// GND trace on F.Cu (layer 0) ends at either pad, so each island holds
/// more of the net than its pad and no F.Cu track of the pad ends over the
/// fill. KiCad 10.0.7 on this route (the CLI's, `--profile enhanced` with
/// both settings and `--max-passes 3`, imported and refilled): no
/// `starved_thermal`; without the rule (K15.42's first fix alone) U8 pad
/// 12 gets a F.Cu trace and KiCad reports it `starved_thermal` ("3 spokes
/// connected to isolated island").
#[test]
fn a_pad_alone_in_its_island_is_joined_through_the_island() {
    let board = routed(LONE, Some(true));
    assert!(joined_pins(&board, C6_2).contains(&(U1_10.x, U1_10.y)));
    for pad in [U1_10, U8_12] {
        let on_top: Vec<_> = traces_at(&board, pad)
            .into_iter()
            .filter(|(layer, _)| *layer == 0)
            .collect();
        assert!(on_top.is_empty(), "{pad:?}: {on_top:?}");
    }
}

const MOTEUS_DIR: &str = "crates/freerouting/tests/data/kicad/moteus-k1542";
const CUT: &str = "c-cut-island";

/// GNDPWR's zone on `c-cut-island`, x 131.2-134.05, y 93.4-99.7 mm (board
/// units, y up).
const ZONE: (i32, i32, i32, i32) = (1_312_000, -997_000, 1_340_500, -934_000);

/// With the pair the route from TP1 to TP2 ran straight along moteus's
/// `+3V` track (x = 133.4799 mm) on In1.Cu through GNDPWR's mode-1 zone,
/// which kept two islands with no GNDPWR item (KiCad 10.0.7: two
/// `isolated_copper`). The island repair removes the cut and routes it
/// again with the zone an obstacle for `+3V`: one island is left (the zone,
/// which has no pad at all; KiCad: one `isolated_copper`), the connection
/// is complete and no `+3V` trace meets the zone's outline. Without the
/// repair (main `0564a6d`) this was `left: 2, right: 1`.
#[test]
fn a_cut_that_isolates_a_kept_island_is_routed_around_it() {
    let board = routed_in(MOTEUS_DIR, CUT, Some(true));
    let islands = isolated_islands(&board);
    assert_eq!(islands.len(), 1, "{islands:?}");
    assert_eq!(board.ledger_incomplete_count(), 0);
    let p3v = board.rules.nets.get_by_name("+3V", 1).unwrap().net_number;
    let zone = TileShape::IntBox(IntBox::new(ZONE.0, ZONE.1, ZONE.2, ZONE.3));
    let crossing: Vec<_> = board
        .items()
        .filter(|i| i.contains_net(p3v) && i.is_trace())
        .flat_map(|i| (0..board.tile_shape_count(i)).filter_map(|k| board.tile_shape(i, k)))
        .filter(|tile| tile.intersects_tile(&zone))
        .map(|tile| tile.bounding_box())
        .collect();
    assert!(crossing.is_empty(), "{crossing:?}");
}

const JOIN: &str = "d-zone-join";
const JOIN_CUT: &str = "e-zone-cut";
const BLOB: &str = "f-blob";

/// The islands [`isolated_islands`] finds on `name` (not routed), as (net
/// name, bounds).
fn isolated_on(name: &str) -> Vec<(String, IntBox)> {
    let board = prepared_in(MOTEUS_DIR, name, Some(true)).board;
    isolated_islands_with_zone_joins(&board)
        .into_iter()
        .map(|i| {
            let net = board.rules.nets.get(i.net).unwrap().name.clone();
            (net, i.bounds)
        })
        .collect()
}

/// K15.45, RED before it: V+'s mode-1 zone of priority 34 holds no V+ item,
/// but KiCad's connectivity joins its fill to the fill of V+'s zone of
/// priority 0 along its top edge, which holds V+'s pad: KiCad 10.0.7 reports
/// no `isolated_copper` on `d-zone-join`. The island repair took the island
/// for isolated (its test only looked at pins, traces and vias).
#[test]
fn a_kept_island_another_zone_of_its_net_joins_to_a_pad_is_not_isolated() {
    let islands = isolated_on(JOIN);
    assert!(islands.is_empty(), "{islands:?}");
}

/// K15.45: with the route's `+5V` track across the lower zone the part of
/// its fill that meets the higher zone is cut off from the pad (and removed,
/// mode 0), and KiCad reports the priority-34 zone `isolated_copper` at
/// (134.1, 93.5) mm: the repair still finds it, x 131.2-134.1, y 93.5-97.3
/// mm.
#[test]
fn a_kept_island_cut_off_with_the_other_zone_is_isolated() {
    let islands = isolated_on(JOIN_CUT);
    assert_eq!(islands.len(), 1, "{islands:?}");
    let (net, b) = &islands[0];
    assert_eq!(net, "V+");
    assert!(
        b.ll.x >= 1_312_000 && b.ur.x <= 1_341_000 && b.ll.y >= -973_000 && b.ur.y <= -935_000,
        "{b:?}"
    );
}

/// K15.45, RED before it: on `f-blob` (moteus's +3V priority-41 and GND
/// priority-42 F.Cu zones, GND's track from C9 pad 2 and /aux2/AUX2P_C's via
/// from the `pcb-on` route `673d2823`) the +3V zone leaves a strip right of
/// the track, x 161.74-162.2, y 106.7-107.12 mm, 0.21 mm across when deflated
/// by half the zone's 0.25 mm minimum thickness. KiCad 10.0.7's filler deletes
/// it as a blob and reports no `isolated_copper`; the repair's model kept it
/// (the cleanup's blob test errs to keeping by a quarter of the radius).
#[test]
fn a_strip_kicad_deletes_as_a_blob_is_no_isolated_island() {
    let islands = isolated_on(BLOB);
    assert!(islands.is_empty(), "{islands:?}");
}

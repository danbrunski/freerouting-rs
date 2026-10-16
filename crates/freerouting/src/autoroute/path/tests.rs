//! Unit tests of the locator (A05.2, A05.4) on a small synthetic board:
//! two SMD pins of net `A` on `F.Cu`, a pin of net `B` between them, 45
//! degree routing. The probe comparison with Java is
//! `tests/autoroute_probe_locate.rs`.

use std::collections::BTreeSet;

use freerouting_geometry::{IntBox, IntPoint, TileShape};

use super::*;
use crate::autoroute::control::AutorouteControl;
use crate::autoroute::session::search::{MazeSearch, SearchResult, SearchSettings};
use crate::autoroute::session::{ExpandableRef, Session};
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::structure::AngleRestriction;
use crate::job::prepare::{JobInput, RulesChoice, prepare_job};
use crate::settings::ResolveContext;

const BOARD: &str = r#"(pcb unit-locate
  (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  0 0  10000 0  10000 4000  0 4000  0 0))
    (via V600)
    (rule (width 250) (clearance 200))
  )
  (placement
    (component SMD (place U1 2000 2000 front 0) (place U2 8000 2000 front 0)
      (place U3 5000 3000 front 0))
  )
  (library
    (image SMD (pin SmdPad 1 0 0))
    (padstack SmdPad (shape (rect F.Cu -400 -400 400 400)) (attach off))
    (padstack V600 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net A (pins U1-1 U2-1))
    (net B (pins U3-1))
    (class kicad_default "" A B (circuit (use_via V600)))
  )
)
"#;

struct Fixture {
    board: RoutingBoard,
    control: AutorouteControl,
    start: BTreeSet<ItemId>,
    dest: BTreeSet<ItemId>,
}

fn fixture() -> Fixture {
    let mut input = JobInput::new(BOARD.as_bytes().to_vec(), "unit-locate");
    input.rules = RulesChoice::None;
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).expect("the board loads");
    let board = prepared.board;
    let net = board
        .rules
        .nets
        .get_by_name("A", 1)
        .expect("net A")
        .net_number;
    let settings = &prepared.board_settings;
    let control = AutorouteControl::for_net(
        &board,
        net,
        settings,
        settings.via_costs(),
        settings.trace_costs(),
    )
    .expect("a control");
    let pins: Vec<ItemId> = board.connectable_items(net).into_iter().collect();
    assert_eq!(pins.len(), 2);
    Fixture {
        start: BTreeSet::from([pins[0]]),
        dest: BTreeSet::from([pins[1]]),
        board,
        control,
    }
}

/// The search result of the fixture's connection.
fn search(
    s: &mut Session<'_>,
    f_start: &BTreeSet<ItemId>,
    f_dest: &BTreeSet<ItemId>,
) -> SearchResult {
    let mut search = MazeSearch::plain(s, SearchSettings::default());
    assert!(search.init(f_start, f_dest).expect("init"));
    search
        .find_connection()
        .expect("the search")
        .expect("a connection")
}

fn pin_center(board: &RoutingBoard, id: ItemId) -> IntPoint {
    board
        .item(id)
        .and_then(|i| i.drill())
        .expect("a pin")
        .center
}

#[test]
fn a_found_connection_is_located_from_the_destination_to_the_start() {
    let mut f = fixture();
    let (start, dest) = (f.start.clone(), f.dest.clone());
    let start_pin = *start.first().expect("a start pin");
    let dest_pin = *dest.first().expect("a destination pin");
    let (start_center, dest_center) = (
        pin_center(&f.board, start_pin),
        pin_center(&f.board, dest_pin),
    );
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let result = search(&mut s, &start, &dest);
    let mut ripped = BTreeSet::new();
    let mut costs = RipupCosts::new();
    let located = locate(
        &mut s,
        &result,
        AngleRestriction::FortyfiveDegree,
        &mut ripped,
        Some(&mut costs),
    )
    .expect("located");
    assert_eq!(located.start_item, Some(start_pin));
    assert_eq!(located.target_item, Some(dest_pin));
    assert_eq!((located.start_layer, located.target_layer), (0, 0));
    assert!(ripped.is_empty() && costs.is_empty());
    assert_eq!(located.items.len(), 1, "{located:?}");
    let corners = &located.items[0].corners;
    assert_eq!(located.items[0].layer, 0);
    assert_eq!(corners.first(), Some(&dest_center), "{corners:?}");
    assert_eq!(corners.last(), Some(&start_center), "{corners:?}");
    assert!(
        corners.windows(2).all(|w| w[0] != w[1]),
        "no consecutive duplicates: {corners:?}"
    );
    // Every segment keeps the 45 degree restriction.
    for w in corners.windows(2) {
        let (dx, dy) = ((w[1].x - w[0].x).abs(), (w[1].y - w[0].y).abs());
        assert!(dx == 0 || dy == 0 || dx == dy, "{w:?} is not at 45 degrees");
    }
    // The chain runs from the destination door to the start door.
    let chain = &located.backtrack;
    assert_eq!(chain.first().map(|e| e.door), Some(result.destination.0));
    assert!(matches!(
        chain.last().map(|e| e.door),
        Some(ExpandableRef::TargetDoor(_))
    ));
}

#[test]
fn ninety_degree_routing_uses_the_45_degree_locator_with_orthogonal_corners() {
    let mut f = fixture();
    let (start, dest) = (f.start.clone(), f.dest.clone());
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let result = search(&mut s, &start, &dest);
    let located = locate(
        &mut s,
        &result,
        AngleRestriction::NinetyDegree,
        &mut BTreeSet::new(),
        None,
    )
    .expect("located");
    for item in &located.items {
        for w in item.corners.windows(2) {
            assert!(w[0].x == w[1].x || w[0].y == w[1].y, "{w:?}");
        }
    }
}

/// Without an angle restriction the any-angle locator (A07.5) runs on the
/// same backtrack chain and ends at the start door.
#[test]
fn any_angle_routing_uses_the_any_angle_locator() {
    let mut f = fixture();
    let (start, dest) = (f.start.clone(), f.dest.clone());
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let result = search(&mut s, &start, &dest);
    let mut ripped = BTreeSet::new();
    let located =
        locate(&mut s, &result, AngleRestriction::None, &mut ripped, None).expect("located");
    assert!(!located.items.is_empty());
    assert!(located.items.iter().all(|item| !item.corners.is_empty()));
    assert!(matches!(
        located.backtrack.last().map(|e| e.door),
        Some(ExpandableRef::TargetDoor(_))
    ));
}

fn square(llx: i32, lly: i32, urx: i32, ury: i32) -> TileShape {
    TileShape::IntOctagon(IntBox::new(llx, lly, urx, ury).to_int_octagon())
}

/// A door between two detached rooms with one section.
fn lone_door(s: &mut Session<'_>) -> crate::autoroute::session::DoorId {
    let a = s.detached_free_room(square(0, 0, 100, 100), 0, 1);
    let b = s.detached_free_room(square(100, 0, 200, 100), 0, 2);
    let door = s.new_door(a, b, 1);
    s.allocate_sections(door, 1);
    door
}

/// `FoundConnectionLocator.java:103-111`: the chain does not start at a
/// target door; Java warns and returns no items and no start item.
#[test]
fn a_chain_that_does_not_start_at_a_target_door_locates_no_items() {
    let mut f = fixture();
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let door = lone_door(&mut s);
    let result = SearchResult {
        destination: (ExpandableRef::Door(door), 0),
    };
    let located = locate(
        &mut s,
        &result,
        AngleRestriction::FortyfiveDegree,
        &mut BTreeSet::new(),
        None,
    )
    .expect("a valid empty result");
    assert!(located.items.is_empty());
    assert_eq!(located.start_item, None);
    assert_eq!(located.target_item, None);
    assert_eq!((located.start_layer, located.target_layer), (0, 0));
    assert_eq!(located.backtrack.len(), 1);
}

/// `FoundConnectionLocator.java:130-134`: the destination is neither a
/// target door nor a drill; Java warns and returns no items, with the start
/// item set.
#[test]
fn a_destination_that_is_neither_a_target_door_nor_a_drill_locates_no_items() {
    let mut f = fixture();
    let (start, dest) = (f.start.clone(), f.dest.clone());
    let start_pin = *start.first().expect("a start pin");
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let found = search(&mut s, &start, &dest);
    let chain = backtrack(&s, &found, &mut BTreeSet::new(), None).expect("a chain");
    let start_door = chain.last().expect("the start").door;
    let door = lone_door(&mut s);
    s.door_sections_mut(door)[0].backtrack = Some((start_door, 0));
    let result = SearchResult {
        destination: (ExpandableRef::Door(door), 0),
    };
    let located = locate(
        &mut s,
        &result,
        AngleRestriction::FortyfiveDegree,
        &mut BTreeSet::new(),
        None,
    )
    .expect("a valid empty result");
    assert!(located.items.is_empty());
    assert_eq!(located.start_item, Some(start_pin));
    assert_eq!(located.target_item, None);
    assert_eq!(located.target_layer, 0);
    assert_eq!(located.backtrack.len(), 2);
}

/// Java's `getMazeSearchElement` of a section the door does not have throws
/// `ArrayIndexOutOfBoundsException`.
#[test]
fn a_destination_section_out_of_range_is_an_index_error() {
    let mut f = fixture();
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let door = lone_door(&mut s);
    let result = SearchResult {
        destination: (ExpandableRef::Door(door), 3),
    };
    let error = locate(
        &mut s,
        &result,
        AngleRestriction::FortyfiveDegree,
        &mut BTreeSet::new(),
        None,
    )
    .expect_err("no section 3");
    assert_eq!(error.java_exception(), "ArrayIndexOutOfBoundsException");
    assert!(
        error
            .to_string()
            .contains("FoundConnectionLocator.backtrack")
    );
}

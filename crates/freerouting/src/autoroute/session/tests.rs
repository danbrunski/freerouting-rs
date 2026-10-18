//! Unit tests of the room database (A02.1, A02.2, A02.5) on a small
//! synthetic board: two SMD pins of net `A` on `F.Cu`, a pin of net `B`
//! between them, 45 degree routing.

use std::collections::BTreeSet;

use freerouting_geometry::{IntBox, PolylineShapeOps, TileShape};

use super::*;
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::job::prepare::{JobInput, RulesChoice, prepare_job};
use crate::settings::ResolveContext;

const BOARD: &str = r#"(pcb unit-rooms
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
      (place U3 5000 2000 front 0))
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
    let mut input = JobInput::new(BOARD.as_bytes().to_vec(), "unit-rooms");
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

fn square(llx: i32, lly: i32, urx: i32, ury: i32) -> TileShape {
    TileShape::IntOctagon(IntBox::new(llx, lly, urx, ury).to_int_octagon())
}

#[test]
fn removing_a_door_keeps_the_order_of_the_others() {
    let mut f = fixture();
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let room = s.detached_free_room(square(0, 0, 100, 100), 0, 1);
    let others: Vec<RoomId> = (0..4)
        .map(|i| s.detached_free_room(square(100, 25 * i, 200, 25 * i + 25), 0, 2 + i))
        .collect();
    let doors: Vec<DoorId> = others
        .iter()
        .map(|other| {
            let door = s.new_door(room, *other, 1);
            s.add_door_to(room, door);
            door
        })
        .collect();
    // A second occurrence of the first door: remove(Object) takes the first.
    s.add_door_to(room, doors[0]);
    assert!(s.remove_door_from(room, doors[1]));
    assert_eq!(
        s.room(room).doors(),
        [doors[0], doors[2], doors[3], doors[0]]
    );
    assert!(s.remove_door_from(room, doors[0]));
    assert_eq!(s.room(room).doors(), [doors[2], doors[3], doors[0]]);
    assert!(!s.remove_door_from(room, doors[1]), "already removed");
}

#[test]
fn incomplete_rooms_append_and_leave_with_their_doors() {
    let mut f = fixture();
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let a = s.add_incomplete(Some(square(0, 0, 10, 10)), 0, None);
    let b = s.add_incomplete(Some(square(10, 0, 20, 10)), 0, None);
    let c = s.add_incomplete(Some(square(20, 0, 30, 10)), 0, None);
    assert_eq!(s.incomplete_rooms(), [a, b, c]);
    assert_eq!(s.first_incomplete(), Some(a));
    // a - b - c by doors: removing b removes its doors, and with them the
    // incomplete neighbours a and c (removeAllDoors cascades).
    let ab = s.new_door(a, b, 1);
    s.add_door_to(a, ab);
    s.add_door_to(b, ab);
    let bc = s.new_door(b, c, 1);
    s.add_door_to(b, bc);
    s.add_door_to(c, bc);
    s.remove_incomplete(b);
    assert!(s.incomplete_rooms().is_empty());
    assert!(s.room(a).doors().is_empty() && s.room(c).doors().is_empty());
    assert_eq!(s.first_incomplete(), None);
}

#[test]
fn completing_a_start_room_inserts_rooms_in_list_order() {
    let mut f = fixture();
    let (start, dest) = (f.start.clone(), f.dest.clone());
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    assert_eq!(s.calculation_mode(), CalculationMode::Degree45);
    assert!(s.init(&start, &dest).expect("init"));
    // One start room per tree shape of the start pin, completed into one
    // complete room; the inserts follow the list.
    let ids: Vec<i32> = s
        .complete_rooms()
        .iter()
        .map(|r| s.room_java_id(*r).unwrap())
        .collect();
    assert!(!ids.is_empty());
    let inserts: Vec<TreeOp> = ids.iter().map(|id| TreeOp::Insert(*id)).collect();
    assert_eq!(s.tree_ops(), inserts);
    assert!(s.room_count() >= ids.len() as i32);
    // The start room is gone from the incomplete list; its target door to
    // the start pin is queued.
    assert!(!s.queue().is_empty());
    let first = s.complete_rooms()[0];
    let targets = s.room(first).target_doors();
    assert!(targets.iter().any(|d| !s.is_destination_door(*d)));
    for &room in s.incomplete_rooms() {
        assert!(matches!(s.room(room), Room::Incomplete { .. }));
        assert_eq!(
            s.room(room).doors().len(),
            1,
            "one door to the complete room"
        );
    }
}

#[test]
fn removing_a_complete_room_gives_its_neighbours_new_incomplete_rooms() {
    let mut f = fixture();
    let (start, dest) = (f.start.clone(), f.dest.clone());
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    assert!(s.init(&start, &dest).expect("init"));
    // Complete an incomplete neighbour, so two complete rooms touch.
    let first = s.complete_rooms()[0];
    s.complete_neighbour_rooms(first).expect("neighbours");
    let complete: Vec<RoomId> = s.complete_rooms().to_vec();
    assert!(complete.len() >= 2, "a neighbour was completed");
    // Every incomplete neighbour of the first room is complete now.
    for &door in s.room(first).doors() {
        let other = s.other_room(door, first).unwrap();
        assert!(s.room(other).is_complete());
    }
    let ops_before = s.tree_ops().len();
    let incomplete_before = s.incomplete_rooms().len();
    let first_id = s.room_java_id(first).unwrap();
    let touching_neighbours = s
        .room(first)
        .doors()
        .iter()
        .filter(|d| {
            let other = s.other_room(**d, first).unwrap();
            let shape = s.room(first).shape().unwrap();
            shape
                .intersection(s.room(other).shape().unwrap())
                .dimension()
                == 1
        })
        .count();
    s.remove_complete(first).expect("remove");
    assert_eq!(&s.tree_ops()[ops_before..], [TreeOp::Remove(first_id)]);
    assert!(!s.complete_rooms().contains(&first));
    assert!(s.room(first).doors().is_empty());
    assert_eq!(
        s.incomplete_rooms().len(),
        incomplete_before + touching_neighbours,
        "one new incomplete room per neighbour touching along a line"
    );
    // clear() removes the remaining complete rooms in list order.
    let remaining: Vec<TreeOp> = s
        .complete_rooms()
        .iter()
        .map(|r| TreeOp::Remove(s.room_java_id(*r).unwrap()))
        .collect();
    let ops_before = s.tree_ops().len();
    s.clear();
    assert_eq!(&s.tree_ops()[ops_before..], remaining);
    assert_eq!(s.room_count(), 0);
    assert!(s.complete_rooms().is_empty() && s.incomplete_rooms().is_empty());
    assert_eq!(s.tree().room_count(), 0, "no room left in the tree");
}

#[test]
fn a_door_whose_section_count_changes_loses_its_state() {
    let mut f = fixture();
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let a = s.detached_free_room(square(0, 0, 1000, 1000), 0, 1);
    let b = s.detached_free_room(square(1000, 0, 2000, 1000), 0, 2);
    let door = s.new_door_of_shapes(a, b).unwrap();
    assert_eq!(s.door(door).dimension, 1);
    let sections = s.section_segments(door, 10.0).unwrap().len();
    assert!(sections > 1);
    assert_eq!(s.door_section_reallocations(), 0);
    s.door_sections_mut(door)[0].occupied = true;
    // The same count keeps the state.
    assert_eq!(s.section_segments(door, 10.0).unwrap().len(), sections);
    assert!(s.door_sections(door)[0].occupied);
    assert_eq!(s.door_section_reallocations(), 0);
    // Another count replaces the sections (ExpansionDoor.java:193-201).
    let fewer = s.section_segments(door, 100.0).unwrap().len();
    assert_ne!(fewer, sections);
    assert_eq!(s.door_section_reallocations(), 1);
    assert_eq!(s.door_sections(door).len(), fewer);
    assert!(
        !s.door_sections(door)[0].occupied,
        "the occupied flag is lost"
    );
}

/// Java's orthogonal calculation gives up on a room that is not a box
/// (`SortedOrthogonalRoomNeighbours.java:117-120` warns and returns null):
/// an octagon room completes to no room.
#[test]
fn an_orthogonal_calculation_completes_no_room_of_an_octagon() {
    let mut f = fixture();
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    s.mode = CalculationMode::Orthogonal;
    let room = s.add_incomplete(
        Some(square(1990, 1990, 2010, 2010)),
        0,
        Some(square(1990, 1990, 2010, 2010)),
    );
    assert_eq!(s.complete_expansion_room(room), Ok(Vec::new()));
    assert!(s.complete_rooms().is_empty());
}

struct StopNow;

impl freerouting_geometry::Stoppable for StopNow {
    fn request_stop(&self) {}

    fn is_stop_requested(&self) -> bool {
        true
    }
}

#[test]
fn a_stop_request_or_an_exhausted_work_limit_fails_the_init() {
    let mut f = fixture();
    let (start, dest) = (f.start.clone(), f.dest.clone());
    let stop = StopNow;
    let mut s = Session::new(&mut f.board, &f.control, Some(&stop), None);
    assert!(!s.init(&start, &dest).expect("init"));
    assert!(s.queue().is_empty());
    drop(s);
    // A limit of 0 units fires at the first check after work was done:
    // the start rooms are completed, then the door loop stops.
    let limit = WorkLimit::nominal_ms(f.board.search_trees().work_meter(), 0);
    let mut s = Session::new(&mut f.board, &f.control, None, Some(limit));
    assert!(!s.init(&start, &dest).expect("init"));
    assert!(
        !s.complete_rooms().is_empty(),
        "the start rooms were completed"
    );
    assert!(s.queue().is_empty());
    assert_eq!(s.cutoff_log().len(), 1);
    assert!(s.cutoff_log()[0].starts_with("WORK_LIMIT_CUTOFF"));
}

/// Q17.1: a policy with two widths (narrow in rooms meeting `near`, wide
/// elsewhere).
struct TwoWidths {
    near: IntBox,
    narrow: i32,
    wide: i32,
}

impl crate::autoroute::width_policy::WidthPolicy for TwoWidths {
    fn half_width(&self, room: Option<&TileShape>, _layer: usize) -> i32 {
        let meets = room.is_none_or(|r| r.bounding_box().intersects_box(&self.near));
        if meets { self.narrow } else { self.wide }
    }
}

/// `expand_to_room_doors`'s door check, `door_is_small(door, 2 ·
/// (compensated half width of the next room + tolerance))`.
fn door_is_small_into(s: &mut Session<'_>, door: DoorId, room: RoomId) -> bool {
    let half_width = f64::from(s.room_compensated_half_width(room, 0));
    let width = 2.0 * (half_width + doors::TRACE_WIDTH_TOLERANCE);
    let settings = search::SearchSettings {
        fanout_max_escape_length_mm: 0.0,
        fanout_min_escape_length_mm: 0.0,
        start_ripup_costs: 100,
    };
    let search = search::MazeSearch::new(s, sink::NoopSink, sink::NoExtraCost, settings);
    search.door_is_small(door, width).expect("a door shape")
}

/// Q17.1: the default (no policy) reads the control's width; a policy
/// that gives the room behind a door a narrower width than the control's
/// turns a door that is small at the control's width into one that is
/// not, and the room it does not narrow keeps the control's decision.
#[test]
fn a_two_width_policy_changes_what_door_is_small_decides() {
    let mut f = fixture();
    let half = f.control.trace_half_width[0];
    let compensation = f.control.compensated_trace_half_width[0] - half;
    let narrow = half / 4;
    // A door a little narrower than the control's trace with clearance.
    let door_length = 2 * (half + compensation) - 10;
    let near = IntBox::new(0, 0, 100, door_length);
    let policy = TwoWidths {
        near,
        narrow,
        wide: half,
    };
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let a = s.detached_free_room(square(0, 0, 100, door_length), 0, 1);
    let b = s.detached_free_room(square(100, 0, 200, door_length), 0, 2);
    let far = s.detached_free_room(square(5000, 0, 5100, door_length), 0, 3);
    let far_neighbour = s.detached_free_room(square(5100, 0, 5200, door_length), 0, 4);
    let ab = s.new_door(a, b, 1);
    let far_door = s.new_door(far, far_neighbour, 1);
    // No policy: the control's width, the door is small either way.
    assert!(door_is_small_into(&mut s, ab, a));
    assert!(door_is_small_into(&mut s, far_door, far));
    // The two-width policy: room `a` meets `near` and gets the narrow width.
    s.set_width_policy(Some(&policy));
    assert_eq!(s.room_half_width(a, 0), narrow);
    assert_eq!(s.room_compensated_half_width(a, 0), narrow + compensation);
    assert!(!door_is_small_into(&mut s, ab, a));
    assert_eq!(s.room_half_width(far, 0), half);
    assert!(door_is_small_into(&mut s, far_door, far));
}

/// Q17.1: Freerouting's policy returns the control's widths in every room,
/// what a session without a policy reads.
#[test]
fn the_freerouting_policy_is_the_controls_width() {
    let mut f = fixture();
    let control = f.control.clone();
    let policy = crate::autoroute::width_policy::FreeroutingWidths { control: &control };
    let mut s = Session::new(&mut f.board, &f.control, None, None);
    let room = s.detached_free_room(square(0, 0, 100, 100), 0, 1);
    let without = (
        s.room_half_width(room, 0),
        s.room_compensated_half_width(room, 0),
    );
    s.set_width_policy(Some(&policy));
    let with = (
        s.room_half_width(room, 0),
        s.room_compensated_half_width(room, 0),
    );
    assert_eq!(without, with);
    assert_eq!(
        with,
        (
            control.trace_half_width[0],
            control.compensated_trace_half_width[0]
        )
    );
}

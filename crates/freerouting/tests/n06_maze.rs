//! N06.5, batch M (maze search and rooms): both behaviours of the batch's switches
//! that a public entry point reaches, and the reachability counters on real
//! searches, in their own process so that no other test resets the process-wide
//! counters (`run_stats`).
//!
//! - `init_failure_room_leak` (`AutorouteEngine.autorouteConnection`, Java at
//!   `a773a70ec`, lines 142-150): [`route_connection`] with a stop request that
//!   fails the init after a start room was completed;
//! - `connect_to_trace_swapped_width` (`FoundConnectionInserter.insertSegments`,
//!   lines 113-140): [`insert_segments`] ending at a trace on another layer, with
//!   per-layer half widths that differ;
//! - `ortho_zero_width_room` (`SortedOrthogonalRoomNeighbours`, lines 296-301) and
//!   `edge_corner_not_advanced` (`Sorted45DegreeRoomNeighbours`, line 263): one
//!   connection per net of J2 in 90 and 45 degree mode, both arms, with the
//!   counters.
//!
//! The pure halves of those two room switches and `failed_attempt_ripped_carryover`
//! (whose rejection the search does not reach on a small board) are unit tests:
//! `neighbours90::tests::right_of_obstacle_room_switch_both_behaviours`,
//! `incomplete45::tests::edge_room_sides_switch_both_behaviours` and
//! `connection::tests::rejected_attempt_ripped_set_switch_both_behaviours`.
//!
//! `true` is Java's behaviour in each switch; the counters count in either arm.
use std::collections::BTreeSet;
use std::sync::Mutex;

use freerouting::autoroute::connection::{Ripped, RouteLimits, route_connection};
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::path::{ResultItem, insert_segments};
use freerouting::autoroute::session::search::SearchSettings;
use freerouting::autoroute::session::sink::{NoopSink, WorkCounters};
use freerouting::autoroute::{AttemptResult, AttemptState};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::search::RoomId;
use freerouting::geometry::{IntPoint, Stoppable};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::run_stats::{self, Counter, Counts};
use freerouting::settings::board::BoardRouterSettings;
use freerouting::settings::{JavaQuirks, ResolveContext};

/// The counters are process-wide: the tests that read them run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn quirks(id: &str, java: bool) -> JavaQuirks {
    JavaQuirks::JAVA.with(id, java).unwrap()
}

/// Two pins of net `A` on `F.Cu` (U1 a through-hole pin, U2 an SMD pin), a pin of
/// net `B` between them, two signal layers, a through via, and a 300 um wire of
/// net `A` on `F.Cu` above them.
const BOARD: &str = r#"(pcb unit-n06-maze
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
    (component THT (place U1 2000 2000 front 0))
    (component SMD (place U2 8000 2000 front 0) (place U3 5000 2000 front 0))
  )
  (library
    (image SMD (pin SmdPad 1 0 0))
    (image THT (pin V600 1 0 0))
    (padstack SmdPad (shape (rect F.Cu -400 -400 400 400)) (attach off))
    (padstack V600 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net A (pins U1-1 U2-1))
    (net B (pins U3-1))
    (class kicad_default "" A B (circuit (use_via V600)))
  )
  (wiring
    (wire (path F.Cu 300 3000 3000 7000 3000) (net A) (type route))
  )
)
"#;

fn prepared(text: &[u8], name: &str) -> (RoutingBoard, BoardRouterSettings) {
    let mut input = JobInput::new(text.to_vec(), name);
    input.rules = RulesChoice::None;
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).expect("the board loads");
    (prepared.board, prepared.board_settings)
}

fn net_a(board: &RoutingBoard) -> i32 {
    board
        .rules
        .nets
        .get_by_name("A", 1)
        .expect("net A")
        .net_number
}

fn room_keys(board: &RoutingBoard, class: i32) -> Vec<i32> {
    let tree = board.search_trees().tree_by_class(class).expect("the tree");
    (-1000..1000)
        .filter(|id| tree.room(RoomId(*id)).is_some())
        .collect()
}

/// Stops from check `.1` on (Java's `isStopRequested` at the four points of
/// `init`).
struct StopAtCheck(std::cell::Cell<u32>, u32);

impl Stoppable for StopAtCheck {
    fn request_stop(&self) {}

    fn is_stop_requested(&self) -> bool {
        self.0.set(self.0.get() + 1);
        self.0.get() >= self.1
    }
}

fn route(
    board: &mut RoutingBoard,
    control: &AutorouteControl,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    stop: Option<&dyn Stoppable>,
) -> AttemptResult {
    let mut ripped = BTreeSet::new();
    let limits = RouteLimits {
        settings: SearchSettings::default(),
        stop,
        work_limit: None,
    };
    let ripped = Ripped {
        items: &mut ripped,
        costs: None,
    };
    route_connection(board, start, dest, control, limits, ripped, NoopSink).0
}

/// `init_failure_room_leak`: the first stop check after a start room of the
/// through-hole pin was completed fails the init. Java leaves the room in the
/// tree, and the next session moves it to a free negative key when it reuses its
/// id; `false` clears the session first, so the tree is empty and the next
/// session has nothing to move aside. Both arms count the failure once.
#[test]
fn init_failure_switch_both_behaviours_and_counter() {
    let _guard = serial();
    let mut leaked_at = None;
    for java in [true, false] {
        let (mut board, settings) = prepared(BOARD.as_bytes(), "n06-maze-init");
        let net = net_a(&board);
        let mut control =
            AutorouteControl::for_net_default(&board, net, &settings).expect("the control");
        control.quirks = quirks("init_failure_room_leak", java);
        let class = control.trace_clearance_class_index;
        let pins = board.connectable_items(net);
        let tht = pins
            .iter()
            .copied()
            .find(|id| {
                board
                    .item(*id)
                    .is_some_and(|i| i.is_pin() && i.first_layer() != i.last_layer())
            })
            .expect("the through-hole pin");
        let start: BTreeSet<ItemId> = [tht].into_iter().collect();
        let dest: BTreeSet<ItemId> = pins
            .iter()
            .copied()
            .filter(|id| *id != tht && board.item(*id).is_some_and(|i| i.is_pin()))
            .collect();
        // The first check that fails the init with a completed room (Java's arm
        // finds it; the other arm uses the same check).
        let checks: Vec<u32> = match leaked_at {
            Some(check) => vec![check],
            None => (1..20).collect(),
        };
        for check in checks {
            let stop = StopAtCheck(std::cell::Cell::new(0), check);
            run_stats::enable();
            let result = route(&mut board, &control, &start, &dest, Some(&stop));
            let leaks = Counts::now().get(Counter::MazeInitFailureRoomLeaks);
            run_stats::disable();
            assert_eq!(result.state, AttemptState::Failed);
            if leaks == 0 {
                assert!(room_keys(&board, class).is_empty(), "check {check}");
                continue;
            }
            assert_eq!(leaks, 1, "check {check}");
            assert!(
                result.details.ends_with("could not be created."),
                "{}",
                result.details
            );
            leaked_at = Some(check);
            break;
        }
        assert!(leaked_at.is_some(), "a check fails the init with a room");
        let leaked = room_keys(&board, class);
        if java {
            // PARITY(quirk, java_quirks.init_failure_room_leak): the room stays.
            assert!(!leaked.is_empty() && leaked.iter().all(|id| *id > 0));
        } else {
            assert!(leaked.is_empty(), "{leaked:?}");
        }
        let result = route(&mut board, &control, &start, &dest, None);
        let after = room_keys(&board, class);
        if java {
            // The leaked rooms are obstacles around the start pin.
            assert_eq!(result.state, AttemptState::Failed, "{}", result.details);
            assert_eq!(after.len(), leaked.len());
            assert!(after.iter().any(|id| *id < 0), "{after:?}");
        } else {
            assert_eq!(result.state, AttemptState::Routed, "{}", result.details);
            assert!(after.is_empty(), "{after:?}");
        }
    }
    run_stats::reset();
}

/// `connect_to_trace_swapped_width`: a path from the wire of net `A` (target,
/// `F.Cu`) down a via to `B.Cu` (start layer). The stub from the via to the wire
/// lies on `F.Cu`; Java gives it the start layer's half width (2000), `false`
/// its own layer's (1000). Both arms count the stub once.
#[test]
fn connect_to_trace_width_switch_both_behaviours_and_counter() {
    let _guard = serial();
    for java in [true, false] {
        let (mut board, settings) = prepared(BOARD.as_bytes(), "n06-maze-insert");
        let net = net_a(&board);
        let mut control =
            AutorouteControl::for_net_default(&board, net, &settings).expect("the control");
        control.trace_half_width = vec![1000, 2000];
        control.quirks = quirks("connect_to_trace_swapped_width", java);
        let wire = board
            .items()
            .find(|i| i.as_trace().is_some())
            .map(|i| i.id())
            .expect("the wire");
        let segment = ResultItem {
            half_widths: Vec::new(),
            corners: vec![IntPoint::new(40000, 20000), IntPoint::new(40000, 10000)],
            layer: 1,
        };
        run_stats::enable();
        let inserted = insert_segments(
            &mut board,
            &control,
            None,
            Some(wire),
            1,
            0,
            &[segment],
            None,
        );
        let flips = Counts::now().get(Counter::InsertConnectToTraceWidthFlips);
        run_stats::disable();
        assert!(inserted.is_ok(), "{inserted:?}");
        assert_eq!(flips, 1, "java = {java}");
        let front: BTreeSet<i32> = board
            .items()
            .filter_map(|i| i.as_trace())
            .filter(|t| t.layer == 0)
            .map(|t| t.half_width)
            .collect();
        let stub = if java { 2000 } else { 1000 };
        assert_eq!(front, [1500, stub].into_iter().collect(), "java = {java}");
    }
    run_stats::reset();
}

/// One connection per net of `board` (its first connectable item to the others),
/// in net order, with the search's work counters summed.
fn route_each_net(
    board: &mut RoutingBoard,
    settings: &BoardRouterSettings,
    quirks: JavaQuirks,
) -> u64 {
    let mut pops = 0;
    for net in 1..200 {
        let items = board.connectable_items(net);
        if items.len() < 2 {
            continue;
        }
        let Ok(mut control) = AutorouteControl::for_net_default(board, net, settings) else {
            continue;
        };
        control.quirks = quirks;
        // With ripup the search enters obstacle rooms and expands their
        // incomplete rooms (the batch router's pass 2 and later).
        control.ripup_allowed = true;
        control.ripup_costs = 100;
        let start: BTreeSet<ItemId> = [items[0]].into_iter().collect();
        let dest: BTreeSet<ItemId> = items[1..].iter().copied().collect();
        let mut ripped = BTreeSet::new();
        let (_, counters) = route_connection(
            board,
            &start,
            &dest,
            &control,
            RouteLimits::unlimited(SearchSettings::default()),
            Ripped {
                items: &mut ripped,
                costs: None,
            },
            WorkCounters::default(),
        );
        pops += counters.pops;
    }
    pops
}

const J2_90: &[u8] = include_bytes!("data/autoroute-variants/Issue026-J2_reference-90.dsn");
const J2_45: &[u8] = include_bytes!("../../../fixtures/Issue026-J2_reference.dsn");

/// The counters of one connection per net, and the queue pops.
fn counted(text: &[u8], name: &str, quirks: JavaQuirks) -> (Counts, u64) {
    let (mut board, settings) = prepared(text, name);
    run_stats::enable();
    let pops = route_each_net(&mut board, &settings, quirks);
    let counts = Counts::now();
    run_stats::disable();
    run_stats::reset();
    (counts, pops)
}

/// `ortho_zero_width_room` on J2 in 90 degree mode: the counter sees the rooms
/// Java leaves out in either arm, and the fixed rooms change the searches (at
/// 1dac842 plus this change: 32 and 33 flips, 6618 and 6610 pops).
#[test]
fn ortho_zero_width_room_reaches_a_90_degree_board() {
    let _guard = serial();
    let (java, java_pops) = counted(J2_90, "j2-90", JavaQuirks::JAVA);
    let (fixed, fixed_pops) = counted(J2_90, "j2-90", quirks("ortho_zero_width_room", false));
    let flips = |c: &Counts| c.get(Counter::MazeOrthoZeroWidthRoomFlips);
    assert!(flips(&java) > 0 && flips(&fixed) > 0);
    assert_ne!(java_pops, fixed_pops, "the fixed rooms change the searches");
    // The 45 degree counters stay 0 in 90 degree mode.
    for counts in [&java, &fixed] {
        assert_eq!(counts.get(Counter::MazeEdgeSweepsWithoutNeighbours), 0);
        assert_eq!(counts.get(Counter::MazeEdgeCornerFlips), 0);
    }
}

/// `edge_corner_not_advanced` on J2 in 45 degree mode: the branch at
/// `neighbours45.rs` (an obstacle room without sorted neighbours) runs, and its
/// sweeps make edge rooms that the switch adds or drops, counted in either arm
/// (27 sweeps, 27 flips at 1dac842 plus this change; the searches' pops are the
/// same in both arms here). The 90 degree counter stays 0.
#[test]
fn edge_corner_counters_on_a_45_degree_board() {
    let _guard = serial();
    let (java, _) = counted(J2_45, "j2-45", JavaQuirks::JAVA);
    let (fixed, _) = counted(J2_45, "j2-45", quirks("edge_corner_not_advanced", false));
    for counts in [&java, &fixed] {
        let sweeps = counts.get(Counter::MazeEdgeSweepsWithoutNeighbours);
        let flips = counts.get(Counter::MazeEdgeCornerFlips);
        assert!(sweeps > 0 && flips > 0, "{sweeps} {flips}");
        assert_eq!(counts.get(Counter::MazeOrthoZeroWidthRoomFlips), 0);
    }
}

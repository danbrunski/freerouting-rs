//! N06.6, batch F (fanout): both behaviours of the batch's three switches and their
//! counters, in their own process so that no other test resets the process-wide
//! counters (`run_stats`).
//!
//! - `fanout_stale_drill_carry` (`RoutingBoard.fanout`, Java at `a773a70ec`, lines
//!   1074-1126: one `AutorouteEngine` for every attempt): [`fanout_pin`] on a pin
//!   whose closest-target attempt fails, so that the all-targets attempt runs. Java's
//!   retry finds the first attempt's drill pages (their drills cached for the net), the
//!   fix computes them again;
//! - `drill_destination_room_index` (`FoundConnectionLocator.backtrack`, line 255):
//!   [`backtrack`] from a blind via's drill (layers 1-2 of 3) as the destination;
//! - `fanout_pin_order_names` (`BatchFanout.java:811-846`): [`FanoutRun::new`] with
//!   the documented spelling `distance_to_closest_on_net`.
//!
//! `true` is Java's behaviour in each switch; the counters count in either arm.
use std::collections::BTreeSet;
use std::sync::Mutex;

use freerouting::autoroute::AttemptState;
use freerouting::autoroute::batch::fanout::FanoutRun;
use freerouting::autoroute::control::AutorouteControl;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::fanout_pin::fanout_pin;
use freerouting::autoroute::path::{LocateError, backtrack};
use freerouting::autoroute::session::search::SearchResult;
use freerouting::autoroute::session::{ExpandableRef, Room, Session};
use freerouting::autoroute::stages::StageCtx;
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::search::WorkLimit;
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::run_stats::{self, Counter, Counts};
use freerouting::settings::board::BoardRouterSettings;
use freerouting::settings::{JavaQuirks, ResolveContext, RouterSettings};

/// The counters are process-wide: the tests that read them run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn quirks(id: &str, java: bool) -> JavaQuirks {
    JavaQuirks::JAVA.with(id, java).unwrap()
}

struct Prepared {
    board: RoutingBoard,
    settings: RouterSettings,
    router: BoardRouterSettings,
}

fn prepared(text: &str, name: &str) -> Prepared {
    let mut input = JobInput::new(text.as_bytes().to_vec(), name);
    input.rules = RulesChoice::None;
    let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).expect("the board loads");
    Prepared {
        board: prepared.board,
        settings: prepared.settings,
        router: prepared.board_settings,
    }
}

fn component_pin(board: &RoutingBoard, component: &str) -> ItemId {
    let id = board
        .components
        .get_by_name(component)
        .expect("the component")
        .id
        .0;
    board
        .items()
        .find(|i| i.is_pin() && i.component_id() == id)
        .expect("the pin")
        .id()
}

/// Net `A`: the SMD pin `U1` on `F.Cu` and two SMD pins `U2`, `U3` on `B.Cu` only,
/// so that the fanout of `U1` reaches no target without a via.
const FANOUT_BOARD: &str = r#"(pcb unit-n06-fanout
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
    (component TOP (place U1 2000 2000 front 0))
    (component BOT (place U2 6000 2000 front 0) (place U3 8000 2000 front 0))
  )
  (library
    (image TOP (pin TopPad 1 0 0))
    (image BOT (pin BotPad 1 0 0))
    (padstack TopPad (shape (rect F.Cu -400 -400 400 400)) (attach off))
    (padstack BotPad (shape (rect B.Cu -400 -400 400 400)) (attach off))
    (padstack V600 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net A (pins U1-1 U2-1 U3-1))
    (class kicad_default "" A (circuit (use_via V600)))
  )
)
"#;

/// [`fanout_pin`] on `U1` with every via site refused by a 50 mm minimum escape
/// length: the closest target (`U2`) fails, then all targets (`U2`, `U3`) fail.
fn fanout_u1(stale: bool) -> (AttemptState, Counts) {
    let mut p = prepared(FANOUT_BOARD, "unit-n06-fanout");
    p.settings.fanout.min_escape_length_mm = 50.0;
    let router = BoardRouterSettings::from_settings(&p.settings)
        .with_java_quirks(quirks("fanout_stale_drill_carry", stale));
    assert_eq!(router.fanout_min_escape_length_mm(), 50.0);
    let pin = component_pin(&p.board, "U1");
    let cancel = CancelToken::new();
    let events = NullEvents;
    let ctx = StageCtx {
        settings: &p.settings,
        board_settings: &router,
        cancel: &cancel,
        events: &events,
    };
    let limit = WorkLimit::nominal_ms(p.board.search_trees().work_meter(), 10_000);
    run_stats::enable();
    let result = fanout_pin(&mut p.board, pin, -1, &limit, &ctx);
    let counts = Counts::now();
    run_stats::disable();
    (result.state, counts)
}

#[test]
fn stale_drill_carry_switch_both_behaviours_and_counters() {
    let _guard = serial();
    let (java_state, java) = fanout_u1(true);
    let (fixed_state, fixed) = fanout_u1(false);
    assert_eq!(java_state, AttemptState::Failed);
    assert_eq!(fixed_state, AttemptState::Failed);
    for counts in [&java, &fixed] {
        // Attempt 2 is the one retry; it fails; it starts on attempt 1's pages.
        assert_eq!(counts.get(Counter::FanoutRetries), 1);
        assert_eq!(counts.get(Counter::FanoutRetriesSucceeded), 0);
        assert_eq!(counts.get(Counter::FanoutStaleDrillCarries), 1);
    }
    // Java's retry finds the page drills of the net cached; the fix computes them
    // again, so the retry repeats the first attempt's drill work.
    let java_drills = java.get(Counter::DrillsComputed);
    let fixed_drills = fixed.get(Counter::DrillsComputed);
    assert_eq!(java_drills, 25, "attempt 1 computes the page drills");
    assert_eq!(
        fixed_drills,
        2 * java_drills,
        "the fresh retry computes the same drills again"
    );
}

/// Net `A` with the SMD pins `U1` and `U2` on `F.Cu`, and a blind via of net `B` on
/// `In1.Cu` and `B.Cu` (layers 1 and 2).
const BLIND_VIA_BOARD: &str = r#"(pcb unit-n06-blind
  (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer In1.Cu (type signal) (property (index 1)))
    (layer B.Cu (type signal) (property (index 2)))
    (boundary (path pcb 0  0 0  10000 0  10000 4000  0 4000  0 0))
    (via V600 BV600)
    (rule (width 250) (clearance 200))
  )
  (placement
    (component TOP (place U1 2000 2000 front 0) (place U2 8000 2000 front 0))
  )
  (library
    (image TOP (pin TopPad 1 0 0))
    (padstack TopPad (shape (rect F.Cu -400 -400 400 400)) (attach off))
    (padstack V600 (shape (circle F.Cu 600)) (shape (circle In1.Cu 600))
      (shape (circle B.Cu 600)) (attach off))
    (padstack BV600 (shape (circle In1.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net A (pins U1-1 U2-1))
    (net B)
    (class kicad_default "" A B (circuit (use_via V600)))
  )
  (wiring
    (via BV600 5000 2000 (net B) (type route))
  )
)
"#;

/// The destination room that [`backtrack`] takes for the blind via's drill at
/// `section`, under `java`: its layer and whether it is the via's obstacle room.
fn destination_room(
    p: &mut Prepared,
    java: bool,
    section: usize,
) -> Result<(i32, bool), LocateError> {
    let net = p
        .board
        .rules
        .nets
        .get_by_name("A", 1)
        .expect("net A")
        .net_number;
    let router = p
        .router
        .with_java_quirks(quirks("drill_destination_room_index", java));
    let control = AutorouteControl::for_net(
        &p.board,
        net,
        &router,
        router.via_costs(),
        router.trace_costs(),
    )
    .expect("a control");
    let via = p
        .board
        .items()
        .find(|i| i.is_via())
        .expect("the blind via")
        .id();
    let mut session = Session::new(&mut p.board, &control, None, None);
    let drill = session.via_drill(via).expect("a drill").expect("a via");
    let layers = (
        session.drill(drill).first_layer,
        session.drill(drill).last_layer,
    );
    assert_eq!(layers, (1, 2));
    let result = SearchResult {
        destination: (ExpandableRef::Drill(drill), section),
    };
    let mut ripped = BTreeSet::new();
    let found = backtrack(&session, &result, &mut ripped, None).map(|chain| {
        let room = chain[0].next_room.expect("the drill's room");
        let own = matches!(session.room(room), Room::Obstacle { item, .. } if *item == via);
        (session.room(room).layer(), own)
    });
    session.clear();
    found
}

#[test]
fn drill_destination_room_index_switch_both_behaviours_and_counter() {
    let _guard = serial();
    let mut p = prepared(BLIND_VIA_BOARD, "unit-n06-blind");
    run_stats::enable();
    // Section 0 is layer 1 (In1.Cu): Java reads `roomArr[1 + 0]`, the room on layer 2.
    let java_0 = destination_room(&mut p, true, 0);
    let fixed_0 = destination_room(&mut p, false, 0);
    // Section 1 is layer 2: Java's `roomArr[2]` is past the end of the 2 rooms.
    let java_1 = destination_room(&mut p, true, 1);
    let fixed_1 = destination_room(&mut p, false, 1);
    let counts = Counts::now();
    run_stats::disable();
    assert_eq!(java_0, Ok((2, true)));
    assert_eq!(fixed_0, Ok((1, true)));
    assert!(matches!(java_1, Err(LocateError::IndexOutOfBounds { .. })));
    assert_eq!(fixed_1, Ok((2, true)));
    // Counted in either arm, once per backtrack from a drill not starting at layer 0.
    assert_eq!(counts.get(Counter::DrillDestinationRoomIndexFlips), 4);
}

/// Three SMD pins of the component `U1`: pin 1 at the origin and pin 3 at -1 mm on
/// net `A`, whose other pin `R1` is 2 mm away; pin 2 at +1 mm on net `B`, whose other
/// pin `R2` is 53 mm away.
const ORDER_BOARD: &str = r#"(pcb unit-n06-order
  (parser (string_quote ") (space_in_quoted_tokens on))
  (resolution um 10)
  (unit um)
  (structure
    (layer F.Cu (type signal) (property (index 0)))
    (layer B.Cu (type signal) (property (index 1)))
    (boundary (path pcb 0  0 0  60000 0  60000 4000  0 4000  0 0))
    (via V600)
    (rule (width 250) (clearance 200))
  )
  (placement
    (component IC (place U1 2000 2000 front 0))
    (component R (place R1 4000 2000 front 0) (place R2 55000 2000 front 0))
  )
  (library
    (image IC (pin SmdPad 1 0 0) (pin SmdPad 2 1000 0) (pin SmdPad 3 -1000 0))
    (image R (pin SmdPad 1 0 0))
    (padstack SmdPad (shape (rect F.Cu -200 -200 200 200)) (attach off))
    (padstack V600 (shape (circle F.Cu 600)) (shape (circle B.Cu 600)) (attach off))
  )
  (network
    (net A (pins U1-1 U1-3 R1-1))
    (net B (pins U1-2 R2-1))
    (class kicad_default "" A B (circuit (use_via V600)))
  )
)
"#;

/// The pin indices of `U1` in fanout order and the flip counter, for the pin order
/// `name` under `java`.
fn ic_pin_order(name: &str, java: bool) -> (Vec<usize>, u64) {
    let mut p = prepared(ORDER_BOARD, "unit-n06-order");
    p.settings.fanout.pin_sorting_order = name.to_string();
    let router = p
        .router
        .with_java_quirks(quirks("fanout_pin_order_names", java));
    let cancel = CancelToken::new();
    let events = NullEvents;
    let ctx = StageCtx {
        settings: &p.settings,
        board_settings: &router,
        cancel: &cancel,
        events: &events,
    };
    let ic = p.board.components.get_by_name("U1").expect("U1").id.0;
    run_stats::enable();
    let run = FanoutRun::new(&p.board, &ctx);
    let flips = Counts::now().get(Counter::FanoutPinOrderNameFlips);
    run_stats::disable();
    let component = run
        .components()
        .iter()
        .find(|c| c.component == ic)
        .expect("the component U1");
    (component.pins.iter().map(|p| p.pin_index).collect(), flips)
}

#[test]
fn pin_order_names_switch_reaches_the_fanout_run_and_counter() {
    let _guard = serial();
    let (java, java_flips) = ic_pin_order("distance_to_closest_on_net", true);
    let (fixed, fixed_flips) = ic_pin_order("distance_to_closest_on_net", false);
    let (camel, camel_flips) = ic_pin_order("distanceToClosestOnNet", true);
    // Java falls back to pin-index order; the fix reads the documented spelling as
    // the camelCase name does.
    assert_eq!(java, vec![0, 1, 2]);
    assert_eq!(fixed, camel);
    assert_ne!(fixed, java, "the board separates the two orders");
    assert_eq!((java_flips, fixed_flips, camel_flips), (1, 1, 0));
}

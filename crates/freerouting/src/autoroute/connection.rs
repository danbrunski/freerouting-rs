//! Routing one connection (A06.5, A06.6): search, locate, rip and insert.
//!
//! Port of `AutorouteEngine.autorouteConnection` (`:130-262`) and
//! `ripConnections` (`:264-290`) at `a773a70ec`. [`route_connection`] is the
//! one entry point: the batch router (A11), fanout (A12) and the GUI call it.
//! Java's single-item `RoutingBoard.autoroute(Item...)` has no caller and is
//! not ported.
//!
//! **Board lifecycle** (A06.6). Java's `RoutingBoard.initAutoroute`
//! (`:882-898`) creates an `AutorouteEngine` per connection (the retained
//! database is off unless a system property is set, A00), `finishAutoroute`
//! (`:900-909`) clears it, and `clearAllItemTemporaryAutorouteData`
//! (`:1275`) drops the items' autoroute data. Here a
//! [`Session`] is created per connection: it takes the autoroute tree of
//! the control's clearance class from the board and owns the items'
//! autoroute data in its side table (B02.8), so dropping it drops that data.
//! No engine is stored on the board. B01.8's
//! `clear_transient_autoroute_state` stays the restore-time reset.
//!
//! **Orders that decide results**, in Java's order:
//! - the session is cleared (its rooms leave the tree in list order, D5)
//!   before the search result is looked at (`:196-204`);
//! - the ripped set and costs are the caller's and are filled while
//!   backtracking, before any check, so a rejected result (a disabled
//!   layer) leaves them filled; a later attempt that reuses them rips the
//!   items an earlier failed attempt found
//!   (`// PARITY(quirk, java_quirks.failed_attempt_ripped_carryover):`, A12;
//!   `false` restores the set and the costs of an attempt rejected before its
//!   rip, N06.5; an attempt that ripped keeps them, since those items left the
//!   board);
//! - the rip: each ripped item's connection (`getConnectionItems`) joins one
//!   set, the items' nets another; the set is removed, then the trace tails
//!   of each net in ascending order.
//!
//! **Kept Java quirk: rooms leak on an init failure.** When
//! `MazeSearchEngine.getInstance` returns null or throws, Java returns
//! `FAILED` before `clear()` (`:142-150`), so the start rooms it completed
//! stay in the autoroute tree as obstacles for later connections of the
//! same clearance class. The port does the same during the parity phase
//! (`// PARITY(quirk, java_quirks.init_failure_room_leak):`, DIVERGENCES "Java bugs
//! kept on purpose"; `false` clears the session before the failure returns,
//! N06.5, so no room leaks and the rekey below never finds one). A later
//! session's room with the same id moves
//! the leaked one to a free key in place (`ShapeSearchTree::rekey_room_aside`);
//! a leaked room is an obstacle but, unlike Java, never a neighbour with a
//! door.
//!
//! With statistics on, both quirks count in either arm (N06.5's reachability
//! counters): `maze_init_failure_room_leaks` (an init failure with completed
//! rooms in the tree) and `maze_ripped_carryovers` (an attempt rejected before
//! its rip whose locate added to the ripped set).
//!
//! Java's debug logging of nets 33, 66 and 67 and the observer notification
//! around the rip and insert are not ported; the detail texts of the
//! results are Java's.

use std::collections::BTreeSet;

use freerouting_geometry::Stoppable;

use super::control::AutorouteControl;
use super::path::{InsertError, Located, RipupCosts, insert_located, locate};
use super::session::Session;
use super::session::drills::DrillCarry;
use super::session::search::{MazeSearch, SearchResult, SearchSettings};
use super::session::sink::{NoExtraCost, SearchSink};
use super::width_policy::WidthPolicy;
use super::{AttemptResult, AttemptState};
use crate::board::RoutingBoard;
use crate::board::items::{AreaKind, Item, ItemId, ItemKind, StopConnectionOption};
use crate::board::search::WorkLimit;

/// The stop checks of one connection's search: the settings it reads, the
/// user's stop request and the work limit (Java's `stoppableThread` and
/// `workLimit` of `initConnection`).
pub struct RouteLimits<'a> {
    pub settings: SearchSettings,
    pub stop: Option<&'a dyn Stoppable>,
    pub work_limit: Option<WorkLimit>,
}

impl RouteLimits<'_> {
    /// No stop request and no work limit.
    pub fn unlimited(settings: SearchSettings) -> RouteLimits<'static> {
        RouteLimits {
            settings,
            stop: None,
            work_limit: None,
        }
    }
}

/// The items the search decided to rip, filled while backtracking (Java's
/// `rippedItemList` and `ripupCosts`, both the caller's).
pub struct Ripped<'a> {
    pub items: &'a mut BTreeSet<ItemId>,
    pub costs: Option<&'a mut RipupCosts>,
}

/// What the search part of [`route_connection`] produced.
enum Searched {
    /// `getInstance` returned null or threw: the session was not cleared.
    InitFailed,
    /// The search ran; the located path if it found one.
    Done {
        result: Option<SearchResult>,
        located: Option<Located>,
    },
}

/// Java `AutorouteEngine.autorouteConnection(startSet, destSet, ctrl,
/// rippedItemList, ripupCosts)`: routes one connection from `start` to
/// `dest` on `board`, stepping the maze search with `sink`.
///
/// `Failed` when the search cannot be created, finds nothing, cannot be
/// located, ends on a disabled signal layer, or cannot be inserted (what was
/// inserted stays; the strict DRC of A11 cleans up); `Skipped` without
/// result items; else `Routed`. The ripped items are removed before the
/// insert. Every angle restriction routes (A07).
pub fn route_connection<S: SearchSink>(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    ctrl: &AutorouteControl,
    limits: RouteLimits<'_>,
    ripped: Ripped<'_>,
    sink: S,
) -> (AttemptResult, S) {
    let mut engine = DrillCarry::default();
    route_connection_on_engine(board, start, dest, ctrl, limits, ripped, sink, &mut engine)
}

/// [`route_connection`] as a further `autorouteConnection` on the Java engine
/// whose drill pages `engine` holds (see [`DrillCarry`]): the search starts
/// with them and leaves them there for the next call. A fresh
/// `DrillCarry::default()` is a new engine.
#[allow(clippy::too_many_arguments)]
pub fn route_connection_on_engine<S: SearchSink>(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    ctrl: &AutorouteControl,
    limits: RouteLimits<'_>,
    ripped: Ripped<'_>,
    sink: S,
    engine: &mut DrillCarry,
) -> (AttemptResult, S) {
    route_connection_with_widths(board, start, dest, ctrl, limits, ripped, sink, engine, None)
}

/// [`route_connection_on_engine`] with the room widths of `widths` (Q17.1):
/// the search, the locator and the inserter read each room's trace half
/// width from it. `None` is [`route_connection_on_engine`].
#[allow(clippy::too_many_arguments)]
pub fn route_connection_with_widths<S: SearchSink>(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    ctrl: &AutorouteControl,
    limits: RouteLimits<'_>,
    ripped: Ripped<'_>,
    sink: S,
    engine: &mut DrillCarry,
    widths: Option<&dyn WidthPolicy>,
) -> (AttemptResult, S) {
    let Ripped { items, mut costs } = ripped;
    let before = RippedBefore::take(ctrl, items, costs.as_deref());
    let stop = limits.stop;
    let (searched, sink) = search_and_locate(
        board,
        start,
        dest,
        ctrl,
        limits,
        items,
        costs.as_deref_mut(),
        sink,
        engine,
        widths,
    );
    let (result, located) = match searched {
        Searched::InitFailed => {
            let details = format!(
                "Failed to route connection between {}, because the maze search algorithm \
                 could not be created.",
                describe_connection(board, start, dest)
            );
            return (
                AttemptResult::with_details(AttemptState::Failed, details),
                sink,
            );
        }
        Searched::Done { result, located } => (result, located),
    };
    let found = result.is_some();
    let result = conclude_attempt(
        board, start, dest, ctrl, found, located, items, costs, before, stop,
    );
    (result, sink)
}

/// The ripped set and costs before an attempt's locate, for
/// `failed_attempt_ripped_carryover` (N06.5): the set's size always, a copy of
/// both only under `false`, which restores them.
struct RippedBefore {
    len: usize,
    saved: Option<(BTreeSet<ItemId>, Option<RipupCosts>)>,
}

impl RippedBefore {
    fn take(ctrl: &AutorouteControl, items: &BTreeSet<ItemId>, costs: Option<&RipupCosts>) -> Self {
        // PARITY(quirk, java_quirks.failed_attempt_ripped_carryover): Java
        // (AutorouteEngine.java:188, 227) keeps what a rejected attempt's locate
        // added; `false` keeps a copy to restore.
        let saved =
            (!ctrl.quirks.failed_attempt_ripped_carryover).then(|| (items.clone(), costs.cloned()));
        RippedBefore {
            len: items.len(),
            saved,
        }
    }
}

/// The checks, rip and insert after the search ([`finish_connection`]), then the
/// ripped set of an attempt rejected before its rip: Java keeps what its locate
/// added, so the next attempt on the same set rips the rejected path's items too
/// (`// PARITY(quirk, java_quirks.failed_attempt_ripped_carryover):`); `false`
/// restores the set and costs of `before`. An attempt that ripped keeps its set:
/// those items have left the board. With statistics on, the rejected attempts
/// whose locate added to the set count as `maze_ripped_carryovers` in either arm.
#[allow(clippy::too_many_arguments)]
fn conclude_attempt(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    ctrl: &AutorouteControl,
    found: bool,
    located: Option<Located>,
    items: &mut BTreeSet<ItemId>,
    costs: Option<&mut RipupCosts>,
    before: RippedBefore,
    stop: Option<&dyn Stoppable>,
) -> AttemptResult {
    let (result, ripped) = finish_connection(board, start, dest, ctrl, found, located, items, stop);
    if !ripped {
        // Locate only inserts into the set, so a larger set is an added item.
        if items.len() > before.len {
            crate::run_stats::record_n06(crate::run_stats::Counter::MazeRippedCarryovers);
        }
        if let Some((saved_items, saved_costs)) = before.saved {
            *items = saved_items;
            if let (Some(costs), Some(saved)) = (costs, saved_costs) {
                *costs = saved;
            }
        }
    }
    result
}

/// [`route_connection_on_engine`] for the batch router, the fanout and the
/// optimizer: with `NoopSink`, or, while the statistics are on
/// ([`run_stats`](crate::run_stats), P01.2), with the A04 sink
/// [`WorkCounters`](super::session::sink::WorkCounters), whose counts go to
/// the statistics. The sink only observes, so the result is the same.
#[allow(clippy::too_many_arguments)]
pub(crate) fn route_connection_counted(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    ctrl: &AutorouteControl,
    limits: RouteLimits<'_>,
    ripped: Ripped<'_>,
    engine: &mut DrillCarry,
    widths: Option<&dyn WidthPolicy>,
) -> AttemptResult {
    if !crate::run_stats::enabled() {
        return route_connection_with_widths(
            board,
            start,
            dest,
            ctrl,
            limits,
            ripped,
            super::session::sink::NoopSink,
            engine,
            widths,
        )
        .0;
    }
    let Ripped { items, costs } = ripped;
    let ripped_before = items.len();
    let (result, counters) = route_connection_with_widths(
        board,
        start,
        dest,
        ctrl,
        limits,
        Ripped {
            items: &mut *items,
            costs,
        },
        super::session::sink::WorkCounters::default(),
        engine,
        widths,
    );
    let added = items.len().saturating_sub(ripped_before);
    crate::run_stats::record_connection(&counters, result.state, added);
    result
}

/// The part of `autorouteConnection` after the session is cleared
/// (`:206-261`): the checks of the search result and the located path, then
/// the rip of `ripped` and the insert. `stop` is the search's stop request,
/// which may end the insert's normalization early. The flag is whether it
/// reached the rip (false: rejected before it, the board unchanged).
#[allow(clippy::too_many_arguments)]
pub(crate) fn finish_connection(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    ctrl: &AutorouteControl,
    found: bool,
    located: Option<Located>,
    ripped: &BTreeSet<ItemId>,
    stop: Option<&dyn Stoppable>,
) -> (AttemptResult, bool) {
    let failed = |board: &RoutingBoard, reason: &str| {
        let details = format!(
            "Failed to route connection between {}{reason}.",
            describe_connection(board, start, dest)
        );
        AttemptResult::with_details(AttemptState::Failed, details)
    };
    if !found {
        return (
            failed(
                board,
                ", because no connection was found between their nets",
            ),
            false,
        );
    }
    let Some(located) = located else {
        return (failed(board, ""), false);
    };
    if is_disabled_signal_layer(board, ctrl, located.start_layer)
        || is_disabled_signal_layer(board, ctrl, located.target_layer)
    {
        // PARITY(quirk, java_quirks.failed_attempt_ripped_carryover): the ripped set stays filled (A06
        // decisions); `conclude_attempt` restores it under `false`.
        return (
            failed(board, ", because some of their layers are disabled"),
            false,
        );
    }
    // Java's `connectionItems == null` (`:233-238`, SKIPPED) cannot happen:
    // the locator always returns a list, possibly empty, which is inserted.
    rip_connections(board, ripped, ctrl.remove_unconnected_vias);
    let result = match insert_located(board, ctrl, &located, stop) {
        Ok(_) => AttemptResult::new(AttemptState::Routed),
        Err(InsertError::Failed) => {
            failed(board, ", because the new connection could not be inserted")
        }
        // Java's NullPointerException reaches the batch router's catch,
        // which returns FAILED without details.
        Err(InsertError::NullCorner) => AttemptResult::new(AttemptState::Failed),
    };
    (result, true)
}

/// The session part of [`route_connection`]: init, search, locate, then
/// `clear()` unless the init failed (`:136-204`).
#[allow(clippy::too_many_arguments)]
fn search_and_locate<S: SearchSink>(
    board: &mut RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
    ctrl: &AutorouteControl,
    limits: RouteLimits<'_>,
    ripped: &mut BTreeSet<ItemId>,
    costs: Option<&mut RipupCosts>,
    sink: S,
    engine: &mut DrillCarry,
    widths: Option<&dyn WidthPolicy>,
) -> (Searched, S) {
    let angle = board.rules.trace_angle_restriction();
    let mut session = Session::new(board, ctrl, limits.stop, limits.work_limit);
    session.set_width_policy(widths);
    // PARITY(quirk, java_quirks.fanout_stale_drill_carry): the engine's drill pages
    // (`AutorouteEngine.java:91`).
    session.install_drill_carry(std::mem::take(engine));
    let (result, sink) = {
        let mut search = MazeSearch::new(&mut session, sink, NoExtraCost, limits.settings);
        // Java catches what `getInstance` throws and treats it as null.
        let initialized = search.init(start, dest).unwrap_or(false);
        // Java catches what `findConnection` throws: no result.
        let result = initialized.then(|| search.find_connection().ok().flatten());
        (result, search.into_sink())
    };
    let Some(result) = result else {
        // PARITY(quirk, java_quirks.init_failure_room_leak):
        // AutorouteEngine.java:142-150, FAILED before `clear()`: the session's
        // rooms stay in the tree. `false` clears them first, which leaves the
        // rekey of `Session::complete_room` nothing to move aside.
        *engine = session.take_drill_carry();
        if !session.complete_rooms().is_empty() {
            // Counted in either arm: the rooms `true` leaves in the tree.
            crate::run_stats::record_n06(crate::run_stats::Counter::MazeInitFailureRoomLeaks);
        }
        if !ctrl.quirks.init_failure_room_leak {
            session.clear();
        }
        return (Searched::InitFailed, sink);
    };
    // Java catches what the locator throws: no located path.
    let located = result
        .as_ref()
        .and_then(|result| locate(&mut session, result, angle, ripped, costs).ok());
    // PARITY: AutorouteEngine.java:196-204, the rooms leave the tree before
    // any check of the result; the drill pages stay with the engine.
    *engine = session.take_drill_carry();
    session.clear();
    (Searched::Done { result, located }, sink)
}

/// Java `AutorouteEngine.ripConnections` (`:264-290`): removes the
/// connections of `ripped` (up to forks and terminals, or fanout vias when
/// unconnected vias are kept) and then the trace tails of their nets, in
/// ascending net order. Returns those nets.
pub fn rip_connections(
    board: &mut RoutingBoard,
    ripped: &BTreeSet<ItemId>,
    remove_unconnected_vias: bool,
) -> BTreeSet<i32> {
    let stop = if remove_unconnected_vias {
        StopConnectionOption::None
    } else {
        StopConnectionOption::FanoutVia
    };
    let mut connections: BTreeSet<ItemId> = BTreeSet::new();
    let mut nets: BTreeSet<i32> = BTreeSet::new();
    for &item in ripped {
        connections.extend(board.connection_items(item, stop));
        if let Some(item) = board.item(item) {
            nets.extend(item.net_numbers().iter().copied());
        }
    }
    board.remove_items(connections);
    for &net in &nets {
        board.remove_trace_tails(net, stop);
    }
    nets
}

/// `isDisabledSignalLayer` (`:292-299`): a signal layer the control does not
/// route on. Plane layers never count: a connection may end on one.
fn is_disabled_signal_layer(board: &RoutingBoard, ctrl: &AutorouteControl, layer: i32) -> bool {
    let Ok(index) = usize::try_from(layer) else {
        return false;
    };
    let active = ctrl.layer_active.get(index).copied().unwrap_or(false);
    let signal = board
        .layer_structure
        .layers
        .get(index)
        .is_some_and(|l| l.is_signal);
    !active && signal
}

/// Java `describeConnection` (`:296-300`): the start items' and the
/// destination items' `toString`, each list joined with ", ", the two lists
/// joined with " and ".
pub fn describe_connection(
    board: &RoutingBoard,
    start: &BTreeSet<ItemId>,
    dest: &BTreeSet<ItemId>,
) -> String {
    let join = |set: &BTreeSet<ItemId>| {
        set.iter()
            .filter_map(|id| board.item(*id))
            .map(java_item_text)
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!("{} and {}", join(start), join(dest))
}

/// Java `Item.toString` and `Pin.toString`: the class's simple name in
/// lower case, the pin index when positive, the component id when
/// positive.
pub fn java_item_text(item: &Item) -> String {
    let name = match &item.kind {
        ItemKind::Pin(_) => "pin",
        ItemKind::Via(_) => "via",
        ItemKind::Trace(_) => "polylinetrace",
        ItemKind::Area(area) => match area.kind {
            AreaKind::Obstacle => "obstaclearea",
            AreaKind::ViaObstacle => "viaobstaclearea",
            AreaKind::TraceObstacle => "traceobstaclearea",
            AreaKind::ComponentObstacle => "componentobstaclearea",
            AreaKind::Conduction { .. } => "conductionarea",
        },
        ItemKind::ComponentOutline(_) => "componentoutline",
        ItemKind::BoardOutline(_) => "boardoutline",
    };
    let mut text = name.to_string();
    if let Some(pin) = item.as_pin()
        && pin.pin_index > 0
    {
        text.push_str(&format!(" #{}", pin.pin_index));
    }
    if item.component_id() > 0 {
        text.push_str(&format!(" of component #{}", item.component_id()));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::autoroute::path::{ResultItem, insert_segments};
    use crate::geometry::IntPoint;
    use crate::job::prepare::{JobInput, RulesChoice, prepare_job};
    use crate::settings::ResolveContext;

    /// Two SMD pins of net `A` on `F.Cu` with a pin of net `B` between them,
    /// two signal layers, a through via.
    const BOARD: &str = r#"(pcb unit-connection
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

    fn board_and_control() -> (RoutingBoard, AutorouteControl, i32) {
        let mut input = JobInput::new(BOARD.as_bytes().to_vec(), "unit-connection");
        input.rules = RulesChoice::None;
        let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).expect("the board loads");
        let board = prepared.board;
        let net = board
            .rules
            .nets
            .get_by_name("A", 1)
            .expect("net A")
            .net_number;
        let control = AutorouteControl::for_net_default(&board, net, &prepared.board_settings)
            .expect("the control");
        (board, control, net)
    }

    fn located(items: Vec<ResultItem>, start_layer: i32, target_layer: i32) -> Located {
        Located {
            items,
            start_item: None,
            target_item: None,
            start_layer,
            target_layer,
            backtrack: Vec::new(),
        }
    }

    fn item_ids(board: &RoutingBoard) -> Vec<ItemId> {
        board.items().map(|i| i.id()).collect()
    }

    fn pin_of_net(board: &RoutingBoard, net: i32) -> ItemId {
        board
            .items()
            .find(|i| i.is_pin() && i.contains_net(net))
            .expect("a pin of the net")
            .id()
    }

    #[test]
    fn a_path_ending_on_a_disabled_signal_layer_fails_and_keeps_the_ripped_set() {
        let (mut board, mut control, _) = board_and_control();
        control.layer_active[1] = false;
        let other = pin_of_net(&board, 2);
        let ripped: BTreeSet<ItemId> = [other].into_iter().collect();
        let before = item_ids(&board);
        let empty = BTreeSet::new();
        let result = finish_connection(
            &mut board,
            &empty,
            &empty,
            &control,
            true,
            Some(located(Vec::new(), 1, 0)),
            &ripped,
            None,
        )
        .0;
        assert_eq!(result.state, AttemptState::Failed);
        assert!(
            result
                .details
                .ends_with("because some of their layers are disabled.")
        );
        assert_eq!(ripped.len(), 1, "the ripped set stays filled");
        assert_eq!(item_ids(&board), before, "nothing was ripped or inserted");
    }

    /// `failed_attempt_ripped_carryover` (N06.5): an attempt rejected before its
    /// rip (a disabled layer) whose locate added an item and a cost. Java keeps
    /// them for the next attempt; `false` restores the set and the costs. An
    /// attempt that ripped keeps its set in both arms, and both arms count the
    /// rejected one as `maze_ripped_carryovers`.
    #[test]
    fn rejected_attempt_ripped_set_switch_both_behaviours() {
        use crate::autoroute::path::put_cost;
        use crate::run_stats::{self, Counter};
        let _guard = run_stats::TEST_SERIAL
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        run_stats::enable();
        // Ids no item has: the rip of a routed attempt removes nothing.
        let (earlier, added) = (ItemId(90_001), ItemId(90_002));
        let empty = BTreeSet::new();
        for java in [true, false] {
            let (mut board, mut control, _) = board_and_control();
            control.layer_active[1] = false;
            control.quirks = crate::settings::JavaQuirks::JAVA
                .with("failed_attempt_ripped_carryover", java)
                .unwrap();
            let attempt = |board: &mut RoutingBoard, start_layer: i32| {
                let mut items: BTreeSet<ItemId> = [earlier].into_iter().collect();
                let mut costs: RipupCosts = vec![(earlier, 5)];
                let before = RippedBefore::take(&control, &items, Some(&costs));
                // What this attempt's locate adds.
                items.insert(added);
                put_cost(&mut costs, earlier, 9);
                put_cost(&mut costs, added, 7);
                let located = Some(located(Vec::new(), start_layer, start_layer));
                let (result, counts) = run_stats::capture(|| {
                    conclude_attempt(
                        board,
                        &empty,
                        &empty,
                        &control,
                        true,
                        located,
                        &mut items,
                        Some(&mut costs),
                        before,
                        None,
                    )
                });
                (
                    result.state,
                    items,
                    costs,
                    counts.get(Counter::MazeRippedCarryovers),
                )
            };
            let board_before = item_ids(&board);
            let (state, items, costs, carried) = attempt(&mut board, 1);
            assert_eq!(state, AttemptState::Failed);
            assert_eq!(carried, 1, "java = {java}");
            assert_eq!(item_ids(&board), board_before, "nothing was ripped");
            if java {
                assert_eq!(items, [earlier, added].into_iter().collect());
                assert_eq!(costs, vec![(earlier, 9), (added, 7)]);
            } else {
                assert_eq!(items, [earlier].into_iter().collect());
                assert_eq!(costs, vec![(earlier, 5)]);
            }
            // On an active layer the empty path is routed after the rip: the
            // set stays as the locate left it, and nothing counts.
            let (state, items, costs, carried) = attempt(&mut board, 0);
            assert_eq!(state, AttemptState::Routed);
            assert_eq!(carried, 0);
            assert_eq!(items, [earlier, added].into_iter().collect());
            assert_eq!(costs, vec![(earlier, 9), (added, 7)]);
        }
        run_stats::disable();
        run_stats::reset();
    }

    #[test]
    fn an_active_layer_or_a_plane_layer_is_not_a_disabled_signal_layer() {
        let (mut board, mut control, _) = board_and_control();
        assert!(!is_disabled_signal_layer(&board, &control, 1));
        control.layer_active[1] = false;
        assert!(is_disabled_signal_layer(&board, &control, 1));
        board.layer_structure.layers[1].is_signal = false;
        assert!(!is_disabled_signal_layer(&board, &control, 1));
        assert!(!is_disabled_signal_layer(&board, &control, -1));
    }

    #[test]
    fn an_empty_located_path_on_one_layer_is_routed_without_new_items() {
        let (mut board, control, _) = board_and_control();
        let before = item_ids(&board);
        let empty = BTreeSet::new();
        let result = finish_connection(
            &mut board,
            &empty,
            &empty,
            &control,
            true,
            Some(located(Vec::new(), 0, 0)),
            &empty,
            None,
        )
        .0;
        assert_eq!(result, AttemptResult::new(AttemptState::Routed));
        assert_eq!(item_ids(&board), before);
    }

    #[test]
    fn an_empty_located_path_across_layers_fails_like_javas_null_pointer() {
        let (mut board, control, _) = board_and_control();
        let empty = BTreeSet::new();
        let result = finish_connection(
            &mut board,
            &empty,
            &empty,
            &control,
            true,
            Some(located(Vec::new(), 1, 0)),
            &empty,
            None,
        )
        .0;
        assert_eq!(result, AttemptResult::new(AttemptState::Failed));
    }

    #[test]
    fn no_search_result_and_no_located_path_fail_with_javas_details() {
        let (mut board, control, net) = board_and_control();
        let start: BTreeSet<ItemId> = [pin_of_net(&board, net)].into_iter().collect();
        let empty = BTreeSet::new();
        let none = finish_connection(
            &mut board, &start, &empty, &control, false, None, &empty, None,
        )
        .0;
        assert_eq!(
            none.details,
            "Failed to route connection between pin of component #2 and , because no \
             connection was found between their nets."
        );
        let unlocated = finish_connection(
            &mut board, &start, &empty, &control, true, None, &empty, None,
        )
        .0;
        assert_eq!(
            unlocated.details,
            "Failed to route connection between pin of component #2 and ."
        );
    }

    #[test]
    fn pin_edge_to_turn_dist_is_restored_after_a_failed_insert() {
        let (mut board, control, _) = board_and_control();
        board.rules.set_pin_edge_to_turn_dist(55.0);
        // From the pin of U1 out of the board: no shove gets past the outline.
        let trace = ResultItem {
            half_widths: Vec::new(),
            corners: vec![IntPoint::new(20000, 20000), IntPoint::new(20000, 60000)],
            layer: 0,
        };
        let result = insert_segments(&mut board, &control, None, None, 0, 0, &[trace], None);
        assert_eq!(result, Err(InsertError::Failed));
        assert_eq!(board.rules.pin_edge_to_turn_dist(), 55.0);
    }

    /// Stops from check `.1` on (Java's `isStopRequested` at the four points
    /// of `init`).
    struct StopAtCheck(std::cell::Cell<u32>, u32);

    impl Stoppable for StopAtCheck {
        fn request_stop(&self) {}

        fn is_stop_requested(&self) -> bool {
            self.0.set(self.0.get() + 1);
            self.0.get() >= self.1
        }
    }

    /// U1 of net `A` is a through-hole pin: one start room per layer.
    fn tht_board() -> (RoutingBoard, AutorouteControl, i32) {
        let text = BOARD
            .replace(
                "(image SMD (pin SmdPad 1 0 0))",
                "(image SMD (pin SmdPad 1 0 0)) (image THT (pin V600 1 0 0))",
            )
            .replace(
                "(component SMD (place U1 2000 2000 front 0)",
                "(component THT (place U1 2000 2000 front 0)) (component SMD",
            );
        let mut input = JobInput::new(text.into_bytes(), "unit-connection-tht");
        input.rules = RulesChoice::None;
        let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).expect("the board loads");
        let board = prepared.board;
        let net = board
            .rules
            .nets
            .get_by_name("A", 1)
            .expect("net A")
            .net_number;
        let control = AutorouteControl::for_net_default(&board, net, &prepared.board_settings)
            .expect("the control");
        (board, control, net)
    }

    fn room_keys(board: &RoutingBoard, class: i32) -> Vec<i32> {
        let tree = board.search_trees().tree_by_class(class).expect("the tree");
        (-1000..1000)
            .filter(|id| tree.room(crate::board::search::RoomId(*id)).is_some())
            .collect()
    }

    #[test]
    fn an_init_failure_leaks_its_rooms_and_a_later_session_moves_them_aside() {
        let (mut board, mut control, net) = tht_board();
        // Java's leak, the `true` half: profile `enhanced` fixes it since N06.13.
        control.quirks.init_failure_room_leak = true;
        let class = control.trace_clearance_class_index;
        let pins: Vec<ItemId> = board
            .items()
            .filter(|i| i.is_pin() && i.contains_net(net))
            .map(|i| i.id())
            .collect();
        let tht = pins
            .iter()
            .copied()
            .find(|id| {
                board
                    .item(*id)
                    .is_some_and(|i| i.first_layer() != i.last_layer())
            })
            .expect("the through-hole pin");
        let start: BTreeSet<ItemId> = [tht].into_iter().collect();
        let dest: BTreeSet<ItemId> = pins.iter().copied().filter(|id| *id != tht).collect();
        let settings = SearchSettings::default();
        // The first stop check after a start room was completed: the init
        // fails there and the room stays in the tree.
        let mut leaked = Vec::new();
        for check in 1..20 {
            let stop = StopAtCheck(std::cell::Cell::new(0), check);
            let limits = RouteLimits {
                settings,
                stop: Some(&stop),
                work_limit: None,
            };
            let mut ripped = BTreeSet::new();
            let (result, _) = route_connection(
                &mut board,
                &start,
                &dest,
                &control,
                limits,
                Ripped {
                    items: &mut ripped,
                    costs: None,
                },
                crate::autoroute::session::sink::NoopSink,
            );
            assert_eq!(result.state, AttemptState::Failed);
            leaked = room_keys(&board, class);
            if !leaked.is_empty() {
                assert!(
                    result.details.ends_with("could not be created."),
                    "{}",
                    result.details
                );
                break;
            }
        }
        // PARITY(quirk, java_quirks.init_failure_room_leak): the completed start rooms stay in the tree.
        assert!(!leaked.is_empty(), "the init failure leaks its rooms");
        assert!(leaked.iter().all(|id| *id > 0));

        let mut ripped = BTreeSet::new();
        let (result, _) = route_connection(
            &mut board,
            &start,
            &dest,
            &control,
            RouteLimits::unlimited(settings),
            Ripped {
                items: &mut ripped,
                costs: None,
            },
            crate::autoroute::session::sink::NoopSink,
        );
        // The leaked rooms are obstacles around the start pin: this search
        // finds no way out (Java's stale doors into them are not reproduced,
        // see the module docs).
        assert_eq!(result.state, AttemptState::Failed, "{}", result.details);
        // The second session cleared its own rooms; the leaked ones remain,
        // moved to free keys where the second session reused their ids.
        let after = room_keys(&board, class);
        assert_eq!(after.len(), leaked.len(), "{leaked:?} {after:?}");
        assert!(after.iter().any(|id| *id < 0), "{leaked:?} {after:?}");
    }

    #[test]
    fn min_trace_width_floors_the_pin_neckdown_and_unset_changes_nothing() {
        let mut input = JobInput::new(BOARD.as_bytes().to_vec(), "unit-connection");
        input.rules = RulesChoice::None;
        // Java's floor alone: profile `legacy` (enhanced adds Q18's class floor).
        input.extra_sources.push((
            crate::settings::Source::Testing,
            crate::settings::profile::legacy_pin(),
        ));
        let prepared = prepare_job(&input, &ResolveContext { cores: 1 }).expect("the board loads");
        let board = prepared.board;
        let net = board
            .rules
            .nets
            .get_by_name("A", 1)
            .expect("net A")
            .net_number;
        let pin = pin_of_net(&board, net);
        let unset = AutorouteControl::for_net_default(&board, net, &prepared.board_settings)
            .expect("the control");
        assert_eq!(unset.min_trace_half_width, 0);
        // The 800 um pad: (int) max(0.5 * 8000 - 1, 1).
        assert_eq!(unset.neckdown_half_width(&board, pin, 0), 3999);
        let with_floor = |um: f64| {
            let mut settings = prepared.settings.clone();
            settings.min_trace_width_um = Some(um);
            let router = crate::settings::board::BoardRouterSettings::from_settings(&settings);
            AutorouteControl::for_net_default(&board, net, &router).expect("the control")
        };
        // 150 um at 10 units per um: half of 1500.
        let low = with_floor(150.0);
        assert_eq!(low.min_trace_half_width, 750);
        assert_eq!(low.neckdown_half_width(&board, pin, 0), 3999);
        // 1000.05 um: half of 10000.5, rounded up.
        let high = with_floor(1000.05);
        assert_eq!(high.min_trace_half_width, 5001);
        assert_eq!(high.neckdown_half_width(&board, pin, 0), 5001);
        assert_eq!(with_floor(0.0).min_trace_half_width, 0);
    }
}

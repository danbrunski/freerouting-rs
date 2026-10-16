//! What the router tells its listeners, and how they stop it.
//!
//! Port of `autoroute/events/*` (`BoardUpdatedEvent`, `BoardSnapshotEvent`,
//! `TaskStateChangedEvent`), `core/RouterCounters` and the two-level stop of
//! `core/StoppableThread`, at `cda540f`.
//!
//! **Events.** The router sends [`RouterEvent`]s through a [`RouterEvents`]
//! sink. [`EventSink`] wraps a bounded `std::sync::mpsc::sync_channel`, for
//! the GUI; [`NullEvents`] drops everything, for the CLI and tests. A
//! [`RouterEvent::Progress`] is sent with `try_send` and dropped when the
//! channel is full, so a slow listener never holds the router up;
//! [`RouterEvent::Delta`], [`RouterEvent::Replaced`] and
//! [`RouterEvent::Finished`] block until there is room, so a listener that
//! keeps up sees every board change. The router emits at work boundaries
//! only (after a connection, a pin or a pass), never on a timer, so the
//! cadence does not depend on time and never changes a result.
//!
//! [`RouterEvent::Replaced`] is sent whenever the router swaps in another
//! board: a history restore or the final best-board restore. Listeners
//! rebuild from it and never assume the board object stays the same. Java's
//! `de7a4fd2c` (after the pin, port ledger "Port: yes") fixed a GUI that
//! kept the board from before the first restore and saved a stale one.
//!
//! The mapping of Java's `RouterCounters` onto [`Progress`]:
//!
//! | Java `RouterCounters` | `Progress` |
//! |---|---|
//! | `phase` (`"fanout"`, `"autoroute"`; null in the optimizer) | `phase` ([`Phase`]) |
//! | `passCount` | `pass` |
//! | `queuedToBeRoutedCount` | `items_to_go` |
//! | `routedCount` | `routed` |
//! | `skippedCount` | `skipped` |
//! | `failedToBeRoutedCount` | `failed` |
//! | `rippedCount` | `ripped` |
//! | `fanoutExtraViasCount` | `extra_vias` |
//! | `incompleteCount` | `incomplete` |
//!
//! Java's counters are nullable `Integer`s that the GUI reads as 0 when
//! null; the port's are plain numbers starting at 0. `violations` and
//! `score` come from the `BoardStatistics` of `BoardUpdatedEvent`, the
//! airline from `BatchAutorouter.getAirLine`, and the optimizer position
//! from `BatchOptimizer` (O06). `TaskStateChangedEvent`'s end states are
//! [`CompletionStatus`] in [`RunSummary`]; its board hash is not carried.
//!
//! **Cancel.** [`CancelToken`] has `StoppableThread`'s two levels: a router
//! stop ends the routing passes and lets the optimizer run; a full stop
//! ends everything. Its [`Stoppable`] implementation has the full-stop
//! meaning, which is what Java's maze search and pull-tight check.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};

use freerouting_geometry::{FloatPoint, Stoppable};

use crate::board::BoardSnapshot;
use crate::board::items::ItemId;
use crate::board::routing_board::RoutingBoard;

/// The drawable geometry of an item added by a [`RouterEvent::Delta`].
///
/// A placeholder until B08.1 ports `ItemGeometry`: for now a delta names
/// the added items by id, and listeners read their geometry from the board.
pub type ItemGeometry = ItemId;

/// The stage the router is in (Java `RouterCounters.phase`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Phase {
    /// The fanout passes (`"fanout"`).
    Fanout,
    /// The routing passes (`"autoroute"`).
    #[default]
    Autoroute,
    /// The optimizer passes (Java leaves the phase null).
    Optimize,
}

/// The router's progress counters (Java `RouterCounters` with the statistics
/// and positions of `BoardUpdatedEvent`; see the module docs for the
/// mapping).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Progress {
    /// The running stage.
    pub phase: Phase,
    /// The pass number.
    pub pass: i32,
    /// Items still queued in this pass.
    pub items_to_go: usize,
    /// Items routed in this pass.
    pub routed: usize,
    /// Items skipped in this pass (already connected, no unconnected nets,
    /// connected to a plane).
    pub skipped: usize,
    /// Items that failed to route in this pass.
    pub failed: usize,
    /// Items ripped up in this pass.
    pub ripped: usize,
    /// Extra vias the fanout inserted in this pass.
    pub extra_vias: usize,
    /// Connections still incomplete.
    pub incomplete: usize,
    /// Clearance violations on the board.
    pub violations: usize,
    /// The board's router score.
    pub score: f32,
    /// The two endpoints of the airline being routed (Java
    /// `BatchAutorouter.getAirLine`), if any.
    pub airline: Option<(FloatPoint, FloatPoint)>,
    /// Where the optimizer is working (a via centre or a trace's first
    /// corner, O06), if it runs.
    pub optimizer_position: Option<FloatPoint>,
}

/// How a run ended (Java's `completionStatus` of the
/// `Auto-routing stage ...` log line).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CompletionStatus {
    /// `completed:`
    #[default]
    Completed,
    /// `completed with timeout:`
    CompletedWithTimeout,
    /// `cancelled:` (the user stopped the job)
    Cancelled,
    /// `interrupted:` (stopped otherwise)
    Interrupted,
}

/// What a run reports at the end (Java's `Auto-routing stage %s started
/// with %d unrouted nets, completed in %.2f seconds, final score: %s`,
/// `RoutingJobSchedulerActionThread.java:165-179`). The times and resource
/// figures of that line are not carried: they are not results.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RunSummary {
    /// How the run ended.
    pub status: CompletionStatus,
    /// Unrouted connections when the routing started (Java
    /// `getInitialUnroutedCount`).
    pub initial_unrouted: usize,
    /// Routing passes completed.
    pub passes: i32,
    /// The final board's router score.
    pub score: f32,
    /// Incomplete connections on the final board.
    pub incomplete: usize,
    /// Clearance violations on the final board.
    pub violations: usize,
    /// The resolved stage selection as `(settings key, stage id)` pairs
    /// (A10.2).
    pub stages: Vec<(String, String)>,
}

/// One message from the router to its listeners.
#[derive(Debug)]
pub enum RouterEvent {
    /// The counters after a work step (Java `BoardUpdatedEvent`). Dropped
    /// when the channel is full.
    Progress(Progress),
    /// Items removed from and added to the board since the last event.
    Delta {
        /// The removed items.
        removed: Vec<ItemId>,
        /// The added items.
        added: Vec<ItemGeometry>,
    },
    /// The router swapped in another board (a history restore or the final
    /// best-board restore; Java `BoardSnapshotEvent`). Listeners rebuild
    /// from the snapshot.
    Replaced {
        /// The new board.
        snapshot: Box<BoardSnapshot>,
    },
    /// The run ended: the board the run reports, moved to the listener, and
    /// the summary.
    Finished {
        /// The final board.
        board: Box<RoutingBoard>,
        /// What the run reports.
        summary: RunSummary,
    },
}

/// Where the router sends its events.
///
/// The router calls these at work boundaries only. The methods never fail:
/// a listener that has gone away is ignored.
pub trait RouterEvents {
    /// True if events reach someone. The router skips building deltas and
    /// snapshots when this is false.
    fn is_listening(&self) -> bool;

    /// Sends a progress update, dropped when the listener is behind.
    /// Returns whether it was sent.
    fn progress(&self, progress: Progress) -> bool;

    /// Sends a board change, waiting for room.
    fn delta(&self, removed: Vec<ItemId>, added: Vec<ItemGeometry>);

    /// Announces a board replacement, waiting for room.
    fn replaced(&self, snapshot: BoardSnapshot);

    /// Hands the final board to the listener, waiting for room. Returns the
    /// board when no listener took it, so it is never lost.
    fn finished(&self, board: RoutingBoard, summary: RunSummary) -> Option<RoutingBoard>;
}

/// A [`RouterEvents`] sink that drops every event (for the CLI and tests).
#[derive(Clone, Copy, Debug, Default)]
pub struct NullEvents;

impl RouterEvents for NullEvents {
    fn is_listening(&self) -> bool {
        false
    }

    fn progress(&self, _progress: Progress) -> bool {
        false
    }

    fn delta(&self, _removed: Vec<ItemId>, _added: Vec<ItemGeometry>) {}

    fn replaced(&self, _snapshot: BoardSnapshot) {}

    fn finished(&self, board: RoutingBoard, _summary: RunSummary) -> Option<RoutingBoard> {
        Some(board)
    }
}

/// A [`RouterEvents`] sink over a bounded channel.
#[derive(Clone, Debug)]
pub struct EventSink {
    sender: SyncSender<RouterEvent>,
}

impl EventSink {
    /// A sink and its receiver over a channel holding at most `bound`
    /// events. With a bound of 0 every send is a rendezvous, so `Progress`
    /// only arrives while the receiver is waiting.
    pub fn channel(bound: usize) -> (EventSink, Receiver<RouterEvent>) {
        let (sender, receiver) = sync_channel(bound);
        (EventSink { sender }, receiver)
    }

    /// A sink over an existing sender.
    pub fn new(sender: SyncSender<RouterEvent>) -> Self {
        EventSink { sender }
    }
}

impl RouterEvents for EventSink {
    fn is_listening(&self) -> bool {
        true
    }

    fn progress(&self, progress: Progress) -> bool {
        match self.sender.try_send(RouterEvent::Progress(progress)) {
            Ok(()) => true,
            Err(TrySendError::Full(_) | TrySendError::Disconnected(_)) => false,
        }
    }

    fn delta(&self, removed: Vec<ItemId>, added: Vec<ItemGeometry>) {
        // A listener that hung up is not an error for the router.
        let _ = self.sender.send(RouterEvent::Delta { removed, added });
    }

    fn replaced(&self, snapshot: BoardSnapshot) {
        let _ = self.sender.send(RouterEvent::Replaced {
            snapshot: Box::new(snapshot),
        });
    }

    fn finished(&self, board: RoutingBoard, summary: RunSummary) -> Option<RoutingBoard> {
        let event = RouterEvent::Finished {
            board: Box::new(board),
            summary,
        };
        match self.sender.send(event) {
            Ok(()) => None,
            Err(error) => match error.0 {
                RouterEvent::Finished { board, .. } => Some(*board),
                _ => unreachable!("the returned event is the one sent"),
            },
        }
    }
}

const STOP_NONE: u8 = 0;
const STOP_ROUTER_ONLY: u8 = 1;
const STOP_ALL: u8 = 2;

/// A two-level stop request (Java `StoppableThread`'s `StopRequestState`
/// `NONE`, `AUTO_ROUTER_ONLY` and `ALL`).
///
/// Share it as `Arc<CancelToken>`; it is an atomic, with no lock. A
/// deadline is a caller that calls [`stop_router`](Self::stop_router) or
/// [`stop_all`](Self::stop_all) when it passes: a stop only ends work, it
/// never changes how work is done.
#[derive(Debug, Default)]
pub struct CancelToken {
    state: AtomicU8,
}

impl CancelToken {
    /// A token with no stop requested.
    pub fn new() -> Self {
        CancelToken::default()
    }

    /// Asks the routing passes to stop while the other stages go on (Java
    /// `requestStopAutoRouter`); no effect once a full stop is set.
    pub fn stop_router(&self) {
        let _ = self.state.compare_exchange(
            STOP_NONE,
            STOP_ROUTER_ONLY,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
    }

    /// Asks everything to stop: fanout, routing and optimizer (Java
    /// `requestStop`).
    pub fn stop_all(&self) {
        self.state.store(STOP_ALL, Ordering::Release);
    }

    /// True for a router stop or a full stop (Java
    /// `isStopAutoRouterRequested`).
    pub fn is_router_stop_requested(&self) -> bool {
        self.state.load(Ordering::Acquire) != STOP_NONE
    }

    /// True for a full stop only (Java `isStopRequested`).
    pub fn is_stop_requested(&self) -> bool {
        self.state.load(Ordering::Acquire) == STOP_ALL
    }
}

impl Stoppable for CancelToken {
    /// A full stop ([`CancelToken::stop_all`]).
    fn request_stop(&self) {
        self.stop_all();
    }

    /// True for a full stop only ([`CancelToken::is_stop_requested`]).
    fn is_stop_requested(&self) -> bool {
        CancelToken::is_stop_requested(self)
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;
    use std::thread;
    use std::time::Duration;

    use super::*;
    use crate::io::{BoardReadResult, read_board};

    fn load(name: &str) -> RoutingBoard {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(name);
        match read_board(&std::fs::read(path).unwrap()) {
            BoardReadResult::Success { board, .. } => *board,
            _ => panic!("{name} must load"),
        }
    }

    fn progress(pass: i32) -> Progress {
        Progress {
            pass,
            ..Progress::default()
        }
    }

    #[test]
    fn full_channel_drops_progress() {
        let (sink, receiver) = EventSink::channel(2);
        assert!(sink.is_listening());
        assert!(sink.progress(progress(1)));
        assert!(sink.progress(progress(2)));
        assert!(!sink.progress(progress(3)), "the third does not fit");
        let passes: Vec<i32> = receiver
            .try_iter()
            .map(|event| match event {
                RouterEvent::Progress(p) => p.pass,
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert_eq!(passes, vec![1, 2]);
        assert!(sink.progress(progress(4)), "room again after reading");
        drop(receiver);
        assert!(!sink.progress(progress(5)), "nobody listens");
    }

    #[test]
    fn delta_and_replaced_block_until_read() {
        let board = load("empty_board.dsn");
        let snapshot = board.snapshot();
        let items = snapshot.item_count();
        let (sink, receiver) = EventSink::channel(1);
        assert!(sink.progress(progress(1)));
        let sent = Arc::new(AtomicBool::new(false));
        let writer = {
            let sink = sink.clone();
            let sent = Arc::clone(&sent);
            thread::spawn(move || {
                sink.delta(vec![ItemId(3)], vec![ItemId(4), ItemId(5)]);
                assert!(!sink.progress(progress(2)), "full again: dropped");
                sink.replaced(snapshot);
                sent.store(true, Ordering::SeqCst);
            })
        };
        // The channel is full, so the writer waits in `delta` until the
        // reader takes the progress event. (The pause only makes a missing
        // block visible; a correct sink never fails here.)
        thread::sleep(Duration::from_millis(50));
        assert!(
            !sent.load(Ordering::SeqCst),
            "delta must block on a full channel"
        );
        let mut events = Vec::new();
        for _ in 0..3 {
            events.push(receiver.recv().unwrap());
        }
        writer.join().unwrap();
        assert!(sent.load(Ordering::SeqCst));
        assert!(matches!(events[0], RouterEvent::Progress(ref p) if p.pass == 1));
        match &events[1] {
            RouterEvent::Delta { removed, added } => {
                assert_eq!(removed, &vec![ItemId(3)]);
                assert_eq!(added, &vec![ItemId(4), ItemId(5)]);
            }
            other => panic!("expected a delta, got {other:?}"),
        }
        match &events[2] {
            RouterEvent::Replaced { snapshot } => assert_eq!(snapshot.item_count(), items),
            other => panic!("expected a replacement, got {other:?}"),
        }
        assert!(receiver.try_recv().is_err());
    }

    #[test]
    fn finished_moves_the_board_to_the_listener() {
        let board = load("empty_board.dsn");
        let items = board.items().count();
        let summary = RunSummary {
            status: CompletionStatus::Interrupted,
            initial_unrouted: 7,
            passes: 3,
            score: 12.5,
            incomplete: 2,
            violations: 1,
            stages: vec![(
                "router.autorouter.algorithm".to_owned(),
                "freerouting-router".to_owned(),
            )],
        };
        let (sink, receiver) = EventSink::channel(1);
        let reader = thread::spawn(move || receiver.recv().unwrap());
        assert!(sink.finished(board, summary.clone()).is_none(), "taken");
        match reader.join().unwrap() {
            RouterEvent::Finished { board, summary: s } => {
                assert_eq!(board.items().count(), items);
                assert_eq!(s, summary);
            }
            other => panic!("expected finished, got {other:?}"),
        }

        // Nobody listening: the board comes back.
        let (sink, receiver) = EventSink::channel(1);
        drop(receiver);
        let back = sink.finished(load("empty_board.dsn"), RunSummary::default());
        assert_eq!(back.map(|b| b.items().count()), Some(items));
    }

    #[test]
    fn null_events_drop_everything_but_return_the_board() {
        let events = NullEvents;
        assert!(!events.is_listening());
        assert!(!events.progress(progress(1)));
        events.delta(vec![ItemId(1)], Vec::new());
        let board = load("empty_board.dsn");
        events.replaced(board.snapshot());
        let items = board.items().count();
        let back = events.finished(board, RunSummary::default()).unwrap();
        assert_eq!(back.items().count(), items);
    }

    #[test]
    fn sink_works_as_a_trait_object() {
        let (sink, receiver) = EventSink::channel(4);
        let sinks: [&dyn RouterEvents; 2] = [&sink, &NullEvents];
        for s in sinks {
            s.progress(progress(9));
        }
        assert_eq!(receiver.try_iter().count(), 1);
    }

    #[test]
    fn stoppable_has_the_full_stop_meaning() {
        let token = CancelToken::new();
        token.stop_router();
        assert!(!Stoppable::is_stop_requested(&token));
        assert!(token.is_router_stop_requested());
        Stoppable::request_stop(&token);
        assert!(Stoppable::is_stop_requested(&token));
        token.stop_router();
        assert!(
            token.is_stop_requested(),
            "a router stop never lowers a full stop"
        );
    }

    #[test]
    fn token_is_shared_across_threads() {
        let token = Arc::new(CancelToken::new());
        let worker = {
            let token = Arc::clone(&token);
            thread::spawn(move || {
                while !token.is_router_stop_requested() {
                    thread::yield_now();
                }
                token.is_stop_requested()
            })
        };
        token.stop_router();
        assert!(!worker.join().unwrap());
    }
}

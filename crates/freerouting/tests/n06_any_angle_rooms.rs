//! N06.16: `router.java_quirks.any_angle_room_side_index` on the smallest case
//! that reproduces Java's unbounded room growth: pass 1 of the fanout of A07's
//! `Issue026-J2_reference-any` (`(snap_angle none)`).
//!
//! Java at `a773a70ec` (and the port under `true`) takes an any-angle room
//! neighbour's touching side on the room's own shape, an `IntOctagon` with 8
//! border lines, and walks the gap between two neighbours on the room's simplex,
//! which has fewer. A side past the simplex's last line is never reached, so the
//! walk adds incomplete rooms until memory runs out: Java's command line ends in
//! `OutOfMemoryError` under `-Xmx4g` in this fanout pass, the port's under a 4 GB
//! address-space cap within 5 s. That arm is not run here; its pure half is
//! `neighbours_any::tests::room_side_index_switch_both_behaviours`.
//!
//! Under `false` every side is one of the simplex's lines, a gap walk ends within
//! `n + 1` steps (a debug assertion in `gap_rooms_any`, on in tests), and the pass
//! ends with the rooms it completed counted. Its own test binary because the run
//! statistics are process-wide.
mod autoroute_common;

use autoroute_common::load;
use freerouting::autoroute::batch::fanout::{FanoutRun, NoFanoutObserver};
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::stages::StageCtx;
use freerouting::run_stats::{self, Counter, Counts};
use freerouting::settings::JavaQuirks;

/// A07's any-angle variant of `Issue026-J2_reference`.
const J2_ANY: &str =
    "crates/freerouting/tests/data/autoroute-variants/Issue026-J2_reference-any.dsn";

/// The rooms one fanout pass of `J2_ANY` may complete under the fix: far above
/// what it takes (2,210), and far below the millions Java's gap walk adds before it
/// runs out of memory.
const ROOM_LIMIT: u64 = 20_000;

#[test]
fn any_angle_fanout_pass_stays_bounded_with_the_fix() {
    let mut loaded = load(J2_ANY);
    let quirks = JavaQuirks::JAVA
        .with("any_angle_room_side_index", false)
        .unwrap();
    loaded.board.set_java_quirks(quirks);
    loaded.router = loaded.router.with_java_quirks(quirks);
    loaded.settings.fanout.max_passes = 1;
    let cancel = CancelToken::new();
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    run_stats::enable();
    let mut run = FanoutRun::new(&loaded.board, &ctx);
    let summary = run.run(&mut loaded.board, &mut ctx, &mut NoFanoutObserver);
    let counts = Counts::now();
    run_stats::disable();
    let mismatches = counts.get(Counter::AnyAngleRoomSideMismatches);
    let rooms = counts.get(Counter::RoomsCompleted);
    println!(
        "{J2_ANY} fanout pass 1, fixed: {summary:?}, rooms {rooms}, side mismatches {mismatches}, \
         connections {} routed {}",
        counts.get(Counter::ConnectionsAttempted),
        counts.get(Counter::ConnectionsRouted)
    );
    assert_eq!(summary.completed_passes, 1);
    assert!(
        mismatches > 0,
        "the pass reaches a room whose sides Java indexes wrongly"
    );
    assert!(rooms > 0 && rooms < ROOM_LIMIT, "rooms completed: {rooms}");
    assert!(summary.escaped > 0, "{summary:?}");
    assert_eq!(
        Counter::AnyAngleRoomSideMismatches.name(),
        "any_angle_room_side_mismatches"
    );
}

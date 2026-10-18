//! N06.12, the (c) rows: both behaviours of `ripup_pass_no_unset` on real batch passes,
//! with its reachability counters, in their own process so that no other test resets
//! the process-wide counters (`run_stats`).
//!
//! `MazeRipupResolver.java:158-162` (Java at `a773a70ec`) multiplies an obstacle's
//! detour by the random factor `0.5 + r * r` from ripup pass 4 on, outside every third
//! pass, but the batch router never sets `AutorouteControl.ripupPassNo`
//! (`AutorouteControl.java:192` leaves it 1), so Java never draws it. `true` (unset) is
//! Java's behaviour; `false` sets the batch pass number, and the factor fires. The
//! generator is the search's, reseeded with the ripup costs before each connection, so
//! the fixed arm is deterministic.
//!
//! The other (c) rows' tests sit with their code: `any_angle::tests::
//! skip_length_switch_both_behaviours_and_counter`, `optimize::batch::tests::
//! threshold_fraction_switch_both_behaviours` and the CLI's single scaling in
//! `tests/settings_path.rs`, the unknown layer scope in `tests/rules_reader.rs`, and the
//! side-0 flank in `tests/n06_shove.rs`.

mod autoroute_common;

use std::sync::Mutex;

use autoroute_common::load;
use freerouting::autoroute::batch::connection::FreeroutingConnectionRouter;
use freerouting::autoroute::batch::ordering::FreeroutingOrdering;
use freerouting::autoroute::batch::pass::{PassRunner, PassStages};
use freerouting::autoroute::batch::ripup_policy::FreeroutingRipupPolicy;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::session::ripup::draws_random_factor;
use freerouting::autoroute::stages::StageCtx;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::run_stats::{self, Counter, Counts};
use freerouting::settings::JavaQuirks;

/// The counters are process-wide: the tests that read them run one at a time.
static SERIAL: Mutex<()> = Mutex::new(());

/// 10 passes in A14 to 0 unrouted (5.1 s in release): pass 1 leaves 25 connections
/// incomplete, and the later passes rip (a pass numbered 4 after pass 1 rips about 10
/// items in either arm).
const BOARD: &str = "fixtures/Issue034-Green14SegLED.dsn";

/// What one arm's passes gave.
#[derive(Debug, PartialEq)]
struct Run {
    /// Per pass after the first: the counts of that pass.
    counts: Vec<(u32, u64, u64)>,
    /// The session after the last pass.
    ses: String,
}

/// Pass 1, then the passes `later` (their numbers decide the ripup costs, the work
/// budget and, under `false`, the random factor), with `ripup_pass_no_unset = java`.
fn run(java: bool, later: &[u32]) -> Run {
    let mut loaded = load(BOARD);
    let quirks = JavaQuirks::JAVA.with("ripup_pass_no_unset", java).unwrap();
    loaded.router = loaded.router.with_java_quirks(quirks);
    loaded.board.set_java_quirks(quirks);
    let mut runner = PassRunner::for_job(&loaded.settings, &loaded.router);
    let mut ordering = FreeroutingOrdering::new();
    let mut router = FreeroutingConnectionRouter::freerouting();
    let mut ripup = FreeroutingRipupPolicy::from_settings(&loaded.settings);
    let cancel = CancelToken::new();
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    let mut counts = Vec::new();
    for pass in std::iter::once(1).chain(later.iter().copied()) {
        let mut stages = PassStages {
            ordering: &mut ordering,
            router: &mut router,
            ripup: &mut ripup,
        };
        let before = Counts::now();
        let outcome = runner.run_pass(&mut loaded.board, pass, &mut stages, &mut ctx);
        let since = Counts::now().since(&before);
        if pass == 1 {
            assert!(outcome.incomplete > 0, "pass 1 must leave connections");
        } else {
            counts.push((
                pass,
                since.get(Counter::RipupRandomPassConnections),
                since.get(Counter::RipupRandomFactorDraws),
            ));
        }
    }
    Run {
        counts,
        ses: write_ses(&loaded.board, "Issue093-interf_u", SesStyle::Java),
    }
}

#[test]
fn the_random_factor_fires_from_pass_4_outside_every_third_pass() {
    let draws: Vec<u32> = (1..=10)
        .filter(|&p| draws_random_factor(p as i32))
        .collect();
    assert_eq!(draws, [4, 5, 7, 8, 10]);
}

/// Pass 1, then a pass numbered 4 (draws) and one numbered 6 (a multiple of 3, no
/// draw). Both arms count pass 4's connections; only `false` draws, and only in pass
/// 4. Pass 1 is the same in both arms, so the boards differ only through pass 4's
/// random factor; `false` repeats itself exactly.
#[test]
fn ripup_pass_no_switch_both_behaviours_and_counters() {
    let _guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
    run_stats::enable();
    let java = run(true, &[4, 6]);
    let fixed = run(false, &[4, 6]);
    let again = run(false, &[4, 6]);
    run_stats::disable();
    run_stats::reset();

    let (_, java_connections, java_draws) = java.counts[0];
    assert!(java_connections > 0, "{:?}", java.counts);
    assert_eq!(java_draws, 0, "Java's pass number stays 1");
    let (_, fixed_connections, fixed_draws) = fixed.counts[0];
    assert!(fixed_connections > 0, "{:?}", fixed.counts);
    assert!(fixed_draws > 0, "{:?}", fixed.counts);
    for run in [&java, &fixed] {
        let (pass, connections, draws) = run.counts[1];
        assert_eq!((pass, connections, draws), (6, 0, 0), "pass 6 never draws");
    }
    assert_ne!(java.ses, fixed.ses, "the random detour changes the routes");
    assert_eq!(
        fixed, again,
        "the reseeded generator makes `false` deterministic"
    );
}

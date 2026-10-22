//! Q23.3: restarts from the best board (`quality::restart`,
//! `router.autorouter.restarts_from_best`).
//!
//! Fixtures from `tests/data/quality/`:
//! - Q06's `q06-sole-wall.dsn`: one layer; the only route from A1 to A2
//!   crosses the routed wire of net B. Pass 1 routes A by ripping B's wire,
//!   and B is then incomplete but routable round A's new trace. With `-mp 1`
//!   the first loop ends there; a restart's pass 1 routes B.
//! - Q06's `q06-fixed-cage.dsn`: a pin caged by fixed copper, which no pass
//!   can reach, so a restart cannot gain.
//!
//! Every run has the optimizer off, so the board compared is the loop's.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::batch::connection::FreeroutingConnectionRouter;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, RouteTask, StageCtx, StageRegistry,
};
use freerouting::board::RoutingBoard;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const SOLE_WALL: &str = "crates/freerouting/tests/data/quality/q06-sole-wall.dsn";
const FIXED_CAGE: &str = "crates/freerouting/tests/data/quality/q06-fixed-cage.dsn";

/// The connection router that requests a full stop at its first connection
/// inside a restart (whose settings have `restarts_from_best` unset), as a
/// timeout or `max_items` does.
const STOP_IN_RESTART: &str = "test-stop-in-restart";

fn prepare(dsn: &str, max_passes: i32, restarts: Option<i32>, router: Option<&str>) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(max_passes);
    profile.patch.autorouter.restarts_from_best = restarts;
    profile.patch.autorouter.connection_router = router.map(str::to_string);
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

/// One routed job: the board, the job log's text and the job's token.
struct Routed {
    board: RoutingBoard,
    log: Vec<String>,
    cancel: Arc<CancelToken>,
}

fn route_with(prepared: PreparedJob, registry: Option<&StageRegistry>) -> Routed {
    let cancel = Arc::new(CancelToken::new());
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::clone(&cancel),
        events: &NullEvents,
        deadline: None,
        registry,
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    Routed {
        board: outcome.board.take().expect("no listener took the board"),
        log: outcome.log.iter().map(|l| l.text.clone()).collect(),
        cancel,
    }
}

fn route(dsn: &str, max_passes: i32, restarts: Option<i32>) -> Routed {
    route_with(prepare(dsn, max_passes, restarts, None), None)
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

fn ses(board: &RoutingBoard) -> String {
    write_ses(board, "restart", SesStyle::Java)
}

fn lines<'a>(log: &'a [String], prefix: &str) -> Vec<&'a String> {
    log.iter().filter(|l| l.starts_with(prefix)).collect()
}

#[test]
fn a_restart_completes_the_connection_the_first_loop_left_open() {
    let off = route(SOLE_WALL, 1, None);
    assert_eq!(incomplete(&off.board), 1, "B is open after pass 1");
    let on = route(SOLE_WALL, 1, Some(1));
    assert_eq!(incomplete(&on.board), 0, "{:#?}", lines(&on.log, "Restart"));
    let starts = lines(&on.log, "Restart #1 from the best board");
    assert_eq!(starts.len(), 1, "{:#?}", on.log);
    let ends = lines(&on.log, "Restart #1 ended");
    assert_eq!(ends.len(), 1);
    assert!(
        ends[0].contains("is kept") && ends[0].contains("(0 unrouted"),
        "{}",
        ends[0]
    );
    let summary = lines(&on.log, "Restarts from the best board");
    assert_eq!(summary.len(), 1);
    assert!(
        summary[0].contains("reached the restart limit: 1 restart(s), 1 kept, unrouted 1 -> 0"),
        "{}",
        summary[0]
    );
    // The restart ran the fanout and the passes again from pass 1.
    let after_start = on
        .log
        .iter()
        .skip_while(|l| !l.starts_with("Restart #1 from"))
        .collect::<Vec<_>>();
    assert!(after_start.iter().any(|l| l.starts_with("Fanout stage")));
    assert!(
        after_start
            .iter()
            .any(|l| l.starts_with("Auto-routing pass #1 "))
    );
}

#[test]
fn a_restart_that_does_not_gain_stops_after_one_and_keeps_the_best_board() {
    let off = route(FIXED_CAGE, 0, None);
    assert_eq!(incomplete(&off.board), 1, "the caged pin stays open");
    let on = route(FIXED_CAGE, 0, Some(3));
    assert_eq!(lines(&on.log, "Restart #").len(), 2, "{:#?}", on.log);
    assert_eq!(lines(&on.log, "Restart #1 from").len(), 1);
    assert!(lines(&on.log, "Restart #2").is_empty());
    let summary = lines(&on.log, "Restarts from the best board");
    assert!(
        summary[0].contains(
            "stopped: a restart did not beat the best board: 1 restart(s), 0 kept, unrouted 1 -> 1"
        ),
        "{}",
        summary[0]
    );
    // The dropped restart leaves the board of the run without restarts.
    assert_eq!(ses(&on.board), ses(&off.board));
}

#[test]
fn unset_zero_and_negative_change_nothing() {
    let unset = route(SOLE_WALL, 1, None);
    for restarts in [0, -2] {
        let run = route(SOLE_WALL, 1, Some(restarts));
        assert_eq!(ses(&run.board), ses(&unset.board), "{restarts}");
        assert_eq!(run.log, unset.log, "{restarts}");
        assert!(lines(&run.log, "Restart").is_empty());
    }
}

#[test]
fn a_restart_is_deterministic() {
    let first = route(SOLE_WALL, 1, Some(2));
    let second = route(SOLE_WALL, 1, Some(2));
    assert_eq!(ses(&first.board), ses(&second.board));
    assert_eq!(first.log, second.log);
    // The board is complete after the first restart, so no second one runs.
    let summary = lines(&first.log, "Restarts from the best board");
    assert!(
        summary[0].contains("stopped: the board is complete: 1 restart(s), 1 kept"),
        "{}",
        summary[0]
    );
}

/// The job's connection router, which requests a full stop of the restart's
/// token before its first connection inside a restart.
struct StopInRestart {
    inner: FreeroutingConnectionRouter,
}

impl ConnectionRouter for StopInRestart {
    fn id(&self) -> &'static str {
        STOP_IN_RESTART
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        if ctx.settings.autorouter.restarts_from_best.is_none() {
            ctx.cancel.stop_all();
        }
        self.inner.route(board, task, p, ctx)
    }
}

#[test]
fn a_stop_during_a_restart_keeps_the_best_board_and_reaches_the_job() {
    let mut registry = StageRegistry::builtin();
    registry.connection_router.insert(STOP_IN_RESTART, |s| {
        Box::new(StopInRestart {
            inner: FreeroutingConnectionRouter::from_settings(s),
        })
    });
    // Outside a restart the test router is Java's, so the run without
    // restarts is the plain one.
    let off = route(SOLE_WALL, 1, None);
    let on = route_with(
        prepare(SOLE_WALL, 1, Some(1), Some(STOP_IN_RESTART)),
        Some(&registry),
    );
    assert_eq!(incomplete(&on.board), 1);
    assert_eq!(ses(&on.board), ses(&off.board));
    let summary = lines(&on.log, "Restarts from the best board");
    assert!(
        summary[0].contains("stopped on request: 1 restart(s), 0 kept, unrouted 1 -> 1"),
        "{}",
        summary[0]
    );
    assert!(on.cancel.is_stop_requested(), "the stop reached the job");
}

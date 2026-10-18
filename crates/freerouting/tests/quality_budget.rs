//! Q08.2: the `earned-budget` connection router (`quality::budget`) off and
//! on. Off (the default router, or a policy that always stops) the
//! routing is Java's; on, it is deterministic and does extend searches that
//! are still closing in when their work limit fires.
//!
//! The work limit of Java's pass 1 rarely fires on a small fixture, so the
//! pass-level tests route with a tight budget (a test router that lowers
//! `ConnectionParams::work_budget` before the router under test sees it).

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::{Loaded, load, repo_root};
use freerouting::autoroute::batch::connection::FreeroutingConnectionRouter;
use freerouting::autoroute::batch::ordering::FreeroutingOrdering;
use freerouting::autoroute::batch::pass::{PassRunner, PassStages};
use freerouting::autoroute::batch::ripup_policy::FreeroutingRipupPolicy;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::session::budget::StopAtLimit;
use freerouting::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, RouteTask, StageCtx, StageRegistry,
};
use freerouting::board::RoutingBoard;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::budget::{BudgetTally, DEFAULT_MAX_EXTENSIONS, EarnedBudget, ROUTER_ID};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const MIN_FR_TEST: &str = "fixtures/Issue269-min_fr_test/min_fr_test.dsn";
const TWO_PIN: &str = "fixtures/scoring-perfect-two-pin.dsn";
/// A board whose searches run out of a 1 ms budget: 6 connections, 10 of
/// Java's attempts over two passes exceed it and 2 stay unrouted.
const RPI_SPLITTER: &str = "fixtures/Issue143-rpi_splitter.dsn";

/// A tight per-connection budget in work units (D5's search-tree nodes):
/// one nominal millisecond, the smallest the limit can express.
const TIGHT_BUDGET: u64 = 64_000;

/// The router a pass runs with.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Arm {
    /// Java's connection router.
    Java,
    /// Java's router through the budget hook with a policy that always
    /// stops.
    StopAtLimit,
    /// `earned-budget`.
    Earned,
}

/// Java's router (or `earned-budget`) with `work_budget` lowered to
/// `budget`.
struct Tight {
    arm: Arm,
    budget: u64,
    java: FreeroutingConnectionRouter,
    earned: EarnedBudget,
    /// Connections whose base budget was exceeded.
    work_limit_hits: usize,
    /// The work units of every connection.
    work_units: u64,
}

impl Tight {
    fn new(arm: Arm, budget: u64) -> Self {
        Tight {
            arm,
            budget,
            java: FreeroutingConnectionRouter::freerouting(),
            earned: EarnedBudget::new(
                FreeroutingConnectionRouter::freerouting(),
                DEFAULT_MAX_EXTENSIONS,
            ),
            work_limit_hits: 0,
            work_units: 0,
        }
    }
}

impl ConnectionRouter for Tight {
    fn id(&self) -> &'static str {
        "tight"
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        let p = ConnectionParams {
            work_budget: self.budget.min(p.work_budget),
            ..p.clone()
        };
        let outcome = match self.arm {
            Arm::Java => self.java.route(board, task, &p, ctx),
            Arm::StopAtLimit => {
                let mut stop = StopAtLimit;
                self.java
                    .route_with_budget(board, task, &p, ctx, Some(&mut stop))
            }
            Arm::Earned => self.earned.route(board, task, &p, ctx),
        };
        self.work_limit_hits += usize::from(outcome.work_limit_hit);
        self.work_units += outcome.work.units;
        outcome
    }
}

/// What `passes` passes of the pass runner leave on `dsn` with `arm`.
struct PassRun {
    hash: u128,
    ses: String,
    work_limit_hits: usize,
    work_units: u64,
    unrouted: usize,
    tally: BudgetTally,
}

fn run_passes(dsn: &str, arm: Arm, budget: u64, passes: u32) -> PassRun {
    let mut loaded: Loaded = load(dsn);
    let mut runner = PassRunner::for_job(&loaded.settings, &loaded.router);
    let mut ordering = FreeroutingOrdering::new();
    let mut router = Tight::new(arm, budget);
    let mut ripup = FreeroutingRipupPolicy::from_settings(&loaded.settings);
    let cancel = CancelToken::new();
    let events = NullEvents;
    let mut ctx = StageCtx {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: &cancel,
        events: &events,
    };
    let mut unrouted = 0;
    for pass in 1..=passes {
        let mut stages = PassStages {
            ordering: &mut ordering,
            router: &mut router,
            ripup: &mut ripup,
        };
        let outcome = runner.run_pass(&mut loaded.board, pass, &mut stages, &mut ctx);
        unrouted = outcome.incomplete;
        if !outcome.continue_routing {
            break;
        }
    }
    PassRun {
        hash: loaded.board.state_hash(),
        ses: write_ses(&loaded.board, dsn, SesStyle::Java),
        work_limit_hits: router.work_limit_hits,
        work_units: router.work_units,
        unrouted,
        tally: router.earned.tally(),
    }
}

#[test]
fn earned_budget_is_registered_under_its_id() {
    let registry = StageRegistry::builtin();
    assert!(registry.connection_router.contains_key(ROUTER_ID));
    let settings = prepare(TWO_PIN, None).settings;
    assert_eq!(
        registry.connection_router[ROUTER_ID](&settings).id(),
        "earned-budget"
    );
}

/// The hook alone changes nothing: a policy that always stops routes
/// exactly as no policy, also where the limit fires.
#[test]
fn a_policy_that_always_stops_routes_like_java() {
    let java = run_passes(RPI_SPLITTER, Arm::Java, TIGHT_BUDGET, 2);
    let stop = run_passes(RPI_SPLITTER, Arm::StopAtLimit, TIGHT_BUDGET, 2);
    assert!(java.work_limit_hits > 0, "the tight budget must fire");
    assert_eq!(stop.hash, java.hash);
    assert_eq!(stop.ses, java.ses);
    assert_eq!(stop.work_units, java.work_units);
    assert_eq!(stop.work_limit_hits, java.work_limit_hits);
}

/// With tight budgets the policy extends searches, and the same input gives
/// the same session and the same tally twice.
#[test]
fn earned_budget_extends_tight_searches_deterministically() {
    let first = run_passes(RPI_SPLITTER, Arm::Earned, TIGHT_BUDGET, 2);
    let second = run_passes(RPI_SPLITTER, Arm::Earned, TIGHT_BUDGET, 2);
    let java = run_passes(RPI_SPLITTER, Arm::Java, TIGHT_BUDGET, 2);
    assert!(first.tally.limits_fired > 0, "{:?}", first.tally);
    assert!(first.tally.extensions > 0, "{:?}", first.tally);
    assert_eq!(
        first.tally.extra_units,
        first.tally.extensions * TIGHT_BUDGET
    );
    assert_eq!(first.tally, second.tally);
    assert_eq!(first.hash, second.hash);
    assert_eq!(first.ses, second.ses);
    assert_eq!(first.work_units, second.work_units);
    // The extensions are real work beyond Java's, and here they pay: the
    // two connections Java's cut-off leaves unrouted are routed.
    assert!(first.work_units > java.work_units);
    assert_eq!((java.unrouted, first.unrouted), (2, 0));
    assert_eq!(
        first.tally,
        BudgetTally {
            searches: 6,
            limits_fired: 9,
            extensions: 8,
            extra_units: 8 * TIGHT_BUDGET,
        }
    );
}

// ---------------------------------------------------------------------------
// The job: the default router against `earned-budget`
// ---------------------------------------------------------------------------

fn prepare(dsn: &str, router: Option<&str>) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(4);
    let mut patch = profile.patch;
    patch.profile = Some("legacy".to_string());
    patch.autorouter.connection_router = router.map(str::to_string);
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn route_ses(dsn: &str, router: Option<&str>) -> String {
    let prepared = prepare(dsn, router);
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome =
        run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{dsn}: {}", f.error));
    let board = outcome.board.take().expect("no listener took the board");
    write_ses(&board, dsn, SesStyle::Java)
}

/// Where Java's budgets never run out, `earned-budget` is never asked and
/// the session is Java's, byte for byte; two runs with it agree.
#[test]
fn where_no_limit_fires_the_job_routes_like_java() {
    for dsn in [TWO_PIN, MIN_FR_TEST] {
        let java = route_ses(dsn, None);
        let earned = route_ses(dsn, Some(ROUTER_ID));
        assert_eq!(earned, java, "{dsn}");
        assert_eq!(route_ses(dsn, Some(ROUTER_ID)), earned, "{dsn}");
    }
}

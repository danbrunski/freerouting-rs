//! Q11: the `transactional-ripup` connection router (`quality::transaction`).
//!
//! The fixtures are synthetic one-layer boards in `tests/data/quality/`:
//! - `q06-sole-wall.dsn` (Q06's): the only route from A1 to A2 crosses B's
//!   routed wire, whose pads sit at the outline; B can go round A's left pad,
//!   so B re-lands;
//! - `q11-locked-cross.dsn`: the same with A's pads at the outline too, so A
//!   rips B and B cannot re-land;
//! - `q11-cycle.dsn`: three routed two-pin nets, the board of the depth-1
//!   termination tests, which drive the transaction with a scripted inner
//!   router that rips in a cycle (A rips B, B rips C, C rips A);
//! - `q06-fixed-cage.dsn` (Q06's): nothing is ripped.

mod autoroute_common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::AttemptState;
use freerouting::autoroute::batch::connection::{FreeroutingConnectionRouter, connection_params};
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, PipelineOutcome, run_pipeline};
use freerouting::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, ConnectionRouter, RouteTask, StageCtx,
};
use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::transaction::{
    CommitRule, TransactionConfig, TransactionStats, TransactionalRipup,
};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const SOLE_WALL: &str = "crates/freerouting/tests/data/quality/q06-sole-wall.dsn";
const LOCKED: &str = "crates/freerouting/tests/data/quality/q11-locked-cross.dsn";
const CYCLE: &str = "crates/freerouting/tests/data/quality/q11-cycle.dsn";
const FIXED_CAGE: &str = "crates/freerouting/tests/data/quality/q06-fixed-cage.dsn";

/// The transaction settings of a run; `router: None` leaves the connection
/// router unset.
#[derive(Clone, Copy, Default)]
struct Arm {
    router: Option<&'static str>,
    rule: Option<&'static str>,
    depth: Option<i32>,
    rip_factor: Option<i32>,
}

const OFF: Arm = Arm {
    router: None,
    rule: None,
    depth: None,
    rip_factor: None,
};

const fn arm(rule: &'static str, depth: i32, rip_factor: i32) -> Arm {
    Arm {
        router: Some("transactional-ripup"),
        rule: Some(rule),
        depth: Some(depth),
        rip_factor: Some(rip_factor),
    }
}

const STRICT: Arm = arm("strict", 0, 0);
const COUNT: Arm = arm("count", 0, 0);
const DEPTH1: Arm = arm("count", 1, 0);
const STRICT_DEPTH1: Arm = arm("strict", 1, 0);
const STRICT_FACTOR: Arm = arm("strict", 0, 2);

/// Every arm of the measurement (Q11.6) with its name.
const ARMS: [(&str, Arm); 5] = [
    ("strict", STRICT),
    ("count", COUNT),
    ("depth1", DEPTH1),
    ("strict depth1", STRICT_DEPTH1),
    ("strict+ripfactor", STRICT_FACTOR),
];

fn prepare(dsn: &str, passes: i32, a: Arm) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(passes);
    profile.patch.autorouter.connection_router = a.router.map(str::to_string);
    profile.patch.transaction.rule = a.rule.map(str::to_string);
    profile.patch.transaction.depth = a.depth;
    profile.patch.transaction.rip_factor = a.rip_factor;
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn run(prepared: PreparedJob) -> Result<PipelineOutcome, String> {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    run_pipeline(prepared.board, &job).map_err(|f| f.error.to_string())
}

/// The routed board and the job log's text.
fn route(dsn: &str, passes: i32, a: Arm) -> (RoutingBoard, Vec<String>) {
    let mut outcome = run(prepare(dsn, passes, a)).expect("the job runs");
    let board = outcome.board.take().expect("no listener took the board");
    (board, outcome.log.iter().map(|l| l.text.clone()).collect())
}

fn ses(board: &RoutingBoard, dsn: &str) -> String {
    write_ses(board, dsn, SesStyle::Java)
}

fn net(board: &RoutingBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("no net {name}"))
        .net_number
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

fn net_incomplete(board: &RoutingBoard, name: &str) -> usize {
    board
        .routing_ledger()
        .net_incomplete_count(board, net(board, name))
}

/// The traces of net `name`, as text, sorted.
fn traces(board: &RoutingBoard, name: &str) -> BTreeSet<String> {
    let n = net(board, name);
    board
        .items()
        .filter(|item| item.is_trace() && item.contains_net(n))
        .map(|item| format!("{:?}", item.kind))
        .collect()
}

fn tally(log: &[String]) -> Vec<&String> {
    log.iter()
        .filter(|l| l.starts_with("Transactional rip-up"))
        .collect()
}

// ---------------------------------------------------------------------
// The fixtures without the router (what Java does)
// ---------------------------------------------------------------------

#[test]
fn without_the_router_a_rips_b_in_pass_1_on_both_fixtures() {
    for dsn in [SOLE_WALL, LOCKED] {
        let (board, log) = route(dsn, 1, OFF);
        assert_eq!(net_incomplete(&board, "A"), 0, "{dsn}: A routed");
        assert_eq!(net_incomplete(&board, "B"), 1, "{dsn}: B ripped");
        assert!(tally(&log).is_empty(), "{dsn}: Java's router logs no tally");
    }
}

// ---------------------------------------------------------------------
// Q11.2 strict and Q11.3 count
// ---------------------------------------------------------------------

#[test]
fn a_victim_that_cannot_reland_keeps_its_copper_and_a_is_retried_without_ripup() {
    let input = prepare(LOCKED, 1, OFF).board;
    let wall = traces(&input, "B");
    assert_eq!(wall.len(), 1, "B's routed wire");
    for (name, a) in [("strict", STRICT), ("count", COUNT), ("depth1", DEPTH1)] {
        let (board, log) = route(LOCKED, 1, a);
        assert_eq!(traces(&board, "B"), wall, "{name}: B keeps its wire");
        assert_eq!(net_incomplete(&board, "B"), 0, "{name}");
        assert_eq!(
            net_incomplete(&board, "A"),
            1,
            "{name}: A fails without rip-up"
        );
        let lines = tally(&log);
        assert_eq!(lines.len(), 1, "{name}: {log:?}");
        // A1 and A2 are both pass items (Java routes each piece): two
        // transactions, both rolled back.
        assert!(
            lines[0].contains("2 transactions, 0 committed, 2 rolled back (0 retries routed)"),
            "{name}: {}",
            lines[0]
        );
    }
}

#[test]
fn a_victim_that_relands_completes_both_nets_in_pass_1() {
    for (name, a) in ARMS {
        let (board, log) = route(SOLE_WALL, 1, a);
        assert_eq!(incomplete(&board), 0, "{name}: A and B complete");
        let lines = tally(&log);
        assert!(
            lines[0].contains("1 transactions, 1 committed, 0 rolled back")
                && lines[0].contains("1 victim nets, 1 re-landed"),
            "{name}: {}",
            lines[0]
        );
    }
}

#[test]
fn the_rule_decides_on_the_ledger() {
    // The rule names in the tally.
    for (rule, a) in [("strict", STRICT), ("count", COUNT)] {
        let (_, log) = route(SOLE_WALL, 1, a);
        assert!(tally(&log)[0].contains(&format!("rule {rule}")), "{log:?}");
    }
}

// ---------------------------------------------------------------------
// Off is byte-identical; determinism
// ---------------------------------------------------------------------

#[test]
fn off_is_byte_identical() {
    for dsn in [SOLE_WALL, LOCKED, FIXED_CAGE, CYCLE] {
        for passes in [1, 3] {
            let (plain, plain_log) = route(dsn, passes, OFF);
            let reference = ses(&plain, dsn);
            // The transaction settings without the router change nothing,
            // and naming Java's router is Java's router.
            let settings_only = Arm {
                router: None,
                ..STRICT_FACTOR
            };
            let java = Arm {
                router: Some("freerouting-connection"),
                ..OFF
            };
            for (what, a) in [("settings only", settings_only), ("java router", java)] {
                let (board, log) = route(dsn, passes, a);
                assert_eq!(
                    ses(&board, dsn),
                    reference,
                    "{dsn}, {passes} passes, {what}"
                );
                let strip = |log: &[String]| -> Vec<String> {
                    log.iter()
                        .filter(|l| !l.starts_with("Stages:"))
                        .cloned()
                        .collect()
                };
                assert_eq!(strip(&log), strip(&plain_log), "{dsn}, {what}");
            }
        }
    }
}

#[test]
fn without_a_rip_the_router_routes_as_java() {
    // Nothing is ripped on the fixed cage: every arm's session is Java's.
    let (plain, _) = route(FIXED_CAGE, 3, OFF);
    for (name, a) in ARMS {
        let (board, log) = route(FIXED_CAGE, 3, a);
        assert_eq!(ses(&board, FIXED_CAGE), ses(&plain, FIXED_CAGE), "{name}");
        assert!(
            tally(&log)[0].contains(": 0 transactions"),
            "{name}: {log:?}"
        );
    }
}

#[test]
fn every_arm_is_deterministic() {
    for dsn in [SOLE_WALL, LOCKED] {
        for (name, a) in ARMS {
            let (first, first_log) = route(dsn, 3, a);
            let (second, second_log) = route(dsn, 3, a);
            assert_eq!(ses(&first, dsn), ses(&second, dsn), "{dsn} {name}");
            assert_eq!(first_log, second_log, "{dsn} {name}");
        }
    }
}

#[test]
fn invalid_transaction_settings_are_refused_before_routing() {
    for (bad, key) in [
        (
            Arm {
                rule: Some("all"),
                ..STRICT
            },
            "router.transaction.rule",
        ),
        (
            Arm {
                depth: Some(2),
                ..STRICT
            },
            "router.transaction.depth",
        ),
        (
            Arm {
                rip_factor: Some(-1),
                ..STRICT
            },
            "router.transaction.rip_factor",
        ),
    ] {
        let error = run(prepare(SOLE_WALL, 1, bad)).expect_err("refused");
        assert!(error.contains(key), "{error}");
    }
}

// ---------------------------------------------------------------------
// Q11.4: depth 1 terminates on a 3-net cycle
// ---------------------------------------------------------------------

/// An inner router that rips in a script: with rip-up allowed, routing net
/// `n` takes the wires of the nets `rips[n]` off the board and reports them
/// ripped (it lays no copper); with rip-up off it fails.
struct Scripted {
    rips: BTreeMap<i32, Vec<i32>>,
    calls: usize,
}

impl ConnectionRouter for Scripted {
    fn id(&self) -> &'static str {
        "scripted"
    }

    fn route(
        &mut self,
        board: &mut RoutingBoard,
        task: &RouteTask,
        p: &ConnectionParams,
        _ctx: &mut StageCtx,
    ) -> ConnectionOutcome {
        self.calls += 1;
        assert!(self.calls < 100, "the transaction does not end");
        if !p.ripup_allowed {
            return ConnectionOutcome::new(AttemptState::Failed, "scripted: no rip-up");
        }
        let victims = self.rips.get(&task.net).cloned().unwrap_or_default();
        let wires: Vec<ItemId> = board
            .items()
            .filter(|item| item.is_trace() && victims.iter().any(|n| item.contains_net(*n)))
            .map(|item| item.id())
            .collect();
        board.remove_items(wires.clone());
        let mut outcome = ConnectionOutcome::new(AttemptState::Routed, "scripted");
        outcome.ripped = wires.into_iter().collect();
        outcome
    }
}

/// Routes net A of the cycle board through a transaction around a
/// [`Scripted`] router with `rips` (by net name).
fn scripted(
    rips: &[(&str, &[&str])],
    config: TransactionConfig,
) -> (TransactionStats, RoutingBoard, RoutingBoard) {
    let prepared = prepare(CYCLE, 1, OFF);
    let mut board = prepared.board;
    let input = board.clone();
    let rips: BTreeMap<i32, Vec<i32>> = rips
        .iter()
        .map(|(n, v)| (net(&board, n), v.iter().map(|m| net(&board, m)).collect()))
        .collect();
    let a = net(&board, "A");
    let pin = board
        .items()
        .filter(|item| item.is_pin() && item.contains_net(a))
        .map(|item| item.id())
        .min()
        .expect("A has pins");
    let task = RouteTask {
        item: pin,
        net: a,
        is_plane: false,
    };
    let cancel = CancelToken::new();
    let mut ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let p = connection_params(
        &board,
        &task,
        1,
        &ctx,
        1000,
        prepared.board_settings.trace_costs(),
        false,
        500,
        false,
    );
    let mut router = TransactionalRipup::new(Box::new(Scripted { rips, calls: 0 }), config);
    let outcome = router.route(&mut board, &task, &p, &mut ctx);
    assert_eq!(
        outcome.state,
        AttemptState::Failed,
        "the retry has no rip-up"
    );
    assert!(outcome.ripped.is_empty());
    (router.stats(), board, input)
}

const DEPTH1_STRICT: TransactionConfig = TransactionConfig {
    rule: CommitRule::Strict,
    depth: 1,
    rip_factor: 0,
};

#[test]
fn depth_1_ends_on_a_three_net_cycle() {
    // A rips B, B (depth 1) rips C, C (the second wave) may not rip: one
    // transaction, two waves, rolled back.
    let (stats, board, input) = scripted(
        &[("A", &["B"]), ("B", &["C"]), ("C", &["A"])],
        DEPTH1_STRICT,
    );
    assert_eq!(stats.transactions, 1);
    assert_eq!(stats.rolled_back, 1);
    assert_eq!(stats.victims, 2, "B and C");
    assert_eq!(stats.victims_relanded, 0);
    assert_eq!(stats.refused_rerips, 0);
    // The rollback brought every wire back.
    assert_eq!(ses(&board, CYCLE), ses(&input, CYCLE));
}

#[test]
fn depth_1_never_rips_a_net_twice() {
    // A rips B and C; B would rip C and C would rip A again: both re-rips
    // are refused and routed without rip-up.
    let (stats, board, input) = scripted(
        &[("A", &["B", "C"]), ("B", &["C"]), ("C", &["A"])],
        DEPTH1_STRICT,
    );
    assert_eq!(stats.transactions, 1);
    assert_eq!(stats.refused_rerips, 2);
    assert_eq!(stats.victims, 2);
    assert_eq!(stats.rolled_back, 1);
    assert_eq!(ses(&board, CYCLE), ses(&input, CYCLE));
}

#[test]
fn depth_0_routes_victims_without_ripup() {
    let config = TransactionConfig {
        depth: 0,
        ..DEPTH1_STRICT
    };
    let (stats, _, _) = scripted(&[("A", &["B"]), ("B", &["C"]), ("C", &["A"])], config);
    assert_eq!(stats.victims, 1, "B only: its attempt cannot rip");
    assert_eq!(stats.rolled_back, 1);
}

#[test]
fn the_real_router_ends_on_the_cycle_board_with_every_arm() {
    // The cycle board is complete: nothing to route, nothing to rip.
    for (name, a) in ARMS {
        let (board, log) = route(CYCLE, 3, a);
        assert_eq!(incomplete(&board), 0, "{name}");
        assert!(tally(&log)[0].contains(": 0 transactions"), "{name}");
    }
}

// ---------------------------------------------------------------------
// Q11.5: rip-count protection
// ---------------------------------------------------------------------

/// The ripup costs of routing A on the sole-wall board with Java's router,
/// with `factors` (by net name) on the parameters.
fn ripped_costs_of_a(factors: Option<&[(&str, i32)]>) -> Vec<(ItemId, i32)> {
    let prepared = prepare(SOLE_WALL, 1, OFF);
    let mut board = prepared.board;
    let a = net(&board, "A");
    let pin = board
        .items()
        .filter(|item| item.is_pin() && item.contains_net(a))
        .map(|item| item.id())
        .min()
        .expect("A has pins");
    let task = RouteTask {
        item: pin,
        net: a,
        is_plane: false,
    };
    let cancel = CancelToken::new();
    let mut ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let mut p = connection_params(
        &board,
        &task,
        1,
        &ctx,
        1000,
        prepared.board_settings.trace_costs(),
        false,
        500,
        false,
    );
    p.ripup_factors = factors.map(|f| {
        Arc::new(
            f.iter()
                .map(|(n, factor)| (net(&board, n), *factor))
                .collect::<BTreeMap<i32, i32>>(),
        )
    });
    let mut router = FreeroutingConnectionRouter::freerouting();
    let outcome = router.route(&mut board, &task, &p, &mut ctx);
    assert_eq!(outcome.state, AttemptState::Routed);
    outcome.ripped_costs
}

#[test]
fn the_rip_factor_multiplies_the_rip_cost_of_the_ripped_net_only() {
    let java = ripped_costs_of_a(None);
    assert_eq!(java.len(), 1, "B's wire: {java:?}");
    let (wall, cost) = java[0];
    assert!(cost > 1, "{cost}");
    // A factor for another net changes nothing; one for B multiplies.
    assert_eq!(ripped_costs_of_a(Some(&[("A", 7)])), java);
    assert_eq!(ripped_costs_of_a(Some(&[("B", 1)])), java);
    assert_eq!(ripped_costs_of_a(Some(&[("B", 5)])), vec![(wall, cost * 5)]);
}

#[test]
fn a_rip_factor_of_0_is_unset() {
    let unset = Arm {
        rip_factor: None,
        ..STRICT
    };
    for dsn in [SOLE_WALL, LOCKED] {
        let (a, a_log) = route(dsn, 3, STRICT);
        let (b, b_log) = route(dsn, 3, unset);
        assert_eq!(ses(&a, dsn), ses(&b, dsn), "{dsn}");
        assert_eq!(a_log, b_log, "{dsn}");
    }
}

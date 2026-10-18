//! Q06: blocker attribution (`quality::attribution`) and one verdict per
//! incomplete connection (`quality::verdict`, the `verdicts` diagnostic).
//!
//! The fixtures are synthetic one-layer boards in `tests/data/quality/`
//! (beside Q01's and Q03's; `fixtures/` boards all need Java references):
//! - `q06-sole-wall.dsn`: the only route from A1 to A2 crosses the routed
//!   (unfixed) wire of net B, whose pads sit 0.2 mm from the outline;
//! - `q06-fixed-cage.dsn`: pad A1 is walled in by eight pads of
//!   single-pin nets, 0.3 mm apart;
//! - Q03's `q03-qfn-0p5.dsn`: net PWR (1.5 mm) cannot leave the 0.25 mm
//!   QFN pad U1-5.
//!
//! The verdicts describe Java's router, so every run is pinned to profile
//! `legacy`; under `enhanced` the neck-down retry (`neck_width_um = 300`,
//! N05.3) routes PWR at 0.3 mm (`qfn_power_pin_routes_necked_under_enhanced`).

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::AttemptState;
use freerouting::autoroute::batch::connection::{FreeroutingConnectionRouter, connection_params};
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::autoroute::stages::{ConnectionRouter, Diagnostics, RouteTask, StageCtx};
use freerouting::board::RoutingBoard;
use freerouting::drc::DesignRulesChecker;
use freerouting::drc::json::JsonValue;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::attribution::SearchEnd;
use freerouting::quality::neckdown::width_report::report as width_report;
use freerouting::quality::verdict::{Verdict, VerdictDiagnostics, VerdictReport};
use freerouting::settings::profile::legacy_pin;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const SOLE_WALL: &str = "crates/freerouting/tests/data/quality/q06-sole-wall.dsn";
const FIXED_CAGE: &str = "crates/freerouting/tests/data/quality/q06-fixed-cage.dsn";
const QFN: &str = "crates/freerouting/tests/data/quality/q03-qfn-0p5.dsn";
const GOLDEN: &str = "crates/freerouting/tests/data/quality/q06-verdicts.golden.json";

/// How a test routes a fixture.
#[derive(Clone, Copy)]
struct Run {
    /// `None` disables the router (and the fanout).
    max_passes: Option<i32>,
    verdicts: bool,
}

const ROUTE_1: Run = Run {
    max_passes: Some(1),
    verdicts: true,
};

fn prepare(dsn: &str, run: Run) -> PreparedJob {
    prepare_as(dsn, run, true)
}

/// `prepare`, pinned to `legacy` when `legacy`, else the default profile.
fn prepare_as(dsn: &str, run: Run, legacy: bool) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    if legacy {
        profile.patch.profile = legacy_pin().profile;
    }
    profile.set_optimizer_enabled(false);
    match run.max_passes {
        Some(passes) => profile.set_max_passes(passes),
        None => {
            profile.set_router_enabled(false);
            profile.set_fanout_enabled(false);
        }
    }
    if run.verdicts {
        profile.patch.special.diagnostics = Some(vec!["verdicts".to_string()]);
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

/// The routed board and the run's verdicts.
fn route(dsn: &str, run: Run) -> (RoutingBoard, Option<VerdictReport>) {
    route_as(dsn, run, true)
}

/// `route` under `legacy` or (`legacy` false) the default profile.
fn route_as(dsn: &str, run: Run, legacy: bool) -> (RoutingBoard, Option<VerdictReport>) {
    let prepared = prepare_as(dsn, run, legacy);
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
    (board, outcome.summary.verdicts)
}

fn net_number(board: &RoutingBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("no net {name}"))
        .net_number
}

fn ses(board: &RoutingBoard, dsn: &str) -> String {
    write_ses(board, dsn, SesStyle::Java)
}

#[test]
fn sole_wall_on_the_input_board_names_the_wall_net() {
    // Router off: the analysis runs on the board as loaded, where A1-A2
    // must cross B's routed wire.
    let run = Run {
        max_passes: None,
        verdicts: true,
    };
    let (board, report) = route(SOLE_WALL, run);
    let report = report.expect("the verdicts ran");
    let b = net_number(&board, "B");
    assert_eq!(report.connections.len(), 1);
    let c = &report.connections[0];
    assert_eq!(c.net_name, "A");
    assert_eq!(c.search, SearchEnd::Exhausted);
    assert_eq!(c.attempt, None, "the router never tried it");
    let Verdict::BlockedBy(a) = &c.verdict else {
        panic!("expected blocked_by, got {:?}", c.verdict);
    };
    let top = a.nets[0];
    assert_eq!(top.net, b);
    assert_eq!(a.nets.len(), 1);
    assert!(top.blocked > 0);
    assert_eq!(top.unique, top.blocked);
    assert_eq!(c.blocker_names[&b], "B");
}

#[test]
fn sole_wall_after_a_pass_is_unrouted_but_routable() {
    // Pass 1 routes A through B's wire by ripping it. B was complete when
    // the pass started, so it is never attempted; A's new trace ends at
    // A1 and A2, so B can now go round it: the analysis finds a path.
    let (board, report) = route(SOLE_WALL, ROUTE_1);
    let report = report.expect("the verdicts ran");
    let a_net = net_number(&board, "A");
    assert_eq!(report.connections.len(), 1);
    let c = &report.connections[0];
    assert_eq!(c.net_name, "B");
    assert_eq!(c.attempt, None);
    assert_eq!(c.search, SearchEnd::Found);
    assert!(matches!(c.verdict, Verdict::Unrouted(_)), "{:?}", c.verdict);
    // The search still met A's trace on its way round.
    assert_eq!(c.attribution.nets[0].net, a_net);
}

#[test]
fn fixed_cage_is_unrouted_with_no_routed_coverage() {
    let run = Run {
        max_passes: Some(2),
        verdicts: true,
    };
    let (_, report) = route(FIXED_CAGE, run);
    let report = report.expect("the verdicts ran");
    assert_eq!(report.connections.len(), 1);
    let c = &report.connections[0];
    assert_eq!(c.net_name, "A");
    let Verdict::Unrouted(coverage) = &c.verdict else {
        panic!("expected unrouted, got {:?}", c.verdict);
    };
    assert_eq!(coverage.routed, 0);
    assert!(coverage.fixed > 0);
    assert_eq!(c.search, SearchEnd::Exhausted);
    let attempt = c.attempt.as_ref().expect("the router tried it");
    assert_eq!(attempt.state, AttemptState::Failed);
    assert!(!attempt.work_limit_hit);
    assert_eq!(attempt.pass, 2);
}

#[test]
fn qfn_power_pin_is_infeasible() {
    let run = Run {
        max_passes: Some(1),
        verdicts: true,
    };
    let (board, report) = route(QFN, run);
    let report = report.expect("the verdicts ran");
    let pwr = net_number(&board, "PWR");
    let c = report
        .connections
        .iter()
        .find(|c| c.net == pwr)
        .expect("PWR stays incomplete");
    let Verdict::Infeasible(pin) = &c.verdict else {
        panic!("expected infeasible, got {:?}", c.verdict);
    };
    assert_eq!((pin.component.as_str(), pin.pin.as_str()), ("U1", "5"));
}

#[test]
fn qfn_power_pin_routes_necked_under_enhanced() {
    // N05.3: the default profile retries the failed 1.5 mm connection with
    // traces clamped to 0.3 mm, which leaves the 0.25 mm pad.
    let (board, report) = route_as(QFN, ROUTE_1, false);
    let report = report.expect("the verdicts ran");
    let pwr = net_number(&board, "PWR");
    assert!(
        report.connections.iter().all(|c| c.net != pwr),
        "PWR routes under enhanced"
    );
    let widths = width_report(&board);
    let net = widths.net("PWR").expect("PWR is a wide net");
    assert!(
        net.length > 0.0 && net.under > 0.0,
        "PWR ships necked copper"
    );
    let narrowest = net.min_width.expect("PWR has traces");
    assert!(narrowest <= 3_000, "at most 0.3 mm: {narrowest}");
}

#[test]
fn a_tiny_work_limit_gives_work_limit() {
    let prepared = prepare(FIXED_CAGE, ROUTE_1);
    let mut board = prepared.board;
    let settings = &prepared.settings;
    let cancel = CancelToken::new();
    let mut ctx = StageCtx {
        settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let mut diagnostics = VerdictDiagnostics::from_settings(settings);
    diagnostics.on_input(&board, &ctx);
    let a = net_number(&board, "A");
    let mut drc = DesignRulesChecker::new(&board);
    drc.calculate_all_incompletes();
    let from = drc.all_airlines()[0].from_item;
    let task = RouteTask {
        item: from,
        net: a,
        is_plane: false,
    };
    let mut p = connection_params(
        &board,
        &task,
        1,
        &ctx,
        100,
        prepared.board_settings.trace_costs(),
        false,
        0,
        false,
    );
    p.work_budget = 0;
    let mut router = FreeroutingConnectionRouter::from_settings(settings);
    let outcome = router.route(&mut board, &task, &p, &mut ctx);
    assert_eq!(outcome.state, AttemptState::Failed);
    assert!(outcome.work_limit_hit);
    diagnostics.after_connection(&board, &task, &p, &outcome, &ctx);
    let report = diagnostics.report(&board, &prepared.board_settings);
    assert_eq!(report.connections.len(), 1);
    assert!(
        matches!(
            report.connections[0].verdict,
            Verdict::WorkLimit { pass: 1, .. }
        ),
        "{:?}",
        report.connections[0].verdict
    );
}

#[test]
fn analysing_a_connection_changes_nothing_on_the_board() {
    let prepared = prepare(SOLE_WALL, ROUTE_1);
    let board = prepared.board;
    let before = ses(&board, "q06-sole-wall");
    let units = board.search_trees().work_meter().units();
    let a = net_number(&board, "A");
    let from = board
        .items()
        .find(|item| item.is_pin() && item.contains_net(a))
        .map(|item| item.id())
        .expect("a pin of A");
    let p = connection_params(
        &board,
        &RouteTask {
            item: from,
            net: a,
            is_plane: false,
        },
        1,
        &StageCtx {
            settings: &prepared.settings,
            board_settings: &prepared.board_settings,
            cancel: &CancelToken::new(),
            events: &NullEvents,
        },
        100,
        prepared.board_settings.trace_costs(),
        false,
        0,
        false,
    );
    let analysis = freerouting::quality::attribution::analyze_connection(
        &board,
        a,
        from,
        &prepared.board_settings,
        &p,
    );
    assert_eq!(analysis.end, SearchEnd::Exhausted);
    assert!(analysis.work > 0);
    assert_eq!(
        board.search_trees().work_meter().units(),
        units,
        "the meter"
    );
    assert_eq!(ses(&board, "q06-sole-wall"), before);
}

#[test]
fn the_ses_is_byte_identical_with_the_verdicts_on_and_off() {
    for dsn in [SOLE_WALL, FIXED_CAGE, QFN] {
        for passes in [1, 3] {
            let on = Run {
                max_passes: Some(passes),
                verdicts: true,
            };
            let off = Run {
                verdicts: false,
                ..on
            };
            let (with, report) = route(dsn, on);
            let (without, none) = route(dsn, off);
            assert!(report.is_some() && none.is_none());
            assert_eq!(
                ses(&with, dsn),
                ses(&without, dsn),
                "{dsn}, {passes} passes"
            );
        }
    }
}

/// The verdict JSON of the three fixtures, as one object: the sole wall as
/// loaded (router off), the others after one pass.
fn fixtures_json() -> String {
    let router_off = Run {
        max_passes: None,
        verdicts: true,
    };
    let members = [
        (SOLE_WALL, router_off),
        (FIXED_CAGE, ROUTE_1),
        (QFN, ROUTE_1),
    ]
    .iter()
    .map(|&(dsn, run)| {
        let (_, report) = route(dsn, run);
        let name = Path::new(dsn)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned();
        (name, report.expect("the verdicts ran").to_json())
    })
    .collect();
    JsonValue::Object(members).to_pretty_string() + "\n"
}

#[test]
fn verdict_json_matches_golden_and_is_deterministic() {
    let text = fixtures_json();
    assert_eq!(
        fixtures_json(),
        text,
        "D2: a second run gives the same JSON"
    );
    let golden = repo_root().join(GOLDEN);
    if std::env::var_os("Q06_BLESS").is_some() {
        std::fs::write(&golden, &text).unwrap();
    }
    let want = std::fs::read_to_string(&golden).unwrap_or_default();
    assert_eq!(
        text, want,
        "regenerate with Q06_BLESS=1 and review the diff"
    );
}

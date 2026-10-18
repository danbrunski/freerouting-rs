//! Q12: the pass-1 portfolio and squeaky-wheel restarts
//! (`quality::portfolio`, `router.autorouter.portfolio`,
//! `router.autorouter.restarts`).
//!
//! The boards:
//! - `fixtures/Issue433-my-board.dsn` (9 connections): Java's pass 1
//!   leaves 1 incomplete; Q07's `conflict-rounds` order routes all;
//! - `tests/data/quality/q12-channel.dsn`, synthetic, one layer: a channel
//!   one trace wide joins two rooms, a wide bridge runs above. B's pads sit
//!   in notches off the channel, so B routes only through it; A routes
//!   through it (short) or over the bridge (long). With the fanout off,
//!   Java's pass 1 routes A first through the channel and B fails; with B
//!   first both route.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, PipelineOutcome, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const ISSUE433: &str = "fixtures/Issue433-my-board.dsn";
const CHANNEL: &str = "crates/freerouting/tests/data/quality/q12-channel.dsn";

/// The settings of one run.
#[derive(Clone, Copy)]
struct Run {
    max_passes: i32,
    optimizer: bool,
    fanout: bool,
    portfolio: Option<i32>,
    restarts: Option<i32>,
    memory_mb: Option<i32>,
    threads: i32,
}

const MP1: Run = Run {
    max_passes: 1,
    optimizer: false,
    fanout: true,
    portfolio: None,
    restarts: None,
    memory_mb: None,
    threads: 1,
};

/// The channel board's runs: pass 1 alone, no fanout.
const CHANNEL_MP1: Run = Run {
    fanout: false,
    ..MP1
};

fn prepare(dsn: &str, run: Run) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(run.optimizer);
    profile.set_fanout_enabled(run.fanout);
    profile.set_max_passes(run.max_passes);
    profile.patch.autorouter.portfolio = run.portfolio;
    profile.patch.autorouter.restarts = run.restarts;
    profile.patch.autorouter.portfolio_memory_mb = run.memory_mb;
    profile.patch.autorouter.max_threads = Some(run.threads);
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 4 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

fn outcome(dsn: &str, run: Run) -> Result<PipelineOutcome, String> {
    let prepared = prepare(dsn, run);
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

/// The routed board, its SES and the job log's text.
fn route(dsn: &str, run: Run) -> (RoutingBoard, String, Vec<String>, PipelineOutcome) {
    let mut outcome = outcome(dsn, run).expect("the job runs");
    let board = outcome.board.take().expect("no listener took the board");
    let ses = write_ses(&board, "q12", SesStyle::Java);
    let log = outcome.log.iter().map(|l| l.text.clone()).collect();
    (board, ses, log, outcome)
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

fn portfolio_lines(log: &[String]) -> Vec<&String> {
    log.iter().filter(|l| l.starts_with("Portfolio")).collect()
}

#[test]
fn portfolio_one_is_byte_identical_to_the_job_without_it() {
    let default_passes = Run {
        max_passes: 0,
        optimizer: true,
        ..MP1
    };
    for (dsn, run) in [
        (ISSUE433, MP1),
        (ISSUE433, default_passes),
        (CHANNEL, CHANNEL_MP1),
    ] {
        let (_, off, off_log, off_outcome) = route(dsn, run);
        assert!(off_outcome.portfolio.is_none());
        assert!(portfolio_lines(&off_log).is_empty());
        let one = Run {
            portfolio: Some(1),
            ..run
        };
        let (_, on, on_log, on_outcome) = route(dsn, one);
        assert_eq!(on, off, "{dsn} (max_passes {})", run.max_passes);
        let portfolio = on_outcome.portfolio.expect("the portfolio ran");
        assert_eq!(portfolio.arms.len(), 1);
        assert_eq!(portfolio.winner, 0);
        // The job log is the same but for the portfolio's lines.
        let rest: Vec<&String> = on_log
            .iter()
            .filter(|l| !l.starts_with("Portfolio"))
            .collect();
        assert_eq!(rest, off_log.iter().collect::<Vec<_>>(), "{dsn}");
        // `0` and unset are off.
        let zero = Run {
            portfolio: Some(0),
            restarts: Some(0),
            ..run
        };
        assert_eq!(route(dsn, zero).1, off, "{dsn}: portfolio 0");
    }
}

#[test]
fn the_kept_arm_is_the_best_by_incomplete_then_score_then_index() {
    let (board, _, log, outcome) = route(
        ISSUE433,
        Run {
            portfolio: Some(4),
            ..MP1
        },
    );
    let portfolio = outcome.portfolio.expect("the portfolio ran");
    let labels: Vec<&str> = portfolio.arms.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(
        labels,
        ["configured", "conflict-rounds", "inside-out", "shuffle-1"]
    );
    let best = portfolio
        .arms
        .iter()
        .min_by(|a, b| {
            (a.figures.incomplete, b.figures.score, a.index)
                .partial_cmp(&(b.figures.incomplete, a.figures.score, b.index))
                .unwrap()
        })
        .unwrap();
    assert_eq!(portfolio.winner, best.index);
    // On this board Java's order leaves one connection and arm 1 none.
    assert_eq!(portfolio.arms[0].figures.incomplete, 1);
    assert_eq!(portfolio.winner, 1, "{log:#?}");
    assert_eq!(incomplete(&board), 0);
    assert_eq!(outcome.summary.incomplete, 0);
    let spread = portfolio.spread.expect("a spread");
    assert_eq!(spread.min, 0);
    assert!(log.iter().any(|l| l.starts_with("Portfolio: 4 arms")
        && l.contains("kept arm 1 (conflict-rounds)")
        && l.contains("min 0")));
}

#[test]
fn one_and_four_threads_give_the_same_ses() {
    let run = |threads| {
        route(
            ISSUE433,
            Run {
                portfolio: Some(4),
                restarts: Some(1),
                threads,
                ..MP1
            },
        )
    };
    let (_, one, one_log, one_outcome) = run(1);
    let (_, four, four_log, four_outcome) = run(4);
    assert_eq!(one, four);
    let (one_p, four_p) = (
        one_outcome.portfolio.unwrap(),
        four_outcome.portfolio.unwrap(),
    );
    assert_eq!(one_p.concurrency, 1);
    assert!(four_p.concurrency > 1 || freerouting::parallel::available_cores() < 2);
    assert_eq!(one_p.arms, four_p.arms);
    // The logs differ only in the concurrency the summary line names.
    let strip = |log: &[String]| -> Vec<String> {
        log.iter()
            .map(|l| {
                l.split(", ")
                    .filter(|part| !part.contains(" at a time"))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .collect()
    };
    assert_eq!(strip(&one_log), strip(&four_log));
}

#[test]
fn the_portfolio_and_restarts_are_deterministic() {
    for run in [
        Run {
            portfolio: Some(4),
            restarts: Some(2),
            threads: 2,
            ..MP1
        },
        Run {
            restarts: Some(3),
            threads: 2,
            ..MP1
        },
    ] {
        let (_, a, a_log, _) = route(ISSUE433, run);
        let (_, b, b_log, _) = route(ISSUE433, run);
        assert_eq!(a, b);
        assert_eq!(a_log, b_log);
    }
}

#[test]
fn a_restart_that_keeps_no_new_best_ends_the_generations() {
    // Arm 0 leaves one connection; restart 1 does not beat it, so restart 2
    // would repeat restart 1 and does not run.
    let (_, ses, log, outcome) = route(
        ISSUE433,
        Run {
            restarts: Some(3),
            ..MP1
        },
    );
    let portfolio = outcome.portfolio.expect("the portfolio ran");
    let labels: Vec<&str> = portfolio.arms.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, ["configured", "restart-1"], "{log:#?}");
    assert_eq!(portfolio.winner, 0);
    assert_eq!(ses, route(ISSUE433, MP1).1, "arm 0 is kept: the job's SES");
}

#[test]
fn a_restart_routes_the_failed_net_first_and_the_channel_ends_complete() {
    let (off, off_ses, _, _) = route(CHANNEL, CHANNEL_MP1);
    assert_eq!(incomplete(&off), 1, "Java's order: A blocks the channel");
    // Restarts off (0) is byte-identical to unset.
    let zero = Run {
        restarts: Some(0),
        ..CHANNEL_MP1
    };
    assert_eq!(route(CHANNEL, zero).1, off_ses);
    let one = Run {
        restarts: Some(1),
        ..CHANNEL_MP1
    };
    let (on, on_ses, log, outcome) = route(CHANNEL, one);
    assert_eq!(incomplete(&on), 0, "{log:#?}");
    let portfolio = outcome.portfolio.expect("the portfolio ran");
    let labels: Vec<&str> = portfolio.arms.iter().map(|a| a.label.as_str()).collect();
    assert_eq!(labels, ["configured", "restart-1"]);
    assert_eq!(portfolio.winner, 1);
    assert!(
        log.iter()
            .any(|l| l.contains("its 1 failed net (B) first, then its 1 other ripped net (A)")),
        "{log:#?}"
    );
    // Determinism.
    assert_eq!(route(CHANNEL, one).1, on_ses);
    // A second generation starts from a complete board, so it does not run.
    let two = Run {
        restarts: Some(2),
        ..CHANNEL_MP1
    };
    let (_, two_ses, _, two_outcome) = route(CHANNEL, two);
    assert_eq!(two_ses, on_ses);
    assert_eq!(two_outcome.portfolio.unwrap().arms.len(), 2);
}

#[test]
fn the_memory_guard_runs_arms_one_at_a_time_and_keeps_every_arm() {
    let run = |memory_mb| {
        route(
            ISSUE433,
            Run {
                portfolio: Some(4),
                memory_mb,
                threads: 4,
                ..MP1
            },
        )
    };
    let (_, roomy, _, roomy_outcome) = run(None);
    let (_, tight, tight_log, tight_outcome) = run(Some(1));
    assert_eq!(tight, roomy, "the guard never changes the result");
    let tight_p = tight_outcome.portfolio.unwrap();
    assert_eq!(tight_p.concurrency, 1);
    assert_eq!(tight_p.arms, roomy_outcome.portfolio.unwrap().arms);
    assert!(
        tight_log
            .iter()
            .any(|l| l.starts_with("Portfolio memory guard:")),
        "{tight_log:#?}"
    );
}

#[test]
fn bad_portfolio_settings_are_refused_before_routing() {
    for (run, key) in [
        (
            Run {
                portfolio: Some(-1),
                ..MP1
            },
            "router.autorouter.portfolio",
        ),
        (
            Run {
                restarts: Some(-2),
                ..MP1
            },
            "router.autorouter.restarts",
        ),
        (
            Run {
                portfolio: Some(2),
                memory_mb: Some(0),
                ..MP1
            },
            "router.autorouter.portfolio_memory_mb",
        ),
    ] {
        let error = outcome(ISSUE433, run).expect_err("refused");
        assert!(error.contains(key), "{error}");
    }
}

//! Q05.1: the `rip-stats` diagnostic (`quality::congestion::rip_stats`).
//!
//! The fixtures are Q06's synthetic boards in `tests/data/quality/`:
//! - `q06-sole-wall.dsn`: the only route from A1 to A2 crosses the routed
//!   (unfixed) wire of net B, so routing A rips B;
//! - `q06-fixed-cage.dsn`: A1 is walled in by fixed pads; nothing is
//!   ripped.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::quality::congestion::rip_stats::RipReport;
use freerouting::settings::profiles::TestingProfile;
use freerouting::settings::{ResolveContext, Source};

const SOLE_WALL: &str = "crates/freerouting/tests/data/quality/q06-sole-wall.dsn";
const FIXED_CAGE: &str = "crates/freerouting/tests/data/quality/q06-fixed-cage.dsn";

fn prepare(dsn: &str, passes: i32, diagnostics: &[&str]) -> PreparedJob {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let mut profile = TestingProfile::default();
    profile.set_optimizer_enabled(false);
    profile.set_max_passes(passes);
    if !diagnostics.is_empty() {
        profile.patch.special.diagnostics =
            Some(diagnostics.iter().map(|d| d.to_string()).collect());
    }
    let mut input = JobInput::new(bytes, JobInput::design_name_of(dsn));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.extra_sources = vec![(Source::Testing, profile.patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{dsn}: {e}"))
}

/// The routed board and the run's rip statistics.
fn route(dsn: &str, passes: i32, diagnostics: &[&str]) -> (RoutingBoard, Option<RipReport>) {
    let prepared = prepare(dsn, passes, diagnostics);
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
    (board, outcome.summary.rip_stats)
}

fn ses(board: &RoutingBoard, dsn: &str) -> String {
    write_ses(board, dsn, SesStyle::Java)
}

fn net_number(board: &RoutingBoard, name: &str) -> i32 {
    board
        .rules
        .nets
        .get_by_name(name, 1)
        .unwrap_or_else(|| panic!("no net {name}"))
        .net_number
}

#[test]
fn routing_through_a_wall_rips_the_wall_net() {
    let (board, report) = route(SOLE_WALL, 1, &["rip-stats"]);
    let report = report.expect("the rip statistics ran");
    let b = net_number(&board, "B");
    let stats = &report.nets[&b];
    assert_eq!(stats.rips, 1, "{stats:?}");
    assert_eq!(stats.removed_items, 1, "B's one wire");
    // B's wire runs 18.6 mm between its pads.
    let torn_mm = stats.torn_length / report.units_per_mm;
    assert!((torn_mm - 18.6).abs() < 1e-9, "{torn_mm}");
    let top = report.top_nets(5);
    assert_eq!(top[0].net, b);
    assert_eq!(top[0].share, 1.0, "B is the only net that lost copper");
    assert_eq!(report.top_share(), Some(1.0));
    let a = net_number(&board, "A");
    assert_eq!(report.nets[&a].rips, 0, "nothing ripped A");
    assert_eq!(report.nets[&b].pins, 2);
}

#[test]
fn later_passes_count_reroutes() {
    let (board, report) = route(SOLE_WALL, 3, &["rip-stats"]);
    let report = report.expect("the rip statistics ran");
    let b = net_number(&board, "B");
    // Pass 1 rips B to route A; pass 2 routes B again, which closes the rip.
    assert_eq!(report.nets[&b].rips, 1);
    assert_eq!(report.nets[&b].reroutes, 1);
    assert_eq!(report.passes[&1].rips, 1);
    assert_eq!(report.passes[&1].reroutes, 0);
    assert_eq!(report.passes[&2].rips, 0);
    assert_eq!(report.passes[&2].reroutes, 1);
}

#[test]
fn nothing_ripped_reports_no_rips() {
    let (_, report) = route(FIXED_CAGE, 1, &["rip-stats"]);
    let report = report.expect("the rip statistics ran");
    assert_eq!(report.rips(), 0);
    assert_eq!(report.top_share(), None);
}

#[test]
fn the_ses_is_byte_identical_with_the_rip_stats_on_and_off() {
    for dsn in [SOLE_WALL, FIXED_CAGE] {
        for passes in [1, 3] {
            let (with, report) = route(dsn, passes, &["rip-stats"]);
            let (both, both_report) = route(dsn, passes, &["verdicts", "rip-stats"]);
            let (without, none) = route(dsn, passes, &[]);
            assert!(report.is_some() && both_report.is_some() && none.is_none());
            let reference = ses(&without, dsn);
            assert_eq!(ses(&with, dsn), reference, "{dsn}, {passes} passes");
            assert_eq!(
                ses(&both, dsn),
                reference,
                "{dsn}, {passes} passes, with verdicts"
            );
            assert_eq!(report, both_report, "the verdicts change no rip statistics");
        }
    }
}

#[test]
fn the_report_is_deterministic() {
    let json = || {
        let (_, report) = route(SOLE_WALL, 3, &["rip-stats"]);
        report.expect("ran").to_json().to_pretty_string()
    };
    assert_eq!(json(), json());
}

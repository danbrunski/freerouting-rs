//! P02.1: the incremental search trees against a rebuild, on a routed
//! board.
//!
//! `router.debug.audit_incremental` makes the batch loop compare every
//! search tree with a fresh build after each ripup and restore and at the
//! end of each pass (`board::search::audit`). These tests route with it on
//! and check that it finds nothing and changes nothing. The seeded fault
//! (a skipped tree removal) is a unit test of `board::search::audit`,
//! because it needs a test build of the library.

mod autoroute_common;

use std::path::Path;
use std::sync::Arc;

use autoroute_common::{Loaded, repo_root};
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, PipelineOutcome, run_pipeline};
use freerouting::io::specctra::ses_writer::{SesStyle, write_ses};
use freerouting::job::LogLevel;
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::settings::{ResolveContext, SettingsPatch, Source};

/// 30 passes with three history restores and two last-mile ripups
/// (A13.2), so every audit point runs.
const CONGFLOCK: &str = "fixtures/PCBench-retroreflectors-CONGFLOCK.dsn";

/// J01.3's route mode (the optimizer off, `max_passes` 100) with the
/// audit on or off.
fn load(dsn: &str, audit: bool) -> Loaded {
    let path = repo_root().join(dsn);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("board");
    let mut input = JobInput::new(bytes, JobInput::design_name_of(file_name));
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    let mut patch = SettingsPatch::default();
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(100);
    patch.debug.audit_incremental = Some(audit);
    input.extra_sources = vec![(Source::Testing, patch)];
    let prepared =
        prepare_job(&input, &ResolveContext { cores: 8 }).unwrap_or_else(|e| panic!("{dsn}: {e}"));
    Loaded {
        board: prepared.board,
        settings: prepared.settings,
        router: prepared.board_settings,
    }
}

/// Routes `dsn` and returns the outcome and the session text.
fn route(dsn: &str, audit: bool) -> (PipelineOutcome, String) {
    let loaded = load(dsn, audit);
    let job = PipelineJob {
        settings: &loaded.settings,
        board_settings: &loaded.router,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome = run_pipeline(loaded.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    let board = outcome.board.take().expect("no listener took the board");
    let ses = write_ses(&board, "board", SesStyle::Java);
    (outcome, ses)
}

#[test]
fn audit_is_clean_and_changes_nothing_on_a_routed_board() {
    let (off, ses_off) = route(CONGFLOCK, false);
    let (on, ses_on) = route(CONGFLOCK, true);
    assert!(off.audit.is_none(), "the audit is off by default");
    let report = on.audit.as_ref().expect("the audit ran");
    assert_eq!(report.first, None, "{:?}", report.first);
    assert_eq!(report.failed_checks, 0);
    // A check at each pass end, each ripping or failing connection, the
    // three restores, the two last-mile ripups and the finish.
    let passes = on.passes.len() as u64;
    assert!(on.restores > 0, "the board restores");
    assert!(report.checks > passes + on.restores as u64, "{report:?}");
    // The audit only reads: the same session, passes and summary.
    assert_eq!(ses_on, ses_off);
    assert_eq!(on.passes, off.passes);
    assert_eq!(on.summary, off.summary);
    let summary: Vec<_> = on
        .log
        .iter()
        .filter(|l| l.text.starts_with("Incremental search-tree audit"))
        .collect();
    assert_eq!(summary.len(), 1, "one summary line, no error line");
    assert_eq!(summary[0].level, LogLevel::Info);
    assert_eq!(summary[0].text, report.summary_line());
}

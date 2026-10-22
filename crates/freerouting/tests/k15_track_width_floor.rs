//! K15.4: a `.kicad_dru` `track_width` rule without a condition is a floor
//! under every trace the router makes, in both profiles.
//!
//! The pcu board's report (`docs/development/pcu-rev2-board-report.md`, bug 2)
//! shipped 0.15, 0.12 and 0.10 mm tracks on a 0.2 mm class, 16 of them under
//! its `.kicad_dru`'s 0.127 mm minimum. K15.3 turned the rule into
//! `router.board_minimums.track_width_um`; these tests route a board through
//! that path and check the copper, not only the setting.
//!
//! Fixture `tests/data/quality/q18-neck-floor.dsn`: net PWR2 (0.2 mm, the only
//! class width) runs from the big pad J3 to U3-2, a 0.12 mm pad between two
//! pads of other nets at 0.35 mm pitch. A 0.2 mm trace does not fit there, so
//! Java's pin neck-down narrows it to 0.1198 mm, and the fanout's micro
//! neck-down would try 0.15, 0.12 and 0.10 mm (3/4, 3/5 and 1/2 of the class
//! width) when the pin neck-down fails.

mod autoroute_common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use autoroute_common::repo_root;
use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::RoutingBoard;
use freerouting::job::prepare::{JobInput, PreparedJob, RulesChoice, prepare_job};
use freerouting::settings::fields::SettingsPatch;
use freerouting::settings::{ResolveContext, Source};

const NECK: &str = "crates/freerouting/tests/data/quality/q18-neck-floor.dsn";

/// A KiCad custom-rules file with one board-wide minimum track width.
fn track_width_rule(min: &str) -> String {
    format!(
        "(version 1)\n(rule \"Minimum trace width\"\n    \
         (constraint track_width (min {min}))\n    (severity error))\n"
    )
}

/// The neck fixture under `profile`, with `dru` as its `--kicad-dru` file.
fn prepare(profile: &str, dru: Option<String>) -> PreparedJob {
    let path = repo_root().join(NECK);
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("{NECK}: {e}"));
    let mut input = JobInput::new(bytes, JobInput::design_name_of(NECK));
    input.rules = RulesChoice::None;
    input.dsn_dir = path.parent().map(Path::to_path_buf);
    input.kicad_dru = dru.map(|text| (PathBuf::from("neck.kicad_dru"), text));
    let mut patch = SettingsPatch {
        profile: Some(profile.to_string()),
        ..SettingsPatch::default()
    };
    patch.optimizer.enabled = Some(false);
    patch.autorouter.max_passes = Some(3);
    input.extra_sources = vec![(Source::Testing, patch)];
    prepare_job(&input, &ResolveContext { cores: 1 }).unwrap_or_else(|e| panic!("{NECK}: {e}"))
}

fn route(prepared: PreparedJob) -> RoutingBoard {
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events: &NullEvents,
        deadline: None,
        registry: None,
    };
    let mut outcome = run_pipeline(prepared.board, &job).unwrap_or_else(|f| panic!("{}", f.error));
    outcome.board.take().expect("no listener took the board")
}

/// The widths of the board's traces in board units (0.1 um), ascending.
fn widths(board: &RoutingBoard) -> Vec<i32> {
    let mut w: Vec<i32> = board
        .items()
        .filter_map(|i| i.as_trace())
        .map(|t| 2 * t.half_width)
        .collect();
    w.sort_unstable();
    w
}

fn incomplete(board: &RoutingBoard) -> usize {
    board.routing_ledger().incomplete_count(board)
}

#[test]
fn without_a_rule_the_legacy_pin_neck_down_goes_under_the_class_width() {
    // The control: nothing known, Java's neck-down into the 0.12 mm pad.
    let board = route(prepare("legacy", None));
    assert_eq!(incomplete(&board), 0);
    assert_eq!(widths(&board).first(), Some(&1198), "{:?}", widths(&board));
}

#[test]
fn an_unconditional_track_width_rule_floors_every_routed_trace() {
    // (rule, floor in board units, connections left open): at 0.15 mm the
    // neck still fits at the floor; at 0.2 mm (the class width) no narrower
    // trace may be made, so the connection stays open. Without the micro
    // neck-down's floor (`insert_fanout_micro_neckdown` skipping candidates
    // under `min_trace_half_width`), `legacy` at 0.2 mm ships its 3/4
    // candidate, a 0.15 mm trace (checked 2026-10-21 by removing the skip).
    for profile in ["legacy", "enhanced"] {
        for (min, floor, open) in [
            ("0.15mm", 1500, 0),
            ("0.127mm", 1270, 0),
            ("0.2mm", 2000, 1),
        ] {
            let prepared = prepare(profile, Some(track_width_rule(min)));
            assert_eq!(
                prepared.settings.board_minimums.track_width_um,
                Some(f64::from(floor) / 10.0),
                "{profile} {min}: the rule reaches the setting"
            );
            let board = route(prepared);
            let w = widths(&board);
            assert!(!w.is_empty(), "{profile} {min}: nothing routed");
            assert!(
                w.iter().all(|w| *w >= floor),
                "{profile} {min}: a trace under the rule: {w:?}"
            );
            assert_eq!(incomplete(&board), open, "{profile} {min}: {w:?}");
        }
    }
}

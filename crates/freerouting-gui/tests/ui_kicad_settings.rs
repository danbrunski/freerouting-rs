//! K15.10: the desktop app routes a `.kicad_pcb` with the board's own
//! minimums, as the CLI does. The session harness opens a board with a
//! `.kicad_pro` minimum and a `.kicad_dru` floor above it; the settings the
//! app starts a job with are those that `prepare_job` makes of the same board
//! with `JobInput::set_kicad` (the CLI's route and `--dump-settings`), values
//! and sources. No window opens: the harness has no renderer.
//!
//! The settings window shows no provenance (Java's window has none), so the
//! sources are compared on `Autoroute::provenance`, which the window does not
//! read.

mod common;

use std::path::{Path, PathBuf};

use common::plain_harness;
use freerouting::job::prepare::{JobInput, prepare_job, read_kicad_for_route};
use freerouting::settings::{ResolveContext, SettingsPatch, Source};
use freerouting_gui::App;
use freerouting_gui::commands::Command;

/// Board Setup's minimum track width (um) in the project file.
const PRO_TRACK_UM: f64 = 150.0;
/// The `.kicad_dru` track width floor (um), above Board Setup's.
const DRU_TRACK_UM: f64 = 250.0;

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("k1510-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// The `classes` board of the synthetic set with a `.kicad_pro` that sets
/// the minimum track width and a `.kicad_dru` text.
fn board_with(dir: &Path, dru: &str) -> PathBuf {
    let synthetic = common::fixtures().join("kicad/synthetic");
    std::fs::copy(
        synthetic.join("classes.kicad_pcb"),
        dir.join("classes.kicad_pcb"),
    )
    .unwrap();
    let pro = std::fs::read_to_string(synthetic.join("classes.kicad_pro")).unwrap();
    let rules = format!(
        "{{\n  \"board\": {{\"design_settings\": {{\"rules\": {{\"min_track_width\": {}, \
         \"min_via_annular_width\": 0.1}}}}}},",
        PRO_TRACK_UM / 1000.0
    );
    let open = pro.find('{').unwrap();
    std::fs::write(
        dir.join("classes.kicad_pro"),
        format!("{rules}{}", &pro[open + 1..]),
    )
    .unwrap();
    std::fs::write(dir.join("classes.kicad_dru"), dru).unwrap();
    dir.join("classes.kicad_pcb")
}

fn floor_rule() -> String {
    format!(
        "(version 1)\n(rule \"minimum track\"\n\t(constraint track_width (min {}mm)))\n",
        DRU_TRACK_UM / 1000.0
    )
}

/// What the CLI's route prepares for `board`: `--dump-settings` and the route
/// both go through `prepare_job` on a `JobInput` that `set_kicad` filled.
fn cli_settings(board: &Path) -> freerouting::job::prepare::PreparedJob {
    let read = read_kicad_for_route(board, true).unwrap();
    let mut input = JobInput::new(Vec::new(), "classes");
    input.dsn_dir = board.parent().map(Path::to_path_buf);
    input.set_kicad(read);
    prepare_job(&input, &ResolveContext::from_machine()).unwrap()
}

fn open_settings(board: &Path) -> (egui_kittest::Harness<'static, App>, SettingsPatch) {
    let mut h = plain_harness(&[board.to_str().unwrap()]);
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, Command::ShowAutorouteSettings);
    let _ = h.run_ok();
    let applied = h.state().autoroute().applied.clone().expect("settings");
    (h, applied)
}

#[test]
fn the_app_routes_with_the_settings_of_the_cli() {
    let dir = scratch("same");
    let board = board_with(&dir, &floor_rule());
    let cli = cli_settings(&board);
    let (h, applied) = open_settings(&board);

    // The values: Board Setup's 150 um is raised to the rule's 250 um.
    assert_eq!(
        cli.patch.board_minimums.track_width_um,
        Some(DRU_TRACK_UM),
        "the CLI's floor"
    );
    assert_eq!(applied.board_minimums.track_width_um, Some(DRU_TRACK_UM));
    let baseline = h.state().autoroute().baseline.clone().expect("baseline");
    assert_eq!(baseline, cli.patch, "the app's baseline is the CLI's patch");
    assert_eq!(applied, cli.patch, "no user changes: the job's patch too");

    // The sources.
    let provenance = h.state().autoroute().provenance.clone().expect("sources");
    assert_eq!(provenance, cli.provenance);
    assert!(
        matches!(
            provenance.get("board_minimums.track_width_um"),
            Some(Source::Custom { name, .. }) if name == "kicad_pro board minimums"
        ),
        "{:?}",
        provenance.get("board_minimums.track_width_um")
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// A rule below Board Setup's minimum leaves Board Setup's, in both.
#[test]
fn a_rule_below_board_setup_leaves_it() {
    let dir = scratch("below");
    let board = board_with(
        &dir,
        "(version 1)\n(rule \"minimum track\"\n\t(constraint track_width (min 0.1mm)))\n",
    );
    let cli = cli_settings(&board);
    let (_h, applied) = open_settings(&board);
    assert_eq!(cli.patch.board_minimums.track_width_um, Some(PRO_TRACK_UM));
    assert_eq!(applied, cli.patch);
    let _ = std::fs::remove_dir_all(dir);
}

/// The app's log has the CLI's lines about the floors, and a `.kicad_dru`
/// that does not parse gives the CLI's warning among the board's warnings.
#[test]
fn the_log_and_warnings_are_the_clis() {
    let dir = scratch("log");
    let board = board_with(&dir, &floor_rule());
    let cli = cli_settings(&board);
    let raised = cli
        .log
        .iter()
        .find(|l| l.text.contains("track_width_um raised from 150 to 250"))
        .expect("the CLI logs the raise")
        .text
        .clone();
    let (h, _) = open_settings(&board);
    assert!(
        h.state().log_entries().iter().any(|e| e.text == raised),
        "{raised}"
    );

    let broken = scratch("broken");
    let board = board_with(&broken, "(rule \"x\" (constraint");
    let read = read_kicad_for_route(&board, true).unwrap();
    let cli_warning = freerouting::job::prepare::KicadSources::from_read(&read)
        .read_log
        .into_iter()
        .map(|l| l.text)
        .find(|t| t.contains(".kicad_dru file could not be read"))
        .expect("the CLI warns about the file");
    let h = plain_harness(&[board.to_str().unwrap()]);
    let warnings = &h.state().loaded().unwrap().session.warnings;
    assert!(
        warnings.contains(&cli_warning),
        "{cli_warning} in {warnings:?}"
    );
    let _ = std::fs::remove_dir_all(dir);
    let _ = std::fs::remove_dir_all(broken);
}

/// K15.11: the app's board has the KiCad rules the CLI's DRC report counts
/// with (the `.kicad_dru`'s unconditional clearance and hole clearance, the
/// pads' holes), so its DRC export counts as `-drc` does.
#[test]
fn the_app_board_has_the_clis_report_rules() {
    let data =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../freerouting/tests/data/kicad/pcu-k1511");
    for case in ["f-rule-clearance", "e-npth-pads"] {
        let dir = scratch(case);
        for ext in ["kicad_pcb", "kicad_pro", "kicad_dru"] {
            std::fs::copy(
                data.join(format!("{case}.{ext}")),
                dir.join(format!("{case}.{ext}")),
            )
            .unwrap();
        }
        let board = dir.join(format!("{case}.kicad_pcb"));
        let cli = cli_settings(&board);
        let rules = cli.board.kicad_report_rules().clone();
        assert!(
            rules.clearance_um.is_some() || !rules.holes.is_empty(),
            "{case}: the CLI's board has report rules: {rules:?}"
        );
        let h = plain_harness(&[board.to_str().unwrap()]);
        let session = &h.state().loaded().unwrap().session;
        assert_eq!(session.board.kicad_report_rules(), &rules, "{case}");
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// K15.19: the app's board gives the pins the real holes the CLI's board gives
/// them (`BasicBoard::set_pad_holes`), so a route in the app keeps the hole
/// clearance from J10's slot as the CLI's does.
#[test]
fn the_app_board_has_the_clis_pad_holes() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../freerouting/tests/data/kicad/k1519");
    let case = "wisweep-j10-slot";
    let dir = scratch(case);
    for ext in ["kicad_pcb", "kicad_pro"] {
        std::fs::copy(
            data.join(format!("{case}.{ext}")),
            dir.join(format!("{case}.{ext}")),
        )
        .unwrap();
    }
    let board = dir.join(format!("{case}.kicad_pcb"));
    let holes = |board: &freerouting::board::RoutingBoard| {
        board
            .items()
            .filter(|item| item.is_pin())
            .filter_map(|item| {
                let drill = item.drill()?;
                let hole = drill.hole?;
                Some((drill.center.x, drill.center.y, hole.radius, hole.half))
            })
            .collect::<Vec<_>>()
    };
    let cli = holes(&cli_settings(&board).board);
    assert!(!cli.is_empty(), "the CLI's board has the pins' holes");
    let h = plain_harness(&[board.to_str().unwrap()]);
    let session = &h.state().loaded().unwrap().session;
    assert_eq!(holes(&session.board), cli);
    let _ = std::fs::remove_dir_all(dir);
}

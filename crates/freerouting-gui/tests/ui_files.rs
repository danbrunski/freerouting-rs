//! G08: Save As, the unsaved-changes prompts, the `.rules` import and the
//! front-end mode (`-de`, `-do`) with its auto-start countdown.

mod common;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use common::{SIZE, fixtures};
use eframe::egui;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use freerouting::dsn::Pcb;
use freerouting::io::specctra::{BoardReadResult, build_board, read_rules, read_session};
use freerouting::scoring::BoardStatistics;
use freerouting_gui::args::Args;
use freerouting_gui::commands::{Answer, Command};
use freerouting_gui::{App, Prompt};

const ISSUE159: &str = "Issue159-setonix_2hp-pcb.dsn";
const ISSUE026: &str = "Issue026-J2_reference";
const ISSUE508: &str = "Issue508-DAC2020_bm05";
const ISSUE593: &str = "Issue593-BBD_Mars-64";

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("g08-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn strings(args: &[&str]) -> Vec<std::ffi::OsString> {
    args.iter().map(std::ffi::OsString::from).collect()
}

/// An app started with the command line `args`, stepped once.
fn app(args: Vec<std::ffi::OsString>) -> Harness<'static, App> {
    let args = Args::parse(args).expect("the arguments parse");
    let mut h = Harness::builder()
        .with_size(SIZE)
        .build_eframe(move |cc| App::build(&cc.egui_ctx, cc.storage, args.clone()));
    h.step();
    h
}

/// An app that opened `files` (names in `fixtures/`, or absolute paths), loaded.
fn open(files: &[&Path]) -> Harness<'static, App> {
    let args = files.iter().map(|f| f.as_os_str().to_owned()).collect();
    let mut h = app(args);
    wait_loaded(&mut h);
    answer_start_prompts(&mut h);
    h
}

fn fixture(name: &str) -> PathBuf {
    fixtures().join(name)
}

/// Steps until the load is done.
fn wait_loaded(h: &mut Harness<'_, App>) {
    let start = Instant::now();
    h.step();
    while h.state().is_loading() {
        assert!(start.elapsed() < Duration::from_secs(300), "no load");
        std::thread::sleep(Duration::from_millis(5));
        h.step();
    }
    assert!(h.state().error().is_none(), "{:?}", h.state().error());
    h.step();
}

/// Says No to the question about the rules next to the design, if asked.
fn answer_start_prompts(h: &mut Harness<'_, App>) {
    for _ in 0..4 {
        h.step();
        if matches!(h.state().prompt(), Some(Prompt::ImportRules { .. })) {
            exec(h, Command::AnswerPrompt(Answer::No));
        }
    }
}

fn exec(h: &mut Harness<'_, App>, command: Command) {
    let ctx = h.ctx.clone();
    h.state_mut().execute(&ctx, command);
    h.step();
    h.step();
}

fn stats(h: &Harness<'_, App>) -> BoardStatistics {
    BoardStatistics::new(&h.state().loaded().unwrap().session.board)
}

/// The numbers and the text between them of a `Debug` string.
fn tokens(text: &str) -> Vec<(bool, String)> {
    let mut out: Vec<(bool, String)> = Vec::new();
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let numeric = chars[i].is_ascii_digit()
            || (chars[i] == '-' && chars.get(i + 1).is_some_and(char::is_ascii_digit));
        let start = i;
        i += 1;
        if numeric {
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '.') {
                i += 1;
            }
        } else {
            while i < chars.len()
                && !(chars[i].is_ascii_digit()
                    || (chars[i] == '-' && chars.get(i + 1).is_some_and(char::is_ascii_digit)))
            {
                i += 1;
            }
        }
        out.push((numeric, chars[start..i].iter().collect()));
    }
    out
}

/// The comparison of `tests/reference_load.rs` for the statistics: counts are
/// equal, floats (the ones with a decimal point) are equal within the
/// rounding of Java's `%.2f` plus the error of summing in another order
/// (`floats_match`: 0.006 + 1e-5 of the value).
fn assert_same_stats(a: &BoardStatistics, b: &BoardStatistics, what: &str) {
    let (da, db) = (format!("{a:?}"), format!("{b:?}"));
    let (ta, tb) = (tokens(&da), tokens(&db));
    let mut diff = Vec::new();
    if ta.len() != tb.len() {
        diff.push("different shapes".to_string());
    }
    for ((na, x), (nb, y)) in ta.iter().zip(&tb) {
        let floats = (*na && *nb && x.contains('.'))
            .then(|| x.parse::<f64>().ok().zip(y.parse::<f64>().ok()))
            .flatten();
        let same = match floats {
            Some((fx, fy)) => (fx - fy).abs() <= 0.006 + fx.abs() * 1e-5,
            None => x == y,
        };
        if !same {
            diff.push(format!("{x} != {y}"));
        }
    }
    assert!(
        diff.is_empty(),
        "{what}: statistics differ: {diff:?}\n{da}\n{db}"
    );
}

fn close_request(h: &mut Harness<'_, App>) {
    h.input_mut()
        .viewports
        .entry(egui::ViewportId::ROOT)
        .or_default()
        .events
        .push(egui::ViewportEvent::Close);
    h.step();
}

/// Whether the frame just run told the window not to close.
fn cancelled_close(h: &Harness<'_, App>) -> bool {
    h.output()
        .viewport_output
        .get(&egui::ViewportId::ROOT)
        .is_some_and(|v| v.commands.contains(&egui::ViewportCommand::CancelClose))
}

/// Steps until `done`, failing after `secs`.
fn step_until(h: &mut Harness<'_, App>, secs: u64, what: &str, done: impl Fn(&App) -> bool) {
    let start = Instant::now();
    while !done(h.state()) {
        assert!(
            start.elapsed() < Duration::from_secs(secs),
            "timed out: {what}"
        );
        h.step();
        std::thread::sleep(Duration::from_millis(2));
    }
}

/// Changes the board: deletes all traces and vias (undoable).
fn edit(h: &mut Harness<'_, App>) {
    exec(h, Command::ConfirmDeleteAll);
}

// ---------------------------------------------------------------------------
// G08.1: Save As
// ---------------------------------------------------------------------------

/// The fixtures with a session next to them that the saving tests use. Their
/// DSN has no wiring of its own: a saved session holds all the wiring of the
/// board, so on a DSN with wiring (Issue313) it would be imported twice.
const ROUTED: [&str; 3] = [ISSUE026, ISSUE508, ISSUE593];

/// Boards whose DSN is written and read back to the same statistics.
/// Issue593's DSN has a footprint without pins or pads (`G***`) that the DSN
/// writer leaves out, so that board has one component less afterwards.
const DSN_ROUND_TRIP: [&str; 3] = [ISSUE026, ISSUE508, "Issue742-tastexx-pcb"];

fn design_and_session(stem: &str) -> [PathBuf; 2] {
    [
        fixture(&format!("{stem}.dsn")),
        fixture(&format!("{stem}.ses")),
    ]
}

#[test]
fn save_ses_then_reopen_has_equal_statistics() {
    let dir = scratch("save-ses");
    for stem in ROUTED {
        let [dsn, ses] = design_and_session(stem);
        let mut h = open(&[&dsn, &ses]);
        let before = stats(&h);
        assert!(before.items.trace_count > 0, "{stem} has a routed session");
        let out = dir.join(format!("{stem}-saved.ses"));
        exec(&mut h, Command::SaveAs(out.clone()));
        assert!(out.is_file(), "{stem}: nothing was written");
        // The rules question follows a session.
        assert!(matches!(h.state().prompt(), Some(Prompt::SaveRules { .. })));
        exec(&mut h, Command::AnswerPrompt(Answer::No));
        assert!(!h.state().loaded().unwrap().session.is_dirty());

        let again = open(&[&dsn, &out]);
        assert_same_stats(&stats(&again), &before, &format!("{stem} after reopening"));
        let summary = again.state().loaded().unwrap().session.ses_summary;
        assert_eq!(summary.unwrap().errors_encountered, 0, "{stem}");
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_dsn_round_trip() {
    let dir = scratch("save-dsn");
    for stem in DSN_ROUND_TRIP {
        let [dsn, ses] = design_and_session(stem);
        let mut h = open(&[&dsn, &ses]);
        let before = stats(&h);
        let out = dir.join(format!("{stem}-saved.dsn"));
        exec(&mut h, Command::SaveAs(out.clone()));
        assert!(out.is_file(), "{stem}: nothing was written");
        // A design file has no rules question.
        assert!(h.state().prompt().is_none());
        let again = open(&[&out]);
        assert_same_stats(&stats(&again), &before, &format!("{stem} after reopening"));
    }
    let _ = std::fs::remove_dir_all(dir);
}

/// A fresh board of the fixture's design.
fn fresh_board(stem: &str) -> freerouting::board::RoutingBoard {
    let text = std::fs::read_to_string(fixture(&format!("{stem}.dsn"))).unwrap();
    let pcb = Pcb::parse(&text).unwrap();
    match build_board(&pcb) {
        BoardReadResult::Success { board, .. } => *board,
        other => panic!("{other:?}"),
    }
}

#[test]
fn rules_prompt_writes_rules_file() {
    let dir = scratch("rules-prompt");
    let [dsn, ses] = design_and_session(ISSUE026);
    let mut h = open(&[&dsn, &ses]);

    // Yes writes `<stem>.rules` next to the output.
    let out = dir.join("yes.ses");
    exec(&mut h, Command::SaveAs(out.clone()));
    assert!(matches!(h.state().prompt(), Some(Prompt::SaveRules { .. })));
    exec(&mut h, Command::AnswerPrompt(Answer::Yes));
    let rules = dir.join("yes.rules");
    assert!(rules.is_file(), "the rules file was not written");
    assert!(h.state().prompt().is_none());
    // C04's reader takes it into a fresh board.
    let text = std::fs::read_to_string(&rules).unwrap();
    let (_board, result) = read_rules(fresh_board(ISSUE026), &text, ISSUE026);
    result.expect("the rules file reads back");

    // No writes nothing.
    let out = dir.join("no.ses");
    exec(&mut h, Command::SaveAs(out.clone()));
    exec(&mut h, Command::AnswerPrompt(Answer::No));
    assert!(out.is_file());
    assert!(!dir.join("no.rules").exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[cfg(unix)]
#[test]
fn failed_write_leaves_old_file() {
    use std::os::unix::fs::PermissionsExt;
    let dir = scratch("read-only");
    let [dsn, ses] = design_and_session(ISSUE026);
    let mut h = open(&[&dsn, &ses]);
    let out = dir.join("old.ses");
    std::fs::write(&out, "old content").unwrap();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o555)).unwrap();
    // As root the permission does nothing; there is nothing to test then.
    let probe = dir.join("probe");
    if std::fs::write(&probe, "x").is_ok() {
        let _ = std::fs::remove_file(&probe);
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
        eprintln!("skipped: the directory is writable despite 0555 (running as root)");
        return;
    }
    exec(&mut h, Command::SaveAs(out.clone()));
    let notice = h.state().notice().unwrap_or_default().to_string();
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        notice.starts_with("Unable to save the Specctra session file"),
        "{notice}"
    );
    assert_eq!(std::fs::read(&out).unwrap(), b"old content");
    // Nothing else was left in the folder, and no rules question was asked.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    assert!(h.state().prompt().is_none());
    // The session still counts as unsaved? It was never edited, so it is clean;
    // the failure must not mark anything saved either.
    assert!(!h.state().loaded().unwrap().session.is_dirty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn save_refuses_other_extensions_and_writes_nothing() {
    let dir = scratch("extension");
    let [dsn, ses] = design_and_session(ISSUE026);
    let mut h = open(&[&dsn, &ses]);
    let out = dir.join("x.scr");
    exec(&mut h, Command::SaveAs(out.clone()));
    assert!(!out.exists());
    assert!(h.state().notice().unwrap().contains("Only .ses and .dsn"));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn ctrl_s_opens_the_save_dialog_and_asks_before_replacing() {
    let dir = scratch("dialog");
    let [dsn, ses] = design_and_session(ISSUE026);
    let mut h = open(&[&dsn, &ses]);
    // The default is `<design dir>/<design stem>.ses`.
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    h.step();
    h.step();
    assert!(h.state().is_saving_dialog_open());
    assert_eq!(h.state().save_dialog_name(), format!("{ISSUE026}.ses"));
    // Escape the dialog without saving: nothing is written beside the fixtures.
    exec(&mut h, Command::SaveCancelled);

    // After a save the dialog starts at that file.
    let out = dir.join("a.ses");
    exec(&mut h, Command::SaveAs(out.clone()));
    exec(&mut h, Command::AnswerPrompt(Answer::No));
    let first = std::fs::read(&out).unwrap();
    exec(&mut h, Command::SaveAsDialog);
    assert_eq!(h.state().save_dialog_name(), "a.ses");
    h.get_by_label("Save").click();
    h.step();
    h.step();
    // The file exists: the dialog asks, nothing is written yet.
    assert!(h.state().is_saving_dialog_open());
    h.get_by_label_contains("already exists");
    h.get_by_label("Replace").click();
    h.step();
    h.step();
    assert!(!h.state().is_saving_dialog_open());
    assert_eq!(std::fs::read(&out).unwrap(), first, "same board, same file");
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// G08.2: the questions
// ---------------------------------------------------------------------------

#[test]
fn close_with_changes_asks() {
    let [dsn, ses] = design_and_session(ISSUE026);
    let mut h = open(&[&dsn, &ses]);
    // Unchanged: the window closes (no CancelClose, no question).
    close_request(&mut h);
    assert!(!cancelled_close(&h));
    assert!(h.state().prompt().is_none());

    edit(&mut h);
    assert!(h.state().loaded().unwrap().session.is_changed());
    close_request(&mut h);
    assert!(cancelled_close(&h), "the close was not held back");
    assert_eq!(h.state().prompt(), Some(&Prompt::Exit));
    // A new window is laid out in its second frame.
    h.step();
    h.get_by_label_contains("Are you sure you want to exit?");
    // No keeps the app running.
    h.get_by_label("No").click();
    h.step();
    h.step();
    assert!(h.state().prompt().is_none());
    assert!(!h.state().close_sent());
    assert!(h.state().loaded().is_some());
    // Asking again, then Yes closes.
    close_request(&mut h);
    exec(&mut h, Command::AnswerPrompt(Answer::Yes));
    assert!(h.state().close_sent());
}

#[test]
fn undo_to_saved_state_is_clean() {
    let [dsn, ses] = design_and_session(ISSUE026);
    let mut h = open(&[&dsn, &ses]);
    edit(&mut h);
    assert!(h.state().is_board_changed());
    exec(&mut h, Command::Undo);
    let session = &h.state().loaded().unwrap().session;
    assert!(session.is_dirty(), "the edit counter does not go back");
    assert!(!session.is_changed(), "the content is the saved one again");
    close_request(&mut h);
    assert!(!cancelled_close(&h), "an undone edit asked about saving");
    assert!(h.state().prompt().is_none());
}

#[test]
fn save_marks_the_state_clean() {
    let dir = scratch("clean");
    let [dsn, ses] = design_and_session(ISSUE026);
    let mut h = open(&[&dsn, &ses]);
    edit(&mut h);
    assert!(h.state().is_board_changed());
    exec(&mut h, Command::SaveAs(dir.join("a.ses")));
    exec(&mut h, Command::AnswerPrompt(Answer::No));
    assert!(!h.state().is_board_changed());
    // A change after the save is a change again; undoing it returns to the saved state.
    exec(&mut h, Command::Undo);
    assert!(h.state().is_board_changed(), "the undo changed the content");
    exec(&mut h, Command::Redo);
    assert!(!h.state().is_board_changed());
    let _ = std::fs::remove_dir_all(dir);
}

fn design_path(h: &Harness<'_, App>) -> PathBuf {
    h.state().loaded().unwrap().session.paths.design.clone()
}

#[test]
fn drop_with_changes_save_then_load() {
    let dir = scratch("drop");
    let [dsn, ses] = design_and_session(ISSUE026);
    let other = fixture("Issue159-setonix_2hp-pcb.dsn");
    let mut h = open(&[&dsn, &ses]);

    // Unchanged: the dropped file loads at once.
    exec(&mut h, Command::Open(other.clone()));
    assert!(h.state().prompt().is_none());
    wait_loaded(&mut h);
    assert_eq!(design_path(&h), other);

    // Changed, Cancel: the old board stays.
    let mut h = open(&[&dsn, &ses]);
    edit(&mut h);
    exec(&mut h, Command::Open(other.clone()));
    assert_eq!(h.state().prompt(), Some(&Prompt::UnsavedBeforeLoad));
    h.get_by_label_contains("unsaved changes");
    h.get_by_label("Cancel").click();
    h.step();
    h.step();
    assert!(h.state().prompt().is_none() && !h.state().is_loading());
    assert_eq!(design_path(&h), dsn);
    assert!(h.state().is_board_changed(), "Cancel changed nothing");

    // Don't Save: loads without writing.
    exec(&mut h, Command::Open(other.clone()));
    h.get_by_label("Don't Save").click();
    h.step();
    wait_loaded(&mut h);
    assert_eq!(design_path(&h), other);

    // Save: Save As opens, and the file loads after the save.
    let mut h = open(&[&dsn, &ses]);
    edit(&mut h);
    exec(&mut h, Command::Open(other.clone()));
    h.get_by_label("Save").click();
    h.step();
    h.step();
    assert!(h.state().is_saving_dialog_open());
    assert_eq!(design_path(&h), dsn, "nothing loads before the save");
    let out = dir.join("kept.ses");
    exec(&mut h, Command::SaveAs(out.clone()));
    assert!(out.is_file());
    // The rules question comes first; the new file loads after the answer.
    assert!(matches!(h.state().prompt(), Some(Prompt::SaveRules { .. })));
    assert_eq!(design_path(&h), dsn);
    exec(&mut h, Command::AnswerPrompt(Answer::No));
    wait_loaded(&mut h);
    assert_eq!(design_path(&h), other);

    // Save, then the dialog is closed without a file: the old board stays.
    let mut h = open(&[&dsn, &ses]);
    edit(&mut h);
    exec(&mut h, Command::Open(other.clone()));
    exec(&mut h, Command::AnswerPrompt(Answer::Yes));
    exec(&mut h, Command::SaveCancelled);
    h.step();
    assert!(!h.state().is_loading());
    assert_eq!(design_path(&h), dsn);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn a_failed_save_does_not_load_the_new_file() {
    let dir = scratch("failed-save-load");
    let [dsn, ses] = design_and_session(ISSUE026);
    let other = fixture(ISSUE159);
    let mut h = open(&[&dsn, &ses]);
    edit(&mut h);
    exec(&mut h, Command::Open(other));
    exec(&mut h, Command::AnswerPrompt(Answer::Yes));
    // A folder that does not exist: the write fails.
    exec(&mut h, Command::SaveAs(dir.join("missing").join("x.ses")));
    h.step();
    assert!(!h.state().is_loading());
    assert_eq!(design_path(&h), dsn);
    assert!(h.state().is_board_changed());
    let _ = std::fs::remove_dir_all(dir);
}

// ---------------------------------------------------------------------------
// G08.3: the .rules file next to the design
// ---------------------------------------------------------------------------

const ISSUE029: &str = "Issue029-hw48na";

fn rules_debug(board: &freerouting::board::RoutingBoard) -> (String, String) {
    (
        format!("{:?}", board.rules.clearance_matrix),
        format!("{:?}", board.rules.net_classes),
    )
}

#[test]
fn rules_next_to_dsn_imported_after_confirm() {
    let dsn = fixture(&format!("{ISSUE029}.dsn"));
    let rules = fixture(&format!("{ISSUE029}.rules"));
    let built = rules_debug(&fresh_board(ISSUE029));

    // What C04's reader gives a fresh board.
    let text = freerouting::dsn::decode(&std::fs::read(&rules).unwrap()).into_owned();
    let (read, result) = read_rules(fresh_board(ISSUE029), &text, ISSUE029);
    result.unwrap();
    let imported = rules_debug(&read);
    assert_ne!(built, imported, "the rules file changes something");

    // Yes imports.
    let mut h = app(strings(&[dsn.to_str().unwrap()]));
    wait_loaded(&mut h);
    step_until(&mut h, 20, "the question", |a| a.prompt().is_some());
    assert!(matches!(
        h.state().prompt(),
        Some(Prompt::ImportRules { .. })
    ));
    h.get_by_label_contains("Please confirm that you want to import the stored rules.");
    assert_eq!(
        rules_debug(&h.state().loaded().unwrap().session.board),
        built
    );
    h.get_by_label("Yes").click();
    h.step();
    h.step();
    assert!(h.state().prompt().is_none());
    assert_eq!(
        rules_debug(&h.state().loaded().unwrap().session.board),
        imported
    );
    assert!(
        h.state().loaded().unwrap().session.is_changed(),
        "the import is an edit"
    );

    // No leaves them as the DSN built them.
    let mut h = app(strings(&[dsn.to_str().unwrap()]));
    wait_loaded(&mut h);
    step_until(&mut h, 20, "the question", |a| a.prompt().is_some());
    h.get_by_label("No").click();
    h.step();
    h.step();
    assert_eq!(
        rules_debug(&h.state().loaded().unwrap().session.board),
        built
    );
    assert!(!h.state().loaded().unwrap().session.is_changed());
}

#[test]
fn file_open_does_not_look_for_rules() {
    // Java's File > Open (`BoardFrameFileActions.open`) never reads the rules
    // file; only the start-up load does.
    let dsn = fixture(&format!("{ISSUE029}.dsn"));
    let mut h = open(&[&fixture(ISSUE159)]);
    exec(&mut h, Command::Open(dsn));
    wait_loaded(&mut h);
    h.step();
    assert!(h.state().prompt().is_none());
}

#[test]
fn dr_flag_imports_without_asking() {
    let dsn = fixture(&format!("{ISSUE029}.dsn"));
    let rules = fixture(&format!("{ISSUE029}_valid.rules"));
    let mut h = app(strings(&[
        "-de",
        dsn.to_str().unwrap(),
        "-dr",
        rules.to_str().unwrap(),
    ]));
    wait_loaded(&mut h);
    step_until(&mut h, 20, "the import", |a| {
        a.loaded().is_some_and(|b| b.session.is_changed())
    });
    assert!(h.state().prompt().is_none());
    let text = freerouting::dsn::decode(&std::fs::read(&rules).unwrap()).into_owned();
    let (read, result) = read_rules(fresh_board(ISSUE029), &text, ISSUE029);
    result.unwrap();
    assert_eq!(
        rules_debug(&h.state().loaded().unwrap().session.board),
        rules_debug(&read)
    );
}

// ---------------------------------------------------------------------------
// G08.4: the front-end mode
// ---------------------------------------------------------------------------

fn frontend(dir: &Path, extra: &[&str]) -> Harness<'static, App> {
    let design = fixture(ISSUE159);
    let out = dir.join("out.ses");
    let mut args = vec![
        "-de",
        design.to_str().unwrap(),
        "-do",
        out.to_str().unwrap(),
        "-mp",
        "2",
    ];
    args.extend_from_slice(extra);
    app(strings(&args))
}

#[test]
fn frontend_deletes_old_output() {
    let dir = scratch("fe-delete");
    let out = dir.join("out.ses");
    std::fs::write(&out, "stale").unwrap();
    // A countdown, so that nothing is written yet.
    let h = frontend(&dir, &["-dct", "60"]);
    assert!(!out.exists(), "the stale output is still there");
    drop(h);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn frontend_writes_on_finish() {
    let dir = scratch("fe-finish");
    let mut h = frontend(&dir, &["-dct", "0"]);
    let out = dir.join("out.ses");
    step_until(&mut h, 300, "the output", |a| a.close_sent());
    assert!(out.is_file());
    assert_eq!(h.state().frontend_exit_code(), 0);
    assert!(h.state().exit_when_finished());
    // The session imports without errors.
    let text = std::fs::read_to_string(&out).unwrap();
    let mut board = fresh_board("Issue159-setonix_2hp-pcb");
    let summary = read_session(&mut board, &text).expect("the session reads");
    assert_eq!(summary.errors_encountered, 0);
    assert!(summary.wires_imported > 0, "the router wrote traces");
    // No temporary file is left.
    assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 1);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn frontend_writes_on_close_when_routed() {
    let dir = scratch("fe-close");
    // More passes than the router needs on this board before the close arrives.
    let design = fixture("Issue684-Autorouter_PCB1_2026-5-8.dsn");
    let out = dir.join("out.ses");
    let mut h = app(strings(&[
        "-de",
        design.to_str().unwrap(),
        "-do",
        out.to_str().unwrap(),
        "-dct",
        "0",
    ]));
    step_until(&mut h, 300, "the job", |a| a.is_routing());
    close_request(&mut h);
    assert!(cancelled_close(&h), "the close waits for the job");
    assert!(!h.state().close_sent());
    step_until(&mut h, 300, "the output", |a| a.close_sent());
    assert!(out.is_file(), "closing during routing wrote nothing");
    assert_eq!(h.state().frontend_exit_code(), 0);
    let text = std::fs::read_to_string(&out).unwrap();
    let mut board = fresh_board("Issue684-Autorouter_PCB1_2026-5-8");
    read_session(&mut board, &text).expect("the session reads");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn frontend_close_after_cancelled_countdown_writes_a_routed_board() {
    // No job ran, but the board has traces (a session was opened): Java writes
    // the output on close anyway.
    let dir = scratch("fe-close-traces");
    let [dsn, ses] = design_and_session(ISSUE026);
    let out = dir.join("out.ses");
    let mut h = app(strings(&[
        "-de",
        dsn.to_str().unwrap(),
        ses.to_str().unwrap(),
        "-do",
        out.to_str().unwrap(),
        "-dct",
        "60",
    ]));
    wait_loaded(&mut h);
    answer_start_prompts(&mut h);
    h.get_by_label("Cancel").click();
    h.step();
    h.step();
    assert!(h.state().autostart_label().is_none());
    close_request(&mut h);
    assert!(out.is_file());
    assert!(h.state().close_sent());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn frontend_close_of_an_untouched_empty_board_writes_nothing() {
    let dir = scratch("fe-close-empty");
    let mut h = frontend(&dir, &["-dct", "60"]);
    wait_loaded(&mut h);
    h.get_by_label("Cancel").click();
    h.step();
    h.step();
    close_request(&mut h);
    // Issue159 has no traces: nothing changed, so nothing is written, and the
    // window closes.
    assert!(!dir.join("out.ses").exists());
    assert!(h.state().close_sent());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn frontend_failed_write_exits_with_1() {
    let dir = scratch("fe-fail");
    let design = fixture(ISSUE159);
    let out = dir.join("no-such-folder").join("out.ses");
    let mut h = app(strings(&[
        "-de",
        design.to_str().unwrap(),
        "-do",
        out.to_str().unwrap(),
        "-dct",
        "0",
        "-mp",
        "1",
    ]));
    step_until(&mut h, 300, "the exit", |a| a.close_sent());
    assert_eq!(h.state().frontend_exit_code(), 1);
    assert!(!out.exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn without_do_nothing_is_written_or_started() {
    let design = fixture(ISSUE159);
    let mut h = app(strings(&["-de", design.to_str().unwrap(), "-dct", "0"]));
    wait_loaded(&mut h);
    for _ in 0..8 {
        h.step();
    }
    assert!(!h.state().is_routing());
    assert!(h.state().autostart_label().is_none());
    close_request(&mut h);
    assert!(!cancelled_close(&h));
}

// ---------------------------------------------------------------------------
// G08.5: the countdown
// ---------------------------------------------------------------------------

#[test]
fn countdown_starts_routing() {
    let dir = scratch("countdown");
    let mut h = frontend(&dir, &[]);
    // The harness advances the GUI clock by a quarter second per step. Its
    // builder already runs frames, and on a busy machine the load can finish
    // and the countdown start inside them, so the first labels may pass
    // unseen: the labels seen are the end of the countdown, each shown for
    // one second of GUI time.
    let start = Instant::now();
    let mut labels: Vec<String> = Vec::new();
    let mut changed_at: Vec<f64> = Vec::new();
    let mut steps_after_load = 0;
    while steps_after_load < 30 {
        h.step();
        if let Some(label) = h.state().autostart_label() {
            if labels.last().map(String::as_str) != Some(label) {
                labels.push(label.to_string());
                changed_at.push(h.ctx.input(|i| i.time));
            }
        }
        if h.state().is_routing() {
            break;
        }
        if h.state().is_loading() {
            assert!(start.elapsed() < Duration::from_secs(300), "no load");
            std::thread::sleep(Duration::from_millis(5));
        } else {
            steps_after_load += 1;
        }
    }
    assert!(h.state().error().is_none(), "{:?}", h.state().error());
    let expected = [
        "Start Now (5)",
        "Start Now (4)",
        "Start Now (3)",
        "Start Now (2)",
        "Start Now (1)",
    ];
    assert!(labels.len() >= 3, "{labels:?}");
    assert_eq!(labels, expected[expected.len() - labels.len()..]);
    for pair in changed_at.windows(2).skip(1) {
        assert!((pair[1] - pair[0] - 1.0).abs() < 1e-6, "{changed_at:?}");
    }
    assert!(
        h.state().is_routing(),
        "the countdown did not start the job"
    );
    assert!(h.state().autostart_label().is_none());
    assert!(h.state().exit_when_finished());
    step_until(&mut h, 300, "the exit", |a| a.close_sent());
    assert!(dir.join("out.ses").is_file());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn countdown_start_now_starts_at_once() {
    let dir = scratch("start-now");
    let mut h = frontend(&dir, &[]);
    wait_loaded(&mut h);
    h.step();
    h.get_by_label_contains("Start Now").click();
    h.step();
    h.step();
    assert!(h.state().is_routing() || h.state().autostart_label().is_none());
    assert!(h.state().exit_when_finished());
    step_until(&mut h, 300, "the exit", |a| a.close_sent());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn countdown_cancel_stays_idle() {
    let dir = scratch("cancel");
    let mut h = frontend(&dir, &[]);
    wait_loaded(&mut h);
    h.step();
    h.get_by_label_contains("The Autorouter is about to start.");
    h.get_by_label("Cancel").click();
    h.step();
    h.step();
    for _ in 0..40 {
        h.step();
    }
    assert!(!h.state().is_routing(), "Cancel left a job");
    assert!(h.state().autostart_label().is_none());
    assert!(!h.state().exit_when_finished());

    // A run started by hand still writes the output, but does not exit.
    exec(&mut h, Command::StartRouting);
    step_until(&mut h, 300, "the job", |a| !a.is_routing());
    h.step();
    assert!(dir.join("out.ses").is_file());
    assert!(!h.state().close_sent(), "a manual run exited the app");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn dct_zero_starts_without_dialog() {
    let dir = scratch("dct0");
    let mut h = frontend(&dir, &["-dct", "0"]);
    wait_loaded(&mut h);
    let mut shown = false;
    for _ in 0..200 {
        h.step();
        shown |= h.state().autostart_label().is_some();
        if h.state().is_routing() || h.state().close_sent() {
            break;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(!shown, "a countdown was shown");
    assert!(h.state().exit_when_finished());
    step_until(&mut h, 300, "the exit", |a| a.close_sent());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn autoroute_flag_is_dct_zero() {
    let dir = scratch("autoroute-flag");
    let mut h = frontend(&dir, &["--autoroute"]);
    step_until(&mut h, 300, "the exit", |a| a.close_sent());
    assert!(dir.join("out.ses").is_file());
    let _ = std::fs::remove_dir_all(dir);
}

/// K02.5: the router flags of the CLI are read by the front-end mode and land
/// on the settings of the job, over the board's baseline. The exit code and
/// the result are those of a routed job.
#[test]
fn frontend_takes_the_routers_flags_of_the_cli() {
    let dir = scratch("fe-router-flags");
    let mut h = frontend(
        &dir,
        &[
            "-dct",
            "0",
            "-mt",
            "2",
            "--router.optimizer.enabled=false",
            "--max-passes",
            "1",
        ],
    );
    step_until(&mut h, 300, "the exit", |a| a.close_sent());
    let applied = h
        .state()
        .autoroute()
        .applied
        .clone()
        .expect("the job started from settings");
    // `--max-passes 1` comes after `-mp 2`: the last one wins, as on the CLI.
    assert_eq!(applied.autorouter.max_passes, Some(1));
    assert_eq!(applied.optimizer.enabled, Some(false));
    assert!(dir.join("out.ses").is_file());
    assert_eq!(h.state().frontend_exit_code(), 0);
    let _ = std::fs::remove_dir_all(dir);
}

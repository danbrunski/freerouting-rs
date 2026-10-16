//! Replays an interactive edit script on a board (I01.8):
//!
//! ```text
//! cargo run --release -p freerouting --example edit_script -- <dsn> [ses] <script.json> <out> [--unlimited]
//! ```
//!
//! The board is loaded as the Java probes load it, the session `ses` (if given) imported, and the
//! script's route commands (start, moves, clicks, layer changes, snapshots, complete, cancel; the
//! format is in `tests/support/edit_script.rs`) replayed with Java's route budgets (3000 nominal
//! ms per check, 2000 for pull-tight, 64,000 work units each; `--unlimited`: none). `<out>` gets
//! one line per command and the canonical item dump. `tools/determinism-check.sh` runs it twice to
//! show the output does not depend on the machine load:
//!
//! ```text
//! tools/determinism-check.sh --out <scratch>/det -- target/release/examples/edit_script \
//!   fixtures/Issue026-J2_reference.dsn crates/freerouting/tests/data/edit_script_j2.json {OUT}
//! ```
//!
//! A script whose settings say `"tool": "editor"` is an editor script (I03.8): every
//! `interactive::Command`, replayed through `Editor::apply` (the format is in
//! `tests/support/editor_script.rs`), with Java's budgets (`--unlimited`: none).
//!
//! A script whose settings say `"tool": "drag"` is a drag script (I02.5): `press`, `drag_to` and
//! `release` commands (the format is in `tests/support/drag_script.rs`), replayed with Java's
//! `MoveComponent` and route budgets (`--unlimited`: none).

#[path = "../tests/board_probe/mod.rs"]
mod board_probe;
#[path = "../tests/support/drag_script.rs"]
mod drag_script;
#[path = "../tests/support/edit_script.rs"]
mod edit_script;
#[path = "../tests/support/editor_script.rs"]
mod editor_script;
#[path = "../tests/support/item_dump.rs"]
mod item_dump;

use std::process::ExitCode;

use freerouting::interactive::{DragBudget, EditorOptions, RouteBudget};

fn canonical(path: &str) -> String {
    std::fs::canonicalize(path).map_or_else(|_| path.to_string(), |p| p.display().to_string())
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let unlimited = if let Some(i) = args.iter().position(|a| a == "--unlimited") {
        args.remove(i);
        true
    } else {
        false
    };
    let (budget, drag_budget) = if unlimited {
        (RouteBudget::UNLIMITED, DragBudget::UNLIMITED)
    } else {
        (RouteBudget::JAVA, DragBudget::JAVA)
    };
    let (dsn, ses, script, out) = match args.as_slice() {
        [dsn, script, out] => (dsn, None, script, out),
        [dsn, ses, script, out] => (dsn, Some(ses), script, out),
        _ => {
            eprintln!("usage: edit_script <dsn> [ses] <script.json> <out> [--unlimited]");
            return ExitCode::from(2);
        }
    };
    let text = match std::fs::read_to_string(script) {
        Ok(text) => text,
        Err(e) => {
            eprintln!("edit_script: {script}: {e}");
            return ExitCode::from(1);
        }
    };
    if editor_script::is_editor_script(&text) {
        let options = EditorOptions {
            route_budget: budget,
            drag_budget,
            ..EditorOptions::default()
        };
        return run_editor(dsn, ses.map(String::as_str), &text, script, out, options);
    }
    let is_drag = drag_script::is_drag_script(&text);
    let parsed = if is_drag {
        drag_script::parse_drag_script(&text).map(|s| (None, Some(s)))
    } else {
        edit_script::parse_script(&text).map(|s| (Some(s), None))
    };
    let (route_script, drag_script) = match parsed {
        Ok(scripts) => scripts,
        Err(e) => {
            eprintln!("edit_script: {script}: {e}");
            return ExitCode::from(1);
        }
    };
    let row = board_probe::Row {
        dsn: canonical(dsn),
        ses: ses.map(|s| canonical(s)),
        board: dsn.clone(),
    };
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    let lines = match (&route_script, &drag_script) {
        (_, Some(drag)) => drag_script::replay_drag(&mut board, drag, drag_budget),
        (Some(route), None) => edit_script::replay(&mut board, route, budget),
        (None, None) => unreachable!("one script was parsed"),
    };
    let mut text = lines.join("\n");
    text.push('\n');
    if let Err(e) = std::fs::write(out, text) {
        eprintln!("edit_script: {out}: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Loads `dsn` (with the script's `dsn_insert`) and `ses`, replays the editor script and writes
/// the lines to `out`.
fn run_editor(
    dsn: &str,
    ses: Option<&str>,
    text: &str,
    script: &str,
    out: &str,
    options: EditorOptions,
) -> ExitCode {
    let fail = |message: String| {
        eprintln!("edit_script: {message}");
        ExitCode::from(1)
    };
    let dsn_text = match std::fs::read_to_string(dsn) {
        Ok(t) => t,
        Err(e) => return fail(format!("{dsn}: {e}")),
    };
    let dsn_text = match editor_script::dsn_insert(text) {
        Some(insert) => match editor_script::insert_before_network(&dsn_text, &insert) {
            Ok(t) => t,
            Err(e) => return fail(format!("{dsn}: {e}")),
        },
        None => dsn_text,
    };
    let mut board = match freerouting::drc::load_design(dsn_text.as_bytes()) {
        Ok(board) => board,
        Err(e) => return fail(format!("{dsn}: {e}")),
    };
    let row = board_probe::Row {
        dsn: canonical(dsn),
        ses: ses.map(canonical),
        board: dsn.to_string(),
    };
    board_probe::import_session(&mut board, &row);
    let parsed = editor_script::parse_editor_script(text, board.layer_count());
    let parsed = match parsed {
        Ok(s) => s,
        Err(e) => return fail(format!("{script}: {e}")),
    };
    let mut lines = editor_script::replay_editor(board, &parsed, options).join("\n");
    lines.push('\n');
    if let Err(e) = std::fs::write(out, lines) {
        return fail(format!("{out}: {e}"));
    }
    ExitCode::SUCCESS
}

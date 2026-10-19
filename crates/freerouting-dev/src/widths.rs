//! `freerouting-dev widths board.dsn [session.ses] [--json out.json]`: the
//! Q02.1 delivered-width report of a board, after importing a session when
//! one is given (so it grades Java's own sessions too). The session replaces
//! the board's wiring that it repeats
//! ([`crate::ses_diff::import_routed_session`]), so the board's own unfixed
//! and user-fixed traces are reported once, not twice.

use std::process::ExitCode;

use freerouting::quality::neckdown::width_report::{WidthReport, report};

use crate::ses_diff::parse_session;
use crate::{load_board, read_text};

const USAGE: &str = "usage: freerouting-dev widths board.dsn [session.ses] [--json out.json]";

/// The width report of `dsn` with `ses` imported (if given).
pub fn widths_of(
    dsn: &std::path::Path,
    ses: Option<&std::path::Path>,
) -> Result<WidthReport, String> {
    let mut board = load_board(dsn)?;
    if let Some(ses) = ses {
        let session = parse_session(&read_text(ses)?)?;
        crate::ses_diff::import_routed_session(&mut board, &session);
    }
    Ok(report(&board))
}

/// Runs the subcommand: prints the report as JSON, or writes it to `--json`.
pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut positional = Vec::new();
    let mut json = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--json" {
            json = Some(
                args.get(i + 1)
                    .ok_or_else(|| format!("--json needs a value\n{USAGE}"))?
                    .clone(),
            );
            i += 2;
            continue;
        }
        positional.push(args[i].clone());
        i += 1;
    }
    let (dsn, ses) = match positional.as_slice() {
        [dsn] => (dsn, None),
        [dsn, ses] => (dsn, Some(ses)),
        _ => return Err(USAGE.to_string()),
    };
    let report = widths_of(std::path::Path::new(dsn), ses.map(std::path::Path::new))?;
    let text = report.to_json().to_pretty_string();
    match json {
        Some(path) => std::fs::write(&path, text).map_err(|e| format!("{path}: {e}"))?,
        None => println!("{text}"),
    }
    Ok(ExitCode::SUCCESS)
}

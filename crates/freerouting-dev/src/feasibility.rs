//! `freerouting-dev feasibility <board.dsn> [--json out.json] [--all-pins]
//! [--step-um N]`: the static infeasible-connection check (Q03), see
//! `freerouting::quality::feasibility`.
//!
//! Prints one line per infeasible pin, the candidate count and the time,
//! and writes the JSON report. The exit status is 0 whatever the verdicts
//! and 2 on a load error.

use std::path::Path;
use std::time::Instant;

use freerouting::quality::feasibility::report::sha256_hex;
use freerouting::quality::feasibility::{FeasibilityOptions, check_board};

/// Runs the subcommand on parsed options.
pub fn run(
    board_path: &Path,
    json: Option<&Path>,
    options: &FeasibilityOptions,
) -> Result<(), String> {
    let bytes = std::fs::read(board_path).map_err(|e| format!("{}: {e}", board_path.display()))?;
    let board = freerouting::drc::load_design(&bytes)
        .map_err(|e| format!("{}: {e}", board_path.display()))?;
    let started = Instant::now();
    let report = check_board(&board, options);
    let seconds = started.elapsed().as_secs_f64();
    print!("{}", report.to_text());
    println!(
        "feasibility: {} candidates checked in {seconds:.2} s (step {} board units)",
        report.candidates, report.step
    );
    if let Some(path) = json {
        let text = report.to_json(&sha256_hex(&bytes)).to_pretty_string() + "\n";
        std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))?;
    }
    Ok(())
}

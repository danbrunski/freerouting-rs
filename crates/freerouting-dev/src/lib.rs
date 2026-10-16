//! Developer tools for the freerouting-rs port, kept out of the
//! user-facing `freerouting` command line so that stays Java-compatible.
//!
//! The `freerouting-dev` binary has these subcommands:
//!
//! - `ses-diff board.dsn a.ses b.ses [--json out.json]`: compares two
//!   sessions for one board, structurally net by net and by the statistics
//!   of the board after importing each ([`ses_diff`]). Exit status 0 when
//!   equivalent, 1 when they differ, 2 on an error.
//! - `bench --set <file> --java <jar> [--rust <bin>] --runs N --out <dir>`:
//!   routes a board set with each engine on a quiet machine and writes
//!   `bench.json` and `bench.md` ([`bench`], [`bench_report`]). Exit status 0
//!   when it ran, 3 when it refused because the machine was busy, 2 on an
//!   error.
//!
//! Wires are compared as multisets of (layer, half width, point list) and
//! vias as multisets of (padstack, position) per net, in board coordinates
//! converted exactly as `read_session` converts them. The order of nets and
//! items, the number format (`1000` or `1000.0`) and the direction a path is
//! written in don't count. Placement and pin swaps are not compared.
//!
//! The statistics are `BoardStatistics` fields after the import, named as in
//! Java's JSON: incomplete connections, clearance violations (the full
//! clearance check), vias, total trace length in mm and bends.

pub mod bench;
pub mod bench_report;
pub mod json_read;
pub mod ses_diff;
pub mod session_geometry;

use std::path::Path;

use freerouting::board::RoutingBoard;
use freerouting::drc::load_design;

/// Reads a DSN file into a board, with the post-load processing of the DRC
/// and the router (`load_design`).
pub fn load_board(path: &Path) -> Result<RoutingBoard, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    load_design(&bytes).map_err(|e| format!("{}: {e}", path.display()))
}

/// Reads a session file as text (Latin-1 if it is not UTF-8, like Java).
pub fn read_text(path: &Path) -> Result<String, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Ok(freerouting::dsn::decode(&bytes).into_owned())
}

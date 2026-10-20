//! Developer tools for the freerouting-rs port, kept out of the
//! user-facing `freerouting-rs` command line so that stays Java-compatible.
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
//! - `gate --records <run.json> --baseline <base.json> [--prereg <reg.json>
//!   [--arms A,B] [--rows ..|--eligible] [--all-pairs]]`
//!   and `gate collect`: the measurement gate ([`gate`], [`gate_prereg`],
//!   [`gate_collect`], [`gate_cli`], F03.1 and F03.4). It compares per-board,
//!   per-arm records with a baseline file, names every problem (INVERTED,
//!   DRIFT, ORPHAN, MISSING, NEW ROW, MALFORMED, UNGRADED, HARD, REFUSED),
//!   applies the pass rule (at least 3 distinct boards, improve on N-1,
//!   regress on none; or with `--rule totals` the total against the set's
//!   noise, [`gate_totals`], N02) and checks a run against its preregistration
//!   (`tools/ab/`). A self-test runs on every invocation. Exit status 0 when
//!   it passes, 1 when it fails, 2 on a usage or file error.
//! - `failed-vs-human board.dsn engine.ses human.dsn [--json out.json]`: every
//!   connection an engine leaves unrouted next to the human-routed original's
//!   layers, vias, widths and clearance headroom, tagged `router gap`,
//!   `no human route` or `infeasible` (Q03) ([`failed_vs_human`], F03.2).
//! - `power-traces board.dsn [session.ses]`: the traces on power layers of a
//!   net without a plane there (Q01.7) or of a plane's net outside its plane
//!   (Q13.2), one line each; exit status 1 when there is one
//!   ([`power_traces`]).
//!
//! - `microbench board.dsn [--runs N] [--kernel a,b] [--json out.json]`: the
//!   hot kernels (overlap queries, room completion, one maze search,
//!   a pull-tight, the DRC clearance pass) with fixed work: the minimum time
//!   over the runs and the work counts, which every run must repeat
//!   ([`microbench`], P01.3).
//!
//! - `tidy board.dsn routed.ses|routed.kicad_pcb [--json out.json] [--connections]`:
//!   the median and 90th percentile of five tidiness measures per routed
//!   connection (detour, bends per 10 mm, acute corners, jogs, vias), from the
//!   copper alone ([`tidy`], Q15.1). Bench records and `gate collect` carry
//!   the same measures.
//!
//! The manifest of the out-of-repo corpus of open KiCad boards
//! (`tools/corpus/`, F03.3) is read by [`corpus`], which also refuses any
//! committed board whose licence is not allow-listed.
//!
//! Wires are compared as multisets of (layer, half width, point list) and
//! vias as multisets of (padstack, position) per net, in board coordinates
//! converted as the writer-scale reader converts them
//! ([`ses_diff::import_at_writer_scale`]). The order of nets and
//! items, the number format (`1000` or `1000.0`) and the direction a path is
//! written in don't count. Placement and pin swaps are not compared.
//!
//! The statistics are `BoardStatistics` fields after the import, named as in
//! Java's JSON: incomplete connections, clearance violations (the full
//! clearance check), vias, total trace length in mm and bends.
//!
//! Every measure that imports a session into its input board (`ses-diff`,
//! the bench's statistics, widths and tidiness, `widths`, `power-traces`,
//! `tidy`, `failed-vs-human`) imports it in place of the wiring the session
//! repeats ([`ses_diff::import_routed_session`]): a session holds every trace
//! and via of the routed board but the system-fixed ones, the input's own
//! unfixed and user-fixed wiring included, so an import on top of the input
//! counted that wiring twice. They all read the session at the scale the
//! writer used (`router.java_quirks.ses_reader_scale` off, whatever the route
//! used), so the copper lands where the router put it (Issue676's session read
//! with Java's scale is 100 times off). `gate collect`'s `via_in_pad` counts only the
//! vias the input does not have ([`gate_collect::added_via_in_pad`]).

pub mod bench;
pub mod bench_report;
pub mod corpus;
pub mod failed_vs_human;
pub mod gate;
pub mod gate_baseline;
pub mod gate_cli;
pub mod gate_collect;
pub mod gate_prereg;
pub mod gate_selftest;
pub mod gate_totals;
pub mod json_read;
pub mod microbench;
pub mod power_traces;
pub mod ses_diff;
pub mod session_geometry;
pub mod tidy;
pub mod tidy_copper;
pub mod tidy_geometry;
pub mod tidy_input;
pub mod widths;

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

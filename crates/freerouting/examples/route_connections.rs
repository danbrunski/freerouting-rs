//! Writes the Rust side of probe `insert` (A06.7, A06.8) for one board:
//!
//! ```text
//! cargo run --release -p freerouting --example route_connections -- <board.dsn> <out.jsonl>
//! ```
//!
//! Every sampled connection is routed with `route_connection` on a fresh load of the board, as
//! `tools/java-reference/probes/autoroute/AutorouteProbe.java` kind `insert` does; `<out.jsonl>`
//! gets the lines, header first, in the format of `fixtures/reference/probe/insert/`.
//! `tools/determinism-check.sh` runs it idle and under load to show the result does not depend
//! on the machine's load (D2).

#[path = "../tests/autoroute_common/mod.rs"]
mod autoroute_common;
#[path = "../tests/support/insert_lines.rs"]
mod insert_lines;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dsn, out] = args.as_slice() else {
        eprintln!("usage: route_connections <board.dsn> <out.jsonl>");
        return ExitCode::from(2);
    };
    // The loader takes a path below the repository root, as the fixture rows name them.
    let root = std::fs::canonicalize(autoroute_common::repo_root()).unwrap_or_default();
    let absolute = std::fs::canonicalize(dsn).unwrap_or_else(|_| Path::new(dsn).to_path_buf());
    let relative = absolute
        .strip_prefix(&root)
        .map_or_else(|_| dsn.clone(), |p| p.display().to_string());
    let lines = insert_lines::insert_lines(&relative);
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    if let Err(e) = std::fs::write(out, text) {
        eprintln!("route_connections: {out}: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

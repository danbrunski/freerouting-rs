//! Writes the Rust side of probe `pass` (A11.7, A11.8) for one board:
//!
//! ```text
//! cargo run --release -p freerouting --example route_pass -- <board.dsn> <out.jsonl>
//! ```
//!
//! Passes 1 and 2 of the job's pass runner with the Freerouting stages on the loaded board (no
//! fanout), as `tools/java-reference/probes/autoroute/AutorouteProbe.java` kind `pass` runs
//! them; `<out.jsonl>` gets the lines, header first, in the format of
//! `fixtures/reference/probe/pass/`. `tools/determinism-check.sh` runs it idle and under load to
//! show the result does not depend on the machine's load (D2).

#[path = "../tests/autoroute_common/mod.rs"]
mod autoroute_common;
#[path = "../tests/support/insert_lines.rs"]
mod insert_lines;
#[path = "../tests/support/pass_lines.rs"]
mod pass_lines;

use std::path::Path;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dsn, out] = args.as_slice() else {
        eprintln!("usage: route_pass <board.dsn> <out.jsonl>");
        return ExitCode::from(2);
    };
    // The loader takes a path below the repository root, as the fixture rows name them.
    let root = std::fs::canonicalize(autoroute_common::repo_root()).unwrap_or_default();
    let absolute = std::fs::canonicalize(dsn).unwrap_or_else(|_| Path::new(dsn).to_path_buf());
    let relative = absolute
        .strip_prefix(&root)
        .map_or_else(|_| dsn.clone(), |p| p.display().to_string());
    let lines = pass_lines::pass_lines(&relative);
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    if let Err(e) = std::fs::write(out, text) {
        eprintln!("route_pass: {out}: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

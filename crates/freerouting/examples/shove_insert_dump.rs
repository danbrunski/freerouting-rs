//! Writes the Rust side of probe `shove-insert` (O04.7, O04.8) for one board:
//!
//! ```text
//! cargo run --release -p freerouting --example shove_insert_dump -- <dsn> <ses|-> <out>
//! ```
//!
//! The board is loaded as the Java probe loads it, the session (unless `-`) imported, and every
//! stage of `tools/java-reference/probes/board/ShoveInsertProbe.java` run (forced trace segments,
//! forced vias, drill item moves, the attach check); `<out>` gets the JSONL lines, header first,
//! in the format of `fixtures/reference/probe/shove-insert/`, ending with the final item dump.
//! `tools/determinism-check.sh` runs it twice to show the result does not depend on load.

#[path = "../tests/board_probe/mod.rs"]
mod board_probe;
#[path = "../tests/support/item_dump.rs"]
mod item_dump;
#[path = "../tests/support/shove_insert_stages.rs"]
mod shove_insert_stages;
#[path = "../tests/support/shove_sampler.rs"]
mod shove_sampler;

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [dsn, ses, out] = args.as_slice() else {
        eprintln!("usage: shove_insert_dump <dsn> <ses|-> <out>");
        return ExitCode::from(2);
    };
    let board_name = dsn
        .replace('\\', "/")
        .rsplit_once("fixtures/")
        .map_or(dsn.as_str(), |(_, rest)| rest)
        .trim_end_matches(".dsn")
        .to_string();
    let row = board_probe::Row {
        dsn: std::fs::canonicalize(dsn).map_or_else(|_| dsn.clone(), |p| p.display().to_string()),
        ses: (ses != "-").then(|| {
            std::fs::canonicalize(ses).map_or_else(|_| ses.clone(), |p| p.display().to_string())
        }),
        board: board_name,
    };
    let mut board = board_probe::load(&row);
    board_probe::import_session(&mut board, &row);
    let mut lines = vec![board_probe::header("shove-insert", &row.board)];
    let steps = shove_insert_stages::insert_steps(&row.board);
    shove_insert_stages::shove_insert_lines(&mut board, steps, &mut lines);
    let mut text = lines.join("\n");
    text.push('\n');
    if let Err(e) = std::fs::write(out, text) {
        eprintln!("shove_insert_dump: {out}: {e}");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

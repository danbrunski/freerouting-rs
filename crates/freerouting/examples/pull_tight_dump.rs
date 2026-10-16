//! Pull-tightens a whole routed board and writes the canonical item dump
//! (O01.8), as probe `pull-tight`'s `board-<angle>` section does: loads the
//! design, imports the session, unfixes every trace and via (the import makes
//! them user-fixed), marks the whole board and runs `opt_changed_area` with
//! accuracy 500, no clip and no limit.
//!
//! ```sh
//! cargo run --release -p freerouting --example pull_tight_dump -- \
//!     board.dsn board.ses out.jsonl [--angle 45|90|none]
//! ```
//!
//! `--angle none` runs the any-angle tightener (O02), as probe
//! `pull-tight-any`'s `board-none` section does. The run time goes to stderr;
//! the output is deterministic (`tools/determinism-check.sh`).

#[path = "../tests/board_probe/mod.rs"]
mod board_probe;
#[path = "../tests/support/item_dump.rs"]
mod item_dump;

use std::process::ExitCode;
use std::time::Instant;

use freerouting::board::items::ItemId;
use freerouting::board::structure::AngleRestriction;
use freerouting::drc::load_design;
use freerouting::io::read_session;
use freerouting::optimize::PullTightParams;

fn run(args: &[String]) -> Result<(), String> {
    let (mut positional, mut angle) = (Vec::new(), AngleRestriction::FortyfiveDegree);
    let mut iter = args.iter();
    while let Some(arg) = iter.next() {
        if arg == "--angle" {
            angle = match iter.next().map(String::as_str) {
                Some("45") => AngleRestriction::FortyfiveDegree,
                Some("90") => AngleRestriction::NinetyDegree,
                Some("none") => AngleRestriction::None,
                other => return Err(format!("--angle takes 45, 90 or none, not {other:?}")),
            };
        } else {
            positional.push(arg.clone());
        }
    }
    let [dsn, ses, out] = positional.as_slice() else {
        return Err("usage: pull_tight_dump <dsn> <ses> <out> [--angle 45|90|none]".into());
    };
    let bytes = std::fs::read(dsn).map_err(|e| format!("{dsn}: {e}"))?;
    let mut board = load_design(&bytes).map_err(|e| format!("{dsn}: {e}"))?;
    let session = std::fs::read(ses).map_err(|e| format!("{ses}: {e}"))?;
    read_session(&mut board, &freerouting::dsn::decode(&session))
        .map_err(|e| format!("{ses}: {e}"))?;
    board.rules.set_trace_angle_restriction(angle);
    let ids: Vec<ItemId> = board
        .items()
        .filter(|i| i.is_trace() || i.as_via().is_some())
        .map(|i| i.id())
        .collect();
    for id in ids {
        board.unfix(id);
    }
    let start = Instant::now();
    board.start_marking_changed_area();
    board.mark_all_changed_area();
    board.opt_changed_area(&PullTightParams::new(500));
    eprintln!("pull_tight_dump: opt_changed_area {:?}", start.elapsed());
    let mut lines = vec![board_probe::header("pull-tight", dsn)];
    lines.extend(item_dump::dump(&board));
    let text: String = lines.iter().map(|l| format!("{l}\n")).collect();
    std::fs::write(out, text).map_err(|e| format!("{out}: {e}"))
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("pull_tight_dump: {e}");
            ExitCode::FAILURE
        }
    }
}

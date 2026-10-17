//! Times the board statistics the batch loop computes after every pass
//! (P03.1), and their parts, on the analysis threads at several budgets:
//!
//! ```sh
//! cargo run --release -p freerouting --example stats_bench -- \
//!     <board.dsn> [<routed.ses>] [--runs N] [--threads 1,2,4,8]
//! ```
//!
//! The board loads as the DRC command line loads it, then `<routed.ses>` is
//! imported (a routed board is what the later passes score). For each
//! thread budget ([`freerouting::parallel::ThreadBudget`]) every part runs
//! `--runs` times (default 7) and the median, minimum and maximum wall
//! times in milliseconds are printed:
//! - `statistics`: `BoardStatistics::new`, what a pass costs;
//! - `clearance`: its clearance check (`all_clearance_violations`);
//! - `base`: the statistics without the clearance check (the fanout counts
//!   and the bounds included);
//! - `bounds`: `calculate_bounds`;
//! - `airlines`: `calculate_all_incompletes`, the airlines of every net.
//!
//! The statistics of every budget must equal those of the first one; the
//! example exits 1 if they don't.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use freerouting::board::structure::Unit;
use freerouting::drc::{DesignRulesChecker, load_design};
use freerouting::io::read_session;
use freerouting::parallel::ThreadBudget;
use freerouting::scoring::{BoardStatistics, calculate_bounds};

struct Options {
    dsn: String,
    ses: Option<String>,
    runs: usize,
    threads: Vec<usize>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut options = Options {
        dsn: String::new(),
        ses: None,
        runs: 7,
        threads: vec![1, 2, 4, 8],
    };
    let mut positional = Vec::new();
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--runs" => {
                i += 1;
                options.runs = args
                    .get(i)
                    .and_then(|v| v.parse().ok())
                    .filter(|&n| n > 0)
                    .ok_or("bad --runs")?;
            }
            "--threads" => {
                i += 1;
                options.threads = args
                    .get(i)
                    .ok_or("bad --threads")?
                    .split(',')
                    .map(|t| t.parse().map_err(|_| format!("bad --threads value '{t}'")))
                    .collect::<Result<_, _>>()?;
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    match positional.as_slice() {
        [dsn] => options.dsn = dsn.clone(),
        [dsn, ses] => {
            options.dsn = dsn.clone();
            options.ses = Some(ses.clone());
        }
        _ => {
            return Err(
                "usage: stats_bench <board.dsn> [<routed.ses>] [--runs N] [--threads 1,2,4,8]"
                    .into(),
            );
        }
    }
    Ok(options)
}

/// The median, minimum and maximum of `runs` timed calls of `f`, in ms.
fn time(runs: usize, mut f: impl FnMut()) -> (f64, f64, f64) {
    let mut ms: Vec<f64> = (0..runs)
        .map(|_| {
            let start = Instant::now();
            f();
            start.elapsed().as_secs_f64() * 1000.0
        })
        .collect();
    ms.sort_by(f64::total_cmp);
    (ms[ms.len() / 2], ms[0], ms[ms.len() - 1])
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("stats_bench: {e}");
            return ExitCode::from(2);
        }
    };
    let read = |path: &str| std::fs::read(path).map_err(|e| format!("{path}: {e}"));
    let board = read(&options.dsn).and_then(|bytes| load_design(&bytes));
    let mut board = match board {
        Ok(board) => board,
        Err(e) => {
            eprintln!("stats_bench: {e}");
            return ExitCode::from(1);
        }
    };
    if let Some(ses) = &options.ses {
        let imported = read(ses).and_then(|bytes| {
            read_session(&mut board, &String::from_utf8_lossy(&bytes)).map_err(|e| e.to_string())
        });
        if let Err(e) = imported {
            eprintln!("stats_bench: {e}");
            return ExitCode::from(1);
        }
    }
    let name = Path::new(&options.dsn)
        .file_stem()
        .map_or_else(String::new, |s| s.to_string_lossy().into_owned());
    println!(
        "{name}: {} items, {} runs; ms median (min-max)",
        board.items().count(),
        options.runs
    );
    let mut reference: Option<String> = None;
    let mut equal = true;
    for &threads in &options.threads {
        let _budget = ThreadBudget::set(threads);
        let stats = format!("{:?}", BoardStatistics::new(&board));
        match &reference {
            None => reference = Some(stats),
            Some(r) if *r != stats => {
                eprintln!("stats_bench: the statistics at {threads} threads differ");
                equal = false;
            }
            Some(_) => {}
        }
        let runs = options.runs;
        let parts = [
            (
                "statistics",
                time(runs, || drop(BoardStatistics::new(&board))),
            ),
            (
                "clearance",
                time(runs, || drop(board.all_clearance_violations())),
            ),
            (
                "base",
                time(runs, || {
                    drop(BoardStatistics::with_options(
                        &board,
                        Unit::Mm,
                        false,
                        false,
                    ))
                }),
            ),
            (
                "bounds",
                time(runs, || {
                    calculate_bounds(&board);
                }),
            ),
            (
                "airlines",
                time(runs, || {
                    DesignRulesChecker::new(&board).calculate_all_incompletes();
                }),
            ),
        ];
        let row: Vec<String> = parts
            .iter()
            .map(|(part, (median, min, max))| format!("{part} {median:.2} ({min:.2}-{max:.2})"))
            .collect();
        println!("  threads {threads}: {}", row.join(", "));
    }
    if equal {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

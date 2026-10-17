//! Runs the batch optimizer (O06) on a routed board and writes the SES:
//!
//! ```text
//! cargo run --release -p freerouting --example optimize_board -- \
//!     <board.dsn> <routed.ses> <out.ses> [--threads N] [--max-passes P] \
//!     [--max-items I] [--chunk C] [--prioritized] [--keep-fixed] [--no-guards]
//!     [--log <file>]
//! ```
//!
//! The board loads as the job does (`prepare_job`: the defaults, the DSN's settings and an
//! adjacent rules file), then `<routed.ses>` is imported and every wire and via is unfixed
//! (Java's `SesReader` imports them user fixed, and a user-fixed item is never an optimizer
//! candidate; `--keep-fixed` keeps them fixed, as Java's own tests do; `--no-guards` turns the
//! preflight guards off). The optimizer runs with the given
//! bounds at the testing priority (default: 3 passes, 200 items, SEQUENTIAL, the settings'
//! thread count), with no timeout, and the result is written with C05's SES writer. `--log`
//! writes the per-candidate result log (one line per consumed candidate and one per pass).
//! The thread count and the chunk size never change the output (O06.8); the summary goes to
//! stderr, and `tools/determinism-check.sh` runs this idle and under load.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use freerouting::autoroute::events::{CancelToken, NullEvents};
use freerouting::autoroute::stages::StageCtx;
use freerouting::io::read_session;
use freerouting::io::specctra::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::optimize::batch::{OptimizerOptions, OptimizerRun, run_batch_optimizer};
use freerouting::scoring::BoardStatistics;
use freerouting::settings::{ItemSelectionStrategy, ResolveContext, SettingsPatch, Source};

struct Options {
    dsn: PathBuf,
    ses: PathBuf,
    out: PathBuf,
    threads: Option<usize>,
    max_passes: i32,
    max_items: i32,
    chunk: Option<usize>,
    prioritized: bool,
    log: Option<PathBuf>,
    keep_fixed: bool,
    no_guards: bool,
}

fn number<T: std::str::FromStr>(args: &[String], i: usize, flag: &str) -> Result<T, String> {
    args.get(i)
        .and_then(|s| s.parse().ok())
        .ok_or_else(|| format!("bad {flag}"))
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut positional = Vec::new();
    let mut o = Options {
        dsn: PathBuf::new(),
        ses: PathBuf::new(),
        out: PathBuf::new(),
        threads: None,
        max_passes: 3,
        max_items: 200,
        chunk: None,
        prioritized: false,
        log: None,
        keep_fixed: false,
        no_guards: false,
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--threads" => {
                i += 1;
                o.threads = Some(number(args, i, "--threads")?);
            }
            "--max-passes" => {
                i += 1;
                o.max_passes = number(args, i, "--max-passes")?;
            }
            "--max-items" => {
                i += 1;
                o.max_items = number(args, i, "--max-items")?;
            }
            "--chunk" => {
                i += 1;
                o.chunk = Some(number(args, i, "--chunk")?);
            }
            "--prioritized" => o.prioritized = true,
            "--keep-fixed" => o.keep_fixed = true,
            "--no-guards" => o.no_guards = true,
            "--log" => {
                i += 1;
                o.log = Some(PathBuf::from(args.get(i).ok_or("bad --log")?));
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    let [dsn, ses, out] = positional.as_slice() else {
        return Err("expected <board.dsn> <routed.ses> <out.ses>".to_string());
    };
    o.dsn = PathBuf::from(dsn);
    o.ses = PathBuf::from(ses);
    o.out = PathBuf::from(out);
    Ok(o)
}

/// The per-candidate log: one line per consumed result, one per pass.
fn result_log(run: &OptimizerRun) -> String {
    let mut text = String::new();
    for pass in &run.pass_log {
        for r in &pass.results {
            let _ = writeln!(
                text,
                "pass {} item {} improved {} vias {}->{} length {:016x}->{:016x} incomplete \
                 {}->{} pct {:08x}",
                pass.pass,
                r.item_id().0,
                r.improved(),
                r.via_count() + r.via_count_reduced(),
                r.via_count(),
                (r.length_reduced() + r.trace_length()).to_bits(),
                r.trace_length().to_bits(),
                r.incomplete_count_before(),
                r.incomplete_count(),
                r.improvement_percentage().to_bits()
            );
        }
        let _ = writeln!(
            text,
            "pass {} candidates {} winner {:?} score {:08x}->{:08x} rejection {:?} early {} \
             hash {:032x}",
            pass.pass,
            pass.candidates.len(),
            pass.winner.map(|w| w.item_id().0),
            pass.score_before.to_bits(),
            pass.score_after.to_bits(),
            pass.rejection,
            pass.early_stop,
            pass.state_hash
        );
    }
    text
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("optimize_board: {e}");
            eprintln!(
                "usage: optimize_board <board.dsn> <routed.ses> <out.ses> [--threads N] \
                 [--max-passes P] [--max-items I] [--chunk C] [--prioritized] [--keep-fixed] \
                 [--no-guards] [--log <file>]"
            );
            return ExitCode::from(2);
        }
    };
    let read = |path: &Path| std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()));
    let (bytes, session) = match (read(&options.dsn), read(&options.ses)) {
        (Ok(b), Ok(s)) => (b, String::from_utf8_lossy(&s).into_owned()),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("optimize_board: {e}");
            return ExitCode::from(1);
        }
    };
    let file_name = options
        .dsn
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("board");
    let design = JobInput::design_name_of(file_name);
    let mut input = JobInput::new(bytes, design.clone());
    input.rules = RulesChoice::Adjacent;
    input.dsn_dir = options.dsn.parent().map(Path::to_path_buf);
    let mut patch = SettingsPatch::default();
    patch.optimizer.max_passes = Some(options.max_passes);
    patch.optimizer.max_items = Some(options.max_items);
    if options.no_guards {
        patch.optimizer.enable_preflight_guards = Some(false);
    }
    if options.prioritized {
        patch.optimizer.item_selection_strategy = Some(ItemSelectionStrategy::Prioritized);
    }
    input.extra_sources.push((Source::Testing, patch));
    let mut prepared = match prepare_job(&input, &ResolveContext { cores: 8 }) {
        Ok(prepared) => prepared,
        Err(e) => {
            eprintln!("optimize_board: {}: {e}", options.dsn.display());
            return ExitCode::from(1);
        }
    };
    if let Err(e) = read_session(&mut prepared.board, &session) {
        eprintln!("optimize_board: {}: {e}", options.ses.display());
        return ExitCode::from(1);
    }
    if !options.keep_fixed {
        // The session's wires and vias come in user fixed (Java's `SesReader`), which no
        // optimizer candidate may be: unfix every wire and via, as the Java probes do.
        for id in prepared.board.item_ids() {
            let routable = prepared
                .board
                .item(id)
                .is_some_and(|i| i.is_trace() || i.is_via());
            if routable {
                prepared.board.unfix(id);
            }
        }
    }
    let cancel = CancelToken::new();
    let ctx = StageCtx {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: &cancel,
        events: &NullEvents,
    };
    let run_options = OptimizerOptions {
        threads: options.threads,
        chunk_size: options.chunk,
        ..OptimizerOptions::default()
    };
    let mut board = prepared.board;
    let candidates = freerouting::optimize::batch::candidates::sorted_candidates(&board);
    eprintln!(
        "optimize_board: {} traces ({} shove fixed, {} with an ignored net), {} vias, {} \
         candidates",
        board.traces().len(),
        board
            .items()
            .filter(|i| i.is_trace() && board.is_shove_fixed(i))
            .count(),
        board
            .items()
            .filter(|i| i.is_trace() && board.has_ignored_nets(i))
            .count(),
        board.vias().len(),
        candidates.len()
    );
    let before = BoardStatistics::new(&board);
    let started = std::time::Instant::now();
    let run = run_batch_optimizer(&mut board, &ctx, &run_options);
    let elapsed = started.elapsed();
    let after = BoardStatistics::new(&board);
    if let Err(e) = std::fs::write(&options.out, write_ses(&board, &design, SesStyle::Java)) {
        eprintln!("optimize_board: {}: {e}", options.out.display());
        return ExitCode::from(1);
    }
    if let Some(path) = &options.log
        && let Err(e) = std::fs::write(path, result_log(&run))
    {
        eprintln!("optimize_board: {}: {e}", path.display());
        return ExitCode::from(1);
    }
    eprintln!(
        "optimize_board: {design}: threads {:?}, bypass {:?}, passes {}, items {}, \
         vias {} -> {}, length {:.2} -> {:.2} mm, incomplete {} -> {}, violations {} -> {}, \
         ranking score {:.2} -> {:.2}, {:.2} s",
        options.threads,
        run.bypass.map(|b| b.to_string()),
        run.passes,
        run.items,
        before.vias.total_count,
        after.vias.total_count,
        before.traces.total_length_mm,
        after.traces.total_length_mm,
        before.connections.incomplete_count,
        after.connections.incomplete_count,
        before.clearance_violations.total_count,
        after.clearance_violations.total_count,
        run.score_before,
        run.score_after,
        elapsed.as_secs_f64()
    );
    for pass in &run.pass_log {
        eprintln!(
            "  pass #{}: {} candidates, {} consumed, winner {:?}, score {:.2} -> {:.2}, {:?}",
            pass.pass,
            pass.candidates.len(),
            pass.results.len(),
            pass.winner.map(|w| w.item_id().0),
            pass.score_before,
            pass.score_after,
            pass.rejection
        );
    }
    ExitCode::SUCCESS
}

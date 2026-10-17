//! Routes one board with `run_pipeline` (A13) and writes its SES:
//!
//! ```text
//! cargo run --release -p freerouting --example route_board -- \
//!     <board.dsn> <out.ses> [--sink noop|counters] [--max-threads N] [--no-bounds] \
//!     [-- <Java CLI router flags>...]
//! ```
//!
//! The board loads as the job does (`prepare_job`: the defaults, the DSN's settings and an
//! adjacent rules file). A board with a row in `tools/java-reference/route-bounds.tsv` is
//! routed with that row's bounds as J01.3's `route` mode does (the optimizer off, its
//! `max_passes` and `max_items` at the testing priority), unless `--no-bounds` is given. Flags
//! after `--` are read as Java's CLI router flags (`-mp 1`,
//! `--router.scoring.start_ripup_costs=1500`, ...) at the CLI priority.
//!
//! `--sink counters` routes with a listening event sink that counts the events (so the
//! board's change journal is on and deltas, snapshots and progress are built); `noop`, the
//! default, routes with no listener. The two must write the same SES (A13.6), and so must
//! `--max-threads 1` and `--max-threads 8` (`autorouter.max_threads`, which Java's batch router
//! never reads). The summary goes to stderr; `tools/determinism-check.sh` runs this idle and
//! under load.

#[path = "../tests/autoroute_common/mod.rs"]
mod autoroute_common;

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use freerouting::autoroute::events::{
    CancelToken, ItemGeometry, NullEvents, Progress, RouterEvents, RunSummary,
};
use freerouting::autoroute::pipeline::{PipelineJob, run_pipeline};
use freerouting::board::items::ItemId;
use freerouting::board::{BoardSnapshot, RoutingBoard};
use freerouting::io::specctra::{SesStyle, write_ses};
use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::settings::{ResolveContext, SettingsPatch, Source, parse_cli_arguments};

/// A listening sink that counts what it gets.
#[derive(Default)]
struct Counters {
    progress: Cell<usize>,
    deltas: Cell<usize>,
    replaced: Cell<usize>,
}

impl RouterEvents for Counters {
    fn is_listening(&self) -> bool {
        true
    }
    fn progress(&self, _: Progress) -> bool {
        self.progress.set(self.progress.get() + 1);
        true
    }
    fn delta(&self, _: Vec<ItemId>, _: Vec<ItemGeometry>) {
        self.deltas.set(self.deltas.get() + 1);
    }
    fn replaced(&self, _: BoardSnapshot) {
        self.replaced.set(self.replaced.get() + 1);
    }
    fn finished(&self, board: RoutingBoard, _: RunSummary) -> Option<RoutingBoard> {
        Some(board)
    }
}

struct Options {
    dsn: PathBuf,
    out: PathBuf,
    counters: bool,
    max_threads: Option<i32>,
    bounds: bool,
    cli: Vec<String>,
}

fn parse(args: &[String]) -> Result<Options, String> {
    let mut positional = Vec::new();
    let mut options = Options {
        dsn: PathBuf::new(),
        out: PathBuf::new(),
        counters: false,
        max_threads: None,
        bounds: true,
        cli: Vec::new(),
    };
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--sink" => {
                i += 1;
                options.counters = match args.get(i).map(String::as_str) {
                    Some("noop") => false,
                    Some("counters") => true,
                    other => return Err(format!("bad --sink {other:?}")),
                };
            }
            "--max-threads" => {
                i += 1;
                let n = args.get(i).and_then(|s| s.parse().ok());
                options.max_threads = Some(n.ok_or("bad --max-threads")?);
            }
            "--no-bounds" => options.bounds = false,
            "--" => {
                options.cli = args[i + 1..].to_vec();
                break;
            }
            other => positional.push(other.to_string()),
        }
        i += 1;
    }
    let [dsn, out] = positional.as_slice() else {
        return Err("expected <board.dsn> <out.ses>".to_string());
    };
    options.dsn = PathBuf::from(dsn);
    options.out = PathBuf::from(out);
    Ok(options)
}

/// The `route-bounds.tsv` row of `dsn`: `(max_passes, max_items)`.
fn bounds_of(dsn: &Path) -> Option<(i32, Option<i32>)> {
    let root = std::fs::canonicalize(autoroute_common::repo_root()).ok()?;
    let absolute = std::fs::canonicalize(dsn).ok()?;
    let relative = absolute.strip_prefix(root.join("fixtures")).ok()?;
    let key = relative.to_str()?.trim_end_matches(".dsn").to_string();
    let stem = dsn.file_stem()?.to_str()?.to_string();
    let text = std::fs::read_to_string(root.join("tools/java-reference/route-bounds.tsv")).ok()?;
    text.lines()
        .filter(|l| !l.starts_with('#') && !l.starts_with("board\t"))
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|cols| cols[0] == key)
        .or_else(|| {
            text.lines()
                .map(|l| l.split('\t').collect::<Vec<_>>())
                .find(|cols| cols[0] == stem)
        })
        .and_then(|cols| {
            let passes = cols.get(1)?.trim().parse().ok()?;
            let items = cols.get(2).and_then(|c| c.trim().parse().ok());
            Some((passes, items))
        })
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let options = match parse(&args) {
        Ok(options) => options,
        Err(e) => {
            eprintln!("route_board: {e}");
            eprintln!(
                "usage: route_board <board.dsn> <out.ses> [--sink noop|counters] \
                 [--max-threads N] [--no-bounds] [-- <Java CLI router flags>]"
            );
            return ExitCode::from(2);
        }
    };
    let bytes = match std::fs::read(&options.dsn) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("route_board: {}: {e}", options.dsn.display());
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
    if !options.cli.is_empty() {
        let cli = parse_cli_arguments(&options.cli);
        for warning in &cli.warnings {
            eprintln!("route_board: {warning}");
        }
        input.extra_sources.push((Source::Cli, cli.patch));
    }
    let bounds = if options.bounds {
        bounds_of(&options.dsn)
    } else {
        None
    };
    let mut patch = SettingsPatch::default();
    if let Some((max_passes, max_items)) = bounds {
        patch.optimizer.enabled = Some(false);
        patch.autorouter.max_passes = Some(max_passes);
        patch.autorouter.max_items = max_items;
    }
    patch.autorouter.max_threads = options.max_threads;
    input.extra_sources.push((Source::Testing, patch));
    let prepared = match prepare_job(&input, &ResolveContext { cores: 8 }) {
        Ok(prepared) => prepared,
        Err(e) => {
            eprintln!("route_board: {}: {e}", options.dsn.display());
            return ExitCode::from(1);
        }
    };
    let counters = Counters::default();
    let events: &dyn RouterEvents = if options.counters {
        &counters
    } else {
        &NullEvents
    };
    let job = PipelineJob {
        settings: &prepared.settings,
        board_settings: &prepared.board_settings,
        cancel: Arc::new(CancelToken::new()),
        events,
        deadline: None,
        registry: None,
    };
    let started = std::time::Instant::now();
    let outcome = match run_pipeline(prepared.board, &job) {
        Ok(outcome) => outcome,
        Err(failure) => {
            eprintln!("route_board: {}: {}", options.dsn.display(), failure.error);
            return ExitCode::from(1);
        }
    };
    let elapsed = started.elapsed();
    let Some(board) = outcome.board else {
        eprintln!("route_board: no board came back");
        return ExitCode::from(1);
    };
    let ses = write_ses(&board, &design, SesStyle::Java);
    if let Err(e) = std::fs::write(&options.out, ses) {
        eprintln!("route_board: {}: {e}", options.out.display());
        return ExitCode::from(1);
    }
    let s = &outcome.summary;
    eprintln!(
        "route_board: {design}: bounds {bounds:?}, max_threads {}, passes {}, stop {:?}, \
         restores {}, initial unrouted {}, final {} unrouted, {} violations, score {:.2}, \
         {:.2} s",
        prepared.settings.autorouter.max_threads,
        s.passes,
        outcome.stop,
        outcome.restores,
        s.initial_unrouted,
        s.incomplete,
        s.violations,
        s.score,
        elapsed.as_secs_f64()
    );
    for record in &outcome.passes {
        eprintln!("  pass #{} {}", record.pass, record.score_text());
    }
    if options.counters {
        eprintln!(
            "  events: {} progress, {} deltas, {} replaced",
            counters.progress.get(),
            counters.deltas.get(),
            counters.replaced.get()
        );
    }
    // P02.5: the trace-shape memo the board and its restores shared.
    let memo = board.search_trees().shape_memo().counts();
    eprintln!(
        "  shape memo: {} hits, {} misses, {} evictions",
        memo.hits, memo.misses, memo.evictions
    );
    // P02.1: `-- --router.debug.audit_incremental=true` turns the audit on.
    if let Some(audit) = &outcome.audit {
        eprintln!("  {}", audit.summary_line());
        if let Some((point, difference)) = &audit.first {
            eprintln!("  first difference {point}: {difference}");
            return ExitCode::from(1);
        }
    }
    ExitCode::SUCCESS
}

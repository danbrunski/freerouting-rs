//! `freerouting-dev bench`: routes a set of boards with Java and Rust and
//! times them on a quiet machine.
//!
//! - Each board of the set routes with each engine `runs` times, bounded by
//!   its row in `tools/java-reference/route-bounds.tsv` (J01.3) when that
//!   file exists, otherwise by `--max-passes`.
//! - The optimizer is off (`--router.optimizer.enabled=false`) unless
//!   `--optimize` is given: Java's optimizer still depends on thread
//!   scheduling.
//! - Before every run the 1-minute load average must be at most `max_load`.
//!   The runner waits up to `load_wait` for it and otherwise refuses,
//!   without timing anything. The load before and after each run is
//!   recorded next to its wall time.
//! - Each result is measured with `ses-diff`'s statistics after importing
//!   its SES into the board, and the engine's `WORK_LIMIT_CUTOFF` log lines
//!   are counted.
//! - With `--rip-stats` (Q05.1) each Rust run also gets
//!   `--router.special.diagnostics=rip-stats --result-json <run>.result.json`
//!   and its record keeps the manifest's `rip_stats` report without the
//!   per-net list (the totals, the top five nets by torn length with their
//!   shares, the passes). The diagnostic changes no route but costs time, so
//!   time such runs apart from plain ones.
//!
//! - With `--jobs N` (P03.3) N boards route at once, each engine run
//!   limited to `max(1, cores / N)` threads
//!   (`--router.autorouter.max_threads` and `--router.optimizer.max_threads`,
//!   which change no route). The boards are handed out in set order and the
//!   results merged in set order, so the report lists them as `--jobs 1`
//!   does. Concurrent runs load the machine, so their wall times are not
//!   timings: `--jobs` above 1 skips the load check and is for correctness
//!   and parity sweeps; timing runs keep `--jobs 1` and the quiet-machine
//!   rule.
//!
//! - Q15.2: each result also gets its tidiness score ([`crate::tidy`]: the
//!   median and 90th percentile of detour, bends per 10 mm, acute corners,
//!   jogs and vias per connection) as the run's `tidiness` object.
//!
//! The report (`bench.json` and `bench.md`) is written by
//! [`crate::bench_report`].

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use crate::ses_diff::{SessionStatistics, parse_session};
use crate::{load_board, read_text};

/// One line of a board set: the path as written and where it resolves.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SetEntry {
    pub written: String,
    pub path: PathBuf,
}

/// Reads a board set: one path per line, `#` starts a comment, blank lines
/// are skipped. `~/` is the home directory; other relative paths are
/// relative to `root`.
pub fn read_set(text: &str, root: &Path) -> Vec<SetEntry> {
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                return None;
            }
            let path = match line.strip_prefix("~/") {
                Some(rest) => std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .unwrap_or_default()
                    .join(rest),
                None if Path::new(line).is_absolute() => PathBuf::from(line),
                None => root.join(line),
            };
            Some(SetEntry {
                written: line.to_string(),
                path,
            })
        })
        .collect()
}

/// The repository root for a set file: the nearest ancestor holding
/// `Cargo.lock`, else the current directory.
pub fn repository_root(set_file: &Path) -> PathBuf {
    let start = std::fs::canonicalize(set_file).unwrap_or_else(|_| set_file.to_path_buf());
    start
        .ancestors()
        .find(|dir| dir.join("Cargo.lock").is_file())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// The routing bounds of one board.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Bounds {
    pub max_passes: u32,
    pub max_items: Option<u32>,
}

/// Reads `route-bounds.tsv`: a header row naming the columns `board`,
/// `max_passes` and optionally `max_items`, then one row per board. `board`
/// is matched against a set entry's file stem, its path as written, or that
/// path without a leading `fixtures/` and the `.dsn` extension (the form
/// `tools/java-reference/route-bounds.tsv` uses).
pub fn read_bounds(text: &str) -> Result<Vec<(String, Bounds)>, String> {
    let mut lines = text
        .lines()
        .filter(|l| !l.trim().is_empty() && !l.starts_with('#'));
    let header: Vec<&str> = lines.next().unwrap_or("").split('\t').collect();
    let column = |name: &str| header.iter().position(|h| h.trim() == name);
    let (Some(board), Some(passes)) = (column("board"), column("max_passes")) else {
        return Err("route bounds need 'board' and 'max_passes' columns".into());
    };
    let items = column("max_items");
    lines
        .map(|line| {
            let cells: Vec<&str> = line.split('\t').map(str::trim).collect();
            let cell = |i: usize| cells.get(i).copied().unwrap_or("");
            let max_passes = cell(passes)
                .parse()
                .map_err(|_| format!("bad max_passes in route bounds line '{line}'"))?;
            let max_items = match items.map(cell) {
                None | Some("") | Some("-") => None,
                Some(v) => Some(
                    v.parse()
                        .map_err(|_| format!("bad max_items in route bounds line '{line}'"))?,
                ),
            };
            Ok((
                cell(board).to_string(),
                Bounds {
                    max_passes,
                    max_items,
                },
            ))
        })
        .collect()
}

/// The bounds of `entry`, or `default` if no row names it.
pub fn bounds_for(table: &[(String, Bounds)], entry: &SetEntry, default: Bounds) -> Bounds {
    let stem = entry
        .path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let under_fixtures = entry
        .written
        .strip_prefix("fixtures/")
        .map(|rest| rest.strip_suffix(".dsn").unwrap_or(rest));
    table
        .iter()
        .find(|(name, _)| {
            *name == stem || *name == entry.written || Some(name.as_str()) == under_fixtures
        })
        .map_or(default, |(_, b)| *b)
}

/// The 1-minute load average, if the platform has one.
pub fn load_average() -> Option<f64> {
    if let Ok(text) = std::fs::read_to_string("/proc/loadavg") {
        return text.split_whitespace().next()?.parse().ok();
    }
    let output = Command::new("sysctl")
        .args(["-n", "vm.loadavg"])
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .trim_matches(|c: char| c == '{' || c == '}' || c.is_whitespace())
        .split_whitespace()
        .next()?
        .parse()
        .ok()
}

/// Waits until `read_load` reports at most `max_load`, checking every
/// `poll`, for at most `wait`. Returns the load it saw last, or an error
/// that says why nothing may be timed.
pub fn wait_for_quiet(
    mut read_load: impl FnMut() -> Option<f64>,
    max_load: f64,
    wait: Duration,
    poll: Duration,
) -> Result<f64, String> {
    let start = Instant::now();
    let mut announced = false;
    loop {
        let Some(load) = read_load() else {
            return Err(
                "can't read the 1-minute load average on this platform; refusing to time \
                 (pass --max-load off to time anyway)"
                    .into(),
            );
        };
        if load <= max_load {
            return Ok(load);
        }
        if start.elapsed() >= wait {
            return Err(format!(
                "refusing to time: the 1-minute load average is {load:.2}, above the limit \
                 {max_load:.2}, after waiting {} s. Stop builds, tests and other jobs, or \
                 raise --max-load.",
                wait.as_secs()
            ));
        }
        if !announced {
            eprintln!(
                "bench: load average {load:.2} is above {max_load:.2}; waiting up to {} s",
                wait.as_secs()
            );
            announced = true;
        }
        std::thread::sleep(
            poll.min(wait.saturating_sub(start.elapsed()))
                .max(Duration::from_millis(10)),
        );
    }
}

/// An engine that routes a DSN into a SES with Java's command line.
#[derive(Clone, Debug)]
pub struct Engine {
    /// `java` or `rust`.
    pub name: &'static str,
    /// The program and its leading arguments (`java -jar x.jar`).
    pub command: Vec<String>,
}

impl Engine {
    /// The full command line for one run.
    pub fn arguments(&self, dsn: &Path, ses: &Path, bounds: Bounds, optimize: bool) -> Vec<String> {
        let mut args = self.command.clone();
        args.extend([
            "-de".to_string(),
            dsn.display().to_string(),
            "-do".to_string(),
            ses.display().to_string(),
            "--gui.enabled=false".to_string(),
            format!("--router.autorouter.max_passes={}", bounds.max_passes),
        ]);
        if let Some(items) = bounds.max_items {
            args.push(format!("--router.autorouter.max_items={items}"));
        }
        if !optimize {
            args.push("--router.optimizer.enabled=false".to_string());
        }
        args
    }
}

/// The arguments that limit one engine run to `threads` threads when
/// boards route in parallel (`bench --jobs`): the autorouter's analyses and
/// the optimizer's candidates. Neither changes a route.
pub fn thread_arguments(threads: usize) -> Vec<String> {
    vec![
        format!("--router.autorouter.max_threads={threads}"),
        format!("--router.optimizer.max_threads={threads}"),
    ]
}

/// The threads each of `jobs` concurrent boards gets on `cores` cores
/// (at least 1).
pub fn thread_share(cores: usize, jobs: usize) -> usize {
    (cores / jobs.max(1)).max(1)
}

/// What one run produced.
#[derive(Clone, Debug, PartialEq)]
pub struct RunRecord {
    pub run: usize,
    pub wall_seconds: f64,
    pub load_before: Option<f64>,
    pub load_after: Option<f64>,
    /// The exit status, `None` if the run was killed at the timeout.
    pub exit_status: Option<i32>,
    pub timed_out: bool,
    pub work_limit_cutoffs: usize,
    /// The statistics of the SES the run wrote, imported into the board.
    pub statistics: Option<SessionStatistics>,
    /// Why there are no statistics, if there are none.
    pub note: Option<String>,
    /// The run's `freerouting-rip-stats/1` report without its `nets` list
    /// (`--rip-stats`, Rust runs only).
    pub rip_stats: Option<freerouting::drc::json::JsonValue>,
    /// Q02.1's delivered widths of the SES imported into the board.
    pub widths: Option<freerouting::quality::neckdown::width_report::WidthReport>,
    /// Q15.2's tidiness of the SES imported into the board.
    pub tidiness: Option<crate::tidy::TidyReport>,
}

/// Starts `args` in `dir` with stdout and stderr in `log`, waits at most
/// `timeout`, and returns the wall time, exit status and whether it timed
/// out.
pub fn run_timed(
    args: &[String],
    dir: &Path,
    log: &Path,
    timeout: Duration,
) -> Result<(f64, Option<i32>, bool), String> {
    let log_file = std::fs::File::create(log).map_err(|e| format!("{}: {e}", log.display()))?;
    let err_file = log_file
        .try_clone()
        .map_err(|e| format!("{}: {e}", log.display()))?;
    let start = Instant::now();
    let mut child = Command::new(&args[0])
        .args(&args[1..])
        .current_dir(dir)
        .stdin(Stdio::null())
        .stdout(log_file)
        .stderr(err_file)
        .spawn()
        .map_err(|e| format!("can't start {}: {e}", args[0]))?;
    loop {
        if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
            return Ok((start.elapsed().as_secs_f64(), status.code(), false));
        }
        if start.elapsed() >= timeout {
            let _ = child.kill();
            let _ = child.wait();
            return Ok((start.elapsed().as_secs_f64(), None, true));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

/// The `WORK_LIMIT_CUTOFF` lines Java logs when a work limit fires.
pub fn count_cutoffs(log: &str) -> usize {
    log.lines()
        .filter(|l| l.contains("WORK_LIMIT_CUTOFF"))
        .count()
}

/// Java's major version from `java -version` output.
pub fn java_major_version(version_output: &str) -> Option<u32> {
    let quoted = version_output.split('"').nth(1)?;
    let mut parts = quoted.split(['.', '-', '+']);
    let first: u32 = parts.next()?.parse().ok()?;
    if first == 1 {
        parts.next()?.parse().ok()
    } else {
        Some(first)
    }
}

/// Checks that `java` is Java 25 or newer and returns its version line.
pub fn check_java(java: &str) -> Result<String, String> {
    let output = Command::new(java)
        .arg("-version")
        .env_remove("JAVA_TOOL_OPTIONS")
        .output()
        .map_err(|e| format!("can't run '{java} -version': {e}"))?;
    let text = String::from_utf8_lossy(&output.stderr).into_owned()
        + &String::from_utf8_lossy(&output.stdout);
    let line = text
        .lines()
        .find(|l| l.contains("version"))
        .unwrap_or("")
        .trim()
        .to_string();
    match java_major_version(&text) {
        Some(v) if v >= 25 => Ok(line),
        Some(v) => Err(format!(
            "Java 25 or newer is needed, '{java}' is Java {v} ({line})"
        )),
        None => Err(format!("can't read the version of '{java}': {text}")),
    }
}

/// Measures the SES a run wrote.
pub fn measure(
    board: &freerouting::board::BasicBoard,
    ses: &Path,
) -> Result<SessionStatistics, String> {
    if !ses.is_file() {
        return Err("no SES written".into());
    }
    let session = parse_session(&read_text(ses)?)?;
    Ok(SessionStatistics::of_session(board, &session).0)
}

/// Q02.1's delivered-width report of the SES a run wrote, imported into a
/// copy of `board`.
pub fn measure_widths(
    board: &freerouting::board::BasicBoard,
    ses: &Path,
) -> Result<freerouting::quality::neckdown::width_report::WidthReport, String> {
    let session = parse_session(&read_text(ses)?)?;
    let mut copy = board.clone();
    freerouting::io::specctra::import_session(&mut copy, &session);
    Ok(freerouting::quality::neckdown::width_report::report(&copy))
}

/// Options of a bench run.
#[derive(Clone, Debug)]
pub struct BenchOptions {
    pub set_file: PathBuf,
    pub out_dir: PathBuf,
    pub runs: usize,
    pub engines: Vec<Engine>,
    pub default_bounds: Bounds,
    pub bounds_file: Option<PathBuf>,
    pub optimize: bool,
    /// `None` times regardless of load.
    pub max_load: Option<f64>,
    pub load_wait: Duration,
    pub timeout: Duration,
    pub java_version: Option<String>,
    /// Q05.1: run the Rust engine with the `rip-stats` diagnostic and keep
    /// its report in the run records.
    pub rip_stats: bool,
    /// P03.3: boards routed at once (at least 1). Above 1 nothing is timed
    /// on a quiet machine: the load check is skipped.
    pub jobs: usize,
}

/// The results of one board and engine.
#[derive(Clone, Debug)]
pub struct EngineResult {
    pub engine: &'static str,
    pub runs: Vec<RunRecord>,
    /// Why the engine was not run, if it wasn't.
    pub skipped: Option<String>,
}

/// The results of one board.
#[derive(Clone, Debug)]
pub struct BoardResult {
    pub entry: SetEntry,
    pub bounds: Bounds,
    pub engines: Vec<EngineResult>,
}

/// Everything a bench run produced.
#[derive(Clone, Debug)]
pub struct BenchResult {
    pub options: BenchOptions,
    pub boards: Vec<BoardResult>,
    /// Set entries whose file is missing.
    pub missing: Vec<SetEntry>,
    /// Set when the run stopped early because the machine got busy.
    pub aborted: Option<String>,
}

/// The output directory name of a board: its path as written, without the
/// extension, with every character other than `[A-Za-z0-9_-]` as `_`.
fn file_name_for(entry: &SetEntry) -> String {
    let name: String = entry
        .written
        .trim_end_matches(".dsn")
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    name.trim_start_matches('_').to_string()
}

/// The Rust CLI arguments of a `--rip-stats` run (Q05.1).
pub fn rip_stats_arguments(result_json: &Path) -> Vec<String> {
    vec![
        "--router.special.diagnostics=rip-stats".to_string(),
        "--result-json".to_string(),
        result_json.display().to_string(),
    ]
}

/// The `rip_stats` member of a result manifest without its `nets` list,
/// if the manifest exists and has one.
pub fn rip_stats_of_manifest(text: &str) -> Option<freerouting::drc::json::JsonValue> {
    use freerouting::drc::json::JsonValue;
    let JsonValue::Object(members) = JsonValue::parse(text).ok()? else {
        return None;
    };
    let (_, stats) = members.into_iter().find(|(k, _)| k == "rip_stats")?;
    match stats {
        JsonValue::Object(stats) => Some(JsonValue::Object(
            stats.into_iter().filter(|(k, _)| k != "nets").collect(),
        )),
        _ => None,
    }
}

fn read_rip_stats(result_json: &Path) -> Option<freerouting::drc::json::JsonValue> {
    rip_stats_of_manifest(&std::fs::read_to_string(result_json).ok()?)
}

/// A Rust CLI that answers exit 2 without a SES doesn't route yet.
const RUST_NOT_ROUTING: &str = "the Rust CLI does not route yet (exit 2, no SES)";

/// Runs the benchmark. Refuses (an error, nothing timed) when the machine
/// is busy before the first run; stops early, keeping what was measured,
/// when it gets busy later. With `jobs` above 1 the boards route in
/// parallel and no load is checked (see the module docs).
pub fn run_bench(options: BenchOptions) -> Result<BenchResult, String> {
    let set_text = std::fs::read_to_string(&options.set_file)
        .map_err(|e| format!("{}: {e}", options.set_file.display()))?;
    let root = repository_root(&options.set_file);
    let bounds_table = match &options.bounds_file {
        Some(path) => read_bounds(
            &std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?,
        )?,
        None => Vec::new(),
    };
    let parallel = options.jobs > 1;
    if !parallel {
        quiet(&options)?;
    }
    std::fs::create_dir_all(&options.out_dir)
        .map_err(|e| format!("{}: {e}", options.out_dir.display()))?;
    let (present, missing): (Vec<SetEntry>, Vec<SetEntry>) = read_set(&set_text, &root)
        .into_iter()
        .partition(|e| e.path.is_file());
    for entry in &missing {
        eprintln!("bench: skipping {} (file missing)", entry.written);
    }
    let mut result = BenchResult {
        options: options.clone(),
        boards: Vec::new(),
        missing,
        aborted: None,
    };
    let rust_unavailable = Mutex::new(None);
    if !parallel {
        for entry in present {
            let bounds = bounds_for(&bounds_table, &entry, options.default_bounds);
            let (board_result, aborted) =
                run_board(&options, entry, bounds, None, &rust_unavailable)?;
            result.boards.push(board_result);
            if aborted.is_some() {
                result.aborted = aborted;
                break;
            }
        }
        return Ok(result);
    }
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    let share = thread_share(cores, options.jobs);
    let next = AtomicUsize::new(0);
    let slots: Vec<Mutex<Option<Result<BoardResult, String>>>> =
        present.iter().map(|_| Mutex::new(None)).collect();
    std::thread::scope(|scope| {
        for _ in 0..options.jobs.min(present.len()) {
            scope.spawn(|| {
                loop {
                    let index = next.fetch_add(1, Ordering::Relaxed);
                    let Some(entry) = present.get(index) else {
                        break;
                    };
                    let bounds = bounds_for(&bounds_table, entry, options.default_bounds);
                    let outcome = run_board(
                        &options,
                        entry.clone(),
                        bounds,
                        Some(share),
                        &rust_unavailable,
                    )
                    .map(|(board, _)| board);
                    *slots[index].lock().unwrap_or_else(|e| e.into_inner()) = Some(outcome);
                }
            });
        }
    });
    for slot in slots {
        let outcome = slot.into_inner().unwrap_or_else(|e| e.into_inner());
        if let Some(outcome) = outcome {
            result.boards.push(outcome?);
        }
    }
    Ok(result)
}

/// Waits for a quiet machine if the options ask for one.
fn quiet(options: &BenchOptions) -> Result<(), String> {
    match options.max_load {
        Some(max) => {
            wait_for_quiet(load_average, max, options.load_wait, Duration::from_secs(5)).map(|_| ())
        }
        None => Ok(()),
    }
}

/// Routes one board with every engine. `threads` limits each run when
/// boards route in parallel, which also skips the load check. Returns the
/// board's results and, when the machine got busy, why the bench stops.
fn run_board(
    options: &BenchOptions,
    entry: SetEntry,
    bounds: Bounds,
    threads: Option<usize>,
    rust_unavailable: &Mutex<Option<String>>,
) -> Result<(BoardResult, Option<String>), String> {
    let board = load_board(&entry.path);
    let dir = options.out_dir.join(file_name_for(&entry));
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut board_result = BoardResult {
        entry: entry.clone(),
        bounds,
        engines: Vec::new(),
    };
    let unavailable = || {
        rust_unavailable
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    };
    for engine in &options.engines {
        let mut engine_result = EngineResult {
            engine: engine.name,
            runs: Vec::new(),
            skipped: None,
        };
        if engine.name == "rust"
            && let Some(reason) = unavailable()
        {
            engine_result.skipped = Some(reason);
            board_result.engines.push(engine_result);
            continue;
        }
        for run in 1..=options.runs {
            if threads.is_none()
                && let Err(e) = quiet(options)
            {
                board_result.engines.push(engine_result);
                return Ok((board_result, Some(e)));
            }
            let stem = format!("{}-run{run}", engine.name);
            let ses = dir.join(format!("{stem}.ses"));
            let log = dir.join(format!("{stem}.log"));
            let _ = std::fs::remove_file(&ses);
            let mut args = engine.arguments(&entry.path, &ses, bounds, options.optimize);
            if let Some(threads) = threads {
                args.extend(thread_arguments(threads));
            }
            let result_json = dir.join(format!("{stem}.result.json"));
            let rip_stats = options.rip_stats && engine.name == "rust";
            if rip_stats {
                let _ = std::fs::remove_file(&result_json);
                args.extend(rip_stats_arguments(&result_json));
            }
            eprintln!("bench: {} {} run {run}", entry.written, engine.name);
            let load_before = load_average();
            let (wall_seconds, exit_status, timed_out) =
                run_timed(&args, &dir, &log, options.timeout)?;
            let load_after = load_average();
            let log_text = std::fs::read_to_string(&log).unwrap_or_default();
            if engine.name == "rust" && exit_status == Some(2) && !ses.is_file() {
                *rust_unavailable.lock().unwrap_or_else(|e| e.into_inner()) =
                    Some(RUST_NOT_ROUTING.into());
                engine_result.skipped = Some(RUST_NOT_ROUTING.into());
                break;
            }
            let measured = match &board {
                Ok(board) => measure(board, &ses),
                Err(e) => Err(format!("board doesn't load: {e}")),
            };
            let widths = match (&board, &measured) {
                (Ok(board), Ok(_)) => measure_widths(board, &ses).ok(),
                _ => None,
            };
            let tidiness = match (&board, &measured) {
                (Ok(board), Ok(_)) => crate::tidy::tidy_session(board, &ses).ok(),
                _ => None,
            };
            engine_result.runs.push(RunRecord {
                run,
                wall_seconds,
                load_before,
                load_after,
                exit_status,
                timed_out,
                work_limit_cutoffs: count_cutoffs(&log_text),
                note: measured.as_ref().err().cloned(),
                statistics: measured.ok(),
                rip_stats: rip_stats.then(|| read_rip_stats(&result_json)).flatten(),
                widths,
                tidiness,
            });
        }
        board_result.engines.push(engine_result);
    }
    Ok((board_result, None))
}

//! The parts of `freerouting-dev bench` that don't launch an engine (F02.3).

use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use freerouting_dev::bench::{
    BenchOptions, BenchResult, BoardResult, Bounds, Engine, EngineResult, RunRecord, SetEntry,
    bounds_for, count_cutoffs, java_major_version, read_bounds, read_set, rip_stats_arguments,
    rip_stats_of_manifest, run_bench, thread_arguments, thread_share, wait_for_quiet,
};
use freerouting_dev::bench_report::{median, spread, to_json, to_markdown};
use freerouting_dev::json_read::Json;
use freerouting_dev::ses_diff::SessionStatistics;

#[test]
fn sets_skip_comments_and_resolve_paths() {
    let root = Path::new("/repo");
    let entries = read_set(
        "# a comment\n\nfixtures/a.dsn  # trailing\n/abs/b.dsn\n~/boards/c.dsn\n",
        root,
    );
    let written: Vec<&str> = entries.iter().map(|e| e.written.as_str()).collect();
    assert_eq!(written, ["fixtures/a.dsn", "/abs/b.dsn", "~/boards/c.dsn"]);
    assert_eq!(entries[0].path, PathBuf::from("/repo/fixtures/a.dsn"));
    assert_eq!(entries[1].path, PathBuf::from("/abs/b.dsn"));
    assert!(entries[2].path.ends_with("boards/c.dsn"));
    assert!(!entries[2].path.starts_with("~"));
}

#[test]
fn route_bounds_by_board_name() {
    let table = read_bounds("board\tmax_passes\tmax_items\nbm01\t100\t\nbig\t20\t5000\n").unwrap();
    let entry = |p: &str| SetEntry {
        written: p.into(),
        path: PathBuf::from(format!("/r/{p}")),
    };
    let default = Bounds {
        max_passes: 5,
        max_items: None,
    };
    assert_eq!(
        bounds_for(&table, &entry("fixtures/bm01.dsn"), default),
        Bounds {
            max_passes: 100,
            max_items: None
        }
    );
    assert_eq!(
        bounds_for(&table, &entry("fixtures/big.dsn"), default),
        Bounds {
            max_passes: 20,
            max_items: Some(5000)
        }
    );
    assert_eq!(bounds_for(&table, &entry("other.dsn"), default), default);
    assert!(read_bounds("name\tpasses\n").is_err());
    assert!(read_bounds("board\tmax_passes\nx\tmany\n").is_err());
}

/// `tools/java-reference/route-bounds.tsv` (J01.3) has a row for every fixture
/// DSN outside `fixtures/kicad`, and the bench finds the rows of boards in
/// sub-directories too.
#[test]
fn committed_route_bounds_cover_every_fixture() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let text = std::fs::read_to_string(root.join("tools/java-reference/route-bounds.tsv")).unwrap();
    let table = read_bounds(&text).unwrap();
    let unset = Bounds {
        max_passes: 0,
        max_items: None,
    };
    let mut dirs = vec![root.join("fixtures")];
    let mut boards = 0;
    while let Some(dir) = dirs.pop() {
        for e in std::fs::read_dir(&dir).unwrap() {
            let path = e.unwrap().path();
            // fixtures/kicad holds KiCad boards that K07 compares itself;
            // like reference_load, the route references leave them out.
            if path.ends_with("fixtures/reference") || path.ends_with("fixtures/kicad") {
                continue;
            }
            if path.is_dir() {
                dirs.push(path);
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case("dsn"))
            {
                let written = path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                let entry = SetEntry {
                    written: written.clone(),
                    path: path.clone(),
                };
                let bounds = bounds_for(&table, &entry, unset);
                assert!(bounds.max_passes > 0, "{written} has no route bounds row");
                boards += 1;
            }
        }
    }
    assert!(boards > 100, "only {boards} fixture boards found");
}

#[test]
fn busy_machine_waits_then_refuses_without_timing() {
    let reads = Cell::new(0);
    let start = Instant::now();
    let result = wait_for_quiet(
        || {
            reads.set(reads.get() + 1);
            Some(7.5)
        },
        1.0,
        Duration::from_millis(200),
        Duration::from_millis(50),
    );
    let message = result.unwrap_err();
    assert!(start.elapsed() >= Duration::from_millis(200));
    assert!(reads.get() >= 3, "polled {} times", reads.get());
    assert!(message.starts_with("refusing to time"), "{message}");
    assert!(
        message.contains("7.50") && message.contains("1.00"),
        "{message}"
    );
}

#[test]
fn quiet_machine_is_timed_once_the_load_drops() {
    let loads = [3.0, 2.0, 0.5];
    let i = Cell::new(0);
    let load = wait_for_quiet(
        || {
            let v = loads[i.get().min(loads.len() - 1)];
            i.set(i.get() + 1);
            Some(v)
        },
        1.0,
        Duration::from_secs(10),
        Duration::from_millis(1),
    );
    assert_eq!(load, Ok(0.5));
    let unreadable = wait_for_quiet(|| None, 1.0, Duration::ZERO, Duration::ZERO);
    assert!(unreadable.unwrap_err().contains("--max-load off"));
}

#[test]
fn java_versions_are_read() {
    assert_eq!(
        java_major_version("openjdk version \"25.0.4.1\" 2026-08-18"),
        Some(25)
    );
    assert_eq!(java_major_version("java version \"1.8.0_402\""), Some(8));
    assert_eq!(
        java_major_version("openjdk version \"21\" 2023-09-19"),
        Some(21)
    );
    assert_eq!(java_major_version("no version here"), None);
}

#[test]
fn medians_and_spreads() {
    assert_eq!(median(&[3.0, 1.0, 2.0]), Some(2.0));
    assert_eq!(median(&[4.0, 1.0, 2.0, 3.0]), Some(2.5));
    assert_eq!(median(&[]), None);
    assert_eq!(spread(&[3.0, 1.0, 2.5]), Some(2.0));
}

#[test]
fn cutoff_lines_are_counted() {
    let log = "INFO x\nINFO WORK_LIMIT_CUTOFF site=maze_search nominal_ms=1 units=2\n\
               INFO WORK_LIMIT_CUTOFF site=pull_tight nominal_ms=1 units=2\n";
    assert_eq!(count_cutoffs(log), 2);
}

fn run(run: usize, wall: f64, vias: usize) -> RunRecord {
    RunRecord {
        run,
        wall_seconds: wall,
        load_before: Some(0.4),
        load_after: Some(0.9),
        exit_status: Some(0),
        timed_out: false,
        work_limit_cutoffs: 0,
        statistics: Some(SessionStatistics {
            incomplete_connections: 0,
            clearance_violations: 0,
            vias,
            trace_length_mm: 12.5,
            bends: 3,
        }),
        note: None,
        rip_stats: None,
        widths: None,
        tidiness: None,
    }
}

fn sample_result() -> BenchResult {
    let options = BenchOptions {
        set_file: "tools/bench/sets/x.txt".into(),
        out_dir: "out".into(),
        runs: 2,
        engines: vec![
            Engine {
                name: "java",
                command: vec!["java".into(), "-jar".into(), "fr.jar".into()],
            },
            Engine {
                name: "rust",
                command: vec!["freerouting".into()],
            },
        ],
        default_bounds: Bounds {
            max_passes: 5,
            max_items: None,
        },
        bounds_file: None,
        optimize: false,
        max_load: Some(1.0),
        load_wait: Duration::from_secs(1),
        timeout: Duration::from_secs(1),
        java_version: Some("openjdk version \"25\"".into()),
        rip_stats: false,
        jobs: 1,
    };
    let board = |name: &str| BoardResult {
        entry: SetEntry {
            written: name.into(),
            path: PathBuf::from(name),
        },
        bounds: options.default_bounds,
        engines: vec![
            EngineResult {
                engine: "java",
                runs: vec![run(1, 2.0, 4), run(2, 3.0, 5)],
                skipped: None,
            },
            EngineResult {
                engine: "rust",
                runs: Vec::new(),
                skipped: Some("the Rust CLI does not route yet (exit 2, no SES)".into()),
            },
        ],
    };
    BenchResult {
        boards: vec![board("fixtures/a.dsn"), board("fixtures/b.dsn")],
        options,
        missing: vec![SetEntry {
            written: "~/gone.dsn".into(),
            path: "/home/gone.dsn".into(),
        }],
        aborted: None,
    }
}

#[test]
fn markdown_has_one_row_per_board_and_engine() {
    let table = to_markdown(&sample_result());
    let rows: Vec<&str> = table
        .lines()
        .filter(|l| l.starts_with("| fixtures/"))
        .collect();
    assert_eq!(rows.len(), 4, "{table}");
    assert!(
        rows[0].starts_with("| fixtures/a.dsn | java | 5 | 2 | 2.50 | 1.00 | 0.40 | 0 | 0 | 4–5 |"),
        "{}",
        rows[0]
    );
    assert!(
        rows[1].starts_with("| fixtures/a.dsn | rust |"),
        "{}",
        rows[1]
    );
    assert!(rows[1].ends_with("skipped: the Rust CLI does not route yet (exit 2, no SES) |"));
    let header_cells = table
        .lines()
        .find(|l| l.starts_with("| board"))
        .unwrap()
        .matches('|')
        .count();
    for row in &rows {
        assert_eq!(row.matches('|').count(), header_cells, "{row}");
    }
    assert!(table.contains("Skipped (file missing):\n- ~/gone.dsn"));
}

#[test]
fn json_records_every_run_with_its_load() {
    let json = Json::parse(&to_json(&sample_result()).to_pretty_string()).unwrap();
    let Some(Json::Array(boards)) = json.get("boards") else {
        panic!("no boards");
    };
    assert_eq!(boards.len(), 2);
    let Some(Json::Array(engines)) = boards[0].get("engines") else {
        panic!("no engines");
    };
    let java = &engines[0];
    assert_eq!(
        java.path("median_wall_seconds").and_then(Json::as_f64),
        Some(2.5)
    );
    let Some(Json::Array(runs)) = java.get("runs") else {
        panic!("no runs");
    };
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[0].get("load_before").and_then(Json::as_f64), Some(0.4));
    assert_eq!(runs[0].get("load_after").and_then(Json::as_f64), Some(0.9));
    assert_eq!(runs[1].get("exit_status").and_then(Json::as_f64), Some(0.0));
    assert!(engines[1].get("skipped").is_some());
    assert_eq!(json.get("optimizer"), Some(&Json::Bool(false)));
}

#[test]
fn rip_stats_records_keep_the_report_without_the_net_list() {
    let manifest = r#"{"schema_version": 1, "rip_stats": {"schema": "freerouting-rip-stats/1",
        "rips": 3, "top_share": 0.6000, "top_nets": [{"net": 2, "share": 0.6000}],
        "nets": [{"net": 2}, {"net": 5}]}}"#;
    let stats = rip_stats_of_manifest(manifest).expect("a rip_stats member");
    let text = stats.to_pretty_string();
    assert!(text.contains("\"top_nets\""), "{text}");
    assert!(text.contains("\"share\": 0.6000"), "{text}");
    assert!(!text.contains("\"nets\""), "{text}");
    assert!(rip_stats_of_manifest(r#"{"schema_version": 1}"#).is_none());
    assert!(rip_stats_of_manifest("not json").is_none());
    assert_eq!(
        rip_stats_arguments(Path::new("r.json")),
        vec![
            "--router.special.diagnostics=rip-stats",
            "--result-json",
            "r.json"
        ]
    );

    let mut record = run(1, 2.0, 4);
    record.rip_stats = Some(stats);
    let mut result = sample_result();
    result.boards[0].engines[0].runs = vec![record];
    let json = to_json(&result).to_pretty_string();
    assert!(json.contains("\"rip_stats\""), "{json}");
}

#[test]
fn parallel_boards_share_the_cores() {
    assert_eq!(thread_share(8, 1), 8);
    assert_eq!(thread_share(8, 4), 2);
    assert_eq!(thread_share(8, 3), 2);
    assert_eq!(thread_share(2, 4), 1);
    assert_eq!(thread_share(8, 0), 8);
    assert_eq!(
        thread_arguments(2),
        [
            "--router.autorouter.max_threads=2",
            "--router.optimizer.max_threads=2"
        ]
    );
}

/// P03.3: `--jobs 3` routes the boards of a set at once and lists them in set order, each run
/// limited to a share of the cores; `--jobs 1` keeps the plain command line. The engine is a
/// shell script that records its arguments in the board's directory and writes no session.
#[cfg(unix)]
#[test]
fn jobs_route_boards_at_once_and_merge_in_set_order() {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR")).join("bench-jobs");
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(root.join("Cargo.lock"), "").unwrap();
    let names = ["e", "a", "d", "b", "c"];
    for name in names {
        std::fs::write(root.join(format!("{name}.dsn")), "(pcb x)").unwrap();
    }
    let set = root.join("set.txt");
    std::fs::write(
        &set,
        names.map(|n| format!("{n}.dsn\n")).concat() + "gone.dsn\n",
    )
    .unwrap();
    let options = |jobs: usize, out: &str| BenchOptions {
        set_file: set.clone(),
        out_dir: root.join(out),
        runs: 1,
        engines: vec![Engine {
            name: "rust",
            command: vec![
                "sh".into(),
                "-c".into(),
                "echo \"$@\" > args.txt".into(),
                "engine".into(),
            ],
        }],
        default_bounds: Bounds {
            max_passes: 2,
            max_items: None,
        },
        bounds_file: None,
        optimize: false,
        max_load: Some(0.0),
        load_wait: Duration::from_millis(1),
        timeout: Duration::from_secs(30),
        java_version: None,
        rip_stats: false,
        jobs,
    };
    // `--jobs 3` checks no load: the limit 0.0 would refuse a `--jobs 1` run on any machine
    // that isn't idle.
    let parallel = run_bench(options(3, "jobs3")).expect("runs");
    let order: Vec<&str> = parallel
        .boards
        .iter()
        .map(|b| b.entry.written.as_str())
        .collect();
    assert_eq!(order, ["e.dsn", "a.dsn", "d.dsn", "b.dsn", "c.dsn"]);
    assert_eq!(parallel.missing.len(), 1);
    assert!(parallel.aborted.is_none());
    let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
    for name in names {
        let args = std::fs::read_to_string(root.join(format!("jobs3/{name}/args.txt"))).unwrap();
        assert!(args.contains(&format!("{name}.dsn")), "{args}");
        for flag in thread_arguments(thread_share(cores, 3)) {
            assert!(args.contains(&flag), "{args}");
        }
    }
    for board in &parallel.boards {
        let run = &board.engines[0].runs[0];
        assert_eq!(run.exit_status, Some(0));
        assert!(run.note.is_some(), "{run:?}");
    }
    let sequential = run_bench(BenchOptions {
        max_load: None,
        ..options(1, "jobs1")
    })
    .expect("runs");
    let args = std::fs::read_to_string(root.join("jobs1/a/args.txt")).unwrap();
    assert!(!args.contains("max_threads"), "{args}");
    let order_1: Vec<&str> = sequential
        .boards
        .iter()
        .map(|b| b.entry.written.as_str())
        .collect();
    assert_eq!(order_1, order);
    let json = to_json(&parallel).to_pretty_string();
    assert!(json.contains("\"jobs\": 3"), "{json}");
    assert!(to_markdown(&parallel).contains("3 boards at once"));
}

fn with_tidiness() -> BenchResult {
    use freerouting_dev::tidy::{Distribution, TidyReport};
    use freerouting_dev::tidy_copper::Measured;
    let d = |median, p90| Some(Distribution { median, p90 });
    let mut result = sample_result();
    result.boards[0].engines[0].runs[0].tidiness = Some(TidyReport {
        measured: Measured {
            connections: Vec::new(),
            skipped: 0,
            nets: 0,
        },
        measures: [
            d(1.25, 2.5),
            d(0.5, 1.5),
            d(0.0, 1.0),
            d(0.0, 0.0),
            d(0.0, 2.0),
        ],
    });
    result
}

#[test]
fn tidiness_is_in_the_run_record_and_the_report() {
    let result = with_tidiness();
    let json = Json::parse(&to_json(&result).to_pretty_string()).unwrap();
    let Some(Json::Array(boards)) = json.get("boards") else {
        panic!("no boards");
    };
    let Some(Json::Array(engines)) = boards[0].get("engines") else {
        panic!("no engines");
    };
    let Some(Json::Array(runs)) = engines[0].get("runs") else {
        panic!("no runs");
    };
    let detour = runs[0].path("tidiness.measures.detour.p90");
    assert_eq!(detour.and_then(Json::as_f64), Some(2.5));
    assert_eq!(
        runs[0]
            .path("tidiness.measures.vias.median")
            .and_then(Json::as_f64),
        Some(0.0)
    );
    // a run without a score has no tidiness member
    assert!(runs[1].get("tidiness").is_none());

    let table = to_markdown(&result);
    assert!(
        table.contains("Tidiness per connection, median / p90"),
        "{table}"
    );
    assert!(
        table.contains("| fixtures/a.dsn | java | 0 | 1.25 / 2.50 | 0.50 / 1.50 | 0.00 / 1.00 |"),
        "{table}"
    );
    // no scores: no section
    assert!(!to_markdown(&sample_result()).contains("Tidiness"));
}

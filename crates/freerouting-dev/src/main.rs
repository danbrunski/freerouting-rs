//! `freerouting-dev`: developer tools for the port (see the library docs).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use freerouting_dev::bench::{
    BenchOptions, Bounds, Engine, check_java, repository_root, run_bench,
};
use freerouting_dev::bench_report;
use freerouting_dev::ses_diff::{SesDiff, parse_session};
use freerouting_dev::{load_board, read_text};

mod feasibility;

const USAGE: &str = "usage:
  freerouting-dev ses-diff board.dsn a.ses b.ses [--json out.json]
  freerouting-dev bench --set <file> --java <jar> [--rust <bin>] [--runs N] --out <dir>
      [--optimize] [--max-passes N] [--bounds <tsv>] [--max-load L|off] [--load-wait S]
      [--timeout S] [--java-bin <java>]
  freerouting-dev feasibility board.dsn [--json out.json] [--all-pins] [--step-um N]
      [--start shape|centre]
  freerouting-dev gate ...       (the measurement gate; `freerouting-dev gate --self-test`)
  freerouting-dev failed-vs-human board.dsn engine.ses human.dsn [--json out.json] [--no-feasibility]";

/// Bench defaults: passes when no route bounds name the board, the load
/// limit, how long to wait for it, and the per-run safety timeout.
const DEFAULT_MAX_PASSES: u32 = 5;
const DEFAULT_MAX_LOAD: f64 = 1.0;
const DEFAULT_LOAD_WAIT_SECONDS: u64 = 120;
const DEFAULT_TIMEOUT_SECONDS: u64 = 1800;
/// Exit status when the bench refuses to time on a busy machine.
const EXIT_BUSY: u8 = 3;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("ses-diff") => ses_diff(&args[1..]),
        Some("bench") => bench(&args[1..]),
        Some("feasibility") => feasibility_command(&args[1..]),
        Some("gate") => freerouting_dev::gate_cli::run(&args[1..]),
        Some("failed-vs-human") => freerouting_dev::failed_vs_human::run(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::from(2)
        }
    }
}

type Options = Vec<(String, String)>;

/// Splits `--name value` options and `--flag` flags (value `""`) from
/// positional arguments.
fn split_options(
    args: &[String],
    names: &[&str],
    flags: &[&str],
) -> Result<(Vec<String>, Options), String> {
    let mut positional = Vec::new();
    let mut options = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let arg = &args[i];
        if let Some(name) = arg.strip_prefix("--") {
            if flags.contains(&name) {
                options.push((name.to_string(), String::new()));
                i += 1;
                continue;
            }
            if !names.contains(&name) {
                return Err(format!("unknown option --{name}\n{USAGE}"));
            }
            let value = args
                .get(i + 1)
                .ok_or_else(|| format!("--{name} needs a value\n{USAGE}"))?;
            options.push((name.to_string(), value.clone()));
            i += 2;
        } else {
            positional.push(arg.clone());
            i += 1;
        }
    }
    Ok((positional, options))
}

fn ses_diff(args: &[String]) -> Result<ExitCode, String> {
    let (positional, options) = split_options(args, &["json"], &[])?;
    let [board, a, b] = positional.as_slice() else {
        return Err(USAGE.to_string());
    };
    let board = load_board(Path::new(board))?;
    let a_session = parse_session(&read_text(Path::new(a))?).map_err(|e| format!("{a}: {e}"))?;
    let b_session = parse_session(&read_text(Path::new(b))?).map_err(|e| format!("{b}: {e}"))?;
    let diff = SesDiff::new(&board, &a_session, &b_session);
    print!("{}", diff.to_text(10));
    for (name, value) in &options {
        if name == "json" {
            std::fs::write(value, diff.to_json().to_pretty_string() + "\n")
                .map_err(|e| format!("{value}: {e}"))?;
        }
    }
    Ok(if diff.equivalent() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn option<'a>(options: &'a Options, name: &str) -> Option<&'a str> {
    options
        .iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

fn parsed<T: std::str::FromStr>(options: &Options, name: &str, default: T) -> Result<T, String> {
    match option(options, name) {
        Some(v) => v
            .parse()
            .map_err(|_| format!("bad value for --{name}: '{v}'")),
        None => Ok(default),
    }
}

fn bench(args: &[String]) -> Result<ExitCode, String> {
    let (positional, options) = split_options(
        args,
        &[
            "set",
            "java",
            "rust",
            "runs",
            "out",
            "max-passes",
            "bounds",
            "max-load",
            "load-wait",
            "timeout",
            "java-bin",
        ],
        &["optimize"],
    )?;
    if !positional.is_empty() {
        return Err(format!("unexpected arguments {positional:?}\n{USAGE}"));
    }
    let need = |name: &str| {
        option(&options, name)
            .map(PathBuf::from)
            .ok_or_else(|| format!("--{name} is required\n{USAGE}"))
    };
    let set_file = need("set")?;
    let out_dir = need("out")?;
    let mut engines = Vec::new();
    let mut java_version = None;
    if let Some(jar) = option(&options, "java") {
        let java = option(&options, "java-bin").unwrap_or("java");
        java_version = Some(check_java(java)?);
        let jar = std::fs::canonicalize(jar).map_err(|e| format!("{jar}: {e}"))?;
        engines.push(Engine {
            name: "java",
            command: vec![java.to_string(), "-jar".into(), jar.display().to_string()],
        });
    }
    if let Some(bin) = option(&options, "rust") {
        let bin = std::fs::canonicalize(bin).map_err(|e| format!("{bin}: {e}"))?;
        engines.push(Engine {
            name: "rust",
            command: vec![bin.display().to_string()],
        });
    }
    if engines.is_empty() {
        return Err(format!("give --java, --rust or both\n{USAGE}"));
    }
    let max_load = match option(&options, "max-load") {
        Some("off") => None,
        _ => Some(parsed(&options, "max-load", DEFAULT_MAX_LOAD)?),
    };
    let default_bounds_file =
        repository_root(&set_file).join("tools/java-reference/route-bounds.tsv");
    let bounds_file = match option(&options, "bounds") {
        Some(path) => Some(PathBuf::from(path)),
        None => default_bounds_file.is_file().then_some(default_bounds_file),
    };
    let bench_options = BenchOptions {
        set_file,
        out_dir: out_dir.clone(),
        runs: parsed(&options, "runs", 1)?,
        engines,
        default_bounds: Bounds {
            max_passes: parsed(&options, "max-passes", DEFAULT_MAX_PASSES)?,
            max_items: None,
        },
        bounds_file,
        optimize: option(&options, "optimize").is_some(),
        max_load,
        load_wait: Duration::from_secs(parsed(&options, "load-wait", DEFAULT_LOAD_WAIT_SECONDS)?),
        timeout: Duration::from_secs(parsed(&options, "timeout", DEFAULT_TIMEOUT_SECONDS)?),
        java_version,
    };
    let result = match run_bench(bench_options) {
        Ok(result) => result,
        Err(e) if e.starts_with("refusing to time") || e.starts_with("can't read the 1-minute") => {
            eprintln!("bench: {e}");
            return Ok(ExitCode::from(EXIT_BUSY));
        }
        Err(e) => return Err(e),
    };
    let json = out_dir.join("bench.json");
    let markdown = out_dir.join("bench.md");
    std::fs::write(
        &json,
        bench_report::to_json(&result).to_pretty_string() + "\n",
    )
    .map_err(|e| format!("{}: {e}", json.display()))?;
    let table = bench_report::to_markdown(&result);
    std::fs::write(&markdown, &table).map_err(|e| format!("{}: {e}", markdown.display()))?;
    print!("{table}");
    eprintln!("bench: wrote {} and {}", json.display(), markdown.display());
    Ok(match result.aborted {
        Some(_) => ExitCode::from(EXIT_BUSY),
        None => ExitCode::SUCCESS,
    })
}

fn feasibility_command(args: &[String]) -> Result<ExitCode, String> {
    let (positional, options) = split_options(args, &["json", "step-um", "start"], &["all-pins"])?;
    let [board] = positional.as_slice() else {
        return Err(USAGE.to_string());
    };
    let defaults = freerouting::quality::feasibility::FeasibilityOptions::default();
    let step_um: f64 = parsed(&options, "step-um", defaults.step_um)?;
    if step_um.is_nan() || step_um <= 0.0 {
        return Err(format!("--step-um must be positive\n{USAGE}"));
    }
    let start = match option(&options, "start") {
        None | Some("shape") => freerouting::quality::feasibility::StartModel::PinShape,
        Some("centre") => freerouting::quality::feasibility::StartModel::PinCentre,
        Some(other) => return Err(format!("bad value for --start: '{other}'\n{USAGE}")),
    };
    let feasibility_options = freerouting::quality::feasibility::FeasibilityOptions {
        step_um,
        start,
        all_pins: option(&options, "all-pins").is_some(),
        ..defaults
    };
    feasibility::run(
        Path::new(board),
        option(&options, "json").map(Path::new),
        &feasibility_options,
    )?;
    Ok(ExitCode::SUCCESS)
}

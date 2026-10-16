//! The `freerouting-dev gate` command line (F03.1, F03.4).
//!
//! ```text
//! freerouting-dev gate --records run.json --baseline base.json [--prereg reg.json]
//!     [--rows a,b] [--arms off,on]
//! freerouting-dev gate --records run.json --baseline base.json --write-baseline [...]
//! freerouting-dev gate collect --arm NAME=bench.json[,oracle=DIR] (twice or more)
//!     --out records.json [--engine java] [--group PREFIX=BOARD] [--arms-json file]
//! ```
//!
//! The self-test ([`crate::gate_selftest`]) runs first on every invocation.
//! Exit status: 0 the gate passes (or a baseline was written), 1 it fails
//! (problems, the pass rule, a missing or malformed baseline, a failed
//! self-test), 2 a usage or file error.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::gate::{Arms, Class, Outcome, Problem, Settings, baseline_from, check, to_writer};
use crate::gate_collect::{ArmSource, CollectOptions, collect, write_records};
use crate::gate_prereg::{Prereg, check_registered, check_registration};
use crate::gate_selftest;
use crate::json_read::Json;

/// The usage text.
pub const USAGE: &str = "usage:
  freerouting-dev gate --records <run.json> --baseline <base.json> [--prereg <reg.json>]
      [--rows a,b] [--arms off,on] [--write-baseline]
  freerouting-dev gate collect --arm NAME=<bench.json>[,oracle=<dir>] --arm NAME=... --out <records.json>
      [--engine java] [--group PREFIX=BOARD] [--arms-json <file>]
  freerouting-dev gate --self-test";

#[derive(Default)]
struct Parsed {
    options: Vec<(String, String)>,
}

impl Parsed {
    fn get(&self, name: &str) -> Option<&str> {
        self.options
            .iter()
            .rev()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    fn all(&self, name: &str) -> Vec<&str> {
        self.options
            .iter()
            .filter(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
            .collect()
    }
}

fn parse_args(args: &[String], names: &[&str], flags: &[&str]) -> Result<Parsed, String> {
    let mut parsed = Parsed::default();
    let mut i = 0;
    while i < args.len() {
        let Some(name) = args[i].strip_prefix("--") else {
            return Err(format!("unexpected argument {:?}\n{USAGE}", args[i]));
        };
        if flags.contains(&name) {
            parsed.options.push((name.to_string(), String::new()));
            i += 1;
        } else if names.contains(&name) {
            let value = args
                .get(i + 1)
                .ok_or_else(|| format!("--{name} needs a value\n{USAGE}"))?;
            parsed.options.push((name.to_string(), value.clone()));
            i += 2;
        } else {
            return Err(format!("unknown option --{name}\n{USAGE}"));
        }
    }
    Ok(parsed)
}

fn read_file(path: &Path) -> Result<String, String> {
    std::fs::read(path)
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .map_err(|e| format!("{}: {e}", path.display()))
}

fn fail(problems: &[Problem]) -> ExitCode {
    for p in problems {
        println!("{p}");
    }
    println!("gate: FAIL");
    ExitCode::from(1)
}

/// Runs `gate`.
pub fn run(args: &[String]) -> Result<ExitCode, String> {
    if let Err(failed) = gate_selftest::run() {
        println!("gate self-test FAILED ({} check(s)):", failed.len());
        for label in failed {
            println!("  {label}");
        }
        return Ok(ExitCode::from(1));
    }
    if args.first().map(String::as_str) == Some("collect") {
        return collect_command(&args[1..]);
    }
    let parsed = parse_args(
        args,
        &["records", "baseline", "prereg", "rows", "arms"],
        &["write-baseline", "self-test"],
    )?;
    if parsed.get("self-test").is_some() {
        println!("gate self-test: ok");
        return Ok(ExitCode::SUCCESS);
    }
    let records_path = PathBuf::from(
        parsed
            .get("records")
            .ok_or_else(|| format!("--records is required\n{USAGE}"))?,
    );
    let baseline_path = PathBuf::from(
        parsed
            .get("baseline")
            .ok_or_else(|| format!("--baseline is required\n{USAGE}"))?,
    );
    let records_text = read_file(&records_path)?;
    let records = Json::parse(&records_text)
        .map_err(|e| format!("{}: not JSON: {e}", records_path.display()))?;
    let prereg = match parsed.get("prereg") {
        Some(path) => match Prereg::parse(&read_file(Path::new(path))?) {
            Ok(p) => Some(p),
            Err(problems) => return Ok(fail(&problems)),
        },
        None => None,
    };
    let mut settings = prereg
        .as_ref()
        .map_or_else(Settings::default, Prereg::settings);
    if let Some(rows) = parsed.get("rows") {
        settings.scope = Some(
            rows.split(',')
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>(),
        );
    }
    if let Some(arms) = parsed.get("arms") {
        let (base, cand) = arms
            .split_once(',')
            .ok_or("--arms takes BASE,CANDIDATE".to_string())?;
        settings.arms = Arms {
            base: base.into(),
            cand: cand.into(),
        };
    }
    if parsed.get("write-baseline").is_some() {
        return write_baseline(&records, &settings, prereg.as_ref(), &baseline_path);
    }
    if !baseline_path.is_file() {
        println!(
            "MALFORMED baseline: {} does not exist. A missing baseline fails; record one \
             with --write-baseline after reading the table",
            baseline_path.display()
        );
        println!("gate: FAIL");
        return Ok(ExitCode::from(1));
    }
    let baseline = match Json::parse(&read_file(&baseline_path)?) {
        Ok(b) => b,
        Err(e) => {
            return Ok(fail(&[Problem::new(
                Class::Malformed,
                format!("baseline {}: not JSON: {e}", baseline_path.display()),
            )]));
        }
    };
    let outcome: Outcome = match &prereg {
        Some(p) => check_registered(&records, &baseline, p),
        None => check(&records, &baseline, &settings),
    };
    print!("{}", outcome.to_text());
    Ok(if outcome.passed() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    })
}

fn write_baseline(
    records: &Json,
    settings: &Settings,
    prereg: Option<&Prereg>,
    path: &Path,
) -> Result<ExitCode, String> {
    let mut problems = prereg.map_or_else(Vec::new, |p| check_registration(p, records));
    match baseline_from(records, settings) {
        Ok(baseline) if problems.is_empty() => {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            std::fs::write(path, to_writer(&baseline).to_pretty_string() + "\n")
                .map_err(|e| format!("{}: {e}", path.display()))?;
            println!("gate: wrote {}", path.display());
            Ok(ExitCode::SUCCESS)
        }
        Ok(_) => Ok(fail(&problems)),
        Err(more) => {
            problems.extend(more);
            Ok(fail(&problems))
        }
    }
}

fn collect_command(args: &[String]) -> Result<ExitCode, String> {
    let parsed = parse_args(args, &["arm", "out", "engine", "group", "arms-json"], &[])?;
    let mut arms = Vec::new();
    for spec in parsed.all("arm") {
        let (name, rest) = spec
            .split_once('=')
            .ok_or_else(|| format!("--arm takes NAME=bench.json[,oracle=DIR], got {spec:?}"))?;
        let mut parts = rest.split(",oracle=");
        let bench = parts.next().unwrap_or("");
        arms.push(ArmSource {
            name: name.to_string(),
            bench_json: PathBuf::from(bench),
            oracle_dir: parts.next().map(PathBuf::from),
        });
    }
    let mut groups = Vec::new();
    for g in parsed.all("group") {
        let (prefix, board) = g
            .split_once('=')
            .ok_or_else(|| format!("--group takes PREFIX=BOARD, got {g:?}"))?;
        groups.push((prefix.to_string(), board.to_string()));
    }
    let arms_description = match parsed.get("arms-json") {
        Some(p) => Some(
            Json::parse(&read_file(Path::new(p))?).map_err(|e| format!("{p}: not JSON: {e}"))?,
        ),
        None => None,
    };
    let out = PathBuf::from(
        parsed
            .get("out")
            .ok_or_else(|| format!("--out is required\n{USAGE}"))?,
    );
    let options = CollectOptions {
        arms,
        engine: parsed.get("engine").unwrap_or("java").to_string(),
        groups,
        arms_description,
    };
    let mut notes = Vec::new();
    let records = collect(&options, &mut notes)?;
    for note in &notes {
        eprintln!("gate collect: {note}");
    }
    write_records(&records, &out)?;
    eprintln!("gate collect: wrote {}", out.display());
    Ok(ExitCode::SUCCESS)
}

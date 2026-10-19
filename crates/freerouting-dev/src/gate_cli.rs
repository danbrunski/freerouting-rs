//! The `freerouting-dev gate` command line (F03.1, F03.4).
//!
//! ```text
//! freerouting-dev gate --records run.json --baseline base.json [--prereg reg.json]
//!     [--rows a,b] [--arms off,on] [--eligible] [--all-pairs]
//!     [--rule paired|totals [--accept improve|no-regress] [--noise BOARD=BAND]...
//!      [--speed METRIC[=PERCENT]]]
//! freerouting-dev gate --records run.json --baseline base.json --write-baseline [...]
//! freerouting-dev gate collect --arm NAME=bench.json[,oracle=DIR] (twice or more)
//!     --out records.json [--engine java] [--group PREFIX=BOARD] [--arms-json file]
//! ```
//!
//! `--rule totals` judges by N02's totals rule ([`crate::gate_totals`]); the
//! other three options set its acceptance, the noise band of a board and the
//! speed clause. With `--prereg` the registration states the rule, so `--rule`
//! may only repeat it and the other three are refused: a run can't choose its
//! own criteria.
//!
//! With `--prereg`, `--arms A,B` judges another pair of registered arms (no
//! baseline comparison: the baseline records the primary pair), `--rows` and
//! `--eligible` (the registration's `eligible` rows) judge a row subset, with
//! `min_boards` and the noise over the judged rows, and `--all-pairs` judges
//! and prints every pair in the registration's `pairs`. A combination that
//! can't be honoured is a usage error (exit 2), never a silent ignore.
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
use crate::gate_prereg::{PairRole, Prereg, check_registered_for, check_registration};
use crate::gate_selftest;
use crate::gate_totals::{Accept, MIN_SPEED_DROP_PERCENT, SpeedClause, TotalsRule};
use crate::json_read::Json;

/// The usage text.
pub const USAGE: &str = "usage:
  freerouting-dev gate --records <run.json> [--baseline <base.json>] [--prereg <reg.json>]
      [--rows a,b] [--arms off,on] [--write-baseline]
      [--eligible] [--all-pairs]
      (--baseline is required unless --prereg judges a pair other than the primary one;
       --eligible and --all-pairs need --prereg: the registration's eligible rows, every listed pair)
      [--rule paired|totals] [--accept improve|no-regress] [--noise BOARD=BAND]... [--speed METRIC[=PERCENT]]
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
        &[
            "records", "baseline", "prereg", "rows", "arms", "rule", "accept", "noise", "speed",
        ],
        &["write-baseline", "self-test", "eligible", "all-pairs"],
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
    let selection = Selection::from_args(&parsed, prereg.as_ref())?;
    let mut settings = Settings::default();
    if let Some(p) = &prereg {
        settings = p.settings();
    }
    settings.scope = selection.scope.clone();
    if let Some(first) = selection.pairs.first().filter(|_| prereg.is_none()) {
        // `--arms` without a registration: it wins over the records' own arms.
        settings.arms = first.arms.clone();
        settings.arms_pinned = parsed.get("arms").is_some();
    }
    match rule_options(&parsed, prereg.as_ref())? {
        RuleOptions::Refused(problems) => return Ok(fail(&problems)),
        RuleOptions::Totals(rule) => settings.totals = Some(rule),
        RuleOptions::Registered => {}
    }
    let baseline_path = parsed.get("baseline").map(PathBuf::from);
    if parsed.get("write-baseline").is_some() {
        let path = baseline_path.ok_or_else(|| format!("--baseline is required\n{USAGE}"))?;
        if let Some(p) = &prereg {
            settings = p.settings_for(&selection.pairs[0].arms, selection.scope.clone());
        }
        return write_baseline(&records, &settings, prereg.as_ref(), &path);
    }
    let baseline = match (&baseline_path, selection.needs_baseline) {
        (Some(path), _) => match load_baseline(path)? {
            Ok(b) => Some(b),
            Err(code) => return Ok(code),
        },
        (None, true) => return Err(format!("--baseline is required\n{USAGE}")),
        (None, false) => None,
    };
    judge_pairs(
        &records,
        baseline.as_ref(),
        &settings,
        prereg.as_ref(),
        &selection,
    )
}

/// Reads the baseline; `Err(code)` inside is a failed gate already printed.
fn load_baseline(path: &Path) -> Result<Result<Json, ExitCode>, String> {
    if !path.is_file() {
        println!(
            "MALFORMED baseline: {} does not exist. A missing baseline fails; record one \
             with --write-baseline after reading the table",
            path.display()
        );
        println!("gate: FAIL");
        return Ok(Err(ExitCode::from(1)));
    }
    Ok(match Json::parse(&read_file(path)?) {
        Ok(b) => Ok(b),
        Err(e) => Err(fail(&[Problem::new(
            Class::Malformed,
            format!("baseline {}: not JSON: {e}", path.display()),
        )])),
    })
}

/// One pair of arms to judge.
struct Judged {
    arms: Arms,
    /// `reported` pairs are printed with their verdict but don't decide the exit status.
    counted: bool,
    /// The registration lists specific rows for the pair.
    rows: Option<Vec<String>>,
    /// How the registration lists it, for the report.
    label: &'static str,
}

/// What `--arms`, `--rows`, `--eligible` and `--all-pairs` select.
struct Selection {
    pairs: Vec<Judged>,
    scope: Option<BTreeSet<String>>,
    /// Whether a pair is the registration's primary pair, which the baseline records.
    needs_baseline: bool,
    all_pairs: bool,
}

impl Selection {
    fn from_args(parsed: &Parsed, prereg: Option<&Prereg>) -> Result<Selection, String> {
        let flag = |name: &str| parsed.get(name).is_some();
        let write = flag("write-baseline");
        let mut scope = parsed.get("rows").map(|rows| {
            rows.split(',')
                .map(str::trim)
                .filter(|r| !r.is_empty())
                .map(str::to_string)
                .collect::<BTreeSet<_>>()
        });
        let arms = match parsed.get("arms") {
            Some(a) => {
                let (base, cand) = a
                    .split_once(',')
                    .ok_or("--arms takes BASE,CANDIDATE".to_string())?;
                Some(Arms {
                    base: base.trim().into(),
                    cand: cand.trim().into(),
                })
            }
            None => None,
        };
        let Some(p) = prereg else {
            for name in ["eligible", "all-pairs"] {
                if flag(name) {
                    return Err(format!(
                        "--{name} needs --prereg: it reads the registration\n{USAGE}"
                    ));
                }
            }
            let arms = arms.unwrap_or_default();
            return Ok(Selection {
                pairs: vec![Judged {
                    arms,
                    counted: true,
                    rows: None,
                    label: "",
                }],
                scope,
                needs_baseline: true,
                all_pairs: false,
            });
        };
        if flag("eligible") {
            if scope.is_some() {
                return Err("--eligible and --rows both choose the rows; use one".to_string());
            }
            let list = p.eligible.as_ref().ok_or_else(|| {
                format!("--eligible: registration {} has no `eligible` list", p.id)
            })?;
            scope = Some(list.iter().cloned().collect());
        }
        if flag("all-pairs") && arms.is_some() {
            return Err("--all-pairs and --arms both choose the pairs; use one".to_string());
        }
        if write && flag("all-pairs") {
            return Err(
                "--write-baseline records one pair; it can't be combined with --all-pairs"
                    .to_string(),
            );
        }
        let pairs: Vec<Judged> = if flag("all-pairs") {
            if p.pairs.is_empty() {
                return Err(format!(
                    "--all-pairs: registration {} lists no `pairs`",
                    p.id
                ));
            }
            p.pairs
                .iter()
                .map(|pair| Judged {
                    arms: pair.arms.clone(),
                    counted: pair.role != PairRole::Reported,
                    rows: pair.rows.clone(),
                    label: match pair.role {
                        PairRole::Verdict => "verdict",
                        PairRole::Reported => "reported, not counted",
                        PairRole::Listed => "listed",
                    },
                })
                .collect()
        } else {
            let arms = arms.unwrap_or_else(|| p.names.clone());
            if !p.has_arms(&arms) || arms.base == arms.cand {
                return Err(format!(
                    "--arms {},{}: both must be two different arms registered in {} (registered arms: {})",
                    arms.base,
                    arms.cand,
                    p.id,
                    p.arm_names()
                ));
            }
            let listed = p.pairs.iter().find(|x| x.arms == arms);
            vec![Judged {
                counted: true,
                rows: listed.and_then(|x| x.rows.clone()),
                label: "",
                arms,
            }]
        };
        if scope.is_none() {
            if let Some(j) = pairs.iter().find(|j| j.rows.is_some()) {
                return Err(format!(
                    "the registration lists rows for the pair {} -> {} ({}); name the rows to judge with --rows",
                    j.arms.base,
                    j.arms.cand,
                    j.rows.as_deref().unwrap_or_default().join(", ")
                ));
            }
        }
        let needs_baseline = pairs.iter().any(|j| j.arms == p.names);
        if !needs_baseline && parsed.get("baseline").is_some() {
            return Err(format!(
                "--baseline records the primary pair ({} -> {}) and can't be compared with the \
                 pair {} -> {}; leave it out",
                p.names.base, p.names.cand, pairs[0].arms.base, pairs[0].arms.cand
            ));
        }
        if write && !needs_baseline {
            return Err(format!(
                "--write-baseline records the primary pair ({} -> {}), not {} -> {}",
                p.names.base, p.names.cand, pairs[0].arms.base, pairs[0].arms.cand
            ));
        }
        Ok(Selection {
            pairs,
            scope,
            needs_baseline,
            all_pairs: flag("all-pairs"),
        })
    }
}

fn judge_pairs(
    records: &Json,
    baseline: Option<&Json>,
    settings: &Settings,
    prereg: Option<&Prereg>,
    selection: &Selection,
) -> Result<ExitCode, String> {
    let Some(p) = prereg else {
        let outcome: Outcome = check(
            records,
            baseline.expect("required without --prereg"),
            settings,
        );
        print!("{}", outcome.to_text());
        return Ok(exit_of(outcome.passed()));
    };
    let judge = |j: &Judged| {
        let settings = p.settings_for(&j.arms, selection.scope.clone());
        let base = baseline.filter(|_| j.arms == p.names);
        check_registered_for(records, base, p, &settings)
    };
    if !selection.all_pairs {
        let outcome = judge(&selection.pairs[0]);
        print!("{}", outcome.to_text());
        return Ok(exit_of(outcome.passed()));
    }
    let mut summary = Vec::new();
    let mut all_ok = true;
    for j in &selection.pairs {
        let outcome = judge(j);
        println!(
            "=== pair {} -> {} ({}) ===",
            j.arms.base, j.arms.cand, j.label
        );
        for line in &outcome.lines {
            println!("{line}");
        }
        for problem in &outcome.problems {
            println!("{problem}");
        }
        let verdict = if outcome.passed() { "PASS" } else { "FAIL" };
        println!("pair {} -> {}: {verdict}", j.arms.base, j.arms.cand);
        summary.push(format!(
            "  {} -> {} ({}): {verdict}",
            j.arms.base, j.arms.cand, j.label
        ));
        all_ok &= outcome.passed() || !j.counted;
    }
    println!("pairs:");
    for line in summary {
        println!("{line}");
    }
    println!("gate: {}", if all_ok { "PASS" } else { "FAIL" });
    Ok(exit_of(all_ok))
}

fn exit_of(passed: bool) -> ExitCode {
    if passed {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// What the rule options of the command line ask for.
enum RuleOptions {
    /// The registration (or the default paired rule) decides.
    Registered,
    /// `--rule totals` without a registration.
    Totals(TotalsRule),
    Refused(Vec<Problem>),
}

fn rule_options(parsed: &Parsed, prereg: Option<&Prereg>) -> Result<RuleOptions, String> {
    let rule = parsed.get("rule");
    if !matches!(rule, None | Some("paired") | Some("totals")) {
        return Err(format!("--rule takes paired or totals\n{USAGE}"));
    }
    let extras = ["accept", "noise", "speed"]
        .into_iter()
        .filter(|n| parsed.get(n).is_some())
        .collect::<Vec<_>>();
    if let Some(p) = prereg {
        let registered = if p.totals_registered {
            "totals"
        } else {
            "paired"
        };
        let mut problems = Vec::new();
        if rule.is_some_and(|r| r != registered) {
            problems.push(Problem::new(
                Class::Refused,
                format!(
                    "--rule {} but the registration {} states rule {registered}",
                    rule.unwrap_or(""),
                    p.id
                ),
            ));
        }
        for name in extras {
            problems.push(Problem::new(
                Class::Refused,
                format!("--{name} with --prereg: the registration states the criteria"),
            ));
        }
        return Ok(if problems.is_empty() {
            RuleOptions::Registered
        } else {
            RuleOptions::Refused(problems)
        });
    }
    if rule != Some("totals") {
        if let Some(name) = extras.first() {
            return Err(format!("--{name} goes with --rule totals\n{USAGE}"));
        }
        return Ok(RuleOptions::Registered);
    }
    let accept = match parsed.get("accept") {
        None => Accept::Improve,
        Some(a) => Accept::parse(a)
            .ok_or_else(|| format!("--accept takes improve or no-regress, got {a:?}\n{USAGE}"))?,
    };
    let mut totals = TotalsRule::new(accept);
    for spec in parsed.all("noise") {
        let (board, band) = spec
            .split_once('=')
            .ok_or_else(|| format!("--noise takes BOARD=BAND, got {spec:?}"))?;
        let band: f64 = band
            .parse()
            .ok()
            .filter(|b: &f64| *b >= 0.0 && b.is_finite())
            .ok_or_else(|| format!("--noise {spec:?}: the band must be a number >= 0"))?;
        totals.noise.insert(board.to_string(), band);
    }
    if let Some(spec) = parsed.get("speed") {
        let (metric, percent) = match spec.split_once('=') {
            Some((m, p)) => (
                m,
                p.parse::<f64>()
                    .map_err(|_| format!("--speed {spec:?}: the percentage must be a number"))?,
            ),
            None => (spec, MIN_SPEED_DROP_PERCENT),
        };
        if percent < MIN_SPEED_DROP_PERCENT || accept == Accept::NoRegress {
            return Ok(RuleOptions::Refused(vec![Problem::new(
                Class::Refused,
                format!(
                    "--speed needs --accept improve and a drop of at least \
                     {MIN_SPEED_DROP_PERCENT} %"
                ),
            )]));
        }
        totals.speed = Some(SpeedClause {
            metric: metric.to_string(),
            min_drop_percent: percent,
        });
    }
    Ok(RuleOptions::Totals(totals))
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

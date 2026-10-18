//! Preregistered A/B runs (F03.4): `freerouting-dev gate --prereg <file>`.
//!
//! Before an A/B, its arms, boards, metrics and pass rule go in a JSON file
//! (`tools/ab/TEMPLATE.json`), committed first. The run is then checked
//! against what was registered, so a result can't choose its own criteria
//! afterwards. This is KiCadRoutingTools' `*_prereg.json` practice: "arms,
//! cells, signals, guards and thresholds are not edited after a number
//! exists". Changes go in the `amendments` list with the reason.
//!
//! ```json
//! { "format": "freerouting-ab-prereg-1", "id": "...", "registered": "2026-10-16",
//!   "question": "...",
//!   "arms": { "off": {"engine": "...", "settings": {}}, "on": {"engine": "..."} },
//!   "baseline_arm": "off", "candidate_arm": "on",
//!   "boards": { "set": "tools/bench/sets/x.txt", "files": { "<row>": "<sha256 of the input>" } },
//!   "metrics": [ {"name": "via_in_pad", "role": "hard"}, {"name": "unrouted"} ],
//!   "pass_rule": { "min_boards": 3, "improve_all_but": 1, "max_regress": 0 },
//!   "amendments": [] }
//! ```
//!
//! `pass_rule` is the paired rule (`"rule": "paired"`, the default). A
//! registration for a change to profile `enhanced` says `"rule": "totals"`
//! and adds a `totals` block (`accept`, `signal`, `noise`, `min_boards`,
//! `speed`), see [`crate::gate_totals`] and `tools/ab/README.md` (N02). A
//! `"rule": "totals"` without that block parses (Q14's hand-judged
//! registration) but a run against it is refused: the gate does not guess the
//! criteria.
//!
//! A run is **refused** (class `REFUSED`) when
//! - its arms differ from the registered ones by name, engine or settings;
//! - a board is not registered, is missing, or its input hash changed since
//!   registration (the board is named);
//! - (not a refusal, but `MISSING`:) a registered metric the run doesn't list
//!   or doesn't carry in a row. Metrics the run measured beyond the
//!   registered ones are not judged;
//! - the registration is weaker than the global rule (fewer than 3 boards,
//!   more than N-1 non-improving boards, any regression allowed), has no
//!   `via_in_pad` hard metric, or downgrades a hard limit.
//!
//! The report prints the sha256 of the registration file; a test pins that
//! of the worked example (`tools/ab/examples/`).

use std::collections::BTreeMap;

use freerouting::quality::feasibility::report::sha256_hex;

use crate::gate::{
    Arms, Class, Direction, Kind, MIN_BOARDS, MetricSpec, Outcome, PassRule, Problem, Role,
    Settings, check, default_metrics, records_rows,
};
use crate::gate_totals::TotalsRule;
use crate::json_read::Json;

/// The format tag of a registration.
pub const PREREG_FORMAT: &str = "freerouting-ab-prereg-1";

/// A parsed registration.
#[derive(Clone, Debug)]
pub struct Prereg {
    pub id: String,
    /// The sha256 of the registration file's bytes.
    pub sha256: String,
    pub arms: BTreeMap<String, Json>,
    pub names: Arms,
    /// Registered input hash per row.
    pub boards: BTreeMap<String, String>,
    pub set: Option<String>,
    pub specs: Vec<MetricSpec>,
    pub rule: PassRule,
    /// `"rule": "totals"` was registered (the paired `rule` is then reported only).
    pub totals_registered: bool,
    /// The `totals` block, when there is one.
    pub totals: Option<TotalsRule>,
    pub amendments: usize,
}

fn malformed(text: impl Into<String>) -> Problem {
    Problem::new(Class::Malformed, format!("prereg: {}", text.into()))
}

fn refused(text: impl Into<String>) -> Problem {
    Problem::new(Class::Refused, format!("prereg: {}", text.into()))
}

fn string_of<'a>(v: &'a Json, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Json::as_str)
}

fn parse_spec(v: &Json, index: usize) -> Result<MetricSpec, Problem> {
    let name = string_of(v, "name")
        .ok_or_else(|| malformed(format!("metrics[{index}] has no `name`")))?
        .to_string();
    let word = |key: &str, default: &str| string_of(v, key).unwrap_or(default).to_string();
    let direction = match word("direction", "lower").as_str() {
        "lower" => Direction::Lower,
        "higher" => Direction::Higher,
        other => return Err(malformed(format!("metric {name}: direction {other:?}"))),
    };
    let kind = match word("kind", "int").as_str() {
        "int" => Kind::Int,
        "float" => Kind::Float,
        "counts" => Kind::Counts,
        other => return Err(malformed(format!("metric {name}: kind {other:?}"))),
    };
    let role = match word("role", "score").as_str() {
        "hard" => Role::Hard,
        "guard" => Role::Guard,
        "score" => Role::Score,
        "report" => Role::Report,
        other => return Err(malformed(format!("metric {name}: role {other:?}"))),
    };
    let tol = match v.get("tol") {
        None => {
            if kind == Kind::Float {
                crate::gate::FLOAT_TOLERANCE
            } else {
                0.0
            }
        }
        Some(Json::Number(t)) if *t >= 0.0 => *t,
        Some(_) => return Err(malformed(format!("metric {name}: tol must be >= 0"))),
    };
    let compared = !matches!(v.get("compared"), Some(Json::Bool(false)));
    Ok(MetricSpec {
        name,
        direction,
        kind,
        role,
        tol,
        compared,
    })
}

impl Prereg {
    /// Parses and validates a registration file's text.
    pub fn parse(text: &str) -> Result<Prereg, Vec<Problem>> {
        let json = Json::parse(text).map_err(|e| vec![malformed(e)])?;
        let mut problems = Vec::new();
        if !matches!(json, Json::Object(_)) {
            return Err(vec![malformed("expected an object")]);
        }
        if string_of(&json, "format") != Some(PREREG_FORMAT) {
            problems.push(malformed(format!("format must be {PREREG_FORMAT:?}")));
        }
        let id = string_of(&json, "id").unwrap_or("").to_string();
        if id.is_empty() {
            problems.push(malformed("no `id`"));
        }
        let arms: BTreeMap<String, Json> = match json.get("arms") {
            Some(Json::Object(a)) if a.len() >= 2 => a.clone(),
            _ => {
                problems.push(malformed("`arms` must be an object with at least two arms"));
                BTreeMap::new()
            }
        };
        let names = Arms {
            base: string_of(&json, "baseline_arm")
                .unwrap_or("off")
                .to_string(),
            cand: string_of(&json, "candidate_arm")
                .unwrap_or("on")
                .to_string(),
        };
        for arm in [&names.base, &names.cand] {
            if !arms.contains_key(arm) {
                problems.push(malformed(format!(
                    "arm {arm:?} is not registered under `arms`"
                )));
            }
        }
        let mut boards = BTreeMap::new();
        let mut set = None;
        match json.get("boards") {
            Some(b @ Json::Object(_)) => {
                set = string_of(b, "set").map(str::to_string);
                match b.get("files") {
                    Some(Json::Object(files)) if !files.is_empty() => {
                        for (row, hash) in files {
                            match hash.as_str() {
                                Some(h)
                                    if h.len() == 64
                                        && h.bytes().all(|c| c.is_ascii_hexdigit()) =>
                                {
                                    boards.insert(row.clone(), h.to_ascii_lowercase());
                                }
                                _ => problems.push(malformed(format!(
                                    "board {row}: the hash must be 64 hex digits"
                                ))),
                            }
                        }
                    }
                    _ => problems.push(malformed("`boards.files` must map rows to sha256")),
                }
            }
            _ => problems.push(malformed("no `boards` object")),
        }
        let mut specs = Vec::new();
        match json.get("metrics") {
            Some(Json::Array(list)) if !list.is_empty() => {
                for (i, m) in list.iter().enumerate() {
                    match parse_spec(m, i) {
                        Ok(spec) => specs.push(spec),
                        Err(p) => problems.push(p),
                    }
                }
            }
            _ => problems.push(malformed("`metrics` must be a non-empty array")),
        }
        let mut rule = PassRule::default();
        if let Some(r) = json.get("pass_rule") {
            let get = |key: &str, default: usize| match r.get(key) {
                Some(Json::Number(n)) if *n >= 0.0 && n.fract() == 0.0 => Ok(*n as usize),
                None => Ok(default),
                Some(_) => Err(malformed(format!("pass_rule.{key} must be a count"))),
            };
            for (key, field) in [
                ("min_boards", &mut rule.min_boards),
                ("improve_all_but", &mut rule.improve_all_but),
                ("max_regress", &mut rule.max_regress),
            ] {
                match get(key, *field) {
                    Ok(v) => *field = v,
                    Err(p) => problems.push(p),
                }
            }
        }
        problems.extend(weaker_than_global(&rule, &specs));
        let totals_registered = match json.get("rule") {
            None => false,
            Some(Json::String(r)) if r == "paired" => false,
            Some(Json::String(r)) if r == "totals" => true,
            Some(other) => {
                problems.push(malformed(format!(
                    "`rule` must be \"paired\" or \"totals\", got {}",
                    crate::gate::show(Some(other))
                )));
                false
            }
        };
        let mut totals = None;
        match json.get("totals") {
            None => {}
            Some(_) if !totals_registered => problems.push(malformed(
                "a `totals` block needs `\"rule\": \"totals\"`".to_string(),
            )),
            Some(block) => match TotalsRule::parse(block, &specs) {
                Ok(t) => totals = Some(t),
                Err(more) => problems.extend(more),
            },
        }
        let amendments = match json.get("amendments") {
            Some(Json::Array(a)) => a.len(),
            None => 0,
            Some(_) => {
                problems.push(malformed("`amendments` must be an array"));
                0
            }
        };
        if !problems.is_empty() {
            return Err(problems);
        }
        Ok(Prereg {
            id,
            sha256: sha256_hex(text.as_bytes()),
            arms,
            names,
            boards,
            set,
            specs,
            rule,
            totals_registered,
            totals,
            amendments,
        })
    }

    /// The settings a registered run is judged by.
    pub fn settings(&self) -> Settings {
        Settings {
            arms: self.names.clone(),
            rule: self.rule,
            scope: None,
            specs: self.specs.clone(),
            totals: self.totals.clone(),
        }
    }
}

/// The registration may tighten the global rule, never loosen it.
fn weaker_than_global(rule: &PassRule, specs: &[MetricSpec]) -> Vec<Problem> {
    let mut out = Vec::new();
    if rule.min_boards < MIN_BOARDS {
        out.push(refused(format!(
            "pass_rule.min_boards is {}, the global rule needs >= {MIN_BOARDS} distinct boards",
            rule.min_boards
        )));
    }
    if rule.improve_all_but > 1 {
        out.push(refused(
            "pass_rule.improve_all_but is above 1: a change must improve on at least N-1 boards"
                .to_string(),
        ));
    }
    if rule.max_regress > 0 {
        out.push(refused(
            "pass_rule.max_regress is above 0: a change may regress on no board".to_string(),
        ));
    }
    for hard in default_metrics().iter().filter(|m| m.role == Role::Hard) {
        match specs.iter().find(|s| s.name == hard.name) {
            Some(s) if s.role != Role::Hard => out.push(refused(format!(
                "metric {} is a hard limit (D8) and may not be registered as another role",
                hard.name
            ))),
            None if hard.name == "via_in_pad" => out.push(refused(
                "metric via_in_pad (role hard) is required: a via in a pad is a failure, not a \
                 quality note"
                    .to_string(),
            )),
            _ => {}
        }
    }
    out
}

/// What differs between the registration and the run's own description.
pub fn check_registration(prereg: &Prereg, records: &Json) -> Vec<Problem> {
    let mut out = Vec::new();
    let mut ignored = Vec::new();
    // Arms: names, engine, settings.
    match records.get("arms") {
        Some(Json::Object(run_arms)) => {
            for (name, registered) in &prereg.arms {
                match run_arms.get(name) {
                    None => out.push(refused(format!("arm {name} is registered but not run"))),
                    Some(run) if run != registered => out.push(refused(format!(
                        "arm {name} differs from the registration: registered {}, run {}",
                        crate::gate::show(Some(registered)),
                        crate::gate::show(Some(run))
                    ))),
                    Some(_) => {}
                }
            }
            for name in run_arms.keys().filter(|n| !prereg.arms.contains_key(*n)) {
                out.push(refused(format!("arm {name} was run but is not registered")));
            }
        }
        _ => out.push(refused(
            "the run does not describe its arms (`arms`), so they can't be compared with the \
             registration"
                .to_string(),
        )),
    }
    for (key, registered) in [
        ("baseline_arm", &prereg.names.base),
        ("candidate_arm", &prereg.names.cand),
    ] {
        if let Some(run) = records.get(key).and_then(Json::as_str) {
            if run != registered {
                out.push(refused(format!(
                    "{key} is {run:?}, registered {registered:?}"
                )));
            }
        }
    }
    // Boards: each row's input hash.
    let rows = records_rows(records, &mut ignored);
    for row in &rows {
        match (prereg.boards.get(row.name), row.input_sha256()) {
            (None, _) => out.push(refused(format!(
                "board {} was run but is not registered",
                row.name
            ))),
            (Some(_), None) => out.push(refused(format!(
                "board {}: the run records no input hash, so it can't be compared with the \
                 registration",
                row.name
            ))),
            (Some(registered), Some(run)) if !registered.eq_ignore_ascii_case(run) => {
                out.push(refused(format!(
                    "board {} changed between registration and run: registered sha256 {}, run \
                     {}",
                    row.name,
                    &registered[..12],
                    &run[..run.len().min(12)]
                )))
            }
            _ => {}
        }
    }
    for registered in prereg.boards.keys() {
        if !rows.iter().any(|r| r.name == registered) {
            out.push(refused(format!(
                "board {registered} is registered but missing from the run"
            )));
        }
    }
    // The totals rule: its block must exist, and its noise must name run boards.
    if prereg.totals_registered {
        match &prereg.totals {
            None => out.push(refused(
                "`rule` is \"totals\" but the registration has no `totals` block (accept, \
                 noise, signal); the gate does not guess the criteria"
                    .to_string(),
            )),
            Some(t) => {
                for board in t.noise.keys() {
                    if !rows.iter().any(|r| r.board == board || r.name == board) {
                        out.push(refused(format!(
                            "totals.noise names board {board}, which no row of the run has"
                        )));
                    }
                }
            }
        }
    }
    // Metrics: the run's own list, when it has one.
    if let Some(Json::Array(run)) = records.get("metrics") {
        let run: Vec<&str> = run.iter().filter_map(Json::as_str).collect();
        for spec in &prereg.specs {
            if !run.contains(&spec.name.as_str()) {
                out.push(Problem::new(
                    Class::Missing,
                    format!(
                        "metric {}: registered, but the run does not list it",
                        spec.name
                    ),
                ));
            }
        }
    }
    out
}

/// Checks a run against a baseline under a registration: the registration's
/// refusals, then the ordinary check with the registered metrics, arms and
/// pass rule.
pub fn check_registered(records: &Json, baseline: &Json, prereg: &Prereg) -> Outcome {
    let refusals = check_registration(prereg, records);
    let mut outcome = check(records, baseline, &prereg.settings());
    let mut lines = vec![format!(
        "prereg {} (sha256 {}, {} amendment(s)): {} refusal(s)",
        prereg.id,
        prereg.sha256,
        prereg.amendments,
        refusals.len()
    )];
    lines.append(&mut outcome.lines);
    outcome.lines = lines;
    outcome.problems.splice(0..0, refusals);
    outcome
}

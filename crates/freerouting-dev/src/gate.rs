//! `freerouting-dev gate`: the baseline comparator and the pass rule of the
//! measurement gate (F03.1).
//!
//! Quality changes (Q series), alternative algorithms (X series) and P04 are
//! kept only if measurements say so (`docs/roadmap/routing-quality.md`).
//! This module is the program that decides, so the rule can't drift from the
//! docs. It follows KiCadRoutingTools' `tests/test_placement_ab.py`:
//! named problem classes, a self-test on every invocation, a baseline file.
//!
//! # Input
//!
//! A *records* file (`freerouting-gate-records-1`), one row per board (or
//! board variant) with the measured values of each arm, written by
//! `gate collect` ([`crate::gate_collect`]) from `bench.json` and the KiCad
//! oracle's `summary.json`, or by hand:
//!
//! ```json
//! { "format": "freerouting-gate-records-1",
//!   "arms": { "off": {"engine": "..."}, "on": {"engine": "..."} },
//!   "metrics": ["unrouted", "vias"],          // optional: what this run measured
//!   "declared": ["rowA", "rowB"],             // optional: every row the harness declares
//!   "partial": false,
//!   "rows": { "rowA": { "board": "boardA", "input_sha256": "...",
//!                       "arms": { "off": {"unrouted": 12}, "on": {"unrouted": 9} } } } }
//! ```
//!
//! and a *baseline* (`freerouting-gate-baseline-1`): the same rows with the
//! computed mark of each, written by `--write-baseline`.
//!
//! # Metrics and the mark of a row
//!
//! Every metric is lower-is-better unless a preregistration says otherwise.
//! The default table is in [`default_metrics`], in the order of importance of
//! routing-quality.md: vias in pads and copper below the board minimums
//! (hard limits: any in the candidate arm is a failure, D8), then clearance
//! and DRC items (none new, ever), unrouted connections, vias, length, bends.
//! Time is reported, not judged, and so is the tidiness of the routes (Q15.2:
//! `tidy_<measure>_median` and `_p90` for detour, bends per 10 mm, acute
//! corners, jogs and vias per connection, [`crate::tidy`]); neither is kept
//! in a baseline.
//!
//! A row's mark compares the candidate arm with the baseline arm: `regress`
//! if a hard metric is above zero or a guard got worse, else the first metric
//! (in table order) that differs decides `improve` or `regress`, else
//! `neutral`. A row without both arms is `skip`.
//!
//! # Problem classes
//!
//! Every class is fatal; they are kept apart because they mean different
//! things:
//!
//! - `INVERTED`: a direction or a mark reversed, checked with no tolerance;
//! - `DRIFT`: a value moved without reversing: integers compare exactly
//!   (D2 makes them exact), floats at a relative tolerance;
//! - `ORPHAN`: the baseline has a row the run doesn't declare any more. Never
//!   scoped away: deleting the row that disagrees deletes its evidence;
//! - `MISSING`: a declared row, arm or registered metric the run didn't
//!   measure;
//! - `NEW ROW`: a row the baseline has never seen;
//! - `MALFORMED`: the baseline (or records) isn't shaped as specified.
//!   Reported, never raised: a panic after a long table is not a verdict;
//! - `UNGRADED`: the oracle couldn't grade a row ("not graded" is never a
//!   pass), or a hard metric is null;
//! - `HARD`: a hard limit is exceeded in the candidate arm;
//! - `REFUSED`: fewer distinct boards than the rule needs, or a
//!   preregistration mismatch ([`crate::gate_prereg`]).
//!
//! # The pass rules
//!
//! The **paired rule**, the default and the rule for opt-in stages: at least 3
//! **distinct boards** (rows of one board count once), improving on at least
//! N-1 and regressing on none. Neutral boards are printed, and so is the luck
//! rate: a change whose per-board direction is a coin flip passes the rule 1
//! run in 2^N.
//!
//! The **totals rule** (`--rule totals`, `"rule": "totals"` in a
//! registration; N02, D10), the rule for changes to profile `enhanced`: the
//! sum of the signal over the boards must beat the set's noise, and no board
//! may be worse than twice its own band. See [`crate::gate_totals`].

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use freerouting::drc::json::JsonValue;

use crate::json_read::Json;

pub use crate::gate_baseline::{baseline_from, baseline_rows, compare_baseline};
use crate::gate_totals::{TotalsRule, judge};

/// The format tag of a records file.
pub const RECORDS_FORMAT: &str = "freerouting-gate-records-1";
/// The format tag of a baseline file.
pub const BASELINE_FORMAT: &str = "freerouting-gate-baseline-1";
/// Per-arm string tags compared exactly (never compare two KiCad versions).
pub const TAG_KEYS: [&str; 2] = ["kicad_version", "engine_commit"];
/// The default relative tolerance of float metrics.
pub const FLOAT_TOLERANCE: f64 = 1e-6;
/// The fewest distinct boards a verdict may rest on.
pub const MIN_BOARDS: usize = 3;

/// Which way is better.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Lower,
    Higher,
}

/// What a metric's value looks like.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Int,
    Float,
    /// An object of integer counts (DRC items by type); drift only.
    Counts,
}

/// How a metric enters the verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Role {
    /// A hard limit: above zero in the candidate arm fails the gate.
    Hard,
    /// Must not get worse than the baseline arm.
    Guard,
    /// Decides the mark by table order.
    Score,
    /// Reported (and compared with the baseline) but not judged.
    Report,
}

/// One compared metric.
#[derive(Clone, Debug, PartialEq)]
pub struct MetricSpec {
    pub name: String,
    pub direction: Direction,
    pub kind: Kind,
    pub role: Role,
    /// Relative tolerance of a float value (0 for integers).
    pub tol: f64,
    /// Whether the baseline records and compares the value.
    pub compared: bool,
}

impl MetricSpec {
    fn new(name: &str, kind: Kind, role: Role) -> MetricSpec {
        MetricSpec {
            name: name.to_string(),
            direction: Direction::Lower,
            kind,
            role,
            tol: if kind == Kind::Float {
                FLOAT_TOLERANCE
            } else {
                0.0
            },
            compared: true,
        }
    }
}

/// The default metric table, in order of importance.
pub fn default_metrics() -> Vec<MetricSpec> {
    let mut wall = MetricSpec::new("wall_seconds", Kind::Float, Role::Report);
    wall.compared = false;
    let mut specs = vec![
        MetricSpec::new("via_in_pad", Kind::Int, Role::Hard),
        MetricSpec::new("below_minimum", Kind::Int, Role::Hard),
        MetricSpec::new("drc_real", Kind::Int, Role::Guard),
        MetricSpec::new("clearance_violations", Kind::Int, Role::Guard),
        MetricSpec::new("unrouted", Kind::Int, Role::Score),
        MetricSpec::new("vias", Kind::Int, Role::Score),
        MetricSpec::new("length_mm", Kind::Float, Role::Score),
        MetricSpec::new("bends", Kind::Int, Role::Score),
        MetricSpec::new("drc_real_by_type", Kind::Counts, Role::Report),
        wall,
    ];
    // Q15.2: the tidiness of the routes is printed, not judged and not
    // recorded in a baseline.
    for (_, _, name) in crate::tidy::GATE_METRICS {
        let mut spec = MetricSpec::new(name, Kind::Float, Role::Report);
        spec.compared = false;
        specs.push(spec);
    }
    specs
}

/// The class of a problem.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Inverted,
    Drift,
    Orphan,
    Missing,
    NewRow,
    Malformed,
    Ungraded,
    Hard,
    Refused,
}

impl Class {
    /// The word printed in front of a problem.
    pub fn word(self) -> &'static str {
        match self {
            Class::Inverted => "INVERTED",
            Class::Drift => "DRIFT",
            Class::Orphan => "ORPHAN",
            Class::Missing => "MISSING",
            Class::NewRow => "NEW ROW",
            Class::Malformed => "MALFORMED",
            Class::Ungraded => "UNGRADED",
            Class::Hard => "HARD",
            Class::Refused => "REFUSED",
        }
    }
}

/// One thing wrong with a run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Problem {
    pub class: Class,
    pub text: String,
}

impl Problem {
    pub fn new(class: Class, text: impl Into<String>) -> Problem {
        Problem {
            class,
            text: text.into(),
        }
    }
}

impl fmt::Display for Problem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}", self.class.word(), self.text)
    }
}

/// The verdict on one row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mark {
    Improve,
    Regress,
    Neutral,
    Skip,
}

impl Mark {
    pub fn name(self) -> &'static str {
        match self {
            Mark::Improve => "improve",
            Mark::Regress => "regress",
            Mark::Neutral => "neutral",
            Mark::Skip => "skip",
        }
    }

    pub fn parse(text: &str) -> Option<Mark> {
        [Mark::Improve, Mark::Regress, Mark::Neutral, Mark::Skip]
            .into_iter()
            .find(|m| m.name() == text)
    }
}

/// The pass rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PassRule {
    /// The fewest distinct boards.
    pub min_boards: usize,
    /// How many boards may fail to improve (N-1: 1).
    pub improve_all_but: usize,
    /// How many boards may regress.
    pub max_regress: usize,
}

impl Default for PassRule {
    fn default() -> Self {
        PassRule {
            min_boards: MIN_BOARDS,
            improve_all_but: 1,
            max_regress: 0,
        }
    }
}

/// The names of the baseline arm and the candidate arm.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Arms {
    pub base: String,
    pub cand: String,
}

impl Default for Arms {
    fn default() -> Self {
        Arms {
            base: "off".into(),
            cand: "on".into(),
        }
    }
}

/// What a run is checked against.
#[derive(Clone, Debug, Default)]
pub struct Settings {
    pub arms: Arms,
    pub rule: PassRule,
    /// Row names this run was asked to run (`--rows`); `None`: all.
    pub scope: Option<BTreeSet<String>>,
    /// The metrics to judge; empty: the default table, narrowed to what the
    /// records contain.
    pub specs: Vec<MetricSpec>,
    /// `Some`: judge by the totals rule instead of the paired `rule`.
    pub totals: Option<TotalsRule>,
}

/// The result of a check.
#[derive(Clone, Debug, Default)]
pub struct Outcome {
    pub problems: Vec<Problem>,
    /// The report: marks, boards, the trial verdict.
    pub lines: Vec<String>,
    /// Whether the pass rule holds (problems are separate).
    pub rule_ok: bool,
}

impl Outcome {
    /// Whether the gate passes: no problem and the pass rule holds.
    pub fn passed(&self) -> bool {
        self.problems.is_empty() && self.rule_ok
    }

    /// The report for people: the lines, then the problems.
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            out.push_str(line);
            out.push('\n');
        }
        for problem in &self.problems {
            out.push_str(&format!("{problem}\n"));
        }
        out.push_str(if self.passed() {
            "gate: PASS\n"
        } else {
            "gate: FAIL\n"
        });
        out
    }
}

// --- JSON helpers -----------------------------------------------------------

pub(crate) fn type_name(v: &Json) -> &'static str {
    match v {
        Json::Null => "null",
        Json::Bool(_) => "a boolean",
        Json::Number(_) => "a number",
        Json::String(_) => "a string",
        Json::Array(_) => "an array",
        Json::Object(_) => "an object",
    }
}

/// A value as printed in a problem.
pub fn show(v: Option<&Json>) -> String {
    match v {
        None => "(absent)".into(),
        Some(Json::Null) => "null".into(),
        Some(Json::Number(n)) => format!("{n}"),
        Some(Json::String(s)) => format!("{s:?}"),
        Some(Json::Bool(b)) => b.to_string(),
        Some(Json::Array(_)) => "[..]".into(),
        Some(Json::Object(o)) => format!(
            "{{{}}}",
            o.iter()
                .map(|(k, v)| format!("{k}: {}", show(Some(v))))
                .collect::<Vec<_>>()
                .join(", ")
        ),
    }
}

pub(crate) fn as_object(v: &Json) -> Option<&BTreeMap<String, Json>> {
    match v {
        Json::Object(o) => Some(o),
        _ => None,
    }
}

pub(crate) fn number(v: Option<&Json>) -> Option<f64> {
    match v {
        Some(Json::Number(n)) => Some(*n),
        _ => None,
    }
}

/// Converts a parsed value to the writer's type (sorted keys; whole numbers
/// as integers).
pub fn to_writer(v: &Json) -> JsonValue {
    match v {
        Json::Null => JsonValue::Null,
        Json::Bool(b) => JsonValue::Bool(*b),
        Json::Number(n) if n.fract() == 0.0 && n.abs() < 1e15 => JsonValue::int(*n as i64),
        Json::Number(n) => JsonValue::Number(format!("{n}")),
        Json::String(s) => JsonValue::string(s.clone()),
        Json::Array(a) => JsonValue::Array(a.iter().map(to_writer).collect()),
        Json::Object(o) => {
            JsonValue::Object(o.iter().map(|(k, v)| (k.clone(), to_writer(v))).collect())
        }
    }
}

// --- records ----------------------------------------------------------------

/// A row of a records file, validated.
#[derive(Clone, Debug)]
pub struct Row<'a> {
    pub name: &'a str,
    pub board: &'a str,
    pub rejected: bool,
    pub body: &'a BTreeMap<String, Json>,
}

impl<'a> Row<'a> {
    pub(crate) fn arm(&self, name: &str) -> Option<&'a BTreeMap<String, Json>> {
        let arms = as_object(self.body.get("arms")?)?;
        as_object(arms.get(name)?)
    }

    pub fn input_sha256(&self) -> Option<&'a str> {
        match self.body.get("input_sha256") {
            Some(Json::String(s)) => Some(s),
            _ => None,
        }
    }
}

/// The rows of a records file; a malformed file or row is a problem.
pub fn records_rows<'a>(records: &'a Json, problems: &mut Vec<Problem>) -> Vec<Row<'a>> {
    let Some(top) = as_object(records) else {
        problems.push(Problem::new(
            Class::Malformed,
            format!(
                "records: expected an object of rows, got {}",
                type_name(records)
            ),
        ));
        return Vec::new();
    };
    let Some(Json::Object(rows)) = top.get("rows") else {
        problems.push(Problem::new(
            Class::Malformed,
            "records: no `rows` object".to_string(),
        ));
        return Vec::new();
    };
    let mut out = Vec::new();
    for (name, row) in rows {
        let Some(body) = as_object(row) else {
            problems.push(Problem::new(
                Class::Malformed,
                format!(
                    "records row {name}: expected an object, got {}",
                    type_name(row)
                ),
            ));
            continue;
        };
        let board = match body.get("board") {
            Some(Json::String(b)) => b.as_str(),
            _ => name.as_str(),
        };
        out.push(Row {
            name,
            board,
            rejected: matches!(body.get("rejected"), Some(Json::Bool(true))),
            body,
        });
    }
    out
}

/// The metrics to judge: `settings.specs`, else the default table narrowed
/// to the names in `records.metrics`, else to those present in some arm.
pub fn active_specs(records: &Json, settings: &Settings) -> Vec<MetricSpec> {
    if !settings.specs.is_empty() {
        return settings.specs.clone();
    }
    let declared: Option<BTreeSet<&str>> = match records.get("metrics") {
        Some(Json::Array(names)) => Some(
            names
                .iter()
                .filter_map(|n| n.as_str())
                .collect::<BTreeSet<_>>(),
        ),
        _ => None,
    };
    let mut present: BTreeSet<String> = BTreeSet::new();
    if let Some(Json::Object(rows)) = records.get("rows") {
        for row in rows.values() {
            if let Some(Json::Object(arms)) = row.get("arms") {
                for arm in arms.values() {
                    if let Json::Object(values) = arm {
                        present.extend(values.keys().cloned());
                    }
                }
            }
        }
    }
    default_metrics()
        .into_iter()
        .filter(|s| match &declared {
            Some(d) => d.contains(s.name.as_str()),
            None => present.contains(&s.name),
        })
        .collect()
}

/// A metric value of an arm.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Value {
    Absent,
    Null,
    Num(f64),
    /// An object of counts (only for [`Kind::Counts`]).
    Counts,
    Bad,
}

fn value_of(arm: &BTreeMap<String, Json>, spec: &MetricSpec) -> Value {
    match arm.get(&spec.name) {
        None => Value::Absent,
        Some(Json::Null) => Value::Null,
        Some(Json::Number(n)) if spec.kind != Kind::Counts => Value::Num(*n),
        Some(Json::Object(_)) if spec.kind == Kind::Counts => Value::Counts,
        Some(_) => Value::Bad,
    }
}

fn worse(spec: &MetricSpec, base: f64, cand: f64) -> bool {
    let band = spec.tol * base.abs().max(1.0);
    match spec.direction {
        Direction::Lower => cand - base > band,
        Direction::Higher => base - cand > band,
    }
}

fn better(spec: &MetricSpec, base: f64, cand: f64) -> bool {
    let band = spec.tol * base.abs().max(1.0);
    match spec.direction {
        Direction::Lower => base - cand > band,
        Direction::Higher => cand - base > band,
    }
}

/// The mark of a row (see the module docs), and the hard limits it breaks
/// as `(metric, value)`.
pub fn row_mark(row: &Row<'_>, specs: &[MetricSpec], arms: &Arms) -> (Mark, Vec<(String, f64)>) {
    let (Some(base), Some(cand)) = (row.arm(&arms.base), row.arm(&arms.cand)) else {
        return (Mark::Skip, Vec::new());
    };
    let mut hard = Vec::new();
    for spec in specs.iter().filter(|s| s.role == Role::Hard) {
        if let Value::Num(v) = value_of(cand, spec) {
            if v > 0.0 {
                hard.push((spec.name.clone(), v));
            }
        }
    }
    if !hard.is_empty() {
        return (Mark::Regress, hard);
    }
    let pair = |spec: &MetricSpec| match (value_of(base, spec), value_of(cand, spec)) {
        (Value::Num(b), Value::Num(c)) => Some((b, c)),
        _ => None,
    };
    for spec in specs.iter().filter(|s| s.role == Role::Guard) {
        if let Some((b, c)) = pair(spec) {
            if worse(spec, b, c) {
                return (Mark::Regress, hard);
            }
        }
    }
    for spec in specs
        .iter()
        .filter(|s| matches!(s.role, Role::Hard | Role::Guard | Role::Score))
    {
        if let Some((b, c)) = pair(spec) {
            if worse(spec, b, c) {
                return (Mark::Regress, hard);
            }
            if better(spec, b, c) {
                return (Mark::Improve, hard);
            }
        }
    }
    (Mark::Neutral, hard)
}

/// The hard guards a row breaks in the candidate arm, as text: a hard metric
/// above zero, a guard metric worse than the baseline arm. The totals rule
/// fails on any of them whatever the signal does.
pub fn guard_failures(row: &Row<'_>, specs: &[MetricSpec], arms: &Arms) -> Vec<String> {
    let (Some(base), Some(cand)) = (row.arm(&arms.base), row.arm(&arms.cand)) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for spec in specs {
        match (spec.role, value_of(base, spec), value_of(cand, spec)) {
            (Role::Hard, _, Value::Num(c)) if c > 0.0 => {
                out.push(format!("{}.{} = {c}", row.name, spec.name));
            }
            (Role::Guard, Value::Num(b), Value::Num(c)) if worse(spec, b, c) => {
                out.push(format!("{}.{} {b} -> {c}", row.name, spec.name));
            }
            _ => {}
        }
    }
    out
}

/// The marks of all rows, by row name.
pub fn marks(rows: &[Row<'_>], specs: &[MetricSpec], arms: &Arms) -> BTreeMap<String, Mark> {
    rows.iter()
        .map(|r| (r.name.to_string(), row_mark(r, specs, arms).0))
        .collect()
}

// --- presence ---------------------------------------------------------------

/// MISSING arms and registered metrics, UNGRADED rows and null hard metrics,
/// HARD failures.
pub fn check_rows(
    rows: &[Row<'_>],
    specs: &[MetricSpec],
    arms: &Arms,
    problems: &mut Vec<Problem>,
) {
    for row in rows {
        for arm_name in [&arms.base, &arms.cand] {
            let Some(arm) = row.arm(arm_name) else {
                problems.push(Problem::new(
                    Class::Missing,
                    format!("{}.{arm_name}: the row has no such arm", row.name),
                ));
                continue;
            };
            if matches!(arm.get("graded"), Some(Json::Bool(false))) {
                let reason = match arm.get("reason") {
                    Some(Json::String(r)) => format!(": {r}"),
                    _ => String::new(),
                };
                problems.push(Problem::new(
                    Class::Ungraded,
                    format!(
                        "{}.{arm_name}: not graded{reason}. A board that can't be graded \
                         fails the run",
                        row.name
                    ),
                ));
                continue;
            }
            for spec in specs {
                match value_of(arm, spec) {
                    Value::Absent => problems.push(Problem::new(
                        Class::Missing,
                        format!(
                            "{}.{arm_name}.{}: a registered metric the run did not measure",
                            row.name, spec.name
                        ),
                    )),
                    Value::Null if spec.role == Role::Hard && arm_name == &arms.cand => problems
                        .push(Problem::new(
                            Class::Ungraded,
                            format!(
                                "{}.{arm_name}.{}: null, but a hard limit must be measured",
                                row.name, spec.name
                            ),
                        )),
                    Value::Bad => problems.push(Problem::new(
                        Class::Malformed,
                        format!(
                            "{}.{arm_name}.{}: expected {}, got {}",
                            row.name,
                            spec.name,
                            if spec.kind == Kind::Counts {
                                "an object"
                            } else {
                                "a number"
                            },
                            show(arm.get(&spec.name))
                        ),
                    )),
                    _ => {}
                }
            }
        }
        for (metric, value) in row_mark(row, specs, arms).1 {
            problems.push(Problem::new(
                Class::Hard,
                format!(
                    "{}.{}.{metric} = {value}: {}",
                    row.name,
                    arms.cand,
                    if metric == "via_in_pad" {
                        "a via in a pad is a failure, not a quality note"
                    } else {
                        "the board's design rules are hard limits (D8); the count must be 0"
                    }
                ),
            ));
        }
    }
}

// --- the pass rule ----------------------------------------------------------

/// The trial verdict: per distinct board, `regress` if any of its rows
/// regresses, else `improve` if any improves, else `neutral`. Pure, so the
/// self-test reaches every branch. Returns the report lines and whether the
/// rule holds.
pub fn trial(rows: &[(String, String, Mark, bool)], rule: &PassRule) -> (bool, Vec<String>) {
    // (row, board, mark, rejected)
    let mut lines = Vec::new();
    let rejected: Vec<&str> = rows.iter().filter(|r| r.3).map(|r| r.0.as_str()).collect();
    if !rejected.is_empty() {
        lines.push(format!(
            "rejected: {} row(s) record a change that was tried and NOT adopted ({}); \
             they are judged against their recorded marks, not against 'the change helps'",
            rejected.len(),
            rejected.join(", ")
        ));
    }
    let mut by_board: BTreeMap<&str, Vec<Mark>> = BTreeMap::new();
    for (_, board, mark, rej) in rows {
        if *rej || *mark == Mark::Skip {
            continue;
        }
        by_board.entry(board.as_str()).or_default().push(*mark);
    }
    if by_board.is_empty() {
        lines.push("on trial: no rows".to_string());
        return (true, lines);
    }
    let n = by_board.len();
    let regressed: Vec<&str> = by_board
        .iter()
        .filter(|(_, m)| m.contains(&Mark::Regress))
        .map(|(b, _)| *b)
        .collect();
    let improved: Vec<&str> = by_board
        .iter()
        .filter(|(b, m)| m.contains(&Mark::Improve) && !regressed.contains(*b))
        .map(|(b, _)| *b)
        .collect();
    let neutral: Vec<&str> = by_board
        .keys()
        .copied()
        .filter(|b| !regressed.contains(b) && !improved.contains(b))
        .collect();
    let mut ok = true;
    if n < rule.min_boards {
        ok = false;
        lines.push(format!(
            "REFUSED: a change is judged on >= {} DISTINCT boards; this run has N={n} \
             distinct boards ({}). Rows of one board count once. Add other boards or run \
             the whole table",
            rule.min_boards,
            by_board.keys().copied().collect::<Vec<_>>().join(", ")
        ));
    }
    let need = n.saturating_sub(rule.improve_all_but).max(1);
    ok = ok && regressed.len() <= rule.max_regress && improved.len() >= need;
    lines.push(format!(
        "on trial: improved {}/{n} board(s), regressed {}, neutral {} (rule: improve >= N-{}, \
         regress <= {})",
        improved.len(),
        regressed.len(),
        neutral.len(),
        rule.improve_all_but,
        rule.max_regress
    ));
    for (label, boards) in [
        ("improved", &improved),
        ("regressed", &regressed),
        ("neutral", &neutral),
    ] {
        if !boards.is_empty() {
            lines.push(format!("  {label}: {}", boards.join(", ")));
        }
    }
    lines.push(format!(
        "  N={n}: a change whose per-board direction is a coin flip passes this rule 1 run \
         in {}",
        1u64.checked_shl(n as u32)
            .map_or("2^N (huge)".into(), |v| v.to_string())
    ));
    (ok, lines)
}

/// The declared row names of a records file (`declared`, else the rows).
pub fn declared_rows(records: &Json, rows: &[Row<'_>]) -> BTreeSet<String> {
    match records.get("declared") {
        Some(Json::Array(names)) => names
            .iter()
            .filter_map(|n| n.as_str().map(str::to_string))
            .collect(),
        _ => rows.iter().map(|r| r.name.to_string()).collect(),
    }
}

/// The arm names a records file asks to compare (`baseline_arm`,
/// `candidate_arm`), else `defaults`.
pub fn arms_of(records: &Json, defaults: &Arms) -> Arms {
    let pick = |key: &str, default: &str| match records.get(key) {
        Some(Json::String(s)) => s.clone(),
        _ => default.to_string(),
    };
    Arms {
        base: pick("baseline_arm", &defaults.base),
        cand: pick("candidate_arm", &defaults.cand),
    }
}

/// Checks a run against a baseline (see the module docs).
pub fn check(records: &Json, baseline: &Json, settings: &Settings) -> Outcome {
    let mut outcome = Outcome::default();
    let mut problems = Vec::new();
    let rows = records_rows(records, &mut problems);
    let specs = active_specs(records, settings);
    let settings = Settings {
        arms: arms_of(records, &settings.arms),
        ..settings.clone()
    };
    let arms = &settings.arms;
    let scoped: Vec<Row<'_>> = rows
        .iter()
        .filter(|r| settings.scope.as_ref().is_none_or(|s| s.contains(r.name)))
        .cloned()
        .collect();
    check_rows(&scoped, &specs, arms, &mut problems);
    let declared = declared_rows(records, &rows);
    problems.extend(compare_baseline(
        &scoped, &declared, baseline, &specs, &settings,
    ));
    outcome.lines = report_lines(&scoped, &specs, arms);
    let trial_rows: Vec<(String, String, Mark, bool)> = scoped
        .iter()
        .map(|r| {
            (
                r.name.to_string(),
                r.board.to_string(),
                row_mark(r, &specs, arms).0,
                r.rejected,
            )
        })
        .collect();
    let (ok, lines) = match &settings.totals {
        Some(rule) => {
            let (ok, lines, more) = judge(&scoped, arms, &specs, rule);
            problems.extend(more);
            (ok, lines)
        }
        None => trial(&trial_rows, &settings.rule),
    };
    outcome.rule_ok = ok;
    outcome.lines.extend(lines);
    problems.sort_by(|a, b| a.class.cmp(&b.class).then_with(|| a.text.cmp(&b.text)));
    outcome.problems = problems;
    outcome
}

/// One line per row: the mark and the judged metrics, base -> candidate.
pub fn report_lines(rows: &[Row<'_>], specs: &[MetricSpec], arms: &Arms) -> Vec<String> {
    let mut lines = vec![format!("rows (arm {} -> {}):", arms.base, arms.cand)];
    for row in rows {
        let (mark, _) = row_mark(row, specs, arms);
        let mut cells = Vec::new();
        if let (Some(b), Some(c)) = (row.arm(&arms.base), row.arm(&arms.cand)) {
            for spec in specs.iter().filter(|s| s.kind != Kind::Counts) {
                cells.push(format!(
                    "{} {}->{}",
                    spec.name,
                    show(b.get(&spec.name)),
                    show(c.get(&spec.name))
                ));
            }
        }
        lines.push(format!(
            "  {} [{}]: {}{}  {}",
            row.name,
            row.board,
            mark.name(),
            if row.rejected { " (rejected)" } else { "" },
            cells.join(", ")
        ));
    }
    lines
}

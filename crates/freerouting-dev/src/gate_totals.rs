//! The totals rule of the gate (N02.1, D10): `freerouting-dev gate --rule totals`.
//!
//! The paired rule of [`crate::gate`] ("improve on N-1 boards, regress on
//! none") treats every board as a vote. A change that moves routes moves most
//! boards by a few connections either way, so it fails the paired rule even
//! when the total improves clearly (P04.1: -12 unrouted over 17 boards and half
//! the maze work, yet 5 boards regressed). The totals rule judges the sum
//! against the noise of the set. It is the rule for changes to profile
//! `enhanced`; the paired rule stays the rule for opt-in stages
//! (`docs/roadmap/routing-quality.md`).
//!
//! # The rule
//!
//! Over the judged rows (rejected rows and rows without both arms are left
//! out):
//!
//! 1. **Hard guards**, as in the paired rule: no hard metric (`via_in_pad`,
//!    `below_minimum`) above zero in the candidate arm, no guard metric
//!    (`drc_real`, `clearance_violations`) worse than the baseline arm. D2
//!    determinism is held by the baseline comparison (DRIFT), which runs
//!    unchanged.
//! 2. **Signal**: per row, the first metric of `signal` that is a number in
//!    both arms. The default list is `kicad_unconnected` (KiCad-graded), then
//!    `unrouted` (the router's own count, for boards without a `.kicad_pcb`).
//!    A row where neither is a number is UNGRADED. The report sums each
//!    source separately as well as together.
//! 3. **Boards**: the rows of one `board` are one board (the canary-pwr
//!    variants), their values summed. At least `min_boards` (3) distinct
//!    boards.
//! 4. **Noise band of a board**: its registered `noise`, else
//!    `max(2, ceil(5 % of connections))`, where `connections` is the number in
//!    the row (the largest over the board's rows). A board with neither is
//!    MISSING.
//! 5. **Set noise** = `sqrt(sum of band^2)` over the boards.
//! 6. **No board worse than twice its band**: `cand - base > 2 * band` on any
//!    board fails.
//! 7. **Total** (`delta` = candidate total minus baseline total):
//!    - `accept: improve`: passes when the improvement `-delta` is above the
//!      set noise, or the **speed clause** holds: the total is not worse than
//!      the set noise and the registered work measure (summed over the judged
//!      rows) dropped by at least `min_drop_percent` (20 or more);
//!    - `accept: no-regress` (correctness fixes, N06): passes when the total
//!      is not worse than the set noise. It needs no improvement and no speed
//!      clause.
//!
//! Comparisons are strict where the checklist says "more than" (an
//! improvement equal to the noise fails; a board exactly twice its band
//! passes) and exact for integer inputs.
//!
//! # Registration
//!
//! `"rule": "totals"` and a `totals` block (see `tools/ab/README.md`):
//!
//! ```json
//! "rule": "totals",
//! "totals": { "accept": "improve",                       // or "no-regress"; required
//!             "signal": ["kicad_unconnected", "unrouted"], // optional, registered metrics
//!             "noise": { "<board>": 10 },                // optional; band per distinct board
//!             "min_boards": 3,                           // optional, not below 3
//!             "speed": { "metric": "maze_nodes", "min_drop_percent": 20 } } // optional
//! ```

use std::collections::{BTreeMap, BTreeSet};

use crate::gate::{
    Arms, Class, MIN_BOARDS, MetricSpec, Problem, Row, guard_failures, number, show,
};
use crate::json_read::Json;

/// The smallest noise band of a board.
pub const MIN_BAND: f64 = 2.0;
/// The default band is this share of the board's connections, rounded up.
pub const BAND_PERCENT: u64 = 5;
/// A board regresses beyond tolerance when it is worse by more than this many bands.
pub const REGRESS_BANDS: f64 = 2.0;
/// The speed clause needs at least this drop of the work measure, in percent.
pub const MIN_SPEED_DROP_PERCENT: f64 = 20.0;
/// The default signal: KiCad's count, else the router's own.
pub const DEFAULT_SIGNAL: [&str; 2] = ["kicad_unconnected", "unrouted"];

/// What the total must do.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accept {
    /// The total improves by more than the set noise (or the speed clause).
    Improve,
    /// The total is not worse than the set noise (a correctness fix).
    NoRegress,
}

impl Accept {
    pub fn name(self) -> &'static str {
        match self {
            Accept::Improve => "improve",
            Accept::NoRegress => "no-regress",
        }
    }

    pub fn parse(text: &str) -> Option<Accept> {
        [Accept::Improve, Accept::NoRegress]
            .into_iter()
            .find(|a| a.name() == text)
    }
}

/// The speed clause: a work measure that must drop.
#[derive(Clone, Debug, PartialEq)]
pub struct SpeedClause {
    /// A registered metric, lower is better (maze nodes, tree nodes visited).
    pub metric: String,
    /// The least drop of its total, in percent (not below 20).
    pub min_drop_percent: f64,
}

/// A parsed totals rule.
#[derive(Clone, Debug, PartialEq)]
pub struct TotalsRule {
    pub accept: Accept,
    pub signal: Vec<String>,
    /// Registered band per distinct board (the row's `board`).
    pub noise: BTreeMap<String, f64>,
    pub min_boards: usize,
    pub speed: Option<SpeedClause>,
}

impl TotalsRule {
    /// The rule with the given acceptance and everything else default.
    pub fn new(accept: Accept) -> TotalsRule {
        TotalsRule {
            accept,
            signal: DEFAULT_SIGNAL.iter().map(|s| s.to_string()).collect(),
            noise: BTreeMap::new(),
            min_boards: MIN_BOARDS,
            speed: None,
        }
    }

    /// Parses the `totals` block of a registration against its metrics.
    pub fn parse(block: &Json, specs: &[MetricSpec]) -> Result<TotalsRule, Vec<Problem>> {
        let mut out = Vec::new();
        let malformed = |t: String| Problem::new(Class::Malformed, format!("prereg: totals: {t}"));
        let refused = |t: String| Problem::new(Class::Refused, format!("prereg: totals: {t}"));
        let Json::Object(map) = block else {
            return Err(vec![malformed("expected an object".into())]);
        };
        for key in map.keys() {
            if !["accept", "signal", "noise", "min_boards", "speed"].contains(&key.as_str()) {
                out.push(malformed(format!("unknown key {key:?}")));
            }
        }
        let registered = |name: &str| specs.iter().any(|s| s.name == name);
        let accept = match block
            .get("accept")
            .and_then(Json::as_str)
            .map(Accept::parse)
        {
            Some(Some(a)) => a,
            _ => {
                out.push(malformed(
                    "`accept` is required and must be \"improve\" or \"no-regress\"".into(),
                ));
                Accept::Improve
            }
        };
        let mut rule = TotalsRule::new(accept);
        match block.get("signal") {
            None => {
                rule.signal.retain(|s| registered(s));
                if rule.signal.is_empty() {
                    out.push(malformed(format!(
                        "none of the default signals {DEFAULT_SIGNAL:?} is a registered metric; \
                         register one or list `signal`"
                    )));
                }
            }
            Some(Json::Array(list)) if !list.is_empty() => {
                rule.signal.clear();
                for name in list {
                    match name.as_str() {
                        Some(n) if registered(n) => rule.signal.push(n.to_string()),
                        _ => out.push(malformed(format!(
                            "`signal` entry {} is not a registered metric",
                            show(Some(name))
                        ))),
                    }
                }
            }
            Some(_) => out.push(malformed("`signal` must be a non-empty array".into())),
        }
        match block.get("noise") {
            None => {}
            Some(Json::Object(bands)) => {
                for (board, band) in bands {
                    match band {
                        Json::Number(b) if *b >= 0.0 && b.is_finite() => {
                            rule.noise.insert(board.clone(), *b);
                        }
                        _ => out.push(malformed(format!(
                            "noise for {board}: expected a number >= 0, got {}",
                            show(Some(band))
                        ))),
                    }
                }
            }
            Some(_) => out.push(malformed("`noise` must map boards to bands".into())),
        }
        match block.get("min_boards") {
            None => {}
            Some(Json::Number(n)) if *n >= 0.0 && n.fract() == 0.0 => {
                rule.min_boards = *n as usize;
                if rule.min_boards < MIN_BOARDS {
                    out.push(refused(format!(
                        "min_boards is {}, the global rule needs >= {MIN_BOARDS} distinct boards",
                        rule.min_boards
                    )));
                }
            }
            Some(_) => out.push(malformed("`min_boards` must be a count".into())),
        }
        match block.get("speed") {
            None => {}
            Some(speed @ Json::Object(_)) => {
                let metric = speed.get("metric").and_then(Json::as_str).unwrap_or("");
                if !registered(metric) {
                    out.push(malformed(format!(
                        "speed.metric {metric:?} is not a registered metric"
                    )));
                }
                let percent = match speed.get("min_drop_percent") {
                    None => MIN_SPEED_DROP_PERCENT,
                    Some(Json::Number(p)) if p.is_finite() && *p <= 100.0 => *p,
                    Some(_) => {
                        out.push(malformed("speed.min_drop_percent must be <= 100".into()));
                        MIN_SPEED_DROP_PERCENT
                    }
                };
                if percent < MIN_SPEED_DROP_PERCENT {
                    out.push(refused(format!(
                        "speed.min_drop_percent is {percent}, the rule needs a drop of at least \
                         {MIN_SPEED_DROP_PERCENT} %"
                    )));
                }
                if accept == Accept::NoRegress {
                    out.push(refused(
                        "a speed clause goes with `accept: improve`; `no-regress` already accepts \
                         a total within noise"
                            .into(),
                    ));
                }
                rule.speed = Some(SpeedClause {
                    metric: metric.to_string(),
                    min_drop_percent: percent,
                });
            }
            Some(_) => out.push(malformed("`speed` must be an object".into())),
        }
        if out.is_empty() { Ok(rule) } else { Err(out) }
    }

    /// The band of a board without a registered one: `max(2, ceil(5 %))`.
    pub fn default_band(connections: u64) -> f64 {
        MIN_BAND.max((connections * BAND_PERCENT).div_ceil(100) as f64)
    }
}

/// One distinct board, summed over its rows.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardTotal {
    pub board: String,
    /// The signal metric(s) used, comma-joined.
    pub signal: String,
    pub base: f64,
    pub cand: f64,
    pub band: f64,
}

/// A work measure summed over the judged rows.
#[derive(Clone, Debug, PartialEq)]
pub struct Work {
    pub metric: String,
    pub base: f64,
    pub cand: f64,
}

fn fmt(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v:.2}")
    }
}

/// The verdict on boards already summed: the pure core, so a self-test
/// reaches every branch. `guards` are the hard-guard failures of the rows
/// (printed, and any fails the rule). Returns whether the rule holds and
/// the report lines.
pub fn totals_trial(
    boards: &[BoardTotal],
    rule: &TotalsRule,
    work: Option<&Work>,
    guards: &[String],
) -> (bool, Vec<String>) {
    let mut lines = vec![format!(
        "totals rule (accept: {}, >= {} distinct boards):",
        rule.accept.name(),
        rule.min_boards
    )];
    let n = boards.len();
    for b in boards {
        let delta = b.cand - b.base;
        let note = if delta > REGRESS_BANDS * b.band {
            format!("  WORSE than {REGRESS_BANDS} x its band")
        } else {
            String::new()
        };
        lines.push(format!(
            "  {} [{}]: {} -> {} (change {}{}, band {}){note}",
            b.board,
            b.signal,
            fmt(b.base),
            fmt(b.cand),
            if delta > 0.0 { "+" } else { "" },
            fmt(delta),
            fmt(b.band)
        ));
    }
    let base: f64 = boards.iter().map(|b| b.base).sum();
    let cand: f64 = boards.iter().map(|b| b.cand).sum();
    let delta = cand - base;
    let sum_sq: f64 = boards.iter().map(|b| b.band * b.band).sum();
    let noise = sum_sq.sqrt();
    let signals: BTreeSet<&str> = boards.iter().map(|b| b.signal.as_str()).collect();
    if signals.len() > 1 {
        for s in signals {
            let (b, c) = boards
                .iter()
                .filter(|x| x.signal == s)
                .fold((0.0, 0.0), |a, x| (a.0 + x.base, a.1 + x.cand));
            lines.push(format!(
                "  signal [{s}] summed separately: {} -> {}",
                fmt(b),
                fmt(c)
            ));
        }
    }
    lines.push(format!(
        "  total {} -> {} (change {}{}); set noise sqrt(sum band^2) = {:.2}",
        fmt(base),
        fmt(cand),
        if delta > 0.0 { "+" } else { "" },
        fmt(delta),
        noise
    ));

    let mut ok = true;
    let mut fail = |lines: &mut Vec<String>, text: String| {
        ok = false;
        lines.push(format!("  FAILS: {text}"));
    };
    for g in guards {
        fail(&mut lines, format!("hard guard: {g}"));
    }
    if n < rule.min_boards {
        fail(
            &mut lines,
            format!(
                "REFUSED: a change is judged on >= {} DISTINCT boards; this run has N={n}. Rows \
                 of one board count once",
                rule.min_boards
            ),
        );
    }
    let beyond: Vec<&str> = boards
        .iter()
        .filter(|b| b.cand - b.base > REGRESS_BANDS * b.band)
        .map(|b| b.board.as_str())
        .collect();
    if !beyond.is_empty() {
        fail(
            &mut lines,
            format!(
                "{} worse by more than {REGRESS_BANDS} x its noise band: {}",
                beyond.len(),
                beyond.join(", ")
            ),
        );
    }
    // Strictly more than the noise: squares, exact for integer inputs.
    let improves = delta < 0.0 && delta * delta > sum_sq;
    let within_noise = delta <= 0.0 || delta * delta <= sum_sq;
    let speed = rule.speed.as_ref().map(|clause| match work {
        Some(w) if w.base > 0.0 => {
            let drop = (w.base - w.cand) * 100.0 >= clause.min_drop_percent * w.base;
            lines.push(format!(
                "  speed clause: {} {} -> {} ({:.1} % drop, needs {} %): {}",
                w.metric,
                fmt(w.base),
                fmt(w.cand),
                (w.base - w.cand) * 100.0 / w.base,
                fmt(clause.min_drop_percent),
                if drop { "holds" } else { "does not hold" }
            ));
            drop
        }
        _ => {
            lines.push(format!(
                "  speed clause: {} not measured in every judged row, so it does not hold",
                clause.metric
            ));
            false
        }
    });
    let speed_ok = speed == Some(true);
    let quality_ok = match rule.accept {
        Accept::Improve => improves || (speed_ok && within_noise),
        Accept::NoRegress => within_noise,
    };
    if !quality_ok {
        let why = match rule.accept {
            Accept::Improve if delta < 0.0 => format!(
                "the improvement {} does not exceed the set noise {:.2}{}",
                fmt(-delta),
                noise,
                if rule.speed.is_some() {
                    ", and the speed clause does not hold"
                } else {
                    ""
                }
            ),
            Accept::Improve if within_noise => format!(
                "the total does not improve (change {}) and the speed clause does not hold",
                fmt(delta)
            ),
            _ => format!(
                "the total is worse by {}, more than the set noise {:.2}",
                fmt(delta),
                noise
            ),
        };
        fail(&mut lines, why);
    }
    lines.push(format!(
        "  totals verdict: {}",
        if ok { "accept" } else { "reject" }
    ));
    (ok, lines)
}

#[derive(Default)]
struct Acc {
    signals: BTreeSet<String>,
    base: f64,
    cand: f64,
    connections: Option<f64>,
}

fn graded(arm: &BTreeMap<String, Json>) -> bool {
    !matches!(arm.get("graded"), Some(Json::Bool(false)))
}

/// Judges records by the totals rule: the report lines, whether the rule
/// holds, and the problems found on the way (UNGRADED, MISSING, MALFORMED).
pub fn judge(
    rows: &[Row<'_>],
    arms: &Arms,
    specs: &[MetricSpec],
    rule: &TotalsRule,
) -> (bool, Vec<String>, Vec<Problem>) {
    let mut problems = Vec::new();
    let mut lines = Vec::new();
    let mut guards = Vec::new();
    let mut by_board: BTreeMap<&str, Acc> = BTreeMap::new();
    let mut rejected = Vec::new();
    let mut work: Option<Work> = rule.speed.as_ref().map(|s| Work {
        metric: s.metric.clone(),
        base: 0.0,
        cand: 0.0,
    });
    for row in rows {
        if row.rejected {
            rejected.push(row.name);
            continue;
        }
        let (Some(base), Some(cand)) = (row.arm(&arms.base), row.arm(&arms.cand)) else {
            continue;
        };
        if !graded(base) || !graded(cand) {
            continue;
        }
        guards.extend(guard_failures(row, specs, arms));
        let picked = rule.signal.iter().find_map(|name| {
            match (number(base.get(name)), number(cand.get(name))) {
                (Some(b), Some(c)) => Some((name, b, c)),
                _ => None,
            }
        });
        let Some((name, b, c)) = picked else {
            problems.push(Problem::new(
                Class::Ungraded,
                format!(
                    "{}: none of the signals {} is a number in both arms; the row can't be \
                     judged",
                    row.name,
                    rule.signal.join(", ")
                ),
            ));
            continue;
        };
        let acc = by_board.entry(row.board).or_default();
        acc.signals.insert(name.clone());
        acc.base += b;
        acc.cand += c;
        match row.body.get("connections") {
            None | Some(Json::Null) => {}
            Some(Json::Number(n)) if *n >= 0.0 && n.fract() == 0.0 => {
                acc.connections = Some(acc.connections.map_or(*n, |m| m.max(*n)));
            }
            Some(other) => problems.push(Problem::new(
                Class::Malformed,
                format!(
                    "{}.connections: expected a count, got {}",
                    row.name,
                    show(Some(other))
                ),
            )),
        }
        if let (Some(w), Some(clause)) = (work.as_mut(), rule.speed.as_ref()) {
            match (
                number(base.get(&clause.metric)),
                number(cand.get(&clause.metric)),
            ) {
                (Some(x), Some(y)) => {
                    w.base += x;
                    w.cand += y;
                }
                _ => work = None,
            }
        }
    }
    if !rejected.is_empty() {
        lines.push(format!(
            "rejected: {} row(s) record a change that was tried and NOT adopted ({}); they are \
             left out of the totals",
            rejected.len(),
            rejected.join(", ")
        ));
    }
    let mut boards = Vec::new();
    for (board, acc) in by_board {
        let band = match (rule.noise.get(board), acc.connections) {
            (Some(b), _) => *b,
            (None, Some(c)) => TotalsRule::default_band(c as u64),
            (None, None) => {
                problems.push(Problem::new(
                    Class::Missing,
                    format!(
                        "{board}: no noise band. Register `totals.noise` for the board, or give \
                         its rows `connections` for the default max(2, 5 %)"
                    ),
                ));
                continue;
            }
        };
        boards.push(BoardTotal {
            board: board.to_string(),
            signal: acc.signals.into_iter().collect::<Vec<_>>().join(","),
            base: acc.base,
            cand: acc.cand,
            band,
        });
    }
    let (ok, mut more) = totals_trial(&boards, rule, work.as_ref(), &guards);
    lines.append(&mut more);
    (ok, lines, problems)
}

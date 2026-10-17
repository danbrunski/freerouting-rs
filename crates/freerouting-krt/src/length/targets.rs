//! Length targets (X07.2): per-net class windows and matched groups.
//!
//! Two sources set a net's target window `[lo, hi]` and the value the
//! stage aims at:
//! - **the class window** from the DSN (`(length max min)` of a class or
//!   net, Java's `Net.getMinimumTraceLength`/`getMaximumTraceLength`): a
//!   net shorter than its minimum is lengthened to the minimum, never past
//!   the maximum (0 = no bound);
//! - **groups** from `router.length_matching.groups`: the target is the
//!   longest member, measured after routing, and every member ends in
//!   `[longest - tolerance, longest]`. The stage never lengthens a member
//!   past the longest, so the longest stays the target and a second run
//!   finds every member in its window (idempotence).
//!
//! A net in a group and with a class window gets the intersection; an empty
//! intersection is reported as a conflict and the net is left alone.
//!
//! **Group syntax.** The settings model has no list-of-objects type (C02),
//! so a group is one string `NAME=GLOB[|GLOB...][@TOLERANCE]`, for example
//! `DDR_DQ=DQ*|DQS*@100um`. Globs match net names with `*` and `?`, case
//! sensitive; the members are the nets any glob matches, in net-number
//! order. The tolerance is `<number>um`, `<number>mm` or `<number>ps` (a
//! time group, X07.5), and defaults to 100 µm (KRT's
//! `--length-match-tolerance` 0.1 mm). A group without a glob, or whose
//! globs match no net, is a settings error. Groups run in name order.
//!
//! Written from KiCadRoutingTools' length-matching groups
//! (`py_router/length_matching.py` `find_nets_matching_patterns` at
//! `0486e799`): KRT builds its groups from Python sets of net names, whose
//! order varies per process; here they are ordered by name and net number.

use std::fmt;

use freerouting::board::BasicBoard;

/// KRT's default length-match tolerance, in micrometres.
pub const DEFAULT_TOLERANCE_UM: f64 = 100.0;

/// A group's tolerance.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Tolerance {
    /// A length tolerance in micrometres.
    Um(f64),
    /// A time tolerance in picoseconds (a time group, X07.5).
    Ps(f64),
}

impl Tolerance {
    /// True for a time tolerance.
    pub fn is_time(self) -> bool {
        matches!(self, Tolerance::Ps(_))
    }
}

/// One `router.length_matching.groups` entry, parsed.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupSpec {
    /// The group's name.
    pub name: String,
    /// The net-name globs.
    pub patterns: Vec<String>,
    /// The tolerance.
    pub tolerance: Tolerance,
}

/// A settings error of `router.length_matching.groups`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SettingsError(pub String);

impl fmt::Display for SettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "router.length_matching.groups: {}", self.0)
    }
}

impl std::error::Error for SettingsError {}

fn error(text: String) -> SettingsError {
    SettingsError(text)
}

/// Parses one group (module docs).
pub fn parse_group(spec: &str) -> Result<GroupSpec, SettingsError> {
    let (name, rest) = spec
        .split_once('=')
        .ok_or_else(|| error(format!("'{spec}' is not NAME=GLOB[|GLOB...][@TOLERANCE]")))?;
    let name = name.trim();
    if name.is_empty() {
        return Err(error(format!("'{spec}' has no group name")));
    }
    let (globs, tolerance) = match rest.rsplit_once('@') {
        Some((globs, tol)) => (globs, parse_tolerance(name, tol.trim())?),
        None => (rest, Tolerance::Um(DEFAULT_TOLERANCE_UM)),
    };
    let patterns: Vec<String> = globs
        .split('|')
        .map(str::trim)
        .filter(|g| !g.is_empty())
        .map(str::to_string)
        .collect();
    if patterns.is_empty() {
        return Err(error(format!("group {name} is empty: it lists no net")));
    }
    Ok(GroupSpec {
        name: name.to_string(),
        patterns,
        tolerance,
    })
}

fn parse_tolerance(name: &str, text: &str) -> Result<Tolerance, SettingsError> {
    let bad = || {
        error(format!(
            "group {name}: tolerance '{text}' is not <number>um, <number>mm or <number>ps"
        ))
    };
    let (number, make): (&str, fn(f64) -> Tolerance) = if let Some(n) = text.strip_suffix("um") {
        (n, Tolerance::Um)
    } else if let Some(n) = text.strip_suffix("mm") {
        (n, |v| Tolerance::Um(v * 1000.0))
    } else if let Some(n) = text.strip_suffix("ps") {
        (n, Tolerance::Ps)
    } else {
        return Err(bad());
    };
    let value: f64 = number.trim().parse().map_err(|_| bad())?;
    if !value.is_finite() || value < 0.0 {
        return Err(bad());
    }
    Ok(make(value))
}

/// Parses every group, sorted by name. A repeated name is an error.
pub fn parse_groups(specs: &[String]) -> Result<Vec<GroupSpec>, SettingsError> {
    let mut groups = specs
        .iter()
        .map(|s| parse_group(s))
        .collect::<Result<Vec<_>, _>>()?;
    groups.sort_by(|a, b| a.name.cmp(&b.name));
    if let Some(w) = groups.windows(2).find(|w| w[0].name == w[1].name) {
        return Err(error(format!("group {} is listed twice", w[0].name)));
    }
    Ok(groups)
}

/// True if `text` matches `pattern` (`*` any run, `?` one character).
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let p: Vec<char> = pattern.chars().collect();
    let t: Vec<char> = text.chars().collect();
    let (mut pi, mut ti) = (0, 0);
    let mut star: Option<(usize, usize)> = None;
    while ti < t.len() {
        if pi < p.len() && (p[pi] == '?' || p[pi] == t[ti]) {
            pi += 1;
            ti += 1;
        } else if pi < p.len() && p[pi] == '*' {
            star = Some((pi, ti));
            pi += 1;
        } else if let Some((sp, st)) = star {
            pi = sp + 1;
            ti = st + 1;
            star = Some((sp, st + 1));
        } else {
            return false;
        }
    }
    p[pi..].iter().all(|&c| c == '*')
}

/// The net numbers of `nets` (`(number, name)`) that any of `patterns`
/// matches, ascending.
pub fn match_nets(nets: &[(i32, String)], patterns: &[String]) -> Vec<i32> {
    let mut out: Vec<i32> = nets
        .iter()
        .filter(|(_, name)| patterns.iter().any(|p| glob_match(p, name)))
        .map(|(n, _)| *n)
        .collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// The normal nets of `board` as `(number, name)`, ascending.
pub fn board_nets(board: &BasicBoard) -> Vec<(i32, String)> {
    (1..=board.rules.nets.max_net_number())
        .filter_map(|n| board.rules.nets.get(n))
        .map(|net| (net.net_number, net.name.clone()))
        .collect()
}

/// A group resolved on a board.
#[derive(Clone, Debug, PartialEq)]
pub struct Group {
    /// The group's name.
    pub name: String,
    /// Its nets, ascending.
    pub nets: Vec<i32>,
    /// Its tolerance.
    pub tolerance: Tolerance,
}

/// Resolves `spec` on `nets` (`board_nets`). A group that matches no net
/// is an error.
pub fn resolve_group(spec: &GroupSpec, nets: &[(i32, String)]) -> Result<Group, SettingsError> {
    let members = match_nets(nets, &spec.patterns);
    if members.is_empty() {
        return Err(error(format!(
            "group {} is empty: {} matches no net",
            spec.name,
            spec.patterns.join("|")
        )));
    }
    Ok(Group {
        name: spec.name.clone(),
        nets: members,
        tolerance: spec.tolerance,
    })
}

/// A net's target, in the metric's units (board units for length,
/// picoseconds for time).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Target {
    /// The net.
    pub net: i32,
    /// The lowest value in tolerance.
    pub lo: f64,
    /// The highest value in tolerance (`f64::INFINITY` for none).
    pub hi: f64,
    /// The value the stage aims at, in `[lo, hi]`.
    pub aim: f64,
}

impl Target {
    /// True if `value` is in `[lo, hi]`.
    pub fn holds(&self, value: f64) -> bool {
        value >= self.lo && value <= self.hi
    }
}

/// The class window of a net as `(min, max)` in board units, 0 = no
/// bound; `None` if the net has neither bound.
pub fn class_window(board: &BasicBoard, net: i32) -> Option<(f64, f64)> {
    let n = board.rules.nets.get(net)?;
    let classes = &board.rules.net_classes;
    let (min, max) = (
        n.minimum_trace_length(classes),
        n.maximum_trace_length(classes),
    );
    (min > 0.0 || max > 0.0).then_some((min.max(0.0), max.max(0.0)))
}

/// The target of a net with class window `(min, max)` (board units, 0 =
/// no bound) measured at `length`: `None` unless the net is shorter than
/// `min`. The aim is `min` plus `margin` (at most half the window), so a
/// bump rounded down to whole board units still ends at or above `min`.
pub fn class_target(net: i32, min: f64, max: f64, length: f64, margin: f64) -> Option<Target> {
    if min <= 0.0 || length >= min {
        return None;
    }
    let hi = if max > 0.0 { max } else { f64::INFINITY };
    if hi < min {
        return None;
    }
    let aim = if hi.is_finite() {
        min + margin.min((hi - min) / 2.0)
    } else {
        min + margin
    };
    Some(Target {
        net,
        lo: min,
        hi,
        aim,
    })
}

/// The targets of a group whose members measure `measured` (`(net,
/// value)`), with tolerance `tolerance` in the same units: the target is
/// the longest member; members already in `[longest - tolerance,
/// longest]` get none.
pub fn group_targets(measured: &[(i32, f64)], tolerance: f64) -> Vec<Target> {
    let Some(longest) = measured.iter().map(|(_, v)| *v).reduce(f64::max) else {
        return Vec::new();
    };
    let lo = longest - tolerance;
    measured
        .iter()
        .filter(|(_, v)| *v < lo)
        .map(|&(net, _)| Target {
            net,
            lo,
            hi: longest,
            aim: longest,
        })
        .collect()
}

/// The intersection of a group target with a class window `(min, max)`
/// (0 = no bound), `None` if it is empty.
pub fn combine(target: Target, min: f64, max: f64) -> Option<Target> {
    let lo = target.lo.max(min);
    let hi = if max > 0.0 {
        target.hi.min(max)
    } else {
        target.hi
    };
    (lo <= hi).then(|| Target {
        net: target.net,
        lo,
        hi,
        aim: target.aim.clamp(lo, hi),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MM: f64 = 10_000.0; // board units per mm at `resolution um 10`

    fn nets(names: &[(i32, &str)]) -> Vec<(i32, String)> {
        names.iter().map(|(n, s)| (*n, s.to_string())).collect()
    }

    #[test]
    fn a_class_min_of_30_mm_makes_a_25_mm_nets_target_30_mm() {
        let t = class_target(4, 30.0 * MM, 0.0, 25.0 * MM, 0.01 * MM).expect("a target");
        assert_eq!(t.lo, 30.0 * MM);
        assert_eq!(t.hi, f64::INFINITY);
        assert!(t.aim >= 30.0 * MM && t.aim <= 30.01 * MM);
        assert!(!t.holds(25.0 * MM));
        assert!(t.holds(30.0 * MM));
        // Long enough already, or no minimum: no target.
        assert_eq!(class_target(4, 30.0 * MM, 0.0, 31.0 * MM, 0.0), None);
        assert_eq!(class_target(4, 0.0, 40.0 * MM, 25.0 * MM, 0.0), None);
        // A maximum caps the window and the aim stays inside it.
        let t = class_target(4, 30.0 * MM, 30.004 * MM, 25.0 * MM, 0.01 * MM).unwrap();
        assert_eq!(t.hi, 30.004 * MM);
        assert!(t.aim <= t.hi && t.aim >= t.lo);
    }

    #[test]
    fn a_group_of_three_nets_targets_the_longest() {
        let measured = [(3, 30.0 * MM), (5, 35.0 * MM), (7, 32.0 * MM)];
        let targets = group_targets(&measured, 0.1 * MM);
        assert_eq!(targets.len(), 2, "the longest needs nothing");
        for t in &targets {
            assert_eq!(t.aim, 35.0 * MM);
            assert_eq!(t.hi, 35.0 * MM);
            assert_eq!(t.lo, 34.9 * MM);
        }
        assert_eq!(
            targets.iter().map(|t| t.net).collect::<Vec<_>>(),
            vec![3, 7]
        );
        // A member within the tolerance gets none.
        let targets = group_targets(&[(1, 34.95 * MM), (2, 35.0 * MM)], 0.1 * MM);
        assert!(targets.is_empty());
    }

    #[test]
    fn glob_patterns_match_in_net_number_order() {
        let board = nets(&[
            (9, "DQ0"),
            (2, "DQ10"),
            (5, "DQS0"),
            (7, "CLK"),
            (1, "DQ1"),
            (3, "dq2"),
        ]);
        let pats = |p: &[&str]| p.iter().map(|s| s.to_string()).collect::<Vec<_>>();
        assert_eq!(match_nets(&board, &pats(&["DQ*"])), vec![1, 2, 5, 9]);
        assert_eq!(match_nets(&board, &pats(&["DQ?"])), vec![1, 9]);
        assert_eq!(match_nets(&board, &pats(&["CLK", "DQS*"])), vec![5, 7]);
        assert_eq!(match_nets(&board, &pats(&["*0"])), vec![2, 5, 9]);
        assert!(glob_match("*", ""));
        assert!(glob_match("a*b*c", "aXbYbZc"));
        assert!(!glob_match("a*b", "aXc"));
    }

    #[test]
    fn an_empty_group_is_a_settings_error() {
        let e = parse_group("DDR=").unwrap_err();
        assert!(e.to_string().contains("group DDR is empty"), "{e}");
        let e = parse_group("DDR= | @100um").unwrap_err();
        assert!(e.to_string().contains("is empty"), "{e}");
        let spec = parse_group("DDR=ADDR*").unwrap();
        let e = resolve_group(&spec, &nets(&[(1, "DQ0")])).unwrap_err();
        assert_eq!(
            e.to_string(),
            "router.length_matching.groups: group DDR is empty: ADDR* matches no net"
        );
        assert!(parse_group("no-equals").is_err());
        assert!(parse_group("=DQ*").is_err());
        assert!(parse_group("X=DQ*@5furlongs").is_err());
        assert!(parse_group("X=DQ*@-1um").is_err());
    }

    #[test]
    fn groups_parse_with_tolerances_and_sort_by_name() {
        let specs: Vec<String> = ["b=DQ*|DQS*@0.2mm", "a=CLK*@5ps", "c=X"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let g = parse_groups(&specs).unwrap();
        assert_eq!(
            g.iter().map(|g| g.name.as_str()).collect::<Vec<_>>(),
            vec!["a", "b", "c"]
        );
        assert_eq!(g[0].tolerance, Tolerance::Ps(5.0));
        assert_eq!(g[1].tolerance, Tolerance::Um(200.0));
        assert_eq!(g[1].patterns, vec!["DQ*".to_string(), "DQS*".to_string()]);
        assert_eq!(g[2].tolerance, Tolerance::Um(DEFAULT_TOLERANCE_UM));
        let twice: Vec<String> = vec!["a=X".into(), "a=Y".into()];
        assert!(parse_groups(&twice).unwrap_err().0.contains("listed twice"));
    }

    #[test]
    fn a_group_target_meets_a_class_window() {
        let t = Target {
            net: 1,
            lo: 34.9 * MM,
            hi: 35.0 * MM,
            aim: 35.0 * MM,
        };
        let c = combine(t, 30.0 * MM, 34.95 * MM).unwrap();
        assert_eq!((c.lo, c.hi, c.aim), (34.9 * MM, 34.95 * MM, 34.95 * MM));
        assert_eq!(combine(t, 36.0 * MM, 0.0), None);
    }
}

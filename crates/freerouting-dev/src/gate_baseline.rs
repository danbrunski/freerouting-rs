//! The baseline file of the gate: comparing a run with it (`INVERTED`,
//! `DRIFT`, `ORPHAN`, `MISSING`, `NEW ROW`, `MALFORMED`) and writing it
//! (`--write-baseline`). See [`crate::gate`] for the problem classes.

use std::collections::{BTreeMap, BTreeSet};

use crate::gate::{
    Arms, BASELINE_FORMAT, Class, Kind, Mark, MetricSpec, Problem, Row, Settings, TAG_KEYS,
    active_specs, as_object, check_rows, declared_rows, effective_arms, number, records_rows,
    row_mark, show, type_name,
};
use crate::json_read::Json;

// --- the baseline comparison ------------------------------------------------

fn sign(a: f64, b: f64) -> i32 {
    match b - a {
        d if d > 0.0 => 1,
        d if d < 0.0 => -1,
        _ => 0,
    }
}

/// The baseline's rows, or the reason it is not shaped like a baseline.
pub fn baseline_rows(baseline: &Json) -> Result<&BTreeMap<String, Json>, Problem> {
    let Some(top) = as_object(baseline) else {
        return Err(Problem::new(
            Class::Malformed,
            format!(
                "baseline: expected an object of rows, got {}",
                type_name(baseline)
            ),
        ));
    };
    match top.get("rows") {
        Some(Json::Object(rows)) => Ok(rows),
        other => Err(Problem::new(
            Class::Malformed,
            format!(
                "baseline: expected a `rows` object, got {}",
                other.map_or("nothing", type_name)
            ),
        )),
    }
}

/// Compares the measured rows with the baseline. `declared` is every row
/// name the harness declares; `scope` narrows which declared baseline rows
/// must be measured.
pub fn compare_baseline(
    rows: &[Row<'_>],
    declared: &BTreeSet<String>,
    baseline: &Json,
    specs: &[MetricSpec],
    settings: &Settings,
) -> Vec<Problem> {
    let expected = match baseline_rows(baseline) {
        Ok(rows) => rows,
        Err(problem) => return vec![problem],
    };
    let arms = &settings.arms;
    let mut problems = Vec::new();
    let mut sorted: Vec<&Row<'_>> = rows.iter().collect();
    sorted.sort_by(|a, b| a.name.cmp(b.name));
    for row in &sorted {
        let name = row.name;
        let Some(exp) = expected.get(name) else {
            problems.push(Problem::new(
                Class::NewRow,
                format!("{name}: not in the baseline"),
            ));
            continue;
        };
        let Some(exp) = as_object(exp) else {
            problems.push(Problem::new(
                Class::Malformed,
                format!(
                    "baseline row {name}: expected an object, got {}",
                    type_name(exp)
                ),
            ));
            continue;
        };
        let (mark, _) = row_mark(row, specs, arms);
        match exp.get("mark") {
            Some(Json::String(m)) if Mark::parse(m).is_some() => {
                if Mark::parse(m) != Some(mark) {
                    problems.push(Problem::new(
                        Class::Inverted,
                        format!("{name}: mark {m} -> {}", mark.name()),
                    ));
                }
            }
            other => problems.push(Problem::new(
                Class::Malformed,
                format!("baseline row {name}: bad mark {}", show(other)),
            )),
        }
        if let Some(Json::String(expected_hash)) = exp.get("input_sha256") {
            if row.input_sha256() != Some(expected_hash.as_str()) {
                problems.push(Problem::new(
                    Class::Drift,
                    format!(
                        "{name}.input_sha256: baseline {expected_hash}, measured {}. Compare \
                         only runs with identical input bytes",
                        row.input_sha256().unwrap_or("(none)")
                    ),
                ));
            }
        }
        compare_row(row, exp, specs, arms, &mut problems);
    }
    for name in expected.keys() {
        if rows.iter().any(|r| r.name == name) {
            continue;
        }
        if !declared.contains(name) {
            problems.push(Problem::new(
                Class::Orphan,
                format!(
                    "{name}: the baseline records this row and the run no longer declares \
                     it. Deleting a row deletes its evidence; re-record deliberately, don't \
                     let it lapse"
                ),
            ));
        } else if settings.scope.as_ref().is_none_or(|s| s.contains(name)) {
            problems.push(Problem::new(
                Class::Missing,
                format!("{name}: in the baseline, not measured in this run"),
            ));
        }
    }
    problems
}

fn compare_row(
    row: &Row<'_>,
    exp: &BTreeMap<String, Json>,
    specs: &[MetricSpec],
    arms: &Arms,
    problems: &mut Vec<Problem>,
) {
    let name = row.name;
    let exp_arm = |arm: &str| -> BTreeMap<String, Json> {
        match exp.get("arms").and_then(as_object).and_then(|a| a.get(arm)) {
            Some(Json::Object(o)) => o.clone(),
            _ => BTreeMap::new(),
        }
    };
    let (e_base, e_cand) = (exp_arm(&arms.base), exp_arm(&arms.cand));
    let (Some(c_base), Some(c_cand)) = (row.arm(&arms.base), row.arm(&arms.cand)) else {
        return; // reported as MISSING by check_rows
    };
    for (arm, c, e) in [(&arms.base, c_base, &e_base), (&arms.cand, c_cand, &e_cand)] {
        for tag in TAG_KEYS {
            let (cv, ev) = (c.get(tag), e.get(tag));
            if let Some(Json::String(expected_tag)) = ev {
                if cv != ev {
                    problems.push(Problem::new(
                        Class::Drift,
                        format!(
                            "{name}.{arm}.{tag}: baseline {expected_tag:?}, measured {}. \
                             Rows from two {tag}s are not comparable",
                            show(cv)
                        ),
                    ));
                }
            }
        }
    }
    for spec in specs.iter().filter(|s| s.compared) {
        let mut local = Vec::new();
        for (arm, c, e) in [(&arms.base, c_base, &e_base), (&arms.cand, c_cand, &e_cand)] {
            compare_value(
                name,
                arm,
                spec,
                c.get(&spec.name),
                e.get(&spec.name),
                &mut local,
            );
        }
        if spec.kind != Kind::Counts {
            let pairs = [
                (
                    number(e_base.get(&spec.name)),
                    number(e_cand.get(&spec.name)),
                ),
                (
                    number(c_base.get(&spec.name)),
                    number(c_cand.get(&spec.name)),
                ),
            ];
            if let [(Some(eb), Some(ec)), (Some(cb), Some(cc))] = pairs {
                let (ed, cd) = (sign(eb, ec), sign(cb, cc));
                if ed != 0 && cd != 0 && ed != cd {
                    // A reversal is named as such; the moved values it
                    // implies are not reported a second time as DRIFT.
                    local.retain(|p| p.class != Class::Drift);
                    local.push(Problem::new(
                        Class::Inverted,
                        format!(
                            "{name}.{}: baseline {eb} -> {ec}, measured {cb} -> {cc}",
                            spec.name
                        ),
                    ));
                }
            }
        }
        problems.append(&mut local);
    }
}

fn compare_value(
    name: &str,
    arm: &str,
    spec: &MetricSpec,
    cur: Option<&Json>,
    exp: Option<&Json>,
    problems: &mut Vec<Problem>,
) {
    let Some(cur) = cur else {
        return; // MISSING is reported once, by check_rows
    };
    if spec.kind == Kind::Counts {
        let empty = BTreeMap::new();
        let (c, e) = match (cur, exp.unwrap_or(&Json::Null)) {
            (Json::Object(c), Json::Object(e)) => (c, e),
            (Json::Object(c), Json::Null) => (c, &empty),
            _ => {
                problems.push(Problem::new(
                    Class::Malformed,
                    format!("{name}.{arm}.{}: expected an object", spec.name),
                ));
                return;
            }
        };
        let keys: BTreeSet<&String> = c.keys().chain(e.keys()).collect();
        for key in keys {
            let (cn, en) = (
                number(c.get(key)).unwrap_or(0.0),
                number(e.get(key)).unwrap_or(0.0),
            );
            if cn != en {
                problems.push(Problem::new(
                    Class::Drift,
                    format!(
                        "{name}.{arm}.{}[{key}]: baseline {en}, measured {cn}",
                        spec.name
                    ),
                ));
            }
        }
        return;
    }
    let c = number(Some(cur));
    let e = number(exp);
    if !matches!(cur, Json::Number(_) | Json::Null) {
        return; // reported as MALFORMED by check_rows
    }
    if c.is_none() != e.is_none() {
        problems.push(Problem::new(
            Class::Drift,
            format!(
                "{name}.{arm}.{}: baseline {}, measured {}",
                spec.name,
                show(exp),
                show(Some(cur))
            ),
        ));
    } else if let (Some(c), Some(e)) = (c, e) {
        if spec.kind == Kind::Int && c.fract() != 0.0 {
            problems.push(Problem::new(
                Class::Malformed,
                format!("{name}.{arm}.{}: {c} is not an integer", spec.name),
            ));
        } else if (c - e).abs() > e.abs().max(1.0) * spec.tol {
            problems.push(Problem::new(
                Class::Drift,
                format!("{name}.{arm}.{}: baseline {e}, measured {c}", spec.name),
            ));
        }
    }
}

// --- writing a baseline -----------------------------------------------------

/// The baseline for `records`, or the reasons it can't be written: a partial
/// run, a missing metric or an ungraded row would put a hole in the evidence.
pub fn baseline_from(records: &Json, settings: &Settings) -> Result<Json, Vec<Problem>> {
    let mut problems = Vec::new();
    let rows = records_rows(records, &mut problems);
    let specs = active_specs(records, settings);
    let arms = effective_arms(records, settings);
    if matches!(records.get("partial"), Some(Json::Bool(true))) {
        problems.push(Problem::new(
            Class::Refused,
            "the run is partial (a board missing, skipped or the bench stopped early); \
             --write-baseline refuses a partial run"
                .to_string(),
        ));
    }
    if let Some(scope) = &settings.scope {
        problems.push(Problem::new(
            Class::Refused,
            format!(
                "--rows narrows the run to {} row(s); --write-baseline refuses a partial run",
                scope.len()
            ),
        ));
    }
    let declared = declared_rows(records, &rows);
    for name in &declared {
        if !rows.iter().any(|r| r.name == name) {
            problems.push(Problem::new(
                Class::Refused,
                format!("{name} is declared but was not measured; refusing a partial run"),
            ));
        }
    }
    if rows.is_empty() {
        problems.push(Problem::new(
            Class::Refused,
            "the run has no rows".to_string(),
        ));
    }
    check_rows(&rows, &specs, &arms, &mut problems);
    problems.retain(|p| p.class != Class::Hard);
    if !problems.is_empty() {
        return Err(problems);
    }
    let mut out_rows = BTreeMap::new();
    for row in &rows {
        let mut body = BTreeMap::new();
        body.insert("board".to_string(), Json::String(row.board.to_string()));
        if let Some(hash) = row.input_sha256() {
            body.insert("input_sha256".to_string(), Json::String(hash.to_string()));
        }
        if row.rejected {
            body.insert("rejected".to_string(), Json::Bool(true));
        }
        body.insert(
            "mark".to_string(),
            Json::String(row_mark(row, &specs, &arms).0.name().to_string()),
        );
        let mut arm_map = BTreeMap::new();
        for arm_name in [&arms.base, &arms.cand] {
            let arm = row.arm(arm_name).expect("checked by check_rows");
            let mut kept = BTreeMap::new();
            for spec in specs.iter().filter(|s| s.compared) {
                kept.insert(spec.name.clone(), arm[&spec.name].clone());
            }
            for tag in TAG_KEYS {
                if let Some(v) = arm.get(tag) {
                    kept.insert(tag.to_string(), v.clone());
                }
            }
            arm_map.insert(arm_name.clone(), Json::Object(kept));
        }
        body.insert("arms".to_string(), Json::Object(arm_map));
        out_rows.insert(row.name.to_string(), Json::Object(body));
    }
    let mut top = BTreeMap::new();
    top.insert(
        "format".to_string(),
        Json::String(BASELINE_FORMAT.to_string()),
    );
    top.insert("rows".to_string(), Json::Object(out_rows));
    Ok(Json::Object(top))
}

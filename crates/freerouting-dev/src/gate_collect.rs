//! `freerouting-dev gate collect`: builds a records file for the gate from
//! `bench.json` files (one per arm) and, optionally, the KiCad oracle's
//! per-board `summary.json` files (`tools/kicad-oracle/grade-set.sh`).
//!
//! Per board and arm the metrics are:
//!
//! | metric | source |
//! |---|---|
//! | `unrouted`, `clearance_violations`, `vias`, `length_mm`, `bends` | the bench run's statistics (Freerouting's board model) |
//! | `wall_seconds` | the bench's median wall time |
//! | `tidy_<measure>_median`, `tidy_<measure>_p90` | the run's tidiness score ([`crate::tidy`], Q15.2), for `detour`, `bends_per_10mm`, `acute`, `jogs` and `vias`; from the bench run's `tidiness`, or the oracle summary's when the bench has none. Reported, not judged. |
//! | `via_in_pad` | vias whose copper overlaps an SMD pad, counted on the imported SES, leaving out the vias the input board already has ([`session_via_in_pad`]) |
//! | `drc_real`, `drc_real_by_type`, `below_minimum`, `kicad_unconnected` | the oracle's summary (KiCad's DRC) |
//! | tags `kicad_version`, `engine_commit` | the oracle and the jar name |
//!
//! A bench run that has no statistics, or whose runs disagree (the engine is
//! deterministic, D2), or an oracle summary with `graded: false`, makes the
//! arm `graded: false`: "not graded" is never a pass. A board the bench
//! skipped or couldn't find makes the run `partial`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use freerouting::board::BasicBoard;
use freerouting::board::items::Item;
use freerouting::geometry::IntPoint;
use freerouting::io::specctra::import_session;
use freerouting::library::PadstackId;
use freerouting::quality::feasibility::report::sha256_hex;

use crate::gate::{TAG_KEYS, to_writer};
use crate::json_read::Json;
use crate::ses_diff::parse_session;
use crate::{load_board, read_text};

/// One arm's inputs.
#[derive(Clone, Debug)]
pub struct ArmSource {
    pub name: String,
    pub bench_json: PathBuf,
    /// The output directory of `grade-set.sh` for this arm.
    pub oracle_dir: Option<PathBuf>,
}

/// What to collect.
#[derive(Clone, Debug, Default)]
pub struct CollectOptions {
    pub arms: Vec<ArmSource>,
    /// The bench engine whose rows are collected (default `java`).
    pub engine: String,
    /// `PREFIX=BOARD`: rows whose name starts with PREFIX belong to BOARD
    /// (variants of one board count once).
    pub groups: Vec<(String, String)>,
    /// A JSON object describing the arms (engine, settings); copied to the
    /// records' `arms` so a registration can be checked against it.
    pub arms_description: Option<Json>,
}

fn num(v: f64) -> Json {
    Json::Number(v)
}

fn text(s: impl Into<String>) -> Json {
    Json::String(s.into())
}

fn object(members: Vec<(&str, Json)>) -> Json {
    Json::Object(
        members
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
    )
}

/// The directory name the bench gives a board (its path as written, without
/// `.dsn`, other characters as `_`, leading `_` removed), which `grade-set.sh` also uses for a
/// project's base name.
pub fn bench_dir_name(written: &str) -> String {
    let name: String = written
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

/// The vias whose copper overlaps an SMD pad of the board (Java fork's
/// `via_in_pad.py`: a via in a pad is a failure, not a quality note).
pub fn via_in_pad_count(board: &BasicBoard) -> usize {
    vias_in_pads(board).count()
}

/// The vias of `board` whose copper overlaps an SMD pad (a pin on one layer),
/// of any net, on a layer both reach.
fn vias_in_pads(board: &BasicBoard) -> impl Iterator<Item = &Item> + '_ {
    let smd_pins: Vec<_> = board
        .items()
        .filter(|item| item.is_pin())
        .filter_map(|item| item.drill())
        .filter(|drill| drill.first_layer == drill.last_layer)
        .collect();
    board
        .items()
        .filter(|item| item.is_via())
        .filter(move |item| {
            let Some(via) = item.drill() else {
                return false;
            };
            (via.first_layer..=via.last_layer).any(|layer| {
                let Some(via_shape) = via.shape_on_layer(layer) else {
                    return false;
                };
                let via_box = via_shape.bounding_box();
                smd_pins.iter().any(|pin| {
                    pin.shape_on_layer(layer).is_some_and(|pad| {
                        pad.bounding_box().intersects_box(&via_box) && pad.intersects(via_shape)
                    })
                })
            })
        })
}

/// What makes two vias the same via: centre, padstack and nets.
type ViaKey = (IntPoint, PadstackId, Vec<i32>);

fn via_key(item: &Item) -> Option<ViaKey> {
    let drill = item.drill()?;
    Some((drill.center, drill.padstack, item.net_numbers().to_vec()))
}

/// Vias in pads that the session adds to the board (those already in the
/// input are not the router's).
///
/// A session repeats the input's wiring (the writer, like Java's
/// `SesWriter`, saves every via and trace but the system-fixed ones), and
/// importing it into the input board adds a second copy of each repeated
/// input via. So a via in a pad counts only when the input has no via with
/// the same centre, padstack and nets. A difference of plain counts counted
/// every repeated input via in a pad again: the designer's 42 thermal vias in
/// smoothieboard's exposed pads and 6 vias touching pads on kit-dev-coldfire
/// read as the router's in every arm.
pub fn session_via_in_pad(board: &BasicBoard, ses_path: &Path) -> Result<usize, String> {
    let session = parse_session(&read_text(ses_path)?)?;
    let mut routed = board.clone();
    import_session(&mut routed, &session);
    Ok(added_via_in_pad(board, &routed))
}

/// The vias in pads of `routed` that are not vias of `input`.
pub fn added_via_in_pad(input: &BasicBoard, routed: &BasicBoard) -> usize {
    let input_vias: BTreeSet<ViaKey> = input
        .items()
        .filter(|item| item.is_via())
        .filter_map(via_key)
        .collect();
    vias_in_pads(routed)
        .filter(|item| via_key(item).is_none_or(|key| !input_vias.contains(&key)))
        .count()
}

fn read_json(path: &Path) -> Result<Json, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    Json::parse(&String::from_utf8_lossy(&bytes)).map_err(|e| format!("{}: {e}", path.display()))
}

fn field<'a>(v: &'a Json, path: &str) -> Option<&'a Json> {
    v.path(path)
}

fn engine_commit(command: &str) -> Option<String> {
    let jar = command
        .split_whitespace()
        .skip_while(|w| *w != "-jar")
        .nth(1)?;
    let stem = Path::new(jar).file_stem()?.to_string_lossy().into_owned();
    Some(
        stem.strip_prefix("freerouting-")
            .unwrap_or(&stem)
            .to_string(),
    )
}

fn ungraded(reason: impl Into<String>) -> Json {
    object(vec![
        ("graded", Json::Bool(false)),
        ("reason", text(reason)),
    ])
}

/// The `tidy_<measure>_<median|p90>` members of a `tidiness` object (a bench
/// run's or an oracle summary's); none for a measure it lacks.
fn tidy_members(tidiness: &Json) -> Vec<(&'static str, Json)> {
    crate::tidy::GATE_METRICS
        .iter()
        .filter_map(|(key, stat, name)| {
            let v = tidiness
                .path(&format!("measures.{key}.{stat}"))
                .and_then(Json::as_f64)?;
            Some((*name, num(v)))
        })
        .collect()
}

/// The arm values of one bench board.
fn arm_from_bench(
    bench_dir: &Path,
    board: &Json,
    engine: &str,
    commit: Option<&str>,
    dsn_board: Option<&BasicBoard>,
) -> Json {
    let written = field(board, "board").and_then(Json::as_str).unwrap_or("?");
    let Some(Json::Array(engines)) = board.get("engines") else {
        return ungraded("bench.json has no engines for this board");
    };
    let Some(entry) = engines
        .iter()
        .find(|e| e.get("engine").and_then(Json::as_str) == Some(engine))
    else {
        return ungraded(format!("the bench has no {engine} engine"));
    };
    if let Some(Json::String(why)) = entry.get("skipped") {
        return ungraded(format!("skipped: {why}"));
    }
    let Some(Json::Array(runs)) = entry.get("runs") else {
        return ungraded("no runs");
    };
    let mut stats = Vec::new();
    let mut walls = Vec::new();
    for run in runs {
        if matches!(run.get("timed_out"), Some(Json::Bool(true))) {
            return ungraded("a run timed out");
        }
        match run.get("statistics") {
            Some(s @ Json::Object(_)) => stats.push(s),
            _ => {
                let note = run
                    .get("note")
                    .and_then(Json::as_str)
                    .unwrap_or("no statistics");
                return ungraded(note.to_string());
            }
        }
        if let Some(w) = field(run, "wall_seconds").and_then(Json::as_f64) {
            walls.push(w);
        }
    }
    let Some(first) = stats.first() else {
        return ungraded("no runs");
    };
    if stats.iter().any(|s| s != first) {
        return ungraded("the runs disagree; the engine is deterministic (D2), so that is a bug");
    }
    // The statistics keys are flat names with dots, as in Java's JSON.
    let get = |name: &str| first.get(name).and_then(Json::as_f64).unwrap_or(f64::NAN);
    let mut members = vec![
        ("unrouted", num(get("connections.incomplete_count"))),
        (
            "clearance_violations",
            num(get("clearance_violations.total_count")),
        ),
        ("vias", num(get("vias.total_count"))),
        ("length_mm", num(get("traces.total_length_mm"))),
        ("bends", num(get("bends.total_count"))),
    ];
    if let Some(tidiness) = runs.iter().find_map(|r| r.get("tidiness")) {
        members.extend(tidy_members(tidiness));
    }
    walls.sort_by(f64::total_cmp);
    if !walls.is_empty() {
        let mid = walls.len() / 2;
        let median = if walls.len() % 2 == 1 {
            walls[mid]
        } else {
            (walls[mid - 1] + walls[mid]) / 2.0
        };
        members.push(("wall_seconds", num(median)));
    }
    let ses = bench_dir
        .join(bench_dir_name(written))
        .join(format!("{engine}-run1.ses"));
    match dsn_board.map(|b| session_via_in_pad(b, &ses)) {
        Some(Ok(n)) => members.push(("via_in_pad", num(n as f64))),
        Some(Err(e)) => return ungraded(format!("via-in-pad check: {e}")),
        None => return ungraded("the board does not load"),
    }
    if let Some(c) = commit {
        members.push(("engine_commit", text(c)));
    }
    object(members)
}

/// Adds the oracle's metrics of `summary` to `arm` (or makes it ungraded).
fn merge_oracle(arm: Json, summary: Option<Json>, dir: &Path, name: &str) -> Json {
    let Json::Object(mut values) = arm else {
        return arm;
    };
    if matches!(values.get("graded"), Some(Json::Bool(false))) {
        return Json::Object(values);
    }
    let Some(summary) = summary else {
        return ungraded(format!(
            "no oracle summary at {}/{}/summary.json",
            dir.display(),
            bench_dir_name(name)
        ));
    };
    if !matches!(summary.get("graded"), Some(Json::Bool(true))) {
        let reason = summary
            .get("reason")
            .and_then(Json::as_str)
            .unwrap_or("not graded");
        return ungraded(format!("oracle: {reason}"));
    }
    let below: f64 = match field(&summary, "copper.below_minimum") {
        Some(Json::Object(o)) => o.values().filter_map(Json::as_f64).sum(),
        _ => 0.0,
    };
    values.insert(
        "drc_real".into(),
        num(summary
            .get("drc_real")
            .and_then(Json::as_f64)
            .unwrap_or(f64::NAN)),
    );
    if let Some(by_type) = summary.get("drc_real_by_type") {
        values.insert("drc_real_by_type".into(), by_type.clone());
    }
    values.insert("below_minimum".into(), num(below));
    if let Some(n) = summary.get("unconnected_items") {
        values.insert("kicad_unconnected".into(), n.clone());
    }
    if let Some(v) = summary.get("kicad_version") {
        values.insert("kicad_version".into(), v.clone());
    }
    if let Some(tidiness) = summary.get("tidiness") {
        for (key, value) in tidy_members(tidiness) {
            values.entry(key.to_string()).or_insert(value);
        }
    }
    Json::Object(values)
}

/// Builds the records document. `notes` collects what the user should know.
pub fn collect(options: &CollectOptions, notes: &mut Vec<String>) -> Result<Json, String> {
    if options.arms.len() < 2 {
        return Err("give at least two --arm NAME=bench.json[,oracle=DIR]".into());
    }
    let engine = if options.engine.is_empty() {
        "java"
    } else {
        &options.engine
    };
    let mut rows: BTreeMap<String, BTreeMap<String, Json>> = BTreeMap::new();
    let mut hashes: BTreeMap<String, String> = BTreeMap::new();
    let mut declared: BTreeMap<String, ()> = BTreeMap::new();
    let mut partial = false;
    let mut set = None;
    let mut boards_cache: BTreeMap<String, Option<BasicBoard>> = BTreeMap::new();
    for source in &options.arms {
        let bench = read_json(&source.bench_json)?;
        let bench_dir = source.bench_json.parent().unwrap_or(Path::new("."));
        set = set.or_else(|| bench.get("set").and_then(Json::as_str).map(str::to_string));
        if let Some(Json::String(why)) = bench.get("aborted") {
            partial = true;
            notes.push(format!("{}: the bench stopped early: {why}", source.name));
        }
        if let Some(Json::Array(missing)) = bench.get("missing") {
            for m in missing.iter().filter_map(Json::as_str) {
                partial = true;
                declared.insert(m.to_string(), ());
                notes.push(format!(
                    "{}: {m} is missing, so the run is partial",
                    source.name
                ));
            }
        }
        let commit = match bench.get("engines") {
            Some(Json::Array(list)) => list
                .iter()
                .find(|e| e.get("engine").and_then(Json::as_str) == Some(engine))
                .and_then(|e| e.get("command").and_then(Json::as_str))
                .and_then(engine_commit),
            _ => None,
        };
        let Some(Json::Array(boards)) = bench.get("boards") else {
            return Err(format!(
                "{}: no `boards` array",
                source.bench_json.display()
            ));
        };
        for board in boards {
            let written = field(board, "board")
                .and_then(Json::as_str)
                .ok_or("a bench board has no name")?
                .to_string();
            declared.insert(written.clone(), ());
            let path = PathBuf::from(
                field(board, "path")
                    .and_then(Json::as_str)
                    .ok_or("a bench board has no path")?,
            );
            if let Ok(bytes) = std::fs::read(&path) {
                let hash = sha256_hex(&bytes);
                if let Some(previous) = hashes.insert(written.clone(), hash.clone()) {
                    if previous != hash {
                        return Err(format!(
                            "{written}: the arms ran different input bytes ({previous} vs {hash})"
                        ));
                    }
                }
            }
            let dsn = boards_cache
                .entry(written.clone())
                .or_insert_with(|| load_board(&path).ok().map(|b| (*b).clone()));
            let mut arm = arm_from_bench(bench_dir, board, engine, commit.as_deref(), dsn.as_ref());
            if let Some(dir) = &source.oracle_dir {
                let summary = read_json(
                    &dir.join(bench_dir_name(
                        Path::new(&written)
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .unwrap_or(&written),
                    ))
                    .join("summary.json"),
                )
                .ok();
                arm = merge_oracle(arm, summary, dir, &written);
            }
            if matches!(arm.get("graded"), Some(Json::Bool(false))) {
                notes.push(format!(
                    "{} {written}: {}",
                    source.name,
                    arm.get("reason")
                        .and_then(Json::as_str)
                        .unwrap_or("not graded")
                ));
            }
            rows.entry(written)
                .or_default()
                .insert(source.name.clone(), arm);
        }
    }
    let arm_names: Vec<&String> = options.arms.iter().map(|a| &a.name).collect();
    let mut out_rows = BTreeMap::new();
    let mut metrics: BTreeMap<String, ()> = BTreeMap::new();
    for (name, arms) in rows {
        if arm_names.iter().any(|a| !arms.contains_key(*a)) {
            partial = true;
            notes.push(format!(
                "{name}: not in every arm's bench; left out, run is partial"
            ));
            continue;
        }
        let board = options
            .groups
            .iter()
            .find(|(prefix, _)| name.starts_with(prefix.as_str()))
            .map_or(name.clone(), |(_, board)| board.clone());
        for arm in arms.values() {
            if let Json::Object(values) = arm {
                for key in values.keys() {
                    if !TAG_KEYS.contains(&key.as_str())
                        && !["graded", "reason"].contains(&key.as_str())
                    {
                        metrics.insert(key.clone(), ());
                    }
                }
            }
        }
        let mut row = vec![("board", text(board)), ("arms", Json::Object(arms))];
        if let Some(hash) = hashes.get(&name) {
            row.push(("input_sha256", text(hash.clone())));
        }
        out_rows.insert(name, object(row));
    }
    let mut top = vec![
        ("format", text(crate::gate::RECORDS_FORMAT)),
        ("partial", Json::Bool(partial)),
        (
            "declared",
            Json::Array(declared.keys().map(|d| text(d.clone())).collect()),
        ),
        (
            "metrics",
            Json::Array(metrics.keys().map(|m| text(m.clone())).collect()),
        ),
        ("rows", Json::Object(out_rows)),
    ];
    if let Some(set) = set {
        top.push(("set", text(set)));
    }
    if let Some(description) = &options.arms_description {
        top.push(("arms", description.clone()));
    }
    if let (Some(first), Some(second)) = (options.arms.first(), options.arms.get(1)) {
        top.push(("baseline_arm", text(first.name.clone())));
        top.push(("candidate_arm", text(second.name.clone())));
    }
    Ok(object(top))
}

/// Writes the records document.
pub fn write_records(records: &Json, path: &Path) -> Result<(), String> {
    std::fs::write(path, to_writer(records).to_pretty_string() + "\n")
        .map_err(|e| format!("{}: {e}", path.display()))
}

//! `freerouting-dev tidy board.dsn routed.ses|routed.kicad_pcb [--json out.json]
//! [--connections]`: a measured score of how tidy a route is (Q15.1).
//!
//! KRT's users call its routes "far more natural and organized" than
//! Freerouting's Manhattan style. Nothing in the port measured that; this
//! does, from the routed copper alone, with no judgement by eye.
//!
//! # The five measures
//!
//! Per **connection** (a pair of pins joined by routed copper, chosen as
//! [`crate::tidy_copper`] describes: the spanning tree of the pins of each
//! piece of copper by copper path length, so a multi-pin net is its pin-to-pin
//! legs, not one per pair):
//!
//! | measure | definition |
//! |---|---|
//! | `detour` | length of the copper path over the straight distance between the two pin centres (1 = a straight track) |
//! | `bends_per_10mm` | direction changes of more than 10 degrees on the path, per 10 mm of copper |
//! | `acute` | bends sharper than 90 degrees (a turn of more than 90) |
//! | `jogs` | segments between two bends shorter than 2 track widths |
//! | `vias` | vias on the path (and through-hole pins it changes layer in) |
//!
//! [`crate::tidy_geometry`] has the exact bend, acute and jog rules. A
//! connection whose copper or pin distance is under one board unit is
//! skipped and counted. The board is scored as a whole by the **median** and
//! the **90th percentile** of each measure over its connections, so a few long
//! nets don't dominate. The median of an even count is the mean of the two
//! middle values; the 90th percentile is the nearest rank, the value at
//! position `ceil(0.9 * n)` of the sorted list. A measure over no connections
//! is left out of the JSON.
//!
//! All copper counts, whoever placed it, so a DSN that already holds routed
//! wiring is scored with it. Zones and pours carry no connection.
//!
//! # Input
//!
//! The board is a DSN. The routed copper is a Specctra session (Freerouting's,
//! Java's, KiCad's `ExportSpecctraSES`) or a `.kicad_pcb` (KiCadRoutingTools
//! writes a board, not a session); [`crate::tidy_input`] explains how a
//! `.kicad_pcb` is matched to the DSN. The same copper read either way scores
//! the same (`tests/tidy.rs`).
//!
//! # Output
//!
//! A table on stdout, or the JSON document `freerouting-tidy-1` with `--json
//! out.json` (`-` for stdout): see [`TidyReport::to_json`]. In bench records
//! (`bench.json`) the same measures are the `tidiness` object of each run.
//! Exit status 0 when it scored, 2 on an error.

use std::path::Path;
use std::process::ExitCode;

use freerouting::board::BasicBoard;
use freerouting::drc::json::JsonValue;

use crate::tidy_copper::{Connection, Measured, measure_board};
use crate::tidy_geometry::{BEND_DEGREES, JOG_WIDTHS};
use crate::tidy_input::{Imported, import_routed, routed_board};

/// The format tag of the JSON document.
pub const FORMAT: &str = "freerouting-tidy-1";

/// The measures, in report order: the JSON key and the table label.
pub const MEASURES: [(&str, &str); 5] = [
    ("detour", "detour"),
    ("bends_per_10mm", "bends/10mm"),
    ("acute", "acute"),
    ("jogs", "jogs"),
    ("vias", "vias/conn"),
];

/// The gate metrics ([`crate::gate::default_metrics`]): measure key,
/// statistic and the metric's name, `tidy_<measure>_<statistic>`.
pub static GATE_METRICS: [(&str, &str, &str); 10] = [
    ("detour", "median", "tidy_detour_median"),
    ("detour", "p90", "tidy_detour_p90"),
    ("bends_per_10mm", "median", "tidy_bends_per_10mm_median"),
    ("bends_per_10mm", "p90", "tidy_bends_per_10mm_p90"),
    ("acute", "median", "tidy_acute_median"),
    ("acute", "p90", "tidy_acute_p90"),
    ("jogs", "median", "tidy_jogs_median"),
    ("jogs", "p90", "tidy_jogs_p90"),
    ("vias", "median", "tidy_vias_median"),
    ("vias", "p90", "tidy_vias_p90"),
];

/// The median and 90th percentile of one measure.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Distribution {
    pub median: f64,
    pub p90: f64,
}

/// The median (mean of the middle two for an even count) and nearest-rank
/// 90th percentile of `values`; `None` if empty.
pub fn distribution(values: &[f64]) -> Option<Distribution> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    let median = if n % 2 == 1 {
        sorted[n / 2]
    } else {
        (sorted[n / 2 - 1] + sorted[n / 2]) / 2.0
    };
    let rank = (n * 9).div_ceil(10).max(1);
    Some(Distribution {
        median,
        p90: sorted[rank - 1],
    })
}

/// The tidiness of a board's routed copper.
#[derive(Clone, Debug, PartialEq)]
pub struct TidyReport {
    pub measured: Measured,
    /// One entry per [`MEASURES`], `None` without connections.
    pub measures: [Option<Distribution>; 5],
}

fn value(connection: &Connection, index: usize) -> f64 {
    match index {
        0 => connection.detour,
        1 => connection.bends_per_10mm,
        2 => connection.acute as f64,
        3 => connection.jogs as f64,
        _ => connection.vias as f64,
    }
}

/// Scores the copper on `board`.
pub fn tidy_board(board: &BasicBoard) -> TidyReport {
    let measured = measure_board(board);
    let mut measures = [None; 5];
    for (index, slot) in measures.iter_mut().enumerate() {
        let values: Vec<f64> = measured
            .connections
            .iter()
            .map(|c| value(c, index))
            .collect();
        *slot = distribution(&values);
    }
    TidyReport { measured, measures }
}

/// Imports the session `ses` into a copy of `board` and scores it (bench).
pub fn tidy_session(board: &BasicBoard, ses: &Path) -> Result<TidyReport, String> {
    let mut copy = board.clone();
    import_routed(&mut copy, ses)?;
    Ok(tidy_board(&copy))
}

fn number(v: f64) -> JsonValue {
    if v.is_finite() {
        JsonValue::Number(format!("{v:.4}"))
    } else {
        JsonValue::Null
    }
}

impl TidyReport {
    /// `{"format", "connections", "skipped", "nets", "bend_degrees",
    /// "jog_widths", "measures": {name: {"median", "p90"}}}`, plus
    /// `"connection_list"` with `detail`. Numbers have 4 decimals; a measure
    /// without connections is left out (the JSON writer drops nulls).
    pub fn to_json(&self, detail: bool) -> JsonValue {
        let measures = MEASURES
            .iter()
            .zip(&self.measures)
            .map(|((key, _), d)| {
                let body = d.map_or(JsonValue::Null, |d| {
                    JsonValue::Object(vec![
                        ("median".to_string(), number(d.median)),
                        ("p90".to_string(), number(d.p90)),
                    ])
                });
                (key.to_string(), body)
            })
            .collect();
        let mut members = vec![
            ("format".to_string(), JsonValue::string(FORMAT)),
            (
                "connections".to_string(),
                JsonValue::int(self.measured.connections.len() as i64),
            ),
            (
                "skipped".to_string(),
                JsonValue::int(self.measured.skipped as i64),
            ),
            (
                "nets".to_string(),
                JsonValue::int(self.measured.nets as i64),
            ),
            ("bend_degrees".to_string(), number(BEND_DEGREES)),
            ("jog_widths".to_string(), number(JOG_WIDTHS)),
            ("measures".to_string(), JsonValue::Object(measures)),
        ];
        if detail {
            members.push((
                "connection_list".to_string(),
                JsonValue::Array(
                    self.measured
                        .connections
                        .iter()
                        .map(connection_json)
                        .collect(),
                ),
            ));
        }
        JsonValue::Object(members)
    }

    /// The table printed by the command.
    pub fn to_text(&self) -> String {
        let mut out = format!(
            "{} connection(s) on {} net(s), {} skipped\n",
            self.measured.connections.len(),
            self.measured.nets,
            self.measured.skipped
        );
        out.push_str(&format!("{:<12} {:>9} {:>9}\n", "measure", "median", "p90"));
        for ((_, label), d) in MEASURES.iter().zip(&self.measures) {
            match d {
                Some(d) => out.push_str(&format!("{label:<12} {:>9.3} {:>9.3}\n", d.median, d.p90)),
                None => out.push_str(&format!("{label:<12} {:>9} {:>9}\n", "-", "-")),
            }
        }
        out
    }
}

fn connection_json(c: &Connection) -> JsonValue {
    JsonValue::Object(vec![
        ("net".to_string(), JsonValue::string(c.net.clone())),
        ("from".to_string(), JsonValue::string(c.from.clone())),
        ("to".to_string(), JsonValue::string(c.to.clone())),
        ("length_mm".to_string(), number(c.length_mm)),
        ("straight_mm".to_string(), number(c.straight_mm)),
        ("detour".to_string(), number(c.detour)),
        ("bends".to_string(), JsonValue::int(c.bends as i64)),
        ("bends_per_10mm".to_string(), number(c.bends_per_10mm)),
        ("acute".to_string(), JsonValue::int(c.acute as i64)),
        ("jogs".to_string(), JsonValue::int(c.jogs as i64)),
        ("vias".to_string(), JsonValue::int(c.vias as i64)),
    ])
}

const USAGE: &str = "usage: freerouting-dev tidy board.dsn routed.ses|routed.kicad_pcb \
[--json out.json|-] [--connections]";

/// Runs the subcommand.
pub fn run(args: &[String]) -> Result<ExitCode, String> {
    let mut positional = Vec::new();
    let mut json = None;
    let mut detail = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--json" => {
                json = Some(
                    args.get(i + 1)
                        .ok_or_else(|| format!("--json needs a value\n{USAGE}"))?
                        .clone(),
                );
                i += 2;
            }
            "--connections" => {
                detail = true;
                i += 1;
            }
            other if other.starts_with("--") => {
                return Err(format!("unknown option {other}\n{USAGE}"));
            }
            other => {
                positional.push(other.to_string());
                i += 1;
            }
        }
    }
    let [dsn, routed] = positional.as_slice() else {
        return Err(USAGE.to_string());
    };
    let (board, imported) = routed_board(Path::new(dsn), Path::new(routed))?;
    note_import(&imported);
    let report = tidy_board(&board);
    match json.as_deref() {
        Some("-") => println!("{}", report.to_json(detail).to_pretty_string()),
        Some(path) => {
            std::fs::write(path, report.to_json(detail).to_pretty_string() + "\n")
                .map_err(|e| format!("{path}: {e}"))?;
            print!("{}", report.to_text());
        }
        None => print!("{}", report.to_text()),
    }
    Ok(ExitCode::SUCCESS)
}

/// Tells on stderr what the import could not place.
fn note_import(imported: &Imported) {
    let s = &imported.summary;
    if s.errors_encountered > 0 {
        eprintln!(
            "tidy: {} net(s), wire(s) or via(s) could not be imported",
            s.errors_encountered
        );
    }
    if imported.via_substitutions > 0 {
        eprintln!(
            "tidy: {} via(s) took the board's first via type (their padstack is not in the DSN)",
            imported.via_substitutions
        );
    }
}

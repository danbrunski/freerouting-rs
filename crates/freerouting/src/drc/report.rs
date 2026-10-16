//! The DRC report in KiCad's JSON schema.
//!
//! Port of `io.kicad.KiCadDrcReport`, `KiCadDrcViolation`,
//! `KiCadDrcViolationItem` and `KiCadDrcPosition`, written like Java's
//! `GsonProvider.GSON`.

use super::json::{JsonValue, iso_offset_date_time_now};

/// The version string Freerouting writes into reports.
pub const FREEROUTING_VERSION: &str = concat!("rs-", env!("CARGO_PKG_VERSION"));

/// A position in a report, in the report's coordinate unit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrcPosition {
    pub x: f64,
    pub y: f64,
}

/// An item of a report violation.
#[derive(Clone, Debug, PartialEq)]
pub struct DrcViolationItem {
    pub description: String,
    pub pos: DrcPosition,
    /// The item id as a string.
    pub uuid: String,
}

/// A violation (or unconnected item) of a report.
#[derive(Clone, Debug, PartialEq)]
pub struct DrcViolation {
    pub description: String,
    pub items: Vec<DrcViolationItem>,
    pub severity: String,
    /// `clearance`, `holeClearance`, `track_dangling`, ...
    pub violation_type: String,
}

/// A DRC report in KiCad's JSON schema (`https://schemas.kicad.org/drc.v1.json`).
#[derive(Clone, Debug, PartialEq)]
pub struct DrcReport {
    pub coordinate_units: String,
    /// The creation time, `ISO_OFFSET_DATE_TIME` in UTC.
    pub date: String,
    pub freerouting_version: String,
    /// The name of the checked design file.
    pub source: String,
    pub unconnected_items: Vec<DrcViolation>,
    pub violations: Vec<DrcViolation>,
    pub quality_score: Option<f64>,
    pub optimizer_score: Option<f64>,
}

impl DrcReport {
    /// An empty report created now.
    pub fn new(coordinate_units: &str, source: &str, version: &str) -> Self {
        DrcReport {
            coordinate_units: coordinate_units.to_string(),
            date: iso_offset_date_time_now(),
            freerouting_version: version.to_string(),
            source: source.to_string(),
            unconnected_items: Vec::new(),
            violations: Vec::new(),
            quality_score: None,
            optimizer_score: None,
        }
    }

    /// The report as a JSON value with Java's member order.
    pub fn to_json_value(&self) -> JsonValue {
        let violations =
            |list: &[DrcViolation]| JsonValue::Array(list.iter().map(violation_json).collect());
        let score = |s: Option<f64>| s.map_or(JsonValue::Null, JsonValue::two_decimals);
        JsonValue::Object(vec![
            (
                "$schema".into(),
                JsonValue::string("https://schemas.kicad.org/drc.v1.json"),
            ),
            (
                "coordinateUnits".into(),
                JsonValue::string(&self.coordinate_units),
            ),
            ("date".into(), JsonValue::string(&self.date)),
            ("kicadVersion".into(), JsonValue::string("N/A")),
            (
                "freeroutingVersion".into(),
                JsonValue::string(&self.freerouting_version),
            ),
            ("source".into(), JsonValue::string(&self.source)),
            (
                "unconnectedItems".into(),
                violations(&self.unconnected_items),
            ),
            ("violations".into(), violations(&self.violations)),
            ("schematicParity".into(), JsonValue::Array(Vec::new())),
            ("qualityScore".into(), score(self.quality_score)),
            ("optimizerScore".into(), score(self.optimizer_score)),
        ])
    }

    /// The report as pretty printed JSON, like Java's
    /// `GsonProvider.GSON.toJson(report)`.
    pub fn to_json(&self) -> String {
        self.to_json_value().to_pretty_string()
    }
}

fn violation_json(v: &DrcViolation) -> JsonValue {
    JsonValue::Object(vec![
        ("description".into(), JsonValue::string(&v.description)),
        (
            "items".into(),
            JsonValue::Array(
                v.items
                    .iter()
                    .map(|item| {
                        JsonValue::Object(vec![
                            ("description".into(), JsonValue::string(&item.description)),
                            (
                                "pos".into(),
                                JsonValue::Object(vec![
                                    ("x".into(), JsonValue::two_decimals(item.pos.x)),
                                    ("y".into(), JsonValue::two_decimals(item.pos.y)),
                                ]),
                            ),
                            ("uuid".into(), JsonValue::string(&item.uuid)),
                        ])
                    })
                    .collect(),
            ),
        ),
        ("severity".into(), JsonValue::string(&v.severity)),
        ("type".into(), JsonValue::string(&v.violation_type)),
    ])
}

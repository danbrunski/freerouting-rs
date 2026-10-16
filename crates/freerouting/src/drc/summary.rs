//! A concise diagnostic summary of the DRC findings.
//!
//! Port of `drc.DrcSummaryResponse`.

use super::json::JsonValue;

/// One violation of the summary.
#[derive(Clone, Debug, PartialEq)]
pub struct DiagnosticViolation {
    pub violation_type: String,
    pub severity: String,
    pub layer: String,
    pub explanation: String,
    pub expected_clearance_mm: Option<f64>,
    pub actual_clearance_mm: Option<f64>,
    pub shortfall_mm: Option<f64>,
    pub items: Vec<String>,
}

/// A cluster of clearance violations.
#[derive(Clone, Debug, PartialEq)]
pub struct CongestionZone {
    pub center_x_mm: f64,
    pub center_y_mm: f64,
    pub radius_mm: f64,
    pub violation_count: usize,
    pub description: String,
}

/// Port of `DrcSummaryResponse`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DrcSummary {
    pub clearance_violations_count: usize,
    pub unconnected_nets_count: usize,
    pub violations: Vec<DiagnosticViolation>,
    pub congestion_zones: Vec<CongestionZone>,
    pub hints: Vec<String>,
}

impl DrcSummary {
    /// The summary as JSON with Java's member names and order.
    pub fn to_json_value(&self) -> JsonValue {
        let opt = |v: Option<f64>| v.map_or(JsonValue::Null, JsonValue::two_decimals);
        JsonValue::Object(vec![
            (
                "clearance_violations_count".into(),
                JsonValue::int(self.clearance_violations_count as i64),
            ),
            (
                "unconnected_nets_count".into(),
                JsonValue::int(self.unconnected_nets_count as i64),
            ),
            (
                "violations".into(),
                JsonValue::Array(
                    self.violations
                        .iter()
                        .map(|v| {
                            JsonValue::Object(vec![
                                ("type".into(), JsonValue::string(&v.violation_type)),
                                ("severity".into(), JsonValue::string(&v.severity)),
                                ("layer".into(), JsonValue::string(&v.layer)),
                                ("explanation".into(), JsonValue::string(&v.explanation)),
                                ("expected_clearance_mm".into(), opt(v.expected_clearance_mm)),
                                ("actual_clearance_mm".into(), opt(v.actual_clearance_mm)),
                                ("shortfall_mm".into(), opt(v.shortfall_mm)),
                                (
                                    "items".into(),
                                    JsonValue::Array(
                                        v.items.iter().map(JsonValue::string).collect(),
                                    ),
                                ),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "congestion_zones".into(),
                JsonValue::Array(
                    self.congestion_zones
                        .iter()
                        .map(|z| {
                            JsonValue::Object(vec![
                                ("center_x_mm".into(), JsonValue::two_decimals(z.center_x_mm)),
                                ("center_y_mm".into(), JsonValue::two_decimals(z.center_y_mm)),
                                ("radius_mm".into(), JsonValue::two_decimals(z.radius_mm)),
                                (
                                    "violation_count".into(),
                                    JsonValue::int(z.violation_count as i64),
                                ),
                                ("description".into(), JsonValue::string(&z.description)),
                            ])
                        })
                        .collect(),
                ),
            ),
            (
                "hints".into(),
                JsonValue::Array(self.hints.iter().map(JsonValue::string).collect()),
            ),
        ])
    }
}

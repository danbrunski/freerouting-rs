//! The comparison dump: the static part of C01.1's `settings` JSON.
//!
//! C01's Java dump writes the final `RouterSettings` with a plain Gson
//! (full-precision numbers, `null`s left out, Java's `transient` fields
//! omitted, except the optimizer strategies, which the harness adds as
//! strings) and normalises machine-dependent values: any `max_threads`
//! equal to `max(1, cores - 1)` is the string `"<cores-1>"`, one equal to
//! `cores` is `"<cores>"`. This writes the same keys. "Static" leaves out
//! `layers`, the two cost arrays and `costs_initialized`, which C03 compares
//! after the board tuning.

use super::fields::{FIELDS, SettingsPatch};
use super::merge::Resolved;
use super::text::{java_double_to_string, java_float_to_string};
use super::types::FieldValue;
use crate::drc::json::JsonValue;

/// The per-layer cost arrays are compared after the board tuning (C03).
const EXCLUDED: &[&str] = &[
    "scoring.preferred_direction_trace_cost",
    "scoring.undesired_direction_trace_cost",
];

fn value_json(path: &str, value: FieldValue, cores: usize) -> JsonValue {
    match value {
        FieldValue::Bool(b) => JsonValue::Bool(b),
        FieldValue::Int(v) => {
            if path.ends_with("max_threads") {
                normalised_threads(i64::from(v), cores)
            } else {
                JsonValue::int(i64::from(v))
            }
        }
        FieldValue::Long(v) => JsonValue::int(v),
        FieldValue::Float(v) => JsonValue::Number(java_float_to_string(v)),
        FieldValue::Double(v) => JsonValue::Number(java_double_to_string(v)),
        FieldValue::Str(s) => JsonValue::String(s),
        FieldValue::StrList(list) => {
            JsonValue::Array(list.into_iter().map(JsonValue::String).collect())
        }
        FieldValue::DoubleList(list) => JsonValue::Array(
            list.into_iter()
                .map(|v| JsonValue::Number(java_double_to_string(v)))
                .collect(),
        ),
        FieldValue::Enum(name) => JsonValue::string(name),
    }
}

fn normalised_threads(value: i64, cores: usize) -> JsonValue {
    let cores = cores as i64;
    if value == (cores - 1).max(1) {
        JsonValue::string("<cores-1>")
    } else if value == cores {
        JsonValue::string("<cores>")
    } else {
        JsonValue::int(value)
    }
}

/// The static settings of a patch as JSON.
pub fn dump_patch(patch: &SettingsPatch, cores: usize) -> JsonValue {
    let mut top: Vec<(String, JsonValue)> = Vec::new();
    let mut sections: Vec<(String, Vec<(String, JsonValue)>)> = Vec::new();
    for field in FIELDS.iter().filter(|f| !EXCLUDED.contains(&f.path)) {
        let Some(value) = (field.get)(patch) else {
            continue;
        };
        let json = value_json(field.path, value, cores);
        if field.section.is_empty() {
            top.push((field.name.to_string(), json));
        } else {
            match sections.iter_mut().find(|(name, _)| name == field.section) {
                Some((_, members)) => members.push((field.name.to_string(), json)),
                None => sections.push((
                    field.section.to_string(),
                    vec![(field.name.to_string(), json)],
                )),
            }
        }
    }
    let mut members: Vec<(String, JsonValue)> = sections
        .into_iter()
        .map(|(name, members)| (name, JsonValue::Object(members)))
        .collect();
    members.extend(top);
    if let Some(warnings) = patch.validation_warnings.as_ref() {
        members.push((
            "validation_warnings".to_string(),
            JsonValue::Array(warnings.iter().cloned().map(JsonValue::String).collect()),
        ));
    }
    JsonValue::Object(members)
}

/// The static part of C01.1's `settings` JSON for a resolved merge.
pub fn dump(resolved: &Resolved) -> JsonValue {
    dump_patch(&resolved.patch, resolved.cores)
}

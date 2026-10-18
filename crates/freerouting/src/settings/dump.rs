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
//!
//! [`effective_rows`] is the other view (N01.4): every field with its
//! effective value, the source that set it, and whether it came from the
//! routing profile or differs from the profile's value. `--dump-settings`
//! prints it as [`effective_text`] or [`effective_json`].

use super::fields::{FIELDS, SettingsPatch};
use super::merge::{Resolved, Source};
use super::profile::Profile;
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

/// One row of the effective settings (N01.4): what `--dump-settings` and
/// the GUI's settings panel show.
#[derive(Clone, Debug, PartialEq)]
pub struct EffectiveRow {
    /// The canonical path, without `router.`.
    pub path: &'static str,
    /// The value; `None` is unset.
    pub value: Option<FieldValue>,
    /// The label of the last source that set it (`defaults`, `profile
    /// enhanced`, `CLI`, ...); `None` when no source did.
    pub source: Option<String>,
    /// The value came from the routing profile's table.
    pub from_profile: bool,
    /// The value differs from the profile's ([`Profile::defaults`]).
    pub differs_from_profile: bool,
}

/// The effective value of every field of `patch`, where it came from and
/// whether it differs from `profile`'s value.
pub fn effective_rows(
    patch: &SettingsPatch,
    provenance: &std::collections::BTreeMap<&'static str, Source>,
    profile: Profile,
    cores: usize,
) -> Vec<EffectiveRow> {
    let baseline = profile.defaults(cores);
    FIELDS
        .iter()
        .map(|field| {
            let value = (field.get)(patch);
            let source = provenance.get(field.path);
            EffectiveRow {
                path: field.path,
                differs_from_profile: field.path != "profile" && value != (field.get)(&baseline),
                value,
                source: source.map(|s| s.label().to_string()),
                from_profile: matches!(source, Some(Source::Profile(_))),
            }
        })
        .collect()
}

/// A field value as plain JSON (no thread normalisation).
pub fn value_to_json(value: FieldValue) -> JsonValue {
    match value {
        FieldValue::Bool(b) => JsonValue::Bool(b),
        FieldValue::Int(v) => JsonValue::int(i64::from(v)),
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

/// The effective settings as JSON: `{"profile": ..., "settings": [{"path",
/// "value", "source", "from_profile", "differs_from_profile"}, ...]}`.
pub fn effective_json(profile: Profile, rows: &[EffectiveRow]) -> JsonValue {
    let rows = rows
        .iter()
        .map(|row| {
            JsonValue::Object(vec![
                ("path".to_string(), JsonValue::string(row.path)),
                (
                    "value".to_string(),
                    row.value.clone().map_or(JsonValue::Null, value_to_json),
                ),
                (
                    "source".to_string(),
                    row.source
                        .clone()
                        .map_or(JsonValue::Null, JsonValue::String),
                ),
                (
                    "from_profile".to_string(),
                    JsonValue::Bool(row.from_profile),
                ),
                (
                    "differs_from_profile".to_string(),
                    JsonValue::Bool(row.differs_from_profile),
                ),
            ])
        })
        .collect();
    JsonValue::Object(vec![
        ("profile".to_string(), JsonValue::string(profile.name())),
        ("settings".to_string(), JsonValue::Array(rows)),
    ])
}

/// The effective settings as text, one `path = value  (source)` line per
/// field after a `profile` line. `[profile]` marks a value that came from
/// the profile's table, `*` one that differs from the profile's value.
pub fn effective_text(profile: Profile, rows: &[EffectiveRow]) -> String {
    let mut out = format!(
        "Routing profile: {} ({})\n\
         [profile] = set by the profile, * = differs from the profile's value\n",
        profile.name(),
        profile.description()
    );
    let width = rows.iter().map(|r| r.path.len()).max().unwrap_or(0);
    for row in rows {
        let value = row
            .value
            .as_ref()
            .map_or_else(|| "(unset)".to_string(), ToString::to_string);
        let source = row.source.as_deref().unwrap_or("-");
        let mark = if row.from_profile { " [profile]" } else { "" };
        let differs = if row.differs_from_profile { "*" } else { " " };
        out.push_str(&format!(
            "{differs} {:width$} = {value}  ({source}){mark}\n",
            row.path
        ));
    }
    out
}

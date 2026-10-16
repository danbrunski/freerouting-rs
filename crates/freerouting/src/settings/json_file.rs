//! Java's `JsonFileSettings`, `RouterSettingsTypeAdapterFactory` and
//! `LegacyRouterSettingsBridge.absorbLegacyJson`: a `freerouting.json` (or a
//! bare router object) to a [`SettingsPatch`].
//!
//! - The settings are the file's `"router"` member; other top-level sections
//!   (`gui`, `profile`, `version`, ...) are ignored, as are unknown keys.
//! - Names match exactly (the JSON name or a Gson alternate), with no case
//!   folding, unlike the CLI.
//! - Gson's lenient number handling is reproduced: numbers given as strings
//!   are accepted, and `42.0` is a valid `Integer`.
//! - Java's `transient` fields (`scoring.preferred_direction_trace_cost`,
//!   `scoring.undesired_direction_trace_cost`, the optimizer strategies)
//!   are not read; `layers` is.
//! - Flat autorouter and cost keys apply only where the nested value is
//!   unset, with one deprecation warning for the autorouter keys.

use std::fmt;
use std::path::{Path, PathBuf};

use super::config_path::{ConfigChoice, EnvLookup, Os, config_file};
use super::fields::{FIELDS, SECTIONS, SettingsPatch};
use super::path::deprecated_path_warning;
use super::text::{parse_boolean, parse_double, parse_int, parse_long};
use super::types::{FieldDesc, FieldKind, FieldValue, LAYER_FIELDS, LayerSettings};
use crate::drc::json::JsonValue;

/// A settings patch read from JSON, with the warnings Java logs.
#[derive(Clone, Debug, Default)]
pub struct JsonSettings {
    pub patch: SettingsPatch,
    pub warnings: Vec<String>,
}

fn type_error(expected: &str, got: &JsonValue) -> String {
    let found = match got {
        JsonValue::Null => "NULL",
        JsonValue::Bool(_) => "BOOLEAN",
        JsonValue::Number(_) => "NUMBER",
        JsonValue::String(_) => "STRING",
        JsonValue::Array(_) => "BEGIN_ARRAY",
        JsonValue::Object(_) => "BEGIN_OBJECT",
    };
    format!("Expected {expected} but was {found}")
}

/// The text a Gson number/string token reads as.
fn scalar_text(value: &JsonValue) -> Option<&str> {
    match value {
        JsonValue::Number(t) | JsonValue::String(t) => Some(t),
        _ => None,
    }
}

/// `JsonReader.nextInt` on a number or string token.
fn gson_int(text: &str) -> Result<i32, String> {
    if let Ok(v) = parse_int(text) {
        return Ok(v);
    }
    let d = parse_double(text)?;
    let r = d as i32;
    if f64::from(r) != d {
        return Err(format!("Expected an int but was {text}"));
    }
    Ok(r)
}

/// `JsonReader.nextLong`.
fn gson_long(text: &str) -> Result<i64, String> {
    if let Ok(v) = parse_long(text) {
        return Ok(v);
    }
    let d = parse_double(text)?;
    let r = d as i64;
    if r as f64 != d {
        return Err(format!("Expected a long but was {text}"));
    }
    Ok(r)
}

/// Converts a JSON value the way Gson's adapter for the field's Java type
/// does. `Ok(None)` is a JSON `null` (the field stays unset).
fn gson_value(kind: FieldKind, value: &JsonValue) -> Result<Option<FieldValue>, String> {
    if matches!(value, JsonValue::Null) {
        return Ok(None);
    }
    let converted = match kind {
        FieldKind::Bool => match value {
            JsonValue::Bool(b) => FieldValue::Bool(*b),
            JsonValue::String(s) => FieldValue::Bool(parse_boolean(s)),
            other => return Err(type_error("a boolean", other)),
        },
        FieldKind::Int => FieldValue::Int(gson_int(
            scalar_text(value).ok_or_else(|| type_error("an int", value))?,
        )?),
        FieldKind::Long => FieldValue::Long(gson_long(
            scalar_text(value).ok_or_else(|| type_error("a long", value))?,
        )?),
        // Gson reads a Float as `(float) in.nextDouble()`.
        FieldKind::Float => FieldValue::Float(parse_double(
            scalar_text(value).ok_or_else(|| type_error("a double", value))?,
        )? as f32),
        FieldKind::Double => FieldValue::Double(parse_double(
            scalar_text(value).ok_or_else(|| type_error("a double", value))?,
        )?),
        FieldKind::Str => match value {
            JsonValue::Bool(b) => FieldValue::Str(b.to_string()),
            other => FieldValue::Str(
                scalar_text(other)
                    .ok_or_else(|| type_error("a string", other))?
                    .to_string(),
            ),
        },
        FieldKind::StrList => {
            let JsonValue::Array(items) = value else {
                return Err(type_error("BEGIN_ARRAY", value));
            };
            let mut names = Vec::new();
            for item in items {
                match item {
                    // A `null` name would be skipped by every user.
                    JsonValue::Null => {}
                    JsonValue::Bool(b) => names.push(b.to_string()),
                    other => names.push(
                        scalar_text(other)
                            .ok_or_else(|| type_error("a string", other))?
                            .to_string(),
                    ),
                }
            }
            FieldValue::StrList(names)
        }
        FieldKind::DoubleList => return Err("double arrays are not read from JSON".to_string()),
        enum_kind => {
            // Enums match the constant name exactly; an unknown name is null.
            let name = match value {
                JsonValue::Bool(b) => b.to_string(),
                other => scalar_text(other)
                    .ok_or_else(|| type_error("a string", other))?
                    .to_string(),
            };
            match FieldValue::enum_constant(enum_kind, &name) {
                Some(FieldValue::Enum(constant)) if constant == name => FieldValue::Enum(constant),
                _ => return Ok(None),
            }
        }
    };
    Ok(Some(converted))
}

fn field_matches_json(field: &FieldDesc, name: &str) -> bool {
    field.json && (field.name == name || field.alternates.contains(&name))
}

fn layers_from_json(value: &JsonValue) -> Result<Option<Vec<LayerSettings>>, String> {
    let items = match value {
        JsonValue::Null => return Ok(None),
        JsonValue::Array(items) => items,
        other => return Err(type_error("BEGIN_ARRAY", other)),
    };
    let mut layers = Vec::new();
    for item in items {
        let mut layer = LayerSettings::default();
        match item {
            // A null layer behaves like an empty one everywhere.
            JsonValue::Null => {}
            JsonValue::Object(members) => {
                for (name, v) in members {
                    if let Some(field) = LAYER_FIELDS.iter().find(|f| f.name == name)
                        && let Some(converted) = gson_value(field.kind, v)?
                    {
                        (field.set)(&mut layer, converted);
                    }
                }
            }
            other => return Err(type_error("BEGIN_OBJECT", other)),
        }
        layers.push(layer);
    }
    Ok(Some(layers))
}

/// `JsonPrimitive.getAsInt` for the legacy flat keys.
fn as_int(value: &JsonValue) -> Result<i32, String> {
    match value {
        JsonValue::Number(text) => {
            if let Ok(v) = parse_int(text) {
                return Ok(v);
            }
            // `LazilyParsedNumber.intValue()`: long, then BigDecimal.
            if let Ok(v) = parse_long(text) {
                return Ok(v as i32);
            }
            Ok(parse_double(text)? as i64 as i32)
        }
        JsonValue::String(text) => parse_int(text),
        JsonValue::Bool(b) => parse_int(&b.to_string()),
        other => Err(type_error("a primitive", other)),
    }
}

/// `JsonPrimitive.getAsDouble`.
fn as_double(value: &JsonValue) -> Result<f64, String> {
    match value {
        JsonValue::Number(text) | JsonValue::String(text) => parse_double(text),
        JsonValue::Bool(b) => parse_double(&b.to_string()),
        other => Err(type_error("a primitive", other)),
    }
}

/// `JsonPrimitive.getAsBoolean`.
fn as_boolean(value: &JsonValue) -> Result<bool, String> {
    match value {
        JsonValue::Bool(b) => Ok(*b),
        JsonValue::Number(text) | JsonValue::String(text) => Ok(parse_boolean(text)),
        other => Err(type_error("a primitive", other)),
    }
}

/// `JsonPrimitive.getAsString`.
fn as_string(value: &JsonValue) -> Result<String, String> {
    match value {
        JsonValue::Bool(b) => Ok(b.to_string()),
        JsonValue::Number(text) | JsonValue::String(text) => Ok(text.clone()),
        other => Err(type_error("a primitive", other)),
    }
}

fn member<'a>(members: &'a [(String, JsonValue)], name: &str) -> Option<&'a JsonValue> {
    // `JsonObject.get` returns the last of duplicate names.
    members
        .iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
        .filter(|v| !matches!(v, JsonValue::Null))
}

/// `LegacyRouterSettingsBridge.absorbLegacyJson`.
fn absorb_legacy(
    members: &[(String, JsonValue)],
    patch: &mut SettingsPatch,
    warnings: &mut Vec<String>,
) -> Result<(), String> {
    let mut warned = false;
    if patch.autorouter.enabled.is_none()
        && let Some(v) = member(members, "enabled")
    {
        patch.autorouter.enabled = Some(as_boolean(v)?);
        warned = true;
    }
    if patch.autorouter.algorithm.is_none()
        && let Some(v) = member(members, "algorithm")
    {
        patch.autorouter.algorithm = Some(as_string(v)?);
        warned = true;
    }
    if patch.autorouter.max_passes.is_none()
        && let Some(v) = member(members, "max_passes")
    {
        patch.autorouter.max_passes = Some(as_int(v)?);
        warned = true;
    }
    if patch.autorouter.max_items.is_none()
        && let Some(v) = member(members, "max_items")
    {
        patch.autorouter.max_items = Some(as_int(v)?);
        warned = true;
    }
    if patch.autorouter.save_intermediate_stages.is_none()
        && let Some(v) = member(members, "save_intermediate_stages")
    {
        patch.autorouter.save_intermediate_stages = Some(as_boolean(v)?);
        warned = true;
    }
    if patch.autorouter.ignore_net_classes.is_none()
        && let Some(JsonValue::Array(items)) = member(members, "ignore_net_classes")
    {
        let mut names = Vec::new();
        for item in items {
            if !matches!(item, JsonValue::Null) {
                names.push(as_string(item)?);
            }
        }
        patch.autorouter.ignore_net_classes = Some(names);
        warned = true;
    }
    if warned {
        warnings.push(deprecated_path_warning(
            "router.max_passes (and other flat autorouter keys)",
            "router.autorouter.max_passes",
        ));
    }
    // The flat cost keys (D8, fork commit `77d6158a8`); nested values win.
    if patch.scoring.via_costs.is_none()
        && let Some(v) = member(members, "via_costs")
    {
        patch.scoring.via_costs = Some(as_int(v)?);
    }
    if patch.scoring.plane_via_costs.is_none()
        && let Some(v) = member(members, "plane_via_costs")
    {
        patch.scoring.plane_via_costs = Some(as_int(v)?);
    }
    if patch.scoring.start_ripup_costs.is_none()
        && let Some(v) = member(members, "start_ripup_costs")
    {
        patch.scoring.start_ripup_costs = Some(as_int(v)?);
    }
    if patch
        .scoring
        .default_preferred_direction_trace_cost
        .is_none()
        && let Some(v) = member(members, "default_preferred_direction_trace_cost")
    {
        patch.scoring.default_preferred_direction_trace_cost = Some(as_double(v)?);
    }
    if patch
        .scoring
        .default_undesired_direction_trace_cost
        .is_none()
        && let Some(v) = member(members, "default_undesired_direction_trace_cost")
    {
        patch.scoring.default_undesired_direction_trace_cost = Some(as_double(v)?);
    }
    Ok(())
}

/// `GSON.fromJson(element, RouterSettings.class)` with the type adapter
/// factory: a JSON object holding router settings to a patch. An `Err` is
/// a `JsonSyntaxException`, which fails the whole read.
pub fn patch_from_router_json(value: &JsonValue) -> Result<JsonSettings, String> {
    let JsonValue::Object(members) = value else {
        return Err(type_error("BEGIN_OBJECT", value));
    };
    let mut out = JsonSettings::default();
    for (name, v) in members {
        if name == "validation_warnings" {
            if let Some(FieldValue::StrList(list)) = gson_value(FieldKind::StrList, v)? {
                out.patch.validation_warnings = Some(list);
            }
            continue;
        }
        if let Some(section) = SECTIONS.iter().find(|s| s.name == name) {
            match v {
                JsonValue::Null => {}
                JsonValue::Object(inner) => {
                    for (field_name, field_value) in inner {
                        if let Some(field) = FIELDS.iter().find(|f| {
                            f.section == section.name && field_matches_json(f, field_name)
                        }) && let Some(converted) = gson_value(field.kind, field_value)?
                        {
                            (field.set)(&mut out.patch, converted);
                        }
                    }
                }
                other => return Err(type_error("BEGIN_OBJECT", other)),
            }
            continue;
        }
        if let Some(field) = FIELDS
            .iter()
            .find(|f| f.section.is_empty() && field_matches_json(f, name))
            && let Some(converted) = gson_value(field.kind, v)?
        {
            (field.set)(&mut out.patch, converted);
        }
    }
    absorb_legacy(members, &mut out.patch, &mut out.warnings)?;
    if let Some(layers) = member_raw(members, "layers") {
        out.patch.layers = layers_from_json(layers)?;
    }
    Ok(out)
}

fn member_raw<'a>(members: &'a [(String, JsonValue)], name: &str) -> Option<&'a JsonValue> {
    members
        .iter()
        .rev()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v)
}

/// `GSON.fromJson(text, RouterSettings.class)`: a JSON text holding router
/// settings (an API payload, not a config file).
pub fn patch_from_json_text(text: &str) -> Result<JsonSettings, String> {
    let value = JsonValue::parse(text).map_err(|e| e.to_string())?;
    patch_from_router_json(&value)
}

/// What `JsonFileSettings` makes of a file's text: the `"router"` member's
/// settings. Missing member, non-object member, a non-object document and
/// malformed JSON all give `Err` here; the caller decides whether that is
/// an empty source (the default file) or an error (`--settings`).
pub fn patch_from_config_text(text: &str) -> Result<JsonSettings, String> {
    let root = JsonValue::parse(text).map_err(|e| e.to_string())?;
    let JsonValue::Object(members) = &root else {
        return Err(type_error("BEGIN_OBJECT", &root));
    };
    match member_raw(members, "router") {
        Some(router @ JsonValue::Object(_)) => patch_from_router_json(router),
        // No router section: nothing to read, not an error.
        _ => Ok(JsonSettings::default()),
    }
}

/// Why a settings file could not be used.
#[derive(Debug)]
pub enum ConfigError {
    /// An explicit `--settings` file does not exist.
    Missing(PathBuf),
    /// The file could not be read.
    Io(PathBuf, std::io::Error),
    /// The file is not valid settings JSON.
    Malformed(PathBuf, String),
}

impl fmt::Display for ConfigError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConfigError::Missing(p) => write!(f, "Settings file not found: {}", p.display()),
            ConfigError::Io(p, e) => write!(f, "Failed to read settings file {}: {e}", p.display()),
            ConfigError::Malformed(p, m) => {
                write!(f, "Failed to parse settings file {}: {m}", p.display())
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Loads a settings file as `JsonFileSettings` does.
///
/// With `explicit == false` (the default config file) a missing file gives
/// empty settings and a malformed one gives a warning and empty settings.
/// With `explicit == true` (`--settings FILE`) either is an error.
pub fn load_config_file(path: &Path, explicit: bool) -> Result<JsonSettings, ConfigError> {
    let text = match std::fs::read(path) {
        Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return if explicit {
                Err(ConfigError::Missing(path.to_path_buf()))
            } else {
                Ok(JsonSettings::default())
            };
        }
        Err(e) => {
            return if explicit {
                Err(ConfigError::Io(path.to_path_buf(), e))
            } else {
                Ok(JsonSettings {
                    patch: SettingsPatch::default(),
                    warnings: vec![format!(
                        "Failed to load settings from JSON file: {}: {e}",
                        path.display()
                    )],
                })
            };
        }
    };
    let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
    match patch_from_config_text(text) {
        Ok(settings) => Ok(settings),
        Err(message) if explicit => Err(ConfigError::Malformed(path.to_path_buf(), message)),
        Err(message) => Ok(JsonSettings {
            patch: SettingsPatch::default(),
            warnings: vec![format!(
                "Failed to parse JSON settings file: {}: {message}",
                path.display()
            )],
        }),
    }
}

/// The config-file source for a command line: the file `choice` points at
/// (see `config_path`), loaded as [`load_config_file`] does. `--settings`
/// replaces the default file, it does not stack with it. Empty settings
/// when there is no default location.
pub fn load_config(
    choice: &ConfigChoice,
    env: &dyn EnvLookup,
    os: Os,
) -> Result<JsonSettings, ConfigError> {
    match config_file(choice, env, os) {
        Some(path) => load_config_file(&path, matches!(choice, ConfigChoice::Explicit(_))),
        None => Ok(JsonSettings::default()),
    }
}

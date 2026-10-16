//! Java's `ReflectionUtil.setFieldValue` and `convertValue` over the field
//! table, `LegacyRouterSettingsBridge.canonicalCliPath` and
//! `CliSettings.applyRouterSetting`.
//!
//! A path such as `autorouter.max_passes`, `optimizer-max_threads` or
//! `layers.routable` addresses one field. Path separators are `.`, `:` and
//! `-`. A segment matches, ignoring case, the JSON name, its lowerCamel
//! form, the Java field name or a Gson alternate.

use std::fmt;

use super::fields::{FIELDS, SECTIONS, SettingsPatch};
use super::text::{
    java_float_to_string, java_split, parse_boolean, parse_double, parse_float, parse_int,
    parse_long,
};
use super::types::{
    FieldDesc, FieldKind, FieldValue, Flat, LAYER_FIELDS, LayerSettings, SectionDesc,
};
use crate::util::equals_ignore_case;

/// Why a path could not be set.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SetError {
    /// No such field (Java's `NoSuchFieldException`).
    UnknownPath(String),
    /// The field exists but the value does not fit (Java's
    /// `NumberFormatException` or `IllegalArgumentException`).
    BadValue(String),
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SetError::UnknownPath(name) => {
                write!(f, "No field found with name or SerializedName: {name}")
            }
            SetError::BadValue(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SetError {}

/// `ReflectionUtil.snakeToLowerCamel`.
pub fn snake_to_lower_camel(name: &str) -> String {
    if !name.contains('_') {
        return name.to_string();
    }
    let parts = java_split(name, '_');
    let mut out = parts.first().map(|p| p.to_lowercase()).unwrap_or_default();
    for part in parts.iter().skip(1) {
        let mut chars = part.chars();
        if let Some(first) = chars.next() {
            out.extend(first.to_uppercase());
            out.push_str(&chars.as_str().to_lowercase());
        }
    }
    out
}

/// `ReflectionUtil.getFieldByNameOrSerializedName` for one candidate field.
/// `json` is `None` for a field without a `@SerializedName`.
fn name_matches(segment: &str, json: Option<&str>, alternates: &[&str], java: &str) -> bool {
    let camel = snake_to_lower_camel(segment);
    let serialized = |s: &str| {
        equals_ignore_case(s, segment)
            || equals_ignore_case(s, &camel)
            || equals_ignore_case(&snake_to_lower_camel(s), segment)
            || equals_ignore_case(&snake_to_lower_camel(s), &camel)
    };
    if let Some(json) = json
        && (serialized(json) || alternates.iter().any(|a| serialized(a)))
    {
        return true;
    }
    equals_ignore_case(java, segment)
        || equals_ignore_case(java, &camel)
        || equals_ignore_case(&snake_to_lower_camel(java), &camel)
}

enum Target {
    Scalar(&'static FieldDesc),
    Section(&'static SectionDesc),
    Layers,
    ValidationWarnings,
    CostsInitialized,
}

fn find_top(segment: &str) -> Option<Target> {
    for field in FIELDS.iter().filter(|f| f.section.is_empty()) {
        if name_matches(segment, Some(field.name), field.alternates, field.java_name) {
            return Some(Target::Scalar(field));
        }
    }
    for section in SECTIONS {
        if name_matches(segment, Some(section.name), &[], section.java_name) {
            return Some(Target::Section(section));
        }
    }
    if name_matches(segment, Some("layers"), &[], "layers") {
        return Some(Target::Layers);
    }
    if name_matches(
        segment,
        Some("validation_warnings"),
        &[],
        "validationWarnings",
    ) {
        return Some(Target::ValidationWarnings);
    }
    if name_matches(segment, None, &[], "boardSpecificTraceCostsApplied") {
        return Some(Target::CostsInitialized);
    }
    None
}

fn find_in_section(section: &SectionDesc, segment: &str) -> Option<&'static FieldDesc> {
    FIELDS
        .iter()
        .filter(|f| f.section == section.name)
        .find(|f| name_matches(segment, Some(f.name), f.alternates, f.java_name))
}

/// `String.split("[.:\\-]")`: separators `.`, `:` and `-`, trailing empty
/// strings dropped.
fn split_path(path: &str) -> Vec<&str> {
    let mut parts: Vec<&str> = path.split(['.', ':', '-']).collect();
    if parts.len() > 1 {
        while parts.last() == Some(&"") {
            parts.pop();
        }
    }
    parts
}

/// `ReflectionUtil.convertValue` for a string value.
pub fn convert_value(kind: FieldKind, text: &str) -> Result<FieldValue, SetError> {
    let bad = |message: String| SetError::BadValue(message);
    match kind {
        FieldKind::Bool => Ok(FieldValue::Bool(match text {
            "0" => false,
            "1" => true,
            other => parse_boolean(other),
        })),
        FieldKind::Int => parse_int(text).map(FieldValue::Int).map_err(bad),
        FieldKind::Long => parse_long(text).map(FieldValue::Long).map_err(bad),
        FieldKind::Float => parse_float(text).map(FieldValue::Float).map_err(bad),
        FieldKind::Double => parse_double(text).map(FieldValue::Double).map_err(bad),
        FieldKind::Str => Ok(FieldValue::Str(text.to_string())),
        FieldKind::StrList => {
            let raw = text.trim();
            if raw.is_empty() {
                return Ok(FieldValue::StrList(Vec::new()));
            }
            Ok(FieldValue::StrList(
                java_split(raw, ',')
                    .into_iter()
                    .map(|t| t.trim().to_string())
                    .collect(),
            ))
        }
        FieldKind::DoubleList => {
            let raw = text.trim();
            if raw.is_empty() {
                return Ok(FieldValue::DoubleList(Vec::new()));
            }
            java_split(raw, ',')
                .into_iter()
                .map(|t| parse_double(t.trim()))
                .collect::<Result<Vec<_>, _>>()
                .map(FieldValue::DoubleList)
                .map_err(bad)
        }
        FieldKind::RouterScoringVersion
        | FieldKind::OptimizerScoringVersion
        | FieldKind::BoardUpdateStrategy
        | FieldKind::ItemSelectionStrategy => FieldValue::enum_constant(kind, text.trim())
            .ok_or_else(|| bad(format!("No enum constant {kind:?}.{}", text.trim()))),
    }
}

fn not_a_leaf(segment: &str) -> SetError {
    SetError::BadValue(format!(
        "Can not set field '{segment}' to java.lang.String: it is not a value"
    ))
}

impl SettingsPatch {
    /// `ReflectionUtil.setFieldValue(this, path, value)`.
    ///
    /// Values convert as `convertValue` does:
    /// - booleans: `"0"` is false, `"1"` is true, otherwise only `true`, in
    ///   any case, is true;
    /// - integers follow `Integer.parseInt`, doubles `Double.parseDouble`
    ///   (whitespace trimmed, `d`/`f` suffix accepted);
    /// - enums by constant name, ignoring case;
    /// - string and number lists split on `,`, each element trimmed, an
    ///   empty value giving an empty list;
    /// - `layers.<field>=a,b` creates an array of the list's length when
    ///   `layers` is unset, otherwise sets the first `min(len, n)` elements.
    pub fn set_path(&mut self, path: &str, value: &str) -> Result<(), SetError> {
        let segments = split_path(path);
        let first = segments[0];
        let rest = &segments[1..];
        let target = find_top(first).ok_or_else(|| SetError::UnknownPath(first.to_string()))?;
        match target {
            Target::Scalar(field) => {
                if !rest.is_empty() {
                    return Err(SetError::UnknownPath(rest[0].to_string()));
                }
                (field.set)(self, convert_value(field.kind, value)?);
                Ok(())
            }
            Target::Section(section) => {
                let Some(&name) = rest.first() else {
                    return Err(not_a_leaf(first));
                };
                let field = find_in_section(section, name)
                    .ok_or_else(|| SetError::UnknownPath(name.to_string()))?;
                if rest.len() > 1 {
                    return Err(SetError::UnknownPath(rest[1].to_string()));
                }
                (field.set)(self, convert_value(field.kind, value)?);
                Ok(())
            }
            Target::Layers => self.set_layers_path(rest, value),
            Target::ValidationWarnings => Err(not_a_leaf(first)),
            Target::CostsInitialized => {
                if !rest.is_empty() {
                    return Err(SetError::UnknownPath(rest[0].to_string()));
                }
                match convert_value(FieldKind::Bool, value)? {
                    FieldValue::Bool(b) => self.costs_initialized = Some(b),
                    _ => unreachable!("a boolean converts to a boolean"),
                }
                Ok(())
            }
        }
    }

    fn set_layers_path(&mut self, rest: &[&str], value: &str) -> Result<(), SetError> {
        let Some(&name) = rest.first() else {
            return Err(not_a_leaf("layers"));
        };
        let tokens = java_split(value, ',');
        let size = tokens.len();
        let layers = self
            .layers
            .get_or_insert_with(|| vec![LayerSettings::default(); size]);
        let limit = layers.len().min(size);
        for (layer, token) in layers.iter_mut().zip(&tokens).take(limit) {
            let field = LAYER_FIELDS
                .iter()
                .find(|f| name_matches(name, Some(f.name), &[], f.java_name))
                .ok_or_else(|| SetError::UnknownPath(name.to_string()))?;
            if rest.len() > 1 {
                return Err(SetError::UnknownPath(rest[1].to_string()));
            }
            (field.set)(layer, convert_value(field.kind, token.trim())?);
        }
        Ok(())
    }
}

/// `LegacyRouterSettingsBridge.normalizeRelativePath`.
fn normalize_relative_path(path: &str) -> String {
    path.strip_prefix("router.").unwrap_or(path).to_lowercase()
}

fn flat_field(relative: &str) -> Option<&'static FieldDesc> {
    let path = normalize_relative_path(relative);
    FIELDS.iter().find(|f| f.flat != Flat::No && f.name == path)
}

/// `LegacyRouterSettingsBridge.canonicalCliPath`: the flat autorouter and
/// cost keys map into their section, other paths are lower-cased only.
pub fn canonical_cli_path(router_relative_path: &str) -> String {
    match flat_field(router_relative_path) {
        Some(field) => field.path.to_string(),
        None => normalize_relative_path(router_relative_path),
    }
}

/// `LegacyRouterSettingsBridge.isDeprecatedFlatAutorouterPath`.
pub fn is_deprecated_flat_autorouter_path(router_relative_path: &str) -> bool {
    flat_field(router_relative_path).is_some_and(|f| f.flat == Flat::Deprecated)
}

/// `LegacyRouterSettingsBridge.warnDeprecatedPath`.
pub fn deprecated_path_warning(old_path: &str, new_path: &str) -> String {
    format!(
        "Deprecated settings path '{old_path}'; use '{new_path}' instead. The old path will be removed in a future release."
    )
}

impl SettingsPatch {
    /// `CliSettings.applyRouterSetting`: applies one `--router.*`,
    /// `--optimizer.*` or `-flag` property with the CLI's rewrites.
    ///
    /// - `router.scoring.version` goes to `router_scoring.version` and
    ///   `optimizer.scoring.version` to `optimizer_scoring.version`;
    ///   `scoring-version` sets both.
    /// - Version aliases: `v1`/`legacy` is `V1_LEGACY`; `v2`/`continuous`
    ///   is `V2_CONTINUOUS` (router) or `V2_LOWER_BOUND` (optimizer);
    ///   `lower_bound`/`lower-bound` is `V2_LOWER_BOUND`.
    /// - `improvement_threshold` values strictly inside (0, 1) are
    ///   multiplied by `100f` in `f32`. This applies on the CLI only, not to
    ///   environment variables or JSON.
    ///
    /// Deprecation warnings are appended to `warnings`. On success the value
    /// as stored is returned (Java's `getParsedArguments()` entry).
    pub fn cli_property(
        &mut self,
        property_name: &str,
        value: &str,
        warnings: &mut Vec<String>,
    ) -> Result<String, SetError> {
        if property_name == "scoring-version" {
            self.cli_property("router.scoring.version", value, warnings)?;
            return self.cli_property("optimizer.scoring.version", value, warnings);
        }
        let mut field_path = property_name
            .strip_prefix("router.")
            .unwrap_or(property_name)
            .to_string();
        if field_path == "scoring.version" {
            field_path = "routerScoring.version".to_string();
        } else if property_name == "optimizer.scoring.version" {
            field_path = "optimizerScoring.version".to_string();
        } else if let Some(relative) = property_name.strip_prefix("router.") {
            let canonical = canonical_cli_path(relative);
            if is_deprecated_flat_autorouter_path(relative) {
                warnings.push(deprecated_path_warning(
                    &format!("router.{relative}"),
                    &format!("router.{canonical}"),
                ));
            }
            field_path = canonical;
        }
        let mut value = value.to_string();
        if field_path.ends_with(".version") {
            let mapped = match value.trim().to_lowercase().as_str() {
                "v1" | "legacy" => Some("V1_LEGACY"),
                "v2" | "continuous" => Some(if field_path.starts_with("optimizer") {
                    "V2_LOWER_BOUND"
                } else {
                    "V2_CONTINUOUS"
                }),
                "lower_bound" | "lower-bound" => Some("V2_LOWER_BOUND"),
                _ => None,
            };
            if let Some(mapped) = mapped {
                value = mapped.to_string();
            }
        }
        let is_threshold = property_name == "router.optimizer.improvement_threshold"
            || field_path.ends_with("improvement_threshold")
            || field_path.ends_with("optimizationImprovementThreshold");
        if is_threshold
            && let Ok(parsed) = parse_float(value.trim())
            && parsed > 0.0
            && parsed < 1.0
        {
            value = java_float_to_string(parsed * 100.0_f32);
        }
        self.set_path(&field_path, &value)?;
        Ok(value)
    }
}

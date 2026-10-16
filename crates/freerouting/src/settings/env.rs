//! Java's `EnvironmentVariablesSource`: `FREEROUTING__ROUTER__*` variables.

use std::collections::BTreeMap;

use super::fields::SettingsPatch;
use super::path::{
    SetError, canonical_cli_path, deprecated_path_warning, is_deprecated_flat_autorouter_path,
};

const PREFIX: &str = "FREEROUTING__ROUTER__";

/// What the environment contributed.
#[derive(Clone, Debug, Default)]
pub struct EnvParse {
    /// The settings the variables set (priority 55).
    pub patch: SettingsPatch,
    /// Java's `getParsedVariables()`: variable name to value.
    pub parsed: BTreeMap<String, String>,
    /// Java's warnings.
    pub warnings: Vec<String>,
}

impl EnvParse {
    /// Java's `getParsedCount()`.
    pub fn parsed_count(&self) -> usize {
        self.parsed.len()
    }
}

/// `new EnvironmentVariablesSource(environment)`.
///
/// - Names are upper-cased and only those starting with
///   `FREEROUTING__ROUTER__` count. The rest has `__` replaced by `.`, goes
///   through `canonical_cli_path` and is set like a CLI path.
/// - An unknown or unparsable value gives a warning and the field stays
///   unset.
/// - Java iterates `System.getenv()` in hash order, so when two variables
///   set the same field the winner depends on the platform. Variables are
///   processed here in sorted order of their name, so the later name wins.
pub fn parse_environment<'a, I>(environment: I) -> EnvParse
where
    I: IntoIterator<Item = (&'a str, &'a str)>,
{
    let mut out = EnvParse::default();
    let sorted: BTreeMap<&str, &str> = environment.into_iter().collect();
    for (raw_key, value) in sorted {
        let upper = raw_key.to_uppercase();
        let Some(rest) = upper.strip_prefix(PREFIX) else {
            continue;
        };
        let property_path = rest.replace("__", ".");
        let canonical = canonical_cli_path(&property_path);
        if is_deprecated_flat_autorouter_path(&property_path) {
            out.warnings.push(deprecated_path_warning(
                raw_key,
                &format!("{PREFIX}{}", canonical.replace('.', "__").to_uppercase()),
            ));
        }
        match out.patch.set_path(&canonical, value) {
            Ok(()) => {
                out.parsed.insert(raw_key.to_string(), value.to_string());
            }
            Err(SetError::UnknownPath(_)) => out.warnings.push(format!(
                "Unknown router setting in environment variable: {raw_key} (property: {canonical})"
            )),
            Err(e) => out.warnings.push(format!(
                "Failed to parse environment variable: {raw_key} = {value}: {e}"
            )),
        }
    }
    out
}

/// The process environment as the source sees it.
pub fn parse_process_environment() -> EnvParse {
    let vars: Vec<(String, String)> = std::env::vars_os()
        .filter_map(|(k, v)| Some((k.into_string().ok()?, v.into_string().ok()?)))
        .collect();
    parse_environment(vars.iter().map(|(k, v)| (k.as_str(), v.as_str())))
}

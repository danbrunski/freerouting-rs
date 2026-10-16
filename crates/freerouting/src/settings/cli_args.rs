//! The settings half of Java's `CliSettings`: command-line arguments to a
//! [`SettingsPatch`]. C07 owns the rest of the argument parser and replaces
//! this entry point with the real one; the field handling here is the final
//! part.

use std::collections::BTreeMap;

use super::fields::SettingsPatch;

/// The result of [`parse_cli_arguments`].
#[derive(Clone, Debug, Default)]
pub struct CliParse {
    /// The settings the arguments set (priority 60).
    pub patch: SettingsPatch,
    /// Java's `getParsedArguments()`, sorted by key.
    pub parsed_arguments: BTreeMap<String, String>,
    /// Java's warnings, in order.
    pub warnings: Vec<String>,
}

impl CliParse {
    fn apply(&mut self, property_name: &str, value: &str) {
        let mut warnings = Vec::new();
        let result = self.patch.cli_property(property_name, value, &mut warnings);
        self.warnings.extend(warnings);
        match result {
            Ok(stored) => {
                self.parsed_arguments
                    .insert(property_name.to_string(), stored);
            }
            Err(e) => self.warnings.push(format!(
                "Failed to apply CLI router setting: {property_name}: {e}"
            )),
        }
    }
}

/// `CliSettings.mapFlagToProperty`.
fn map_flag_to_property(flag: &str) -> Option<&'static str> {
    Some(match flag {
        "mp" => "router.max_passes",
        "mt" => "router.max_threads",
        "inc" => "router.autorouter.ignore_net_classes",
        "oit" => "router.optimizer.improvement_threshold",
        "router-scoring-version" => "router.scoring.version",
        "optimizer-scoring-version" => "optimizer.scoring.version",
        "scoring-version" => "scoring-version",
        _ => return None,
    })
}

/// `new CliSettings(args)`: the router settings in a command line.
///
/// `--router.*=value`, `--optimizer.*=value`, `--scoring-version=`,
/// `--router-scoring-version=` and `--optimizer-scoring-version=` set fields
/// by path; `-mp`, `-mt`, `-inc`, `-oit` and the scoring flags take the next
/// argument as their value. A command line with both `-de` and `-do` and no
/// explicit `--router.enabled=` switches the router on (Java's legacy batch
/// mode).
pub fn parse_cli_arguments(args: &[String]) -> CliParse {
    let mut out = CliParse::default();
    let mut has_input = false;
    let mut has_output = false;
    let mut explicit_enabled = false;
    let mut i = 0;
    while i < args.len() {
        let arg = args[i].as_str();
        if let Some(body) = arg.strip_prefix("--") {
            if let Some((property_name, value)) = body.split_once('=') {
                if property_name == "router.enabled" || property_name == "router.autorouter.enabled"
                {
                    explicit_enabled = true;
                }
                if property_name == "scoring-version" {
                    out.apply("router.scoring.version", value);
                    out.apply("optimizer.scoring.version", value);
                } else if property_name.starts_with("router.")
                    || property_name.starts_with("optimizer.")
                    || property_name == "router-scoring-version"
                    || property_name == "optimizer-scoring-version"
                {
                    let normalized = match property_name {
                        "router-scoring-version" => "router.scoring.version",
                        "optimizer-scoring-version" => "optimizer.scoring.version",
                        other => other,
                    };
                    out.apply(normalized, value);
                }
            }
        } else if let Some(flag) = arg.strip_prefix('-') {
            let value = if i + 1 < args.len() && !args[i + 1].starts_with('-') {
                i += 1;
                args[i].as_str()
            } else {
                ""
            };
            match flag {
                "de" => has_input = true,
                "do" => has_output = true,
                _ => {}
            }
            if flag == "oit" {
                out.warnings.push(
                    "The '-oit' command-line flag is deprecated; use '--router.optimizer.improvement_threshold' instead."
                        .to_string(),
                );
            } else if flag == "inc" {
                out.warnings.push(
                    "The '-inc' command-line flag is deprecated; use '--router.autorouter.ignore_net_classes' instead."
                        .to_string(),
                );
            }
            if let Some(property_name) = map_flag_to_property(flag)
                && (property_name.starts_with("router.")
                    || property_name.starts_with("optimizer.")
                    || property_name == "scoring-version")
            {
                out.apply(property_name, value);
            }
        }
        i += 1;
    }
    if has_input && has_output && !explicit_enabled {
        out.patch.autorouter.enabled = Some(true);
    }
    out
}

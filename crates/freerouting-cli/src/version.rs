//! `--version` and `--capabilities` (C08.2): the handshake the KiCad plugin
//! pins on (K01.4, K02).
//!
//! `--version` prints one line, `freerouting <crate version> (git <sha>)`,
//! where the sha comes from the engine's `build.rs` (R01.2:
//! `FREEROUTING_GIT_SHA`, else `git rev-parse --short=12 HEAD`, else
//! `unknown`) through [`freerouting::build_info`]. `--capabilities` prints JSON:
//! the version, the output formats, every flag from the tables in
//! [`args`](crate::args), the exit codes, the schema versions of the
//! progress stream and the result manifest, and the registered stage ids.

use freerouting::autoroute::stages::StageRegistry;
use freerouting::drc::json::JsonValue;

use crate::args::{EXTRA_FLAGS, FLAGS, LONG_FLAGS, Matching, Setting};

/// The crate version (the workspace version, as the engine reports it).
pub const VERSION: &str = freerouting::build_info::VERSION;

/// The git commit the binary was built from (12 hex digits), or `unknown`.
pub const GIT_SHA: &str = freerouting::build_info::GIT_SHA;

/// The Java commit the port follows.
pub const JAVA_PIN: &str = "a773a70ec";

/// The schema version of the capabilities JSON.
pub const CAPABILITIES_SCHEMA_VERSION: u32 = 1;

/// The `--version` line, without the newline.
pub fn version_line() -> String {
    format!("freerouting {VERSION} (git {GIT_SHA})")
}

fn strings(items: impl IntoIterator<Item = impl Into<String>>) -> JsonValue {
    JsonValue::Array(items.into_iter().map(|s| JsonValue::string(s)).collect())
}

fn setting_members(setting: Setting) -> (JsonValue, JsonValue) {
    match setting {
        Setting::Paths(paths) => (strings(paths.iter().copied()), JsonValue::Null),
        Setting::CliOnly(reason) => (JsonValue::Null, JsonValue::string(reason)),
    }
}

fn flag_json(name: &str, takes_value: bool, matching: &str, setting: Setting) -> JsonValue {
    let (settings, note) = setting_members(setting);
    JsonValue::Object(vec![
        ("name".to_string(), JsonValue::string(name)),
        ("takes_value".to_string(), JsonValue::Bool(takes_value)),
        ("matching".to_string(), JsonValue::string(matching)),
        ("settings".to_string(), settings),
        ("note".to_string(), note),
    ])
}

/// Every flag of the command line: the single-dash flags, the long flags
/// and the C08 extras, in table order.
pub fn flag_names() -> Vec<&'static str> {
    FLAGS
        .iter()
        .map(|f| f.name)
        .chain(LONG_FLAGS.iter().map(|(n, _)| *n))
        .chain(EXTRA_FLAGS.iter().map(|(n, _, _)| *n))
        .collect()
}

/// The stage ids of the built-in registry, by kind.
fn stages_json() -> JsonValue {
    fn ids<T>(map: &std::collections::BTreeMap<&'static str, T>) -> JsonValue {
        strings(map.keys().copied())
    }
    let registry = StageRegistry::builtin();
    JsonValue::Object(vec![
        ("algorithm".to_string(), ids(&registry.bundles)),
        ("fanout".to_string(), ids(&registry.fanout)),
        ("ordering".to_string(), ids(&registry.ordering)),
        (
            "connection_router".to_string(),
            ids(&registry.connection_router),
        ),
        ("ripup_policy".to_string(), ids(&registry.ripup_policy)),
        (
            "pass_controller".to_string(),
            ids(&registry.pass_controller),
        ),
        ("plane_taps".to_string(), ids(&registry.plane_taps)),
        ("neck_down".to_string(), ids(&registry.neck_down)),
        ("board_stages".to_string(), ids(&registry.board_stages)),
        ("optimizer".to_string(), ids(&registry.optimizer)),
    ])
}

/// The capabilities document.
pub fn capabilities() -> JsonValue {
    let mut flags = Vec::new();
    for f in FLAGS {
        let matching = match f.matching {
            Matching::Prefix => "prefix",
            Matching::Exact => "exact",
            Matching::ExactIgnoreCase => "exact_ignore_case",
        };
        flags.push(flag_json(f.name, f.takes_value, matching, f.setting));
    }
    for (name, setting) in LONG_FLAGS {
        flags.push(flag_json(name, true, "exact", *setting));
    }
    for (name, takes_value, setting) in EXTRA_FLAGS {
        flags.push(flag_json(name, *takes_value, "exact", *setting));
    }
    let int = |n: u32| JsonValue::int(i64::from(n));
    JsonValue::Object(vec![
        (
            "schema_version".to_string(),
            int(CAPABILITIES_SCHEMA_VERSION),
        ),
        ("name".to_string(), JsonValue::string("freerouting")),
        ("version".to_string(), JsonValue::string(VERSION)),
        ("git".to_string(), JsonValue::string(GIT_SHA)),
        ("java_pin".to_string(), JsonValue::string(JAVA_PIN)),
        (
            "output_formats".to_string(),
            strings([".ses", ".dsn", ".rules", ".drc", ".drc.json", ".kicad_pcb"]),
        ),
        ("input_formats".to_string(), strings([".dsn", ".kicad_pcb"])),
        (
            "schemas".to_string(),
            JsonValue::Object(vec![
                ("progress".to_string(), int(crate::progress::SCHEMA_VERSION)),
                (
                    "result_manifest".to_string(),
                    int(crate::manifest::SCHEMA_VERSION),
                ),
            ]),
        ),
        (
            "exit_codes".to_string(),
            JsonValue::Object(vec![
                ("0".to_string(), JsonValue::string("an output was written")),
                ("1".to_string(), JsonValue::string("failure with no output")),
                ("2".to_string(), JsonValue::string("usage error")),
                (
                    "130".to_string(),
                    JsonValue::string("aborted before an output was written"),
                ),
            ]),
        ),
        (
            "stop_commands".to_string(),
            strings(crate::stop::COMMANDS.iter().copied()),
        ),
        ("flags".to_string(), JsonValue::Array(flags)),
        (
            "setting_flag_families".to_string(),
            strings(["--router.<path>=<value>", "--optimizer.<path>=<value>"]),
        ),
        ("stages".to_string(), stages_json()),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_line_has_the_documented_shape() {
        let line = version_line();
        let rest = line.strip_prefix("freerouting ").unwrap();
        let (version, rest) = rest.split_once(" (git ").unwrap();
        assert_eq!(version.split('.').count(), 3, "{line}");
        assert!(version.split('.').all(|p| p.parse::<u32>().is_ok()));
        let sha = rest.strip_suffix(')').unwrap();
        assert!(
            sha == "unknown" || (sha.len() == 12 && sha.chars().all(|c| c.is_ascii_hexdigit())),
            "{line}"
        );
    }
}

//! Ports of Java's argument tests and the comparison with C01.2's `argv`
//! dumps (C07.1 to C07.3).
//!
//! Java tests ported (at `a773a70ec`):
//! - `GlobalSettingsCommandLineTest`: every `-de` / `-do` case. Not ported:
//!   the `--api_server*` cases (the server settings are accepted and ignored,
//!   `accepted_and_ignored_flags` covers them without reading the value into
//!   a settings object) and `newLanguagesViaCommandLineArgument` (it tests
//!   the Swing translation of the cancel button; `-l` is accepted and
//!   ignored).
//! - `GlobalSettingsTest`: the argument cases (`applyCommandLineArguments*`
//!   and `setUnknownProperty`). Not ported: nothing; the other methods of
//!   that class test settings-file handling (C02).
//! - `SettingsMergerTest`: the four legacy router cases.
//! - `Issue885SettingsInitializationTest`: `cliSettingsParsesIncFlagIntoIgnoreNetClasses`
//!   (the other methods test `RouterSettings.applyBoardSpecificOptimizations`
//!   and `ReflectionUtil.copyFields`, which `settings_board.rs` and
//!   `settings_merge.rs` hold).
//!
//! `argv_json_*` compare the parser with Java's own output for every entry of
//! `fixtures/reference/argv.json` (C01.2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use freerouting::drc::json::JsonValue;
use freerouting::job::{LogLevel, LogLine};
use freerouting::settings::dump::dump_patch;
use freerouting::settings::{ResolveContext, Source, defaults, resolve};
use freerouting_cli::args::{Args, Unsupported, parse};

const CTX: ResolveContext = ResolveContext { cores: 8 };

fn parse_strs(args: &[&str]) -> Args {
    parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
}

fn count(args: &Args, level: LogLevel) -> usize {
    args.log.iter().filter(|l| l.level == level).count()
}

fn has(args: &Args, level: LogLevel, text: &str) -> bool {
    args.log.iter().any(|l| l.level == level && l.text == text)
}

fn repo(path: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn scratch(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("args-java-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

// ---- GlobalSettingsCommandLineTest ------------------------------------

#[test]
fn single_dsn_file() {
    let a = parse_strs(&["-de", "myboard.dsn"]);
    assert_eq!(a.design_file.as_deref(), Some("myboard.dsn"));
    assert_eq!(a.session_file, None);
    assert_eq!(a.rules_file, None);
}

#[test]
fn dsn_and_ses_with_plus_separator() {
    let a = parse_strs(&["-de", "myboard.dsn+myboard.ses"]);
    assert_eq!(a.design_file.as_deref(), Some("myboard.dsn"));
    assert_eq!(a.session_file.as_deref(), Some("myboard.ses"));
    assert_eq!(a.rules_file, None);
}

#[test]
fn dsn_and_rules_with_plus_separator() {
    let a = parse_strs(&["-de", "myboard.dsn+myboard.rules"]);
    assert_eq!(a.design_file.as_deref(), Some("myboard.dsn"));
    assert_eq!(a.session_file, None);
    assert_eq!(a.rules_file.as_deref(), Some("myboard.rules"));
}

#[test]
fn all_three_files_with_plus_separator() {
    let a = parse_strs(&["-de", "myboard.dsn+myboard.ses+myboard.rules"]);
    assert_eq!(a.design_file.as_deref(), Some("myboard.dsn"));
    assert_eq!(a.session_file.as_deref(), Some("myboard.ses"));
    assert_eq!(a.rules_file.as_deref(), Some("myboard.rules"));
}

#[test]
fn files_in_different_order() {
    let a = parse_strs(&["-de", "myboard.rules+myboard.dsn+myboard.ses"]);
    assert_eq!(a.design_file.as_deref(), Some("myboard.dsn"));
    assert_eq!(a.session_file.as_deref(), Some("myboard.ses"));
    assert_eq!(a.rules_file.as_deref(), Some("myboard.rules"));
}

#[test]
fn space_separated_files() {
    let a = parse_strs(&["-de", "myboard.dsn", "myboard.ses", "myboard.rules"]);
    assert_eq!(a.design_file.as_deref(), Some("myboard.dsn"));
    assert_eq!(a.session_file.as_deref(), Some("myboard.ses"));
    assert_eq!(a.rules_file.as_deref(), Some("myboard.rules"));
}

#[test]
fn filename_with_spaces() {
    let a = parse_strs(&["-de", "sonde xilinx.dsn"]);
    assert_eq!(a.design_file.as_deref(), Some("sonde xilinx.dsn"));
    assert_eq!(a.session_file, None);
    assert_eq!(a.rules_file, None);
}

#[test]
fn mixed_separators() {
    let a = parse_strs(&["-de", "myboard.dsn+myboard.ses", "myboard.rules"]);
    assert_eq!(a.design_file.as_deref(), Some("myboard.dsn"));
    assert_eq!(a.session_file.as_deref(), Some("myboard.ses"));
    assert_eq!(a.rules_file.as_deref(), Some("myboard.rules"));
}

#[test]
fn files_with_paths() {
    let a = parse_strs(&["-de", "/path/to/myboard.dsn+/path/to/myboard.ses"]);
    assert_eq!(a.design_file.as_deref(), Some("/path/to/myboard.dsn"));
    assert_eq!(a.session_file.as_deref(), Some("/path/to/myboard.ses"));
}

#[test]
fn case_insensitive_extensions() {
    let a = parse_strs(&["-de", "myboard.DSN+myboard.SES+myboard.RULES"]);
    assert_eq!(a.design_file.as_deref(), Some("myboard.DSN"));
    assert_eq!(a.session_file.as_deref(), Some("myboard.SES"));
    assert_eq!(a.rules_file.as_deref(), Some("myboard.RULES"));
}

#[test]
fn multiple_dsn_files_uses_last() {
    let a = parse_strs(&["-de", "board1.dsn+board2.dsn"]);
    assert_eq!(a.design_file.as_deref(), Some("board2.dsn"));
    assert!(has(
        &a,
        LogLevel::Warn,
        "Multiple DSN files provided in -de argument. Only the last one will be used."
    ));
    assert_eq!(count(&a, LogLevel::Warn), 1);
}

#[test]
fn only_ses_file() {
    let a = parse_strs(&["-de", "myboard.ses"]);
    assert_eq!(a.design_file, None);
    assert_eq!(a.session_file.as_deref(), Some("myboard.ses"));
}

#[test]
fn only_rules_file() {
    let a = parse_strs(&["-de", "myboard.rules"]);
    assert_eq!(a.design_file, None);
    assert_eq!(a.session_file, None);
    assert_eq!(a.rules_file.as_deref(), Some("myboard.rules"));
}

#[test]
fn empty_argument() {
    let a = parse_strs(&["-de"]);
    assert_eq!(a.design_file, None);
    assert_eq!(a.session_file, None);
    assert_eq!(a.rules_file, None);
}

#[test]
fn with_other_arguments() {
    let a = parse_strs(&[
        "-de",
        "myboard.dsn+myboard.ses",
        "-do",
        "output.ses",
        "-mp",
        "10",
    ]);
    assert_eq!(a.design_file.as_deref(), Some("myboard.dsn"));
    assert_eq!(a.session_file.as_deref(), Some("myboard.ses"));
    assert_eq!(a.output_file.as_deref(), Some("output.ses"));
    assert_eq!(a.patch.autorouter.max_passes, Some(10));
}

#[test]
fn output_file_with_plus_in_name_not_split() {
    let name = "scripts/benchmark/outputs/mechkeys_MF68+10--unrouted--2.5.0-RC5.ses";
    let a = parse_strs(&["-do", name]);
    assert_eq!(a.output_file.as_deref(), Some(name));
    assert!(a.additional_outputs.is_empty());
}

#[test]
fn output_file_with_trailing_plus_not_split() {
    let name = "scripts/benchmark/outputs/perfplusplus_Ard-perf++--unrouted--2.5.0-RC5.ses";
    let a = parse_strs(&["-do", name]);
    assert_eq!(a.output_file.as_deref(), Some(name));
    assert!(a.additional_outputs.is_empty());
}

#[test]
fn multi_file_output_with_plus_split() {
    let a = parse_strs(&["-do", "output.ses+output.kicad_pcb"]);
    assert_eq!(a.output_file.as_deref(), Some("output.ses"));
    assert_eq!(a.additional_outputs, ["output.kicad_pcb"]);
}

#[test]
fn file_with_dot_and_plus_in_name_not_split() {
    let a = parse_strs(&["-do", "board.v1+final.dsn"]);
    assert_eq!(a.output_file.as_deref(), Some("board.v1+final.dsn"));
    assert!(a.additional_outputs.is_empty());
    let a = parse_strs(&["-de", "board.v1+final.dsn"]);
    assert_eq!(a.design_file.as_deref(), Some("board.v1+final.dsn"));
    assert_eq!(a.session_file, None);
}

/// `new File(rawArg).exists()` comes first: a file whose name has a `+` and
/// valid extensions on both sides is used whole when it exists.
#[test]
fn an_existing_path_with_a_plus_is_used_whole() {
    let dir = scratch("existing");
    let name = dir.join("a.dsn+b.ses");
    std::fs::write(&name, b"x").unwrap();
    let text = name.to_str().unwrap();
    let a = parse_strs(&["-de", text]);
    // Whole, so it ends in `.ses`: it is the session, and there is no DSN.
    assert_eq!(a.session_file.as_deref(), Some(text));
    assert_eq!(a.design_file, None);
    let a = parse_strs(&["-de", "a.dsn+b.ses"]);
    assert_eq!(a.design_file.as_deref(), Some("a.dsn"));
    assert_eq!(a.session_file.as_deref(), Some("b.ses"));
}

#[test]
fn json_is_the_input_before_a_dsn_and_the_session_after_one() {
    let a = parse_strs(&["-de", "a.json+b.dsn"]);
    assert_eq!(a.design_file.as_deref(), Some("b.dsn"));
    assert_eq!(a.session_file, None);
    let a = parse_strs(&["-de", "a.dsn+b.json"]);
    assert_eq!(a.design_file.as_deref(), Some("a.dsn"));
    assert_eq!(a.session_file.as_deref(), Some("b.json"));
}

#[test]
fn unknown_file_types_and_second_session_files_warn() {
    let a = parse_strs(&["-de", "a.dsn", "b.txt", "c.ses", "d.ses"]);
    assert!(has(
        &a,
        LogLevel::Warn,
        "Unknown file type in -de argument: b.txt. Expected .dsn, .json, .ses, or .rules"
    ));
    assert!(has(
        &a,
        LogLevel::Warn,
        "Multiple SES files provided in -de argument. Only the last one will be used."
    ));
    assert_eq!(a.session_file.as_deref(), Some("d.ses"));
}

#[test]
fn extra_outputs_after_the_first_are_additional() {
    let a = parse_strs(&["-do", "a.ses", "b.drc.json", "-do", "c.dsn"]);
    assert_eq!(a.output_file.as_deref(), Some("a.ses"));
    assert_eq!(a.additional_outputs, ["b.drc.json", "c.dsn"]);
}

// ---- GlobalSettingsTest ------------------------------------------------

#[test]
fn apply_command_line_arguments_valid_arguments() {
    let a = parse_strs(&["-mp", "10", "-di", "/tmp", "-dl"]);
    assert_eq!(a.patch.autorouter.max_passes, Some(10));
    // `-mp` is deprecated: one warning.
    assert_eq!(count(&a, LogLevel::Warn), 1);
    assert_eq!(count(&a, LogLevel::Error), 0);
}

#[test]
fn apply_command_line_arguments_log_level() {
    let a = parse_strs(&["-ll", "DEBUG"]);
    assert_eq!(a.console_level.as_deref(), Some("DEBUG"));
    assert_eq!(count(&a, LogLevel::Warn), 0);
}

#[test]
fn apply_command_line_arguments_unknown_flag() {
    let a = parse_strs(&["-unknownFlag"]);
    assert_eq!(count(&a, LogLevel::Warn), 1);
    assert!(has(
        &a,
        LogLevel::Warn,
        "Unknown command line argument: -unknownFlag"
    ));
}

#[test]
fn apply_command_line_arguments_unknown_value() {
    // `-mp 10` consumes two arguments; `extraValue` is loose.
    let a = parse_strs(&["-mp", "10", "extraValue"]);
    assert_eq!(
        count(&a, LogLevel::Warn),
        2,
        "the deprecation and the extra value"
    );
    assert!(has(
        &a,
        LogLevel::Warn,
        "Unknown command line argument: extraValue"
    ));
}

#[test]
fn apply_command_line_arguments_valid_double_hyphen() {
    let a = parse_strs(&["--router.max_passes=20"]);
    assert_eq!(a.patch.autorouter.max_passes, Some(20));
    assert_eq!(
        count(&a, LogLevel::Warn),
        1,
        "the deprecated flat key warns"
    );
}

#[test]
fn apply_command_line_arguments_nested_autorouter_max_threads() {
    let a = parse_strs(&[
        "--router.autorouter.max_threads=1",
        "--router.optimizer.max_threads=4",
    ]);
    assert_eq!(a.patch.autorouter.max_threads, Some(1));
    assert_eq!(a.patch.optimizer.max_threads, Some(4));
    assert_eq!(count(&a, LogLevel::Warn), 0);
}

#[test]
fn set_unknown_property() {
    let a = parse_strs(&["--unknown_settings.unknown_field=true"]);
    assert_eq!(count(&a, LogLevel::Warn), 1);
    assert_eq!(count(&a, LogLevel::Error), 0);
    assert!(has(
        &a,
        LogLevel::Warn,
        "Unknown settings property: unknown_settings.unknown_field"
    ));
}

#[test]
fn apply_command_line_arguments_malformed_double_hyphen() {
    let a = parse_strs(&["--someflag"]);
    assert_eq!(count(&a, LogLevel::Warn), 1);
    assert!(has(
        &a,
        LogLevel::Warn,
        "Unknown command line argument: --someflag"
    ));
}

#[test]
fn unknown_flags_each_give_exactly_one_warning() {
    for (args, text) in [
        (vec!["-im"], "Unknown command line argument: -im"),
        (vec!["-hr"], "Unknown command line argument: -hr"),
        (vec!["-x"], "Unknown command line argument: -x"),
        (vec!["--x"], "Unknown command line argument: --x"),
        (vec!["--"], "Unknown command line argument: --"),
        (vec!["--=x"], "Unknown settings property: "),
        (vec!["--a=b=c"], "Unknown settings property: a"),
    ] {
        let a = parse_strs(&args);
        assert_eq!(a.log.len(), 1, "{args:?}: {:?}", a.log);
        assert_eq!(a.log[0].level, LogLevel::Warn);
        assert_eq!(a.log[0].text, text, "{args:?}");
    }
}

// ---- C07.2: SettingsMergerTest and Issue885 ----------------------------

fn merged_run_router(args: &[&str], json_disables_router: bool) -> (bool, bool) {
    let cli = parse_strs(args).patch;
    let mut sources = vec![(Source::Defaults, defaults(&CTX))];
    if json_disables_router {
        let mut json = freerouting::settings::SettingsPatch::default();
        json.autorouter.enabled = Some(false);
        sources.push((Source::ConfigFile, json));
    }
    sources.push((Source::Cli, cli));
    let merged = resolve(&sources, &CTX);
    (
        merged.settings.run_router(),
        merged.settings.run_optimizer(),
    )
}

#[test]
fn legacy_batch_mode_enables_router_when_json_disables_it() {
    let (router, _) = merged_run_router(
        &[
            "-de",
            "fixtures/Issue508-DAC2020_bm05.dsn",
            "-do",
            "fixtures/Issue508-DAC2020_bm05.ses",
        ],
        true,
    );
    assert!(router);
}

#[test]
fn explicit_router_enabled_false_overrides_legacy_batch_mode() {
    let (router, _) = merged_run_router(
        &[
            "-de",
            "fixtures/Issue508-DAC2020_bm05.dsn",
            "-do",
            "fixtures/Issue508-DAC2020_bm05.ses",
            "--router.enabled=false",
        ],
        false,
    );
    assert!(!router);
}

#[test]
fn cli_routable_layers_does_not_disable_router() {
    let (router, _) = merged_run_router(
        &[
            "-de",
            "fixtures/Issue508-DAC2020_bm05.dsn",
            "--router.layers.routable=false,true",
        ],
        false,
    );
    assert!(router);
}

#[test]
fn cli_can_disable_router_and_optimizer() {
    let (router, optimizer) = merged_run_router(
        &["--router.enabled=false", "--router.optimizer.enabled=false"],
        false,
    );
    assert!(!router);
    assert!(!optimizer);
}

#[test]
fn cli_settings_parses_inc_flag_into_ignore_net_classes() {
    let a = parse_strs(&["-inc", "POWER,GND"]);
    assert_eq!(
        a.patch.autorouter.ignore_net_classes.as_deref(),
        Some(&["POWER".to_string(), "GND".to_string()][..])
    );
}

/// `-mt` sets the flat `max_threads`, which no stage reads headless:
/// `optimizer.max_threads` and `autorouter.max_threads` keep their defaults.
#[test]
fn mt_does_nothing_headless() {
    let a = parse_strs(&["-mt", "3"]);
    assert_eq!(a.patch.max_threads, Some(3));
    assert_eq!(a.patch.optimizer.max_threads, None);
    assert_eq!(a.patch.autorouter.max_threads, None);
    let baseline = resolve(&[(Source::Defaults, defaults(&CTX))], &CTX);
    let with_mt = resolve(
        &[(Source::Defaults, defaults(&CTX)), (Source::Cli, a.patch)],
        &CTX,
    );
    assert_eq!(
        with_mt.settings.optimizer.max_threads,
        baseline.settings.optimizer.max_threads
    );
    assert_eq!(
        with_mt.settings.autorouter.max_threads,
        baseline.settings.autorouter.max_threads
    );
    assert_eq!(with_mt.settings.max_threads, 3);
}

#[test]
fn inc_escaped_comma_names_one_class() {
    let a = parse_strs(&["-inc", "Power\\,Default"]);
    assert_eq!(
        a.patch.autorouter.ignore_net_classes.as_deref(),
        Some(&["Power,Default".to_string()][..])
    );
    // Java's split.
    let a = parse_strs(&["-inc", "Power,Default"]);
    assert_eq!(
        a.patch.autorouter.ignore_net_classes.as_deref(),
        Some(&["Power".to_string(), "Default".to_string()][..])
    );
    // The last `-inc` wins, as in Java.
    let a = parse_strs(&["-inc", "A", "-inc", "B\\,C"]);
    assert_eq!(
        a.patch.autorouter.ignore_net_classes.as_deref(),
        Some(&["B,C".to_string()][..])
    );
}

#[test]
fn mp_10000_is_unlimited() {
    let a = parse_strs(&["-mp", "10000"]);
    assert_eq!(a.patch.autorouter.max_passes, Some(10000));
    let merged = resolve(
        &[(Source::Defaults, defaults(&CTX)), (Source::Cli, a.patch)],
        &CTX,
    );
    assert_eq!(merged.settings.autorouter.max_passes, 0);
    assert!(
        merged
            .warnings
            .iter()
            .any(|w| w.contains("Invalid maxPasses value: 10000")),
        "{:?}",
        merged.warnings
    );
    // 9999 is kept.
    let a = parse_strs(&["-mp", "9999"]);
    let merged = resolve(
        &[(Source::Defaults, defaults(&CTX)), (Source::Cli, a.patch)],
        &CTX,
    );
    assert_eq!(merged.settings.autorouter.max_passes, 9999);
}

// ---- C07.3 --------------------------------------------------------------

/// The Java plugin's exact command line (`router_dsn.py::_build_command`),
/// with a host string with spaces and quotes: no warning, and the same inputs,
/// outputs and patch as `-de in.dsn -do out.ses` alone.
#[test]
fn java_plugin_command_line_parses_cleanly() {
    let plugin = parse_strs(&[
        "-de",
        "in.dsn",
        "-do",
        "out.ses",
        "-host",
        "KiCad \"9.0.0\" it's  nightly",
        "--gui.enabled=false",
        "--api_server.enabled=false",
        "--mcp_server.enabled=false",
        "--logging.file.location=/tmp/fr logs",
    ]);
    let plain = parse_strs(&["-de", "in.dsn", "-do", "out.ses"]);
    let problems: Vec<_> = plugin
        .log
        .iter()
        .filter(|l| l.level >= LogLevel::Info)
        .collect();
    assert!(problems.is_empty(), "{problems:?}");
    assert_eq!(plugin.design_file, plain.design_file);
    assert_eq!(plugin.session_file, plain.session_file);
    assert_eq!(plugin.rules_file, plain.rules_file);
    assert_eq!(plugin.output_file, plain.output_file);
    assert_eq!(plugin.additional_outputs, plain.additional_outputs);
    assert_eq!(
        dump_patch(&plugin.patch, 1000).to_pretty_string(),
        dump_patch(&plain.patch, 1000).to_pretty_string()
    );
    assert_eq!(plugin.patch.autorouter.enabled, Some(true));
}

#[test]
fn accepted_and_ignored_flags() {
    for args in [
        vec!["-host", "KiCad 9"],
        vec!["-di", "/tmp"],
        vec!["-l", "de"],
        vec!["-dct", "7"],
        vec!["-dl"],
        vec!["-da"],
        vec!["-us", "global"],
        vec!["-is", "prioritized"],
        vec!["--gui.enabled=false"],
        vec!["--api_server-endpoints=http://0.0.0.0:37864,http://127.0.0.1:37865"],
        vec!["--api_server.corsOrigins=https://example.com?foo=bar"],
        vec!["--mcp_server.rate_limit.enabled=true"],
        vec!["--usage_and_diagnostic_data.disable_analytics=true"],
        vec!["--feature_flags.x=1"],
        vec!["--profile.user_email=a@b"],
        vec!["--debug.enable_detailed_logging=true"],
        vec!["--logging.file.enabled=false"],
        vec!["--user_data_path=/tmp/x"],
        vec!["--settings", "/tmp/x.json"],
        vec!["--settings=/tmp/x.json"],
    ] {
        let a = parse_strs(&args);
        let loud: Vec<&LogLine> = a.log.iter().filter(|l| l.level >= LogLevel::Info).collect();
        assert!(loud.is_empty(), "{args:?}: {loud:?}");
        assert!(
            a.design_file.is_none() && a.output_file.is_none(),
            "{args:?}"
        );
    }
    let a = parse_strs(&["--logging.console.level=warn"]);
    assert_eq!(a.console_level.as_deref(), Some("WARN"));
    // `-dct -3`: the value starts with `-`, so it is a loose argument.
    let a = parse_strs(&["-dct", "-3"]);
    assert!(has(&a, LogLevel::Warn, "Unknown command line argument: -3"));
}

#[test]
fn help_and_the_refused_commands() {
    for flag in ["-help", "--help", "-h", "-HELP", "-H"] {
        assert!(parse_strs(&[flag]).help, "{flag}");
    }
    assert_eq!(
        parse_strs(&["--compare-boards=a.dsn,b.dsn"]).unsupported,
        Some(Unsupported::CompareBoards)
    );
    assert_eq!(
        parse_strs(&["--calculate-benchmark-scores"]).unsupported,
        Some(Unsupported::CalculateBenchmarkScores)
    );
    assert_eq!(
        parse_strs(&["--calculate-benchmark-scores.input=a", "--output=b"]).unsupported,
        Some(Unsupported::CalculateBenchmarkScores)
    );
    // Like Java, a comparison that does not name two files is ignored.
    assert_eq!(parse_strs(&["--compare-boards=a.dsn"]).unsupported, None);
}

#[test]
fn drc_without_a_file_keeps_the_stdout_report() {
    let a = parse_strs(&["-de", "a.dsn", "-drc"]);
    assert!(a.drc);
    assert_eq!(a.drc_report_file, None);
}

#[test]
fn settings_flags_choose_the_config_file() {
    use freerouting::settings::config_path::ConfigChoice;
    assert_eq!(
        parse_strs(&["--settings", "/x/f.json"]).config,
        ConfigChoice::Explicit(PathBuf::from("/x/f.json"))
    );
    assert_eq!(
        parse_strs(&["--user_data_path=/x"]).config,
        ConfigChoice::UserDataPath(PathBuf::from("/x"))
    );
    assert_eq!(parse_strs(&["-mp", "3"]).config, ConfigChoice::Default);
}

// ---- argv.json ----------------------------------------------------------

fn member<'a>(value: &'a JsonValue, key: &str) -> Option<&'a JsonValue> {
    match value {
        JsonValue::Object(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
        _ => None,
    }
}

fn string_of(value: Option<&JsonValue>) -> Option<String> {
    match value {
        Some(JsonValue::String(s)) => Some(s.clone()),
        _ => None,
    }
}

fn strings_of(value: Option<&JsonValue>) -> Vec<String> {
    match value {
        Some(JsonValue::Array(items)) => items.iter().filter_map(|v| string_of(Some(v))).collect(),
        _ => Vec::new(),
    }
}

fn entries() -> Vec<JsonValue> {
    let text = std::fs::read_to_string(repo("fixtures/reference/argv.json")).unwrap();
    let root = JsonValue::parse(&text).unwrap();
    match member(&root, "entries") {
        Some(JsonValue::Array(items)) => items.clone(),
        _ => panic!("argv.json has no entries"),
    }
}

fn args_of(entry: &JsonValue) -> Vec<String> {
    strings_of(member(entry, "args"))
}

/// For every `argv.json` entry the file fields equal Java's
/// `GlobalSettings`: input, session, rules, outputs, the DRC report file and
/// the help flag.
#[test]
fn argv_json_file_fields_equal_java() {
    let mut compared = 0;
    for entry in entries() {
        let argv = args_of(&entry);
        let java = member(&entry, "globalSettings").unwrap();
        let ours = parse(&argv);
        let label = format!("{argv:?}");
        assert_eq!(
            ours.design_file,
            string_of(member(java, "initialInputFile")),
            "{label}"
        );
        assert_eq!(
            ours.session_file,
            string_of(member(java, "designSessionFilename")),
            "{label}"
        );
        assert_eq!(
            ours.rules_file,
            string_of(member(java, "initialRulesFile")),
            "{label}"
        );
        assert_eq!(
            ours.output_file,
            string_of(member(java, "initialOutputFile")),
            "{label}"
        );
        assert_eq!(
            ours.additional_outputs,
            strings_of(member(java, "additionalOutputFiles")),
            "{label}"
        );
        let report = member(java, "drcReportFile").and_then(|r| string_of(member(r, "filename")));
        assert_eq!(ours.drc_report_file, report, "{label}");
        assert_eq!(
            ours.help,
            matches!(member(java, "showHelpOption"), Some(JsonValue::Bool(true))),
            "{label}"
        );
        compared += 1;
    }
    assert!(compared >= 100, "only {compared} entries");
}

/// The warnings Java's `GlobalSettings` logs for the arguments it rejects
/// are ours, once each (Java's text without its level word). Java also warns
/// about router settings that its other parser accepts
/// (`Unknown settings property: router.*`, `scoring-version`, ...); those
/// are not compared.
#[test]
fn argv_json_rejection_warnings_equal_java() {
    let mut compared = 0;
    for entry in entries() {
        let argv = args_of(&entry);
        let java = member(&entry, "globalSettings").unwrap();
        let log = member(java, "log").unwrap();
        let mut expected: Vec<String> = strings_of(member(log, "texts"))
            .into_iter()
            .filter_map(|t| {
                t.strip_prefix("WARNING ")
                    .or_else(|| t.strip_prefix("ERROR "))
                    .map(str::to_string)
            })
            .filter(|t| {
                (t.starts_with("Unknown command line argument")
                    || t.starts_with("Multiple ")
                    || t.starts_with("Unknown file type")
                    || t.starts_with("There was a problem parsing"))
                    || (t.starts_with("Unknown settings property: ")
                        && !t["Unknown settings property: ".len()..].starts_with("router")
                        && !t.contains("scoring-version"))
            })
            .collect();
        let mut ours: Vec<String> = parse(&argv)
            .log
            .iter()
            .filter(|l| l.level >= LogLevel::Warn)
            .map(|l| l.text.clone())
            .filter(|t| {
                t.starts_with("Unknown command line argument")
                    || t.starts_with("Multiple ")
                    || t.starts_with("Unknown file type")
                    || t.starts_with("There was a problem parsing")
                    || t.starts_with("Unknown settings property: ")
            })
            .collect();
        expected.sort();
        ours.sort();
        assert_eq!(ours, expected, "{argv:?}");
        compared += 1;
    }
    assert!(compared >= 100);
}

/// Sorts the members of every object, so the comparison does not depend on
/// the order of the keys.
fn canonical(value: &JsonValue) -> String {
    match value {
        JsonValue::Object(members) => {
            let sorted: BTreeMap<&str, String> = members
                .iter()
                .filter(|(_, v)| !matches!(v, JsonValue::Null))
                .map(|(k, v)| (k.as_str(), canonical(v)))
                .collect();
            let body: Vec<String> = sorted.iter().map(|(k, v)| format!("{k:?}:{v}")).collect();
            format!("{{{}}}", body.join(","))
        }
        JsonValue::Array(items) => {
            format!(
                "[{}]",
                items.iter().map(canonical).collect::<Vec<_>>().join(",")
            )
        }
        JsonValue::String(s) => format!("{s:?}"),
        JsonValue::Number(n) => n.clone(),
        JsonValue::Bool(b) => b.to_string(),
        JsonValue::Null => "null".to_string(),
    }
}

/// For every `argv.json` entry the CLI patch equals the non-null fields of
/// Java's `CliSettings.getSettings()`, and the arguments it parsed are the
/// same (`layers` and the per-layer cost arrays are compared after the board
/// tuning, as in `dump_patch`).
#[test]
fn argv_json_cli_patch_equals_java() {
    let mut compared = 0;
    for entry in entries() {
        let argv = args_of(&entry);
        let java = member(&entry, "cliSettings").unwrap();
        let ours = parse(&argv);
        // Cores far from any value the dump uses, so no thread count is
        // renamed to `<cores>`.
        let patch = dump_patch(&ours.patch, 1000);
        let patch = JsonValue::parse(&patch.to_pretty_string()).unwrap();
        let expected = member(java, "settings").unwrap();
        assert_eq!(
            canonical(&patch),
            canonical(&static_part(expected)),
            "{argv:?}"
        );
        let parsed: BTreeMap<String, String> = ours.parsed_arguments.clone();
        let java_parsed: BTreeMap<String, String> = match member(java, "parsedArguments") {
            Some(JsonValue::Object(m)) => m
                .iter()
                .filter_map(|(k, v)| string_of(Some(v)).map(|v| (k.clone(), v)))
                .collect(),
            _ => BTreeMap::new(),
        };
        assert_eq!(parsed, java_parsed, "{argv:?}");
        compared += 1;
    }
    assert!(compared >= 100);
}

/// `dump_patch` leaves out `layers` and the two per-layer cost arrays.
fn static_part(java: &JsonValue) -> JsonValue {
    let JsonValue::Object(top) = java else {
        return java.clone();
    };
    JsonValue::Object(
        top.iter()
            .filter(|(k, _)| k != "layers")
            .map(|(k, v)| match (k.as_str(), v) {
                ("scoring", JsonValue::Object(s)) => (
                    k.clone(),
                    JsonValue::Object(
                        s.iter()
                            .filter(|(n, _)| {
                                n != "preferred_direction_trace_cost"
                                    && n != "undesired_direction_trace_cost"
                            })
                            .cloned()
                            .collect(),
                    ),
                ),
                _ => (k.clone(), v.clone()),
            })
            .collect(),
    )
}

// ---- C08.6: the long-form aliases and the reserved seed ----------------

/// The plugin-facing flags are aliases of settings: they set the same patch
/// fields as their Java forms and never warn as unknown flags. Rust-only
/// (Java has no such flags), so the expected values are the spec's.
mod plugin_flags {
    use super::*;
    use freerouting_cli::args::{EXTRA_FLAGS, ProgressMode, parse_timeout_seconds, timeout_text};

    fn timeout_patch(value: &str) -> (Option<String>, Args) {
        let a = parse_strs(&["--timeout", value]);
        (a.patch.job_timeout.clone(), a)
    }

    #[test]
    fn timeout_is_job_timeout() {
        for (given, expected) in [
            ("90s", "00:01:30"),
            ("10m", "00:10:00"),
            ("2h", "02:00:00"),
            ("1:30", "00:01:30"),
            ("01:02:03", "01:02:03"),
            ("45", "00:00:45"),
            ("24h", "24:00:00"),
        ] {
            let (patch, a) = timeout_patch(given);
            assert_eq!(patch.as_deref(), Some(expected), "{given}");
            assert!(a.usage_errors.is_empty(), "{given}: {:?}", a.usage_errors);
            assert_eq!(count(&a, LogLevel::Warn), 0, "{given}");
        }
        // The `=` form.
        assert_eq!(
            parse_strs(&["--timeout=10m"]).patch.job_timeout.as_deref(),
            Some("00:10:00")
        );
    }

    #[test]
    fn a_timeout_over_24_hours_is_capped_with_a_warning() {
        let (patch, a) = timeout_patch("25h");
        assert_eq!(patch.as_deref(), Some("24:00:00"));
        assert_eq!(count(&a, LogLevel::Warn), 1, "{:?}", a.log);
        assert!(a.usage_errors.is_empty());
    }

    #[test]
    fn a_bad_timeout_is_a_usage_error() {
        for bad in [
            "x",
            "",
            "10x",
            "-5",
            "1:2:3:4",
            "h",
            "1.5h",
            "9999999999999999999999s",
        ] {
            let a = parse_strs(&["--timeout", bad]);
            assert_eq!(a.patch.job_timeout, None, "{bad}");
            assert_eq!(a.usage_errors.len(), 1, "{bad}: {:?}", a.usage_errors);
        }
        let a = parse_strs(&["--timeout"]);
        assert_eq!(a.usage_errors, ["--timeout needs a value"]);
    }

    #[test]
    fn timeout_text_and_parse_agree() {
        assert_eq!(parse_timeout_seconds("90s"), Some(90));
        assert_eq!(parse_timeout_seconds(" 2H "), Some(7200));
        assert_eq!(timeout_text(86_400), "24:00:00");
        for seconds in [0, 1, 59, 60, 3599, 3600, 86_399] {
            assert_eq!(parse_timeout_seconds(&timeout_text(seconds)), Some(seconds));
        }
    }

    #[test]
    fn max_passes_is_mp() {
        for value in ["3", "0", "9999", "0x10"] {
            let long = parse_strs(&["--max-passes", value]);
            let short = parse_strs(&["-mp", value]);
            assert_eq!(
                format!("{:?}", long.patch),
                format!("{:?}", short.patch),
                "{value}"
            );
            assert!(long.usage_errors.is_empty());
        }
        assert_eq!(
            parse_strs(&["--max-passes", "3"])
                .patch
                .autorouter
                .max_passes,
            Some(3)
        );
        assert_eq!(
            parse_strs(&["--max-passes=7"]).patch.autorouter.max_passes,
            Some(7)
        );
        let bad = parse_strs(&["--max-passes", "many"]);
        assert_eq!(bad.usage_errors.len(), 1);
    }

    #[test]
    fn threads_sets_both_thread_fields() {
        let a = parse_strs(&["--threads", "4"]);
        assert_eq!(a.patch.autorouter.max_threads, Some(4));
        assert_eq!(a.patch.optimizer.max_threads, Some(4));
        assert!(a.usage_errors.is_empty());
        for bad in ["0", "-1", "x", "2.5"] {
            let a = parse_strs(&["--threads", bad]);
            assert_eq!(a.usage_errors.len(), 1, "{bad}");
            assert_eq!(a.patch.autorouter.max_threads, None, "{bad}");
        }
    }

    #[test]
    fn seed_zero_is_accepted_and_any_other_value_is_refused() {
        let a = parse_strs(&["--seed", "0"]);
        assert!(a.usage_errors.is_empty(), "{:?}", a.usage_errors);
        assert_eq!(count(&a, LogLevel::Warn), 0);
        let a = parse_strs(&["--seed", "5"]);
        assert_eq!(a.usage_errors.len(), 1);
        assert!(
            a.usage_errors[0].contains("seeded variation is not supported yet"),
            "{:?}",
            a.usage_errors
        );
        assert_eq!(parse_strs(&["--seed", "x"]).usage_errors.len(), 1);
        assert_eq!(parse_strs(&["--seed"]).usage_errors.len(), 1);
    }

    #[test]
    fn result_json_progress_version_and_capabilities() {
        let a = parse_strs(&["--result-json", "out/r.json"]);
        assert_eq!(a.patch.result_json.as_deref(), Some("out/r.json"));
        let b = parse_strs(&["--router.result_json=out/r.json"]);
        assert_eq!(format!("{:?}", a.patch), format!("{:?}", b.patch));

        assert_eq!(
            parse_strs(&["--progress", "json"]).progress,
            ProgressMode::Json
        );
        assert_eq!(
            parse_strs(&["--progress=json"]).progress,
            ProgressMode::Json
        );
        assert_eq!(parse_strs(&[]).progress, ProgressMode::Off);
        assert_eq!(parse_strs(&["--progress", "xml"]).usage_errors.len(), 1);

        assert!(parse_strs(&["--version"]).version);
        assert!(parse_strs(&["--capabilities"]).capabilities);
        assert!(!parse_strs(&["-de", "a.dsn"]).version);
    }

    /// The extras are known flags: none warns as unknown, and a value after
    /// one is not read as a file.
    #[test]
    fn the_extras_are_not_unknown_arguments() {
        let a = parse_strs(&[
            "-de",
            "a.dsn",
            "-do",
            "a.ses",
            "--progress",
            "json",
            "--timeout",
            "10m",
            "--max-passes",
            "3",
            "--threads",
            "2",
            "--seed",
            "0",
            "--result-json",
            "r.json",
        ]);
        assert_eq!(count(&a, LogLevel::Warn), 0, "{:?}", a.log);
        assert!(a.usage_errors.is_empty());
        assert_eq!(a.design_file.as_deref(), Some("a.dsn"));
        assert_eq!(a.output_file.as_deref(), Some("a.ses"));
        assert_eq!(EXTRA_FLAGS.len(), 12);
    }

    /// K10.3: `--apply-design-rules` is a switch without a value: it sets
    /// the flag, warns about nothing and leaves the next argument alone.
    #[test]
    fn apply_design_rules_is_a_switch() {
        let a = parse_strs(&["--apply-design-rules", "-de", "a.kicad_pcb", "-do", "a.ses"]);
        assert!(a.apply_design_rules);
        assert_eq!(count(&a, LogLevel::Warn), 0, "{:?}", a.log);
        assert!(a.usage_errors.is_empty());
        assert_eq!(a.design_file.as_deref(), Some("a.kicad_pcb"));
        assert!(!parse_strs(&["-de", "a.dsn"]).apply_design_rules);
    }

    /// The settings file path stays together with its flag.
    #[test]
    fn settings_keeps_its_value_even_if_it_looks_like_a_flag() {
        let a = parse_strs(&["--settings", "--version", "-de", "a.dsn"]);
        assert!(!a.version);
    }
}

//! Settings parity gates (C07.7, KRT's rule): mechanical checks that the
//! command line, its help and `main.rs` stay in step with the settings table.
//! Each gate is a function over a table or a text, run on the real one by the
//! gate test and on a damaged copy by a negative control that must fail.

use std::path::Path;

use freerouting::job::prepare::{JobInput, RulesChoice, prepare_job};
use freerouting::settings::dump::dump_patch;
use freerouting::settings::{FIELDS, ResolveContext, field_by_path, parse_cli_arguments};
use freerouting_cli::args::{self, FLAGS, Flag, LONG_FLAGS, Setting};
use freerouting_cli::cli::build_job_input;

mod common;
use common::repo;

const CORES: usize = 8;

fn source(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join(name)).unwrap()
}

// ---- gate 1: every flag maps to a FIELDS path or says why not ----------

/// The problems of a flag table: a path that is not in `FIELDS`, a `CliOnly`
/// without a reason, a flag missing from the help.
fn check_flag_table(flags: &[Flag], long_flags: &[(&str, Setting)], help: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut rows: Vec<(&str, Setting)> = flags.iter().map(|f| (f.name, f.setting)).collect();
    rows.extend(long_flags.iter().copied());
    for (name, setting) in rows {
        match setting {
            Setting::Paths(paths) => {
                for path in paths {
                    if field_by_path(path).is_none() {
                        problems.push(format!("{name}: '{path}' is not a FIELDS path"));
                    }
                }
                if paths.is_empty() {
                    problems.push(format!("{name}: no path"));
                }
            }
            Setting::CliOnly(reason) => {
                if reason.trim().is_empty() {
                    problems.push(format!("{name}: CLI-only without a reason"));
                }
            }
        }
        if !help.contains(name) {
            problems.push(format!("{name}: not in help.txt"));
        }
    }
    problems
}

#[test]
fn every_flag_maps_to_a_fields_path_or_is_cli_only() {
    let help = source("src/help.txt");
    assert_eq!(
        check_flag_table(FLAGS, LONG_FLAGS, &help),
        Vec::<String>::new()
    );
}

#[test]
fn an_unmapped_flag_fails_the_gate() {
    let help = source("src/help.txt");
    let mut flags = FLAGS.to_vec();
    flags.push(Flag {
        name: "-zz",
        matching: args::Matching::Prefix,
        id: args::FlagId::Host,
        takes_value: true,
        setting: Setting::Paths(&["autorouter.no_such_setting"]),
    });
    let problems = check_flag_table(&flags, LONG_FLAGS, &help);
    assert!(
        problems.iter().any(|p| p.contains("not a FIELDS path")),
        "{problems:?}"
    );
    assert!(
        problems.iter().any(|p| p.contains("not in help.txt")),
        "{problems:?}"
    );
    flags.push(Flag {
        name: "-yy",
        matching: args::Matching::Prefix,
        id: args::FlagId::Host,
        takes_value: true,
        setting: Setting::CliOnly(""),
    });
    let problems = check_flag_table(&flags, LONG_FLAGS, &help);
    assert!(
        problems.iter().any(|p| p.contains("without a reason")),
        "{problems:?}"
    );
}

/// The mapping is true: each flag with paths, given the default value of its
/// field, sets exactly those fields.
#[test]
fn mapped_flags_set_their_fields() {
    fn value_of(path: &str) -> String {
        let field = field_by_path(path).unwrap();
        match (field.default)(CORES) {
            Some(freerouting::settings::FieldValue::StrList(_)) | None => "A,B".to_string(),
            Some(v) => v.to_string(),
        }
    }
    let mut checked = 0;
    for flag in FLAGS {
        let Setting::Paths(paths) = flag.setting else {
            continue;
        };
        let a = args::parse(&[flag.name.to_string(), value_of(paths[0])]);
        for path in paths {
            let field = field_by_path(path).unwrap();
            assert!(
                (field.get)(&a.patch).is_some(),
                "{} did not set {path}",
                flag.name
            );
            checked += 1;
        }
    }
    for (name, setting) in LONG_FLAGS {
        let Setting::Paths(paths) = setting else {
            continue;
        };
        let a = args::parse(&[format!("{name}=v2")]);
        for path in *paths {
            let field = field_by_path(path).unwrap();
            assert!((field.get)(&a.patch).is_some(), "{name} did not set {path}");
            checked += 1;
        }
    }
    assert!(checked >= 8, "{checked}");
}

/// Every settings field is reachable as `--router.<path>=<value>`.
#[test]
fn every_field_is_reachable_from_the_command_line() {
    let mut checked = 0;
    for field in FIELDS.iter() {
        let Some(value) = (field.default)(CORES) else {
            continue;
        };
        let text = match value {
            freerouting::settings::FieldValue::StrList(_) => "A".to_string(),
            other => other.to_string(),
        };
        let a = args::parse(&[format!("--router.{}={text}", field.path)]);
        assert!(
            (field.get)(&a.patch).is_some(),
            "--router.{} is not accepted: {:?}",
            field.path,
            a.log
        );
        checked += 1;
    }
    assert!(checked > 40, "{checked}");
}

// ---- gate 2: no default in help.txt or the argument parser differs -----

/// The `Setting: <paths> [default: <v>]` lines of the help against `FIELDS`.
fn check_help_defaults(help: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for line in help.lines() {
        let Some(rest) = line.trim().strip_prefix("Setting: ") else {
            continue;
        };
        let (paths, default) = match rest.split_once(" [default: ") {
            Some((paths, default)) => (paths, default.strip_suffix(']')),
            None => (rest, None),
        };
        let paths: Vec<&str> = paths.split(", ").collect();
        for path in &paths {
            if field_by_path(path).is_none() {
                problems.push(format!("help names '{path}', which is not a FIELDS path"));
            }
        }
        if let (Some(default), [path]) = (default, paths.as_slice())
            && let Some(field) = field_by_path(path)
        {
            let actual = (field.default)(CORES).map(|v| v.to_string());
            if actual.as_deref() != Some(default) {
                problems.push(format!(
                    "help says {path} defaults to {default}, FIELDS says {actual:?}"
                ));
            }
        }
    }
    problems
}

/// Lines of `text` that write a numeric default of their own: `unwrap_or(7)`.
fn inline_defaults(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| !l.starts_with("//"))
        .filter(|l| {
            ["unwrap_or(", "unwrap_or_else(|| ", "or(Some("]
                .iter()
                .any(|p| {
                    l.split(p)
                        .skip(1)
                        .any(|r| r.starts_with(|c: char| c.is_ascii_digit() || c == '-'))
                })
        })
        .map(str::to_string)
        .collect()
}

#[test]
fn no_default_in_help_or_args_differs_from_fields() {
    let help = source("src/help.txt");
    assert_eq!(check_help_defaults(&help), Vec::<String>::new());
    assert!(
        help.contains("[default: "),
        "the help names at least one default"
    );
    for file in ["src/args.rs", "src/cli.rs"] {
        assert_eq!(
            inline_defaults(&source(file)),
            Vec::<String>::new(),
            "{file}"
        );
    }
}

#[test]
fn a_changed_default_fails_the_gate() {
    let help = source("src/help.txt");
    let changed = help.replacen("[default: 2.5]", "[default: 3.5]", 1);
    assert_ne!(changed, help);
    let problems = check_help_defaults(&changed);
    assert!(
        problems
            .iter()
            .any(|p| p.contains("optimizer.improvement_threshold")),
        "{problems:?}"
    );
    let renamed = help.replacen("autorouter.max_passes", "autorouter.max_pass", 1);
    assert!(!check_help_defaults(&renamed).is_empty());
    // An inlined default in the parser.
    let damaged = format!(
        "{}\nlet passes = value.parse().unwrap_or(25);\n",
        source("src/args.rs")
    );
    assert_eq!(inline_defaults(&damaged).len(), 1);
}

// ---- gate 3: main.rs holds no behaviour --------------------------------

/// `text` without its `//` comments.
fn code_of(text: &str) -> String {
    let mut code = String::new();
    for line in text.lines() {
        code.push_str(line.split("//").next().unwrap_or(""));
        code.push('\n');
    }
    code
}

/// The names called in `text` (an identifier followed by `(`), comments and
/// string literals removed.
fn calls(text: &str) -> Vec<String> {
    let code = code_of(text);
    let mut names = Vec::new();
    let bytes = code.as_bytes();
    let mut i = 0;
    let mut in_string = false;
    while i < bytes.len() {
        let c = bytes[i];
        if c == b'"' {
            in_string = !in_string;
        }
        if !in_string && (c.is_ascii_alphabetic() || c == b'_') {
            let start = i;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            if bytes.get(i) == Some(&b'(') {
                names.push(code[start..i].to_string());
            }
            continue;
        }
        i += 1;
    }
    names
}

/// `main.rs` may only call `main` itself, `std::env::args`, the iterator
/// plumbing that collects them and `freerouting_cli::cli::run`.
fn main_rs_problems(text: &str) -> Vec<String> {
    const ALLOWED: &[&str] = &["main", "args", "skip", "collect", "run"];
    let mut problems: Vec<String> = calls(text)
        .into_iter()
        .filter(|name| !ALLOWED.contains(&name.as_str()))
        .map(|name| format!("main.rs calls {name}()"))
        .collect();
    if code_of(text).contains("freerouting::") {
        problems.push("main.rs names the engine".to_string());
    }
    problems
}

/// `cli.rs` reaches the engine only through `drc`, `job` and `settings`: it
/// holds no board logic.
fn cli_rs_problems(text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for module in [
        "board",
        "io",
        "autoroute",
        "scoring",
        "rules",
        "library",
        "geometry",
        "dsn",
        "quality",
        "datastructures",
    ] {
        if code_of(text).contains(&format!("freerouting::{module}")) {
            problems.push(format!("cli.rs uses freerouting::{module}"));
        }
    }
    problems
}

#[test]
fn main_rs_holds_no_behaviour() {
    assert_eq!(
        main_rs_problems(&source("src/main.rs")),
        Vec::<String>::new()
    );
    assert_eq!(cli_rs_problems(&source("src/cli.rs")), Vec::<String>::new());
    // The entry point calls the library and nothing else.
    assert!(calls(&source("src/main.rs")).contains(&"run".to_string()));
}

#[test]
fn a_stray_call_in_main_rs_fails_the_gate() {
    let damaged = source("src/main.rs").replace(
        "freerouting_cli::cli::run(&argv)",
        "{ let _ = std::fs::read(\"x\"); freerouting_cli::cli::run(&argv) }",
    );
    assert_ne!(damaged, source("src/main.rs"));
    let problems = main_rs_problems(&damaged);
    assert_eq!(problems, ["main.rs calls read()"]);
    let engine = source("src/main.rs").replace(
        "freerouting_cli::cli::run(&argv)",
        "{ let _ = freerouting::board::BasicBoard::default(); freerouting_cli::cli::run(&argv) }",
    );
    assert!(!main_rs_problems(&engine).is_empty());
    let cli = format!(
        "use freerouting::board::RoutingBoard;\n{}",
        source("src/cli.rs")
    );
    assert_eq!(cli_rs_problems(&cli), ["cli.rs uses freerouting::board"]);
}

// ---- gate 4: the CLI path and prepare_job agree ------------------------

fn strings(args: &[&str]) -> Vec<String> {
    args.iter().map(|s| s.to_string()).collect()
}

/// The `cli` scenario of C01.1 (the arguments `settings_reference.rs` gives
/// Java), parsed here and by the engine's `parse_cli_arguments`.
fn scenario_args(base: &str) -> Vec<String> {
    strings(&[
        "-de",
        &format!("{base}.dsn"),
        "-do",
        &format!("{base}.ses"),
        "-mp",
        "7",
        "-mt",
        "3",
        "-oit",
        "0.035",
        "-inc",
        "POWER",
        "--router.via_costs=77",
        "--router.scoring.plane_via_costs=7",
        "--router.optimizer.enabled=false",
        "--router.default_undesired_direction_trace_cost=1.5",
    ])
}

/// C02.8's `cli` scenario goes through this parser and gives the patch it
/// gave before (`settings_reference.rs` compares that patch with Java's dumps
/// of the scenario on every board).
#[test]
fn the_cli_scenario_patch_is_the_engines() {
    for base in ["Issue413-test", "Issue187-processor.Z80", "x"] {
        let argv = scenario_args(base);
        let ours = args::parse(&argv);
        let engine = parse_cli_arguments(&argv);
        assert_eq!(
            dump_patch(&ours.patch, 1000).to_pretty_string(),
            dump_patch(&engine.patch, 1000).to_pretty_string()
        );
        assert_eq!(ours.parsed_arguments, engine.parsed_arguments);
        assert_eq!(
            ours.design_file.as_deref(),
            Some(format!("{base}.dsn").as_str())
        );
    }
}

#[test]
fn the_cli_path_and_prepare_job_give_equal_dumps() {
    let ctx = ResolveContext { cores: CORES };
    let dsn = std::fs::read(repo("fixtures/Issue413-test.dsn")).unwrap();
    for extra in [
        vec![],
        vec!["--router.autorouter.enabled=false"],
        vec!["-mp", "10000", "-inc", "Power\\,Default"],
        vec![
            "--router.copper_to_edge_clearance_um=100",
            "--router.job_timeout=00:10:00",
        ],
    ] {
        let mut argv = scenario_args("Issue413-test");
        argv.extend(extra.iter().map(|s| s.to_string()));
        let parsed = args::parse(&argv);

        let via_cli = build_job_input(&parsed, "Issue413-test.dsn", dsn.clone(), None);
        let cli_dump = dump_patch(&prepare_job(&via_cli, &ctx).unwrap().patch, CORES);

        let mut direct = JobInput::new(dsn.clone(), "Issue413-test");
        direct.rules = RulesChoice::Adjacent;
        direct.cli = Some(parsed.patch.clone());
        let direct_dump = dump_patch(&prepare_job(&direct, &ctx).unwrap().patch, CORES);
        assert_eq!(
            cli_dump.to_pretty_string(),
            direct_dump.to_pretty_string(),
            "{extra:?}"
        );
    }
}

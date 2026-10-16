//! The command line: files, modes and settings flags (C07.1 to C07.3).
//!
//! Port of Java's two argument parsers into one: `GlobalSettings.
//! applyCommandLineArguments` (files, modes, application settings) and
//! `CliSettings.parseArguments` (router settings), at `a773a70ec`. Java runs
//! both on the same arguments and logs some warnings twice; [`parse`] runs one
//! walk for the files and flags and hands the router settings to
//! [`freerouting::settings::parse_cli_arguments`], so each warning appears
//! once. Logs are not results.
//!
//! # Java's rules, kept
//!
//! - Flags match by prefix, in the order of [`FLAGS`] (`-dex` is `-de`;
//!   `-drc` is tried before `-dr`).
//! - A value is taken only when the next argument does not start with `-`.
//!   `-mp -5` therefore leaves `-5` as an unknown argument.
//! - `-de` takes arguments until the next `-...`. An argument that is an
//!   existing path is used whole; otherwise it is split on `+` when every
//!   part ends in a valid extension (`.dsn .json .ses .rules`), so
//!   `mechkeys_MF68+10--...ses` and `board.v1+final.dsn` stay whole. The
//!   files are classified in argument order; the last DSN wins, with Java's
//!   warnings. `.json` is the input before a DSN and the session after one.
//! - `-do`: the first file is the primary output, the others (and `+` parts)
//!   are extra outputs.
//! - `-mp` and `-mt` values are read with `Integer.decode`; a value Java
//!   cannot decode is logged as an error and left to be read as an argument.
//! - `-mt` sets the flat `max_threads`, which no stage reads headless, so it
//!   does nothing (`mt_does_nothing_headless`).
//! - `-de` with `-do` switches the router on, unless `--router.enabled` or
//!   `--router.autorouter.enabled` is given.
//!
//! # Differences from Java
//!
//! - `-inc` splits on commas, and a backslash keeps one: `-inc 'Power\,Default'`
//!   names the class `Power,Default`, which KiCad writes (DIVERGENCES).
//! - `-drc` without a file keeps the report on standard output; Java falls
//!   through to routing.
//! - `--settings FILE` and `--user_data_path=DIR` choose the settings file
//!   ([`ConfigChoice`]).
//! - `--compare-boards` and `--calculate-benchmark-scores` are recognized and
//!   refused ([`Unsupported`]).
//! - `--x=y` for an application setting (`gui`, `api_server`, ...) is accepted
//!   and ignored; `-l`, `-us`, `-is`, `-host`, `-di`, `-dct`, `-dl`, `-da` too.

use std::collections::BTreeMap;
use std::path::Path;

use freerouting::job::{LogLevel, LogLine};
use freerouting::settings::config_path::ConfigChoice;
use freerouting::settings::{SettingsPatch, parse_cli_arguments};

/// How a flag is matched.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Matching {
    /// `arg.startsWith(name)`.
    Prefix,
    /// `arg.equals(name)`.
    Exact,
    /// `arg.equalsIgnoreCase(name)`.
    ExactIgnoreCase,
}

/// What a flag does to the settings.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Setting {
    /// Sets these `FIELDS` paths (through `CliSettings`).
    Paths(&'static [&'static str]),
    /// Sets no router setting, for this reason.
    CliOnly(&'static str),
}

/// The flags this parser knows, in Java's order of testing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlagId {
    Help,
    DesignFiles,
    InputDirectory,
    OutputFiles,
    DrcMode,
    RulesFile,
    MaxPasses,
    MaxThreads,
    ImprovementThreshold,
    UpdateStrategy,
    ItemSelection,
    Language,
    DisableFileLog,
    DisableAnalytics,
    Host,
    IgnoreNetClasses,
    DialogTimeout,
    LogLevel,
}

/// One row of [`FLAGS`].
#[derive(Clone, Copy, Debug)]
pub struct Flag {
    pub name: &'static str,
    pub matching: Matching,
    pub id: FlagId,
    pub takes_value: bool,
    pub setting: Setting,
}

const fn flag(
    name: &'static str,
    matching: Matching,
    id: FlagId,
    takes_value: bool,
    setting: Setting,
) -> Flag {
    Flag {
        name,
        matching,
        id,
        takes_value,
        setting,
    }
}

/// Every single-dash flag, in the order `GlobalSettings` tests them (a
/// prefix match takes the first row that matches). The settings each flag
/// reaches are named here and nowhere else (C07.7 checks them against
/// `FIELDS`). The long flags `--scoring-version=`, `--router-scoring-version=`
/// and `--optimizer-scoring-version=` and the whole `--router.<path>=` and
/// `--optimizer.<path>=` family are in [`LONG_FLAGS`].
pub const FLAGS: &[Flag] = &[
    flag(
        "-help",
        Matching::ExactIgnoreCase,
        FlagId::Help,
        false,
        Setting::CliOnly("prints the help"),
    ),
    flag(
        "-h",
        Matching::ExactIgnoreCase,
        FlagId::Help,
        false,
        Setting::CliOnly("prints the help"),
    ),
    flag(
        "-de",
        Matching::Prefix,
        FlagId::DesignFiles,
        true,
        Setting::CliOnly("input files"),
    ),
    flag(
        "-di",
        Matching::Prefix,
        FlagId::InputDirectory,
        true,
        Setting::CliOnly("GUI dialog folder; ignored"),
    ),
    flag(
        "-do",
        Matching::Prefix,
        FlagId::OutputFiles,
        true,
        Setting::CliOnly("output files"),
    ),
    flag(
        "-drc",
        Matching::Prefix,
        FlagId::DrcMode,
        true,
        Setting::CliOnly("design rules check mode"),
    ),
    flag(
        "-dr",
        Matching::Prefix,
        FlagId::RulesFile,
        true,
        Setting::CliOnly("rules file"),
    ),
    flag(
        "-mp",
        Matching::Prefix,
        FlagId::MaxPasses,
        true,
        Setting::Paths(&["autorouter.max_passes"]),
    ),
    flag(
        "-mt",
        Matching::Prefix,
        FlagId::MaxThreads,
        true,
        Setting::Paths(&["max_threads"]),
    ),
    flag(
        "-oit",
        Matching::Prefix,
        FlagId::ImprovementThreshold,
        true,
        Setting::Paths(&["optimizer.improvement_threshold"]),
    ),
    flag(
        "-us",
        Matching::Prefix,
        FlagId::UpdateStrategy,
        true,
        Setting::CliOnly("Java applies it to a settings object no stage reads headless; ignored"),
    ),
    flag(
        "-is",
        Matching::Prefix,
        FlagId::ItemSelection,
        true,
        Setting::CliOnly("Java applies it to a settings object no stage reads headless; ignored"),
    ),
    flag(
        "-l",
        Matching::Exact,
        FlagId::Language,
        true,
        Setting::CliOnly("GUI language; ignored"),
    ),
    flag(
        "-dl",
        Matching::Prefix,
        FlagId::DisableFileLog,
        false,
        Setting::CliOnly("file logging is not written; ignored"),
    ),
    flag(
        "-da",
        Matching::Prefix,
        FlagId::DisableAnalytics,
        false,
        Setting::CliOnly("there is no analytics; ignored"),
    ),
    flag(
        "-host",
        Matching::Prefix,
        FlagId::Host,
        true,
        Setting::CliOnly("the calling program's name; ignored"),
    ),
    flag(
        "-help",
        Matching::Prefix,
        FlagId::Help,
        false,
        Setting::CliOnly("prints the help"),
    ),
    flag(
        "-inc",
        Matching::Prefix,
        FlagId::IgnoreNetClasses,
        true,
        Setting::Paths(&["autorouter.ignore_net_classes"]),
    ),
    flag(
        "-dct",
        Matching::Prefix,
        FlagId::DialogTimeout,
        true,
        Setting::CliOnly("GUI dialog timeout; ignored"),
    ),
    flag(
        "-ll",
        Matching::Prefix,
        FlagId::LogLevel,
        true,
        Setting::CliOnly("console log level"),
    ),
];

/// The `--name=value` flags that reach router settings by a name of their
/// own. `--router.<path>=` and `--optimizer.<path>=` take any `FIELDS` path.
pub const LONG_FLAGS: &[(&str, Setting)] = &[
    (
        "--scoring-version",
        Setting::Paths(&["router_scoring.version", "optimizer_scoring.version"]),
    ),
    (
        "--router-scoring-version",
        Setting::Paths(&["router_scoring.version"]),
    ),
    (
        "--optimizer-scoring-version",
        Setting::Paths(&["optimizer_scoring.version"]),
    ),
    ("--settings", Setting::CliOnly("chooses the settings file")),
    (
        "--user_data_path",
        Setting::CliOnly("chooses the settings directory"),
    ),
    (
        "--logging.console.level",
        Setting::CliOnly("console log level"),
    ),
];

/// The sections of Java's application settings: `--section.key=value` and
/// `--section-key=value` are accepted for them and have no effect here.
pub const APPLICATION_SECTIONS: &[&str] = &[
    "profile",
    "gui",
    "drc",
    "usage_and_diagnostic_data",
    "feature_flags",
    "api_server",
    "mcp_server",
    "statistics",
    "logging",
    "network",
    "debug",
    "version",
];

/// A Java command that is recognized and refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Unsupported {
    /// `--compare-boards=<a>,<b>`.
    CompareBoards,
    /// `--calculate-benchmark-scores`.
    CalculateBenchmarkScores,
}

impl Unsupported {
    /// The flag, for the message.
    pub fn flag(&self) -> &'static str {
        match self {
            Unsupported::CompareBoards => "--compare-boards",
            Unsupported::CalculateBenchmarkScores => "--calculate-benchmark-scores",
        }
    }
}

/// The parsed command line.
#[derive(Debug, Default)]
pub struct Args {
    /// `initialInputFile`: the design file (a `.json` too, which is refused
    /// later).
    pub design_file: Option<String>,
    /// `designSessionFilename`.
    pub session_file: Option<String>,
    /// `initialRulesFile`.
    pub rules_file: Option<String>,
    /// `initialOutputFile`: the primary output.
    pub output_file: Option<String>,
    /// `additionalOutputFiles`.
    pub additional_outputs: Vec<String>,
    /// `-drc` was given.
    pub drc: bool,
    /// The file after `-drc`.
    pub drc_report_file: Option<String>,
    /// `-help`, `--help` or `-h`.
    pub help: bool,
    /// A Java command that is refused.
    pub unsupported: Option<Unsupported>,
    /// The router settings of the command line (priority 60).
    pub patch: SettingsPatch,
    /// Java's `getParsedArguments()`.
    pub parsed_arguments: BTreeMap<String, String>,
    /// Where the settings file is (`--settings`, `--user_data_path=`).
    pub config: ConfigChoice,
    /// `-ll` or `--logging.console.level`, upper-cased.
    pub console_level: Option<String>,
    /// The log of the parse: Java's warnings and errors, and a debug line
    /// for each flag that is accepted and ignored.
    pub log: Vec<LogLine>,
}

impl Args {
    fn note(&mut self, level: LogLevel, text: impl Into<String>) {
        self.log.push(LogLine {
            level,
            text: text.into(),
        });
    }

    fn ignored(&mut self, what: &str) {
        self.note(
            LogLevel::Debug,
            format!("Ignored command line argument: {what}"),
        );
    }
}

/// `Integer.decode`: an optional sign, then decimal, `0x`/`0X`/`#` hexadecimal
/// or leading-`0` octal digits. `None` where Java throws.
pub fn java_decode_int(text: &str) -> Option<i32> {
    let (negative, rest) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (radix, digits) = if let Some(hex) = rest
        .strip_prefix("0x")
        .or_else(|| rest.strip_prefix("0X"))
        .or_else(|| rest.strip_prefix('#'))
    {
        (16, hex)
    } else if rest.len() > 1 && rest.starts_with('0') {
        (8, &rest[1..])
    } else {
        (10, rest)
    };
    if digits.is_empty() || digits.starts_with(['-', '+']) {
        return None;
    }
    let magnitude = i64::from_str_radix(digits, radix).ok()?;
    i32::try_from(if negative { -magnitude } else { magnitude }).ok()
}

/// `GlobalSettings.isMultiFileConcatenation`: whether `raw` is `a.ext+b.ext`
/// (every part non-empty with a valid extension) rather than one name with a
/// `+` in it.
pub fn is_multi_file_concatenation(raw: &str, valid_extensions: &[&str]) -> bool {
    if !raw.contains('+') {
        return false;
    }
    let parts: Vec<&str> = raw.split('+').collect();
    if parts.len() < 2 {
        return false;
    }
    parts.iter().all(|part| {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            return false;
        }
        let last_slash = trimmed.rfind(['/', '\\']);
        let Some(last_dot) = trimmed.rfind('.') else {
            return false;
        };
        if last_slash.is_some_and(|slash| last_dot <= slash) || last_dot == trimmed.len() - 1 {
            return false;
        }
        let extension = trimmed[last_dot..].to_lowercase();
        valid_extensions.contains(&extension.as_str())
    })
}

/// `DE_VALID_EXTENSIONS`, and `.kicad_pcb` (K07.2: a KiCad board read directly; Java refuses it).
pub const DE_VALID_EXTENSIONS: &[&str] = &[".dsn", ".json", ".ses", ".rules", ".kicad_pcb"];
/// `DO_VALID_EXTENSIONS`.
pub const DO_VALID_EXTENSIONS: &[&str] = &[
    ".ses",
    ".dsn",
    ".scr",
    ".rules",
    ".json",
    ".drc",
    ".frb",
    ".kicad_pcb",
];

fn split_plus(raw: &str) -> Vec<String> {
    raw.split('+')
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .map(str::to_string)
        .collect()
}

/// The first row of [`FLAGS`] that matches `arg`.
fn flag_of(arg: &str) -> Option<&'static Flag> {
    FLAGS.iter().find(|f| match f.matching {
        Matching::Prefix => arg.starts_with(f.name),
        Matching::Exact => arg == f.name,
        Matching::ExactIgnoreCase => arg.eq_ignore_ascii_case(f.name),
    })
}

/// The value after `argv[i]`, if the next argument does not start with `-`.
fn value_after(argv: &[String], i: usize) -> Option<&String> {
    argv.get(i + 1).filter(|a| !a.starts_with('-'))
}

/// `-de`: the files up to the next `-...` argument. Returns the index of the
/// last argument consumed.
fn parse_design_files(argv: &[String], i: usize, out: &mut Args) -> usize {
    if value_after(argv, i).is_none() {
        return i;
    }
    let mut files: Vec<String> = Vec::new();
    let mut j = i + 1;
    while j < argv.len() && !argv[j].starts_with('-') {
        let raw = argv[j].trim();
        if Path::new(raw).exists() {
            files.push(raw.to_string());
        } else if is_multi_file_concatenation(raw, DE_VALID_EXTENSIONS) {
            files.extend(split_plus(raw));
        } else {
            files.push(raw.to_string());
        }
        j += 1;
    }
    let (mut has_dsn, mut has_ses, mut has_rules) = (false, false, false);
    for file in files {
        let file = file.trim().to_string();
        if file.is_empty() {
            continue;
        }
        let lower = file.to_lowercase();
        if lower.ends_with(".dsn") || lower.ends_with(".kicad_pcb") {
            if has_dsn {
                out.note(
                    LogLevel::Warn,
                    "Multiple DSN files provided in -de argument. Only the last one will be used.",
                );
            }
            out.design_file = Some(file);
            has_dsn = true;
        } else if lower.ends_with(".json") {
            if !has_dsn {
                out.design_file = Some(file);
                has_dsn = true;
            } else {
                if has_ses {
                    out.note(
                        LogLevel::Warn,
                        "Multiple session files (SES/JSON) provided in -de argument. Only the \
                         last one will be used.",
                    );
                }
                out.session_file = Some(file);
                has_ses = true;
            }
        } else if lower.ends_with(".ses") {
            if has_ses {
                out.note(
                    LogLevel::Warn,
                    "Multiple SES files provided in -de argument. Only the last one will be used.",
                );
            }
            out.session_file = Some(file);
            has_ses = true;
        } else if lower.ends_with(".rules") {
            if has_rules {
                out.note(
                    LogLevel::Warn,
                    "Multiple RULES files provided in -de argument. Only the last one will be \
                     used.",
                );
            }
            out.rules_file = Some(file);
            has_rules = true;
        } else {
            out.note(
                LogLevel::Warn,
                format!(
                    "Unknown file type in -de argument: {file}. Expected .dsn, .json, .ses, or \
                     .rules"
                ),
            );
        }
    }
    j - 1
}

/// `-do`: the output files up to the next `-...` argument.
fn parse_output_files(argv: &[String], i: usize, out: &mut Args) -> usize {
    if value_after(argv, i).is_none() {
        return i;
    }
    let mut files: Vec<String> = Vec::new();
    let mut j = i + 1;
    while j < argv.len() && !argv[j].starts_with('-') {
        let raw = argv[j].trim();
        if is_multi_file_concatenation(raw, DO_VALID_EXTENSIONS) {
            files.extend(split_plus(raw));
        } else if !raw.is_empty() {
            files.push(raw.to_string());
        }
        j += 1;
    }
    if !files.is_empty() {
        if out.output_file.is_none() {
            out.output_file = Some(files[0].clone());
            out.additional_outputs.extend(files.into_iter().skip(1));
        } else {
            out.additional_outputs.extend(files);
        }
    }
    j - 1
}

/// The part of a `--name=value` flag that names an application section:
/// the text before the first `.`, `:` or `-`.
fn section_of(name: &str) -> &str {
    name.split(['.', ':', '-']).next().unwrap_or("")
}

/// A `--name[=value]` argument.
fn parse_long_flag(argv: &[String], i: usize, out: &mut Args) -> usize {
    let arg = argv[i].as_str();
    let body = &arg[2..];
    // `--settings FILE`: the file is the next argument, whatever it looks like.
    if body == "settings" {
        return if i + 1 < argv.len() { i + 1 } else { i };
    }
    let lower = arg.to_lowercase();
    if lower == "--help" {
        out.help = true;
        return i;
    }
    if lower == "--calculate-benchmark-scores"
        || arg.starts_with("--calculate-benchmark-scores.input=")
        || arg.starts_with("--calculate-benchmark-scores.output=")
    {
        out.unsupported = Some(Unsupported::CalculateBenchmarkScores);
        return i;
    }
    if arg.starts_with("--input=") || arg.starts_with("--output=") {
        return i;
    }
    if let Some(list) = arg.strip_prefix("--compare-boards=") {
        if list.split(',').count() == 2 {
            out.unsupported = Some(Unsupported::CompareBoards);
        }
        return i;
    }
    let (name, value) = match body.split_once('=') {
        Some((name, value)) => (name, Some(value)),
        None => (body, None),
    };
    match value {
        Some(value) if name != "user_data_path" => {
            // Router settings are read by `parse_cli_arguments`.
            let router_setting = name.starts_with("router.")
                || name.starts_with("optimizer.")
                || matches!(
                    name,
                    "scoring-version" | "router-scoring-version" | "optimizer-scoring-version"
                );
            if router_setting {
                return i;
            }
            if name == "settings" {
                return i;
            }
            if name == "logging.console.level" {
                out.console_level = Some(value.to_uppercase());
            } else if APPLICATION_SECTIONS.contains(&section_of(name)) {
                out.ignored(arg);
            } else {
                out.note(LogLevel::Warn, format!("Unknown settings property: {name}"));
            }
        }
        Some(_) => {}
        None if name == "user_data_path" => {}
        None => out.note(
            LogLevel::Warn,
            format!("Unknown command line argument: {arg}"),
        ),
    }
    i
}

/// Reads a flag's value as `Integer.decode` does. On success the value is
/// consumed (the returned index is past it); on failure Java logs an error
/// and the value is read again as an argument.
fn decoded_value(argv: &[String], i: usize, out: &mut Args) -> usize {
    let Some(value) = value_after(argv, i) else {
        return i;
    };
    if java_decode_int(value).is_some() {
        i + 1
    } else {
        out.note(
            LogLevel::Error,
            format!("There was a problem parsing the '{}' parameter", argv[i]),
        );
        i
    }
}

/// Parses the arguments after the program name.
pub fn parse(argv: &[String]) -> Args {
    let mut out = Args {
        config: ConfigChoice::from_args(argv),
        ..Args::default()
    };
    let cli = parse_cli_arguments(argv);
    out.patch = cli.patch;
    out.parsed_arguments = cli.parsed_arguments;
    for warning in cli.warnings {
        out.note(LogLevel::Warn, warning);
    }
    apply_escaped_inc(argv, &mut out);

    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if arg.starts_with("--") {
            i = parse_long_flag(argv, i, &mut out) + 1;
            continue;
        }
        let Some(flag) = flag_of(arg) else {
            out.note(
                LogLevel::Warn,
                format!("Unknown command line argument: {arg}"),
            );
            i += 1;
            continue;
        };
        i = match flag.id {
            FlagId::Help => {
                out.help = true;
                i
            }
            FlagId::DesignFiles => parse_design_files(argv, i, &mut out),
            FlagId::OutputFiles => parse_output_files(argv, i, &mut out),
            FlagId::DrcMode => {
                out.drc = true;
                match value_after(argv, i) {
                    Some(file) => {
                        out.drc_report_file = Some(file.clone());
                        i + 1
                    }
                    None => i,
                }
            }
            FlagId::RulesFile => match value_after(argv, i) {
                Some(file) => {
                    out.rules_file = Some(file.clone());
                    i + 1
                }
                None => i,
            },
            FlagId::MaxPasses => decoded_value(argv, i, &mut out),
            FlagId::MaxThreads => {
                let next = decoded_value(argv, i, &mut out);
                if next > i {
                    out.note(
                        LogLevel::Info,
                        "-mt has no effect on a headless run (as in Java); use \
                         --router.optimizer.max_threads to set the optimizer's threads",
                    );
                }
                next
            }
            FlagId::DialogTimeout => match value_after(argv, i) {
                Some(value) if value.parse::<i32>().is_ok() => {
                    out.ignored(arg);
                    i + 1
                }
                Some(_) => {
                    out.note(
                        LogLevel::Error,
                        format!("There was a problem parsing the '{arg}' parameter"),
                    );
                    i
                }
                None => i,
            },
            FlagId::LogLevel => match value_after(argv, i) {
                Some(level) => {
                    out.console_level = Some(level.to_uppercase());
                    i + 1
                }
                None => i,
            },
            FlagId::ImprovementThreshold
            | FlagId::UpdateStrategy
            | FlagId::ItemSelection
            | FlagId::Host
            | FlagId::InputDirectory
            | FlagId::IgnoreNetClasses => {
                if matches!(
                    flag.id,
                    FlagId::UpdateStrategy
                        | FlagId::ItemSelection
                        | FlagId::Host
                        | FlagId::InputDirectory
                ) {
                    out.ignored(arg);
                }
                if value_after(argv, i).is_some() {
                    i + 1
                } else {
                    i
                }
            }
            FlagId::Language => {
                out.ignored(arg);
                if value_after(argv, i).is_some() {
                    i + 1
                } else {
                    i
                }
            }
            FlagId::DisableFileLog | FlagId::DisableAnalytics => {
                out.ignored(arg);
                i
            }
        } + 1;
    }
    out
}

/// The names of `-inc VALUE` with `\,` keeping a comma. Java's `split(",")`
/// removes trailing empty strings; each name is trimmed as `FIELDS` lists are.
pub fn split_net_classes(value: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = String::new();
    let mut chars = value.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&',') => {
                chars.next();
                current.push(',');
            }
            ',' => names.push(std::mem::take(&mut current)),
            other => current.push(other),
        }
    }
    names.push(current);
    while names.last().is_some_and(String::is_empty) {
        names.pop();
    }
    names.into_iter().map(|n| n.trim().to_string()).collect()
}

/// Re-reads the last `-inc VALUE` of the command line when its value has a
/// `\,` in it, and replaces the patch's `ignore_net_classes` by the names
/// [`split_net_classes`] gives. The walk is `CliSettings`': a value is taken
/// only when the next argument does not start with `-`.
fn apply_escaped_inc(argv: &[String], out: &mut Args) {
    let mut last: Option<&str> = None;
    let mut i = 0;
    while i < argv.len() {
        let arg = argv[i].as_str();
        if arg.starts_with("--") {
            i += 1;
            continue;
        }
        if let Some(flag) = arg.strip_prefix('-') {
            let value = if i + 1 < argv.len() && !argv[i + 1].starts_with('-') {
                i += 1;
                argv[i].as_str()
            } else {
                ""
            };
            if flag == "inc" {
                last = Some(value);
            }
        }
        i += 1;
    }
    if let Some(value) = last
        && value.contains("\\,")
    {
        out.patch.autorouter.ignore_net_classes = Some(split_net_classes(value));
        out.parsed_arguments.insert(
            "router.autorouter.ignore_net_classes".to_string(),
            value.to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_strs(args: &[&str]) -> Args {
        parse(&args.iter().map(|s| s.to_string()).collect::<Vec<_>>())
    }

    #[test]
    fn parses_drc_arguments() {
        let args = parse_strs(&["-de", "a.dsn", "b.SES", "-drc", "out.json"]);
        assert_eq!(args.design_file.as_deref(), Some("a.dsn"));
        assert_eq!(args.session_file.as_deref(), Some("b.SES"));
        assert!(args.drc);
        assert_eq!(args.drc_report_file.as_deref(), Some("out.json"));

        let args = parse_strs(&["-de", "a.dsn+c.rules", "-drc"]);
        assert_eq!(args.rules_file.as_deref(), Some("c.rules"));
        assert!(args.drc);
        assert_eq!(args.drc_report_file, None);

        let args = parse_strs(&["-drc", "-de", "a.dsn", "-dr", "x.rules"]);
        assert_eq!(args.design_file.as_deref(), Some("a.dsn"));
        assert_eq!(args.rules_file.as_deref(), Some("x.rules"));
        assert_eq!(args.drc_report_file, None);
    }

    #[test]
    fn integer_decode_matches_java() {
        for (text, expected) in [
            ("10", Some(10)),
            ("-5", Some(-5)),
            ("+7", Some(7)),
            ("0x10", Some(16)),
            ("0X1f", Some(31)),
            ("#ff", Some(255)),
            ("010", Some(8)),
            ("0", Some(0)),
            ("-0x80000000", Some(i32::MIN)),
            ("2147483648", None),
            ("abc", None),
            ("", None),
            ("--5", None),
            ("0x", None),
        ] {
            assert_eq!(java_decode_int(text), expected, "{text:?}");
        }
    }

    #[test]
    fn net_class_names_keep_an_escaped_comma() {
        assert_eq!(split_net_classes("Power\\,Default"), ["Power,Default"]);
        assert_eq!(split_net_classes("Power,Default"), ["Power", "Default"]);
        assert_eq!(split_net_classes("a\\,b,c"), ["a,b", "c"]);
        assert_eq!(split_net_classes("GND,,"), ["GND"]);
        assert_eq!(split_net_classes(" a , b "), ["a", "b"]);
    }
}

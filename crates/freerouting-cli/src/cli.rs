//! What the command line does: help, the design rules check and the job.
//!
//! This module reads files and prints; everything else is the engine's:
//! [`freerouting::job::run::run_job`] runs a job and writes its outputs,
//! [`freerouting::drc::run_drc`] checks a design. The exit codes are Java's
//! (`CliRunner.computeCliExitCode`): 0 when the job finished (or timed out)
//! and wrote an output of more than 0 bytes, 1 otherwise. A refused Java
//! command (`--compare-boards`, `--calculate-benchmark-scores`) exits 2.
//!
//! The settings sources are the engine's: the settings file
//! ([`load_config`]), the `FREEROUTING__ROUTER__*` environment
//! ([`parse_process_environment`]) and the command line
//! ([`Args::patch`](crate::args::Args::patch)). The core count the settings
//! merge uses is the machine's; it only sets the default thread counts, and
//! thread counts never change results (D2).

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use freerouting::drc::{DrcRunOptions, run_drc};
use freerouting::job::outputs::{self, OutputFormat};
use freerouting::job::prepare::{
    DesignKind, JobInput, KicadReadError, ReadOptions, RulesChoice, design_kind, read_kicad_input,
};
use freerouting::job::run::{JobContext, run_job};
use freerouting::job::{LogLevel, LogLine};
use freerouting::settings::ResolveContext;
use freerouting::settings::config_path::{Os, ProcessEnv};
use freerouting::settings::env::parse_process_environment;
use freerouting::settings::json_file::load_config;

use crate::args::{self, Args};
use crate::log::{self, ConsoleLevel};

/// The text of `-help`.
pub const HELP: &str = include_str!("help.txt");

/// The environment variable that makes a run stop between the temporary write
/// and the rename of an output. Only the `killed_run_leaves_no_partial_file`
/// test sets it.
pub const ABORT_BEFORE_RENAME_VAR: &str = "FREEROUTING_TEST_ABORT_BEFORE_RENAME";

/// Runs the command line and returns the exit code.
pub fn run(argv: &[String]) -> ExitCode {
    let parsed = args::parse(argv);
    let level = ConsoleLevel::from_name(parsed.console_level.as_deref());
    log::print(&parsed.log, level);
    if parsed.help {
        print!("{HELP}");
        return ExitCode::SUCCESS;
    }
    if let Some(command) = &parsed.unsupported {
        log::print(
            &[line(
                LogLevel::Error,
                format!(
                    "{} is not supported; use freerouting-dev ses-diff",
                    command.flag()
                ),
            )],
            level,
        );
        return ExitCode::from(2);
    }
    let code = if parsed.drc {
        run_drc_mode(&parsed, level)
    } else {
        run_route_mode(&parsed, level)
    };
    ExitCode::from(code)
}

fn line(level: LogLevel, text: impl Into<String>) -> LogLine {
    LogLine {
        level,
        text: text.into(),
    }
}

fn fail(level: ConsoleLevel, text: impl Into<String>) -> u8 {
    log::print(&[line(LogLevel::Error, text)], level);
    1
}

fn abort_before_rename(_temp: &Path) {
    std::process::abort();
}

/// `-drc`: checks the design and writes the report.
fn run_drc_mode(parsed: &Args, level: ConsoleLevel) -> u8 {
    let Some(design) = &parsed.design_file else {
        return fail(
            level,
            "An input file must be specified with -de argument in DRC mode.",
        );
    };
    let mut options = DrcRunOptions::new(design);
    options.session_file = parsed.session_file.as_ref().map(PathBuf::from);
    options.rules_file = parsed.rules_file.as_ref().map(PathBuf::from);
    let run = match run_drc(&options) {
        Ok(run) => run,
        Err(e) => return fail(level, e),
    };
    let messages: Vec<LogLine> = run
        .messages
        .iter()
        .map(|m| line(LogLevel::Info, m.clone()))
        .collect();
    log::print(&messages, level);
    let json = run.report.to_json();
    match &parsed.drc_report_file {
        Some(file) => {
            let path = Path::new(file);
            let written = outputs::write_atomic(path, OutputFormat::DrcReport, false, None, |w| {
                std::io::Write::write_all(w, json.as_bytes())
            });
            if let Err(e) = written {
                return fail(
                    level,
                    format!("Couldn't save the DRC report to '{file}': {e}"),
                );
            }
            log::print(
                &[line(
                    LogLevel::Info,
                    format!("DRC report written to: {file}"),
                )],
                level,
            );
        }
        None => println!("{json}"),
    }
    0
}

/// The job of `-de ... -do ...`: Java's `CliRunner.initializeCli`.
fn run_route_mode(parsed: &Args, level: ConsoleLevel) -> u8 {
    let (Some(design), Some(output)) = (&parsed.design_file, &parsed.output_file) else {
        return fail(
            level,
            "Both an input file and an output file must be specified with command line \
             arguments if you are running in CLI mode.",
        );
    };
    // Refuse what cannot be done before any work.
    let paths: Vec<PathBuf> = std::iter::once(output)
        .chain(parsed.additional_outputs.iter())
        .map(PathBuf::from)
        .collect();
    let specs = match outputs::classify(&paths) {
        Ok(specs) => specs,
        Err(e) => return fail(level, e.to_string()),
    };
    for file in [Some(design), parsed.session_file.as_ref()]
        .into_iter()
        .flatten()
    {
        if file.to_lowercase().ends_with(".json") {
            return fail(
                level,
                format!("KiCad JSON files are not supported as input: '{file}'"),
            );
        }
    }

    let mut log_lines = Vec::new();
    let (config, environment) = match settings_sources(parsed, &mut log_lines) {
        Ok(sources) => sources,
        Err(e) => {
            log::print(&log_lines, level);
            return fail(level, e);
        }
    };
    let kicad = design_kind(Path::new(design)) == DesignKind::KicadPcb;
    let (dsn, kicad_input) = if kicad {
        match read_kicad_input(Path::new(design), ReadOptions::default()) {
            Ok(input) => (Vec::new(), Some(input)),
            Err(KicadReadError::Io(e)) => {
                log::print(&log_lines, level);
                return unreadable_input(design, &e, level);
            }
            Err(KicadReadError::Refused(message)) => {
                log::print(&log_lines, level);
                let name = Path::new(design)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                return fail(
                    level,
                    format!(
                        "There was a parse error while reading board file at '{name}': {message}"
                    ),
                );
            }
        }
    } else {
        match std::fs::read(design) {
            Ok(bytes) => (bytes, None),
            Err(e) => {
                log::print(&log_lines, level);
                let message = format!("Couldn't load the input file '{design}': {e}");
                return unreadable_input(design, &message, level);
            }
        }
    };
    let session = read_session_file(parsed, &mut log_lines);
    let mut input = build_job_input(parsed, design, dsn, session);
    if let Some(read) = kicad_input {
        input.set_kicad(read);
    }
    input.config_file = config;
    input.environment = environment;

    let mut ctx = JobContext::new(ResolveContext::from_machine());
    if std::env::var_os(ABORT_BEFORE_RENAME_VAR).is_some() {
        ctx.before_rename = Some(abort_before_rename);
    }
    let outcome = run_job(input, &specs, &ctx);
    log_lines.extend(outcome.log.iter().cloned());
    log::print(&log_lines, level);
    outcome.exit_code() as u8
}

/// `Couldn't load the input file`, as Java logs an unreadable design.
fn unreadable_input(design: &str, message: &str, level: ConsoleLevel) -> u8 {
    log::print(
        &[
            line(LogLevel::Error, message),
            line(
                LogLevel::Warn,
                format!("Couldn't read the input file '{design}', aborting."),
            ),
        ],
        level,
    );
    1
}

/// The settings file and the environment as sources, with their warnings
/// appended to `log`. An unreadable `--settings` file is an error.
#[allow(clippy::type_complexity)]
fn settings_sources(
    parsed: &Args,
    log: &mut Vec<LogLine>,
) -> Result<
    (
        Option<freerouting::settings::SettingsPatch>,
        Option<freerouting::settings::SettingsPatch>,
    ),
    String,
> {
    let config =
        load_config(&parsed.config, &ProcessEnv, Os::current()).map_err(|e| e.to_string())?;
    log.extend(
        config
            .warnings
            .iter()
            .map(|w| line(LogLevel::Warn, w.clone())),
    );
    let environment = parse_process_environment();
    log.extend(
        environment
            .warnings
            .iter()
            .map(|w| line(LogLevel::Warn, w.clone())),
    );
    Ok((Some(config.patch), Some(environment.patch)))
}

/// The session file of `-de a.dsn+a.ses`: read when it exists; a missing or
/// unreadable file is a warning and the job goes on without it
/// (`RoutingJobScheduler`).
fn read_session_file(parsed: &Args, log: &mut Vec<LogLine>) -> Option<(Vec<u8>, String)> {
    let file = parsed.session_file.as_ref()?;
    let path = Path::new(file);
    if !path.exists() {
        log.push(line(
            LogLevel::Warn,
            format!("Session file not found: {file}"),
        ));
        return None;
    }
    match std::fs::read(path) {
        Ok(bytes) => {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            Some((bytes, name))
        }
        Err(_) => {
            log.push(line(
                LogLevel::Warn,
                format!("Failed to read session file: {}", path.display()),
            ));
            None
        }
    }
}

/// The engine's job input for a command line: the design, the session, the
/// rules choice (`-dr` or the adjacent file) and the CLI's settings patch. The
/// settings file and the environment are added by the caller.
pub fn build_job_input(
    parsed: &Args,
    design: &str,
    dsn: Vec<u8>,
    session: Option<(Vec<u8>, String)>,
) -> JobInput {
    let path = Path::new(design);
    let file_name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut input = JobInput::new(dsn, JobInput::design_name_of(&file_name));
    input.dsn_dir = Some(path.parent().map(Path::to_path_buf).unwrap_or_default());
    if let Some((bytes, name)) = session {
        input.ses = Some(bytes);
        input.ses_name = Some(name);
    }
    input.rules = match &parsed.rules_file {
        Some(file) => RulesChoice::Explicit(PathBuf::from(file)),
        None => RulesChoice::Adjacent,
    };
    input.cli = Some(parsed.patch.clone());
    input
}

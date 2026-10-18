//! What the command line does: help, the design rules check and the job.
//!
//! This module reads files and prints; everything else is the engine's:
//! [`freerouting::job::run::run_job`] runs a job and writes its outputs,
//! [`freerouting::drc::run_drc`] checks a design.
//!
//! **Exit codes** (C08.1, [`Code`]): 0 when the job finished (or timed out)
//! and wrote an output of more than 0 bytes (Java's
//! `CliRunner.computeCliExitCode`), 1 for a failure with no output, 2 for a
//! usage error found before any work (including a refused Java command), 130
//! for a run that was aborted before it wrote an output.
//!
//! **Plugin extras** (C08): `--version` and `--capabilities` print and exit
//! ([`version`](crate::version)), and so do `--list-stages` and
//! `--dump-settings` (N01.4, [`listing`](crate::listing)); `--progress json` turns every stderr line
//! into a JSON event ([`progress`](crate::progress)); `--result-json PATH`
//! writes the result manifest when the run ends ([`manifest`](crate::manifest));
//! a route run reads `stop` and `stop-router` lines on standard input
//! ([`stop`](crate::stop)).
//!
//! The settings sources are the engine's: the settings file
//! ([`load_config`]), the `FREEROUTING__ROUTER__*` environment
//! ([`parse_process_environment`]) and the command line
//! ([`Args::patch`](crate::args::Args::patch)). The core count the settings
//! merge uses is the machine's; it only sets the default thread counts, and
//! thread counts never change results (D2).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

use freerouting::drc::{DrcRunOptions, run_drc};
use freerouting::job::outputs::{self, OutputFormat};
use freerouting::job::prepare::{
    DesignKind, JobInput, KicadReadError, ReadOptions, RulesChoice, design_kind, read_kicad_input,
    same_file,
};
use freerouting::job::run::{JobContext, JobObserver, JobOutcome, run_job};
use freerouting::job::{LogLevel, LogLine};
use freerouting::settings::ResolveContext;
use freerouting::settings::config_path::{Os, ProcessEnv};
use freerouting::settings::env::parse_process_environment;
use freerouting::settings::json_file::load_config;

use crate::args::{self, Args, ProgressMode};
use crate::exit::Code;
use crate::log::ConsoleLevel;
use crate::manifest::{self, ManifestInput};
use crate::progress::{JsonSink, Printer};
use crate::stop::{self, Interrupts, TEST_INTERRUPT_VAR, TestHook};
use crate::version;

/// The text of `-help`.
pub const HELP: &str = include_str!("help.txt");

/// The environment variable that makes a run stop between the temporary write
/// and the rename of an output. Only the `killed_run_leaves_no_partial_file`
/// test sets it.
pub const ABORT_BEFORE_RENAME_VAR: &str = "FREEROUTING_TEST_ABORT_BEFORE_RENAME";

/// How a command ended: the exit code, the state `--progress json` reports,
/// and the job's outcome if there was a job.
struct Finish {
    code: Code,
    state: String,
    outcome: Option<JobOutcome>,
}

impl Finish {
    fn of(code: Code) -> Finish {
        Finish {
            code,
            state: code.state_name().to_string(),
            outcome: None,
        }
    }

    fn of_outcome(outcome: JobOutcome) -> Finish {
        let code = Code::of_outcome(&outcome);
        Finish {
            code,
            state: outcome.state.name().to_lowercase(),
            outcome: Some(outcome),
        }
    }
}

/// Runs the command line and returns the exit code. `--stats json`
/// (P01.2, [`stats`](crate::stats)) is taken out first and writes its file
/// when the run ends.
pub fn run(argv: &[String]) -> ExitCode {
    match crate::stats::extract(argv) {
        Ok(None) => run_command(argv),
        Ok(Some(request)) => {
            request.start();
            let code = run_command(&request.argv);
            if let Err(e) = request.finish() {
                eprintln!("{e}");
            }
            code
        }
        Err(e) => {
            eprintln!("{e}");
            Code::usage().into()
        }
    }
}

fn run_command(argv: &[String]) -> ExitCode {
    let parsed = args::parse(argv);
    if parsed.version {
        println!("{}", version::version_line());
        return ExitCode::SUCCESS;
    }
    if parsed.capabilities {
        println!("{}", version::capabilities().to_pretty_string());
        return ExitCode::SUCCESS;
    }
    let level = ConsoleLevel::from_name(parsed.console_level.as_deref());
    let json = (parsed.progress == ProgressMode::Json).then(|| Arc::new(JsonSink::stderr()));
    let printer = Printer::new(level, json);
    let mode = if parsed.help {
        "help"
    } else if parsed.drc {
        "drc"
    } else {
        "route"
    };
    if let Some(sink) = printer.json() {
        sink.start(mode);
    }
    printer.print(&parsed.log);
    let finish = dispatch(&parsed, &printer);
    if let Some(sink) = printer.json() {
        sink.done(finish.code, &finish.state);
    }
    finish.code.into()
}

fn dispatch(parsed: &Args, printer: &Printer) -> Finish {
    if parsed.help {
        print!("{HELP}");
        return Finish::of(Code::success());
    }
    if let Some(command) = &parsed.unsupported {
        return usage_error(
            printer,
            format!(
                "{} is not supported; use freerouting-dev ses-diff",
                command.flag()
            ),
        );
    }
    if !parsed.usage_errors.is_empty() {
        let lines: Vec<LogLine> = parsed
            .usage_errors
            .iter()
            .map(|e| line(LogLevel::Error, e.clone()))
            .collect();
        printer.print(&lines);
        return Finish::of(Code::usage());
    }
    if parsed.list_stages.is_some() || parsed.dump_settings.is_some() {
        return Finish::of(run_listing(parsed, printer));
    }
    if parsed.drc {
        Finish::of(run_drc_mode(parsed, printer))
    } else {
        run_route_mode(parsed, printer)
    }
}

fn line(level: LogLevel, text: impl Into<String>) -> LogLine {
    LogLine {
        level,
        text: text.into(),
    }
}

/// An error that ends the run with exit code 1.
fn fail(printer: &Printer, text: impl Into<String>) -> Code {
    printer.print(&[line(LogLevel::Error, text)]);
    Code::failure()
}

/// An error found before any work: exit code 2.
fn usage_error(printer: &Printer, text: impl Into<String>) -> Finish {
    printer.print(&[line(LogLevel::Error, text)]);
    Finish::of(Code::usage())
}

fn abort_before_rename(_temp: &Path) {
    std::process::abort();
}

/// `-drc`: checks the design and writes the report.
/// `--list-stages` and `--dump-settings` (N01.4): print and exit 0; a bad
/// settings file or profile is a usage error, a design that does not load
/// exit 1.
fn run_listing(parsed: &Args, printer: &Printer) -> Code {
    let mut log_lines = Vec::new();
    let (config, environment) = match settings_sources(parsed, &mut log_lines) {
        Ok(sources) => sources,
        Err(e) => {
            printer.print(&log_lines);
            return usage_error(printer, e).code;
        }
    };
    printer.print(&log_lines);
    let text = match (parsed.list_stages, parsed.dump_settings) {
        (Some(format), _) => crate::listing::list_stages(parsed, config, environment, format),
        (None, Some(format)) => crate::listing::dump_settings(parsed, config, environment, format),
        (None, None) => unreachable!("run_listing runs for one of the two flags"),
    };
    match text {
        Ok(text) => {
            print!("{text}");
            Code::success()
        }
        Err(e) if e.starts_with("router.profile:") => usage_error(printer, e).code,
        Err(e) => fail(printer, e),
    }
}

fn run_drc_mode(parsed: &Args, printer: &Printer) -> Code {
    let Some(design) = &parsed.design_file else {
        printer.print(&[line(
            LogLevel::Error,
            "An input file must be specified with -de argument in DRC mode.",
        )]);
        return Code::usage();
    };
    let mut options = DrcRunOptions::new(design);
    options.session_file = parsed.session_file.as_ref().map(PathBuf::from);
    options.rules_file = parsed.rules_file.as_ref().map(PathBuf::from);
    let run = match run_drc(&options) {
        Ok(run) => run,
        Err(e) => return fail(printer, e),
    };
    let messages: Vec<LogLine> = run
        .messages
        .iter()
        .map(|m| line(LogLevel::Info, m.clone()))
        .collect();
    printer.print(&messages);
    let json = run.report.to_json();
    match &parsed.drc_report_file {
        Some(file) => {
            let path = Path::new(file);
            let written = outputs::write_atomic(path, OutputFormat::DrcReport, false, None, |w| {
                std::io::Write::write_all(w, json.as_bytes())
            });
            if let Err(e) = written {
                return fail(
                    printer,
                    format!("Couldn't save the DRC report to '{file}': {e}"),
                );
            }
            printer.print(&[line(
                LogLevel::Info,
                format!("DRC report written to: {file}"),
            )]);
        }
        None => println!("{json}"),
    }
    Code::success()
}

/// The job of `-de ... -do ...`: Java's `CliRunner.initializeCli`, then the
/// result manifest if one was asked for.
fn run_route_mode(parsed: &Args, printer: &Printer) -> Finish {
    let mut finish = route_job(parsed, printer);
    apply_strict_sizes(printer, &mut finish);
    write_manifest(parsed, printer, &mut finish);
    finish
}

/// Q02.2: a successful run with `router.neck_down.strict` that shipped
/// copper under its class width ends with exit code 3.
fn apply_strict_sizes(printer: &Printer, finish: &mut Finish) {
    let Some(outcome) = &finish.outcome else {
        return;
    };
    if finish.code != Code::success() {
        return;
    }
    if let Some(nets) = outcome.report.strict_sizes_violations() {
        printer.print(&[line(
            LogLevel::Error,
            format!(
                "Strict sizes: {nets} net(s) have copper under their class width (exit code 3)."
            ),
        )]);
        finish.code = Code::strict_sizes();
    }
}

/// The result manifest of `--result-json` / `router.result_json`, written
/// when the run ends, whatever its outcome (C08.4).
fn write_manifest(parsed: &Args, printer: &Printer, finish: &mut Finish) {
    let path = finish
        .outcome
        .as_ref()
        .and_then(|o| o.report.result_json.clone())
        .or_else(|| parsed.patch.result_json.clone())
        .filter(|p| !p.trim().is_empty());
    let Some(path) = path else { return };
    let empty = freerouting::job::run::JobReport::default();
    let (report, final_state, output_written) = match &finish.outcome {
        Some(o) => (
            &o.report,
            o.state.name().to_string(),
            o.written.iter().any(|(_, size)| *size > 0),
        ),
        None => (
            &empty,
            if finish.code == Code::aborted() {
                "CANCELLED"
            } else {
                "TERMINATED"
            }
            .to_string(),
            false,
        ),
    };
    let input = ManifestInput {
        design: parsed.design_file.as_deref(),
        final_state: &final_state,
        exit_code: finish.code.value(),
        output_written,
        report,
    };
    match manifest::write(Path::new(&path), &input) {
        Ok(_) => printer.print(&[line(
            LogLevel::Info,
            format!("Routing result manifest written to: {path}"),
        )]),
        Err(e) => printer.print(&[line(
            LogLevel::Error,
            format!("Couldn't write routing result manifest to '{path}': {e}"),
        )]),
    }
}

/// Why a `.kicad_pcb` output (K08) cannot be written: it needs a
/// `.kicad_pcb` input, and it must not be the input itself (the job deletes
/// an old primary output before it starts).
fn kicad_output_problem(design: &str, specs: &[outputs::OutputSpec]) -> Option<String> {
    let design_path = Path::new(design);
    for spec in specs.iter().filter(|s| s.format == OutputFormat::KicadPcb) {
        if design_kind(design_path) != DesignKind::KicadPcb {
            return Some(format!(
                "Unsupported output file '{}': a .kicad_pcb output needs a .kicad_pcb input \
                 (-de board.kicad_pcb)",
                spec.path.display()
            ));
        }
        if same_file(design_path, &spec.path) {
            return Some(format!(
                "Refusing to overwrite the input board '{design}': write the routed board to \
                 another file"
            ));
        }
    }
    None
}

/// Everything from the usage checks to the end of the job.
fn route_job(parsed: &Args, printer: &Printer) -> Finish {
    let (Some(design), Some(output)) = (&parsed.design_file, &parsed.output_file) else {
        return usage_error(
            printer,
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
        Err(e) => return usage_error(printer, e.to_string()),
    };
    if let Some(message) = kicad_output_problem(design, &specs) {
        return usage_error(printer, message);
    }
    for file in [Some(design), parsed.session_file.as_ref()]
        .into_iter()
        .flatten()
    {
        if file.to_lowercase().ends_with(".json") {
            return usage_error(
                printer,
                format!("KiCad JSON files are not supported as input: '{file}'"),
            );
        }
    }

    let mut log_lines = Vec::new();
    let (config, environment) = match settings_sources(parsed, &mut log_lines) {
        Ok(sources) => sources,
        Err(e) => {
            printer.print(&log_lines);
            return usage_error(printer, e);
        }
    };
    let wants_manifest = [
        parsed.patch.result_json.as_ref(),
        config.as_ref().and_then(|p| p.result_json.as_ref()),
        environment.as_ref().and_then(|p| p.result_json.as_ref()),
    ]
    .into_iter()
    .flatten()
    .any(|p| !p.trim().is_empty());

    // Standard input can stop the run from here on, even during the load.
    let mut ctx = JobContext::new(ResolveContext::from_machine());
    ctx.collect_report = wants_manifest;
    ctx.registry = Some(freerouting_stages::shared_registry());
    let interrupts = Arc::new(Interrupts::new(ctx.clone()));
    {
        let printer = printer.clone();
        stop::spawn_stdin_reader(Arc::clone(&ctx.cancel), move |command| {
            let what = match command {
                stop::Command::Stop => "stop",
                stop::Command::StopRouter => "stop-router",
            };
            printer.print(&[line(
                LogLevel::Info,
                format!("Received '{what}' on standard input."),
            )]);
        });
    }
    {
        let printer = printer.clone();
        let installed = stop::install_signals(Arc::clone(&interrupts), move |what| {
            let text = match what {
                stop::Interrupt::Stop => "Interrupted: stopping and keeping the best board.",
                stop::Interrupt::Abort => "Interrupted again: aborting without output.",
            };
            printer.print(&[line(LogLevel::Info, text)]);
        });
        if let Err(e) = installed {
            log_lines.push(line(
                LogLevel::Debug,
                format!("Signal handling is off: {e}"),
            ));
        }
    }
    let hook = std::env::var(TEST_INTERRUPT_VAR)
        .ok()
        .and_then(|spec| TestHook::parse(&spec));
    ctx.observer = observer(printer, hook, interrupts);
    if std::env::var_os(ABORT_BEFORE_RENAME_VAR).is_some() {
        ctx.before_rename = Some(abort_before_rename);
    }

    let kicad = design_kind(Path::new(design)) == DesignKind::KicadPcb;
    if parsed.apply_design_rules && !kicad {
        log_lines.push(line(
            LogLevel::Warn,
            "--apply-design-rules only applies to a .kicad_pcb input; it is ignored here",
        ));
    }
    let read_options = ReadOptions {
        apply_design_rules: parsed.apply_design_rules,
        ..ReadOptions::default()
    };
    let (dsn, kicad_input) = if kicad {
        match read_kicad_input(Path::new(design), read_options) {
            Ok(input) => (Vec::new(), Some(input)),
            Err(KicadReadError::Io(e)) => {
                printer.print(&log_lines);
                return Finish::of(unreadable_input(design, &e, printer));
            }
            Err(KicadReadError::Refused(message)) => {
                printer.print(&log_lines);
                let name = Path::new(design)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                return Finish::of(fail(
                    printer,
                    format!(
                        "There was a parse error while reading board file at '{name}': {message}"
                    ),
                ));
            }
        }
    } else {
        match std::fs::read(design) {
            Ok(bytes) => (bytes, None),
            Err(e) => {
                printer.print(&log_lines);
                let message = format!("Couldn't load the input file '{design}': {e}");
                return Finish::of(unreadable_input(design, &message, printer));
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

    printer.print(&log_lines);
    let outcome = run_job(input, &specs, &ctx);
    // With a JSON observer the job's lines went out as they happened.
    if printer.json().is_none() {
        printer.print(&outcome.log);
    }
    Finish::of_outcome(outcome)
}

/// The listener of a route run: the JSON events, and the test hook that
/// injects interrupts.
fn observer(
    printer: &Printer,
    hook: Option<TestHook>,
    interrupts: Arc<Interrupts>,
) -> Option<JobObserver> {
    let json = printer.json().map(JsonSink::observer);
    if json.is_none() && hook.is_none() {
        return None;
    }
    Some(Arc::new(move |event| {
        if let Some(hook) = &hook {
            hook.observe(&event, &interrupts);
        }
        if let Some(json) = &json {
            json(event);
        }
    }))
}

/// `Couldn't load the input file`, as Java logs an unreadable design.
fn unreadable_input(design: &str, message: &str, printer: &Printer) -> Code {
    printer.print(&[
        line(LogLevel::Error, message),
        line(
            LogLevel::Warn,
            format!("Couldn't read the input file '{design}', aborting."),
        ),
    ]);
    Code::failure()
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

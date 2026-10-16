//! `prepare_job`: Java's load sequence for a routing job (C03.6).
//!
//! Port of the part of `CliRunner.initializeCli`, `RoutingJobScheduler`
//! (`:92-191`) and `HeadlessBoardManager` (`loadFromSpecctraDsn`,
//! `applyRouterSettingsForLoadedBoard`, `applyImmediatePostLoadProcessing`)
//! that turns a DSN file and the settings sources into a board ready to
//! route (Java at `cda540f`). The steps run in Java's order, including the
//! step that looks redundant:
//!
//! 1. the first merge ([`resolve`]): defaults, config file, DSN, environment,
//!    CLI (and any extra sources, for example a testing profile). The rules
//!    file of `-dr` joins this merge at priority 40 when C04 lands;
//! 2. read the board;
//! 3. `set_layer_count` if the counts differ, then the board-specific cost
//!    tuning, the net-class exclusions and the five overrides. **The costs
//!    are tuned on the bounding box before it is expanded**, and the tuning
//!    sets `costs_initialized`, so the second tuning in step 6 is a no-op:
//!    trace costs come from the unexpanded box;
//! 4. expand the bounding box to all items and reduce the nets;
//! 5. the rules file onto the board and its `autoroute_settings` onto the
//!    settings (the C04 hook, [`apply_rules_file`]);
//! 6. the tuning and the net-class exclusions again;
//! 7. the SES import.
//!
//! **The scheduler's second merge is left out.** Java re-merges at
//! `RoutingJobScheduler.java:101-174`; the first merge's result comes back
//! through `ApiSettings` at priority 70, and after step 3 that result has no
//! null field a lower source could fill, so the re-merge changes nothing.
//! `tests/job_prepare_reference.rs` compares the result with Java's dumps,
//! which run the real second merge, on every fixture and scenario. If that
//! ever fails the second merge goes back in (docs/DIVERGENCES.md, Settings).

use std::collections::BTreeMap;
use std::fmt;
use std::path::PathBuf;

use super::overrides::{self, OverrideState};
use super::{LogLevel, LogLine};
use crate::board::RoutingBoard;
use crate::dsn::ast::Pcb;
use crate::io::specctra::{BoardReadResult, SesImportSummary, build_board, read_board_from_str};
use crate::settings::autoroute_scope::patch_from_dsn;
use crate::settings::board::{
    BoardRouterSettings, apply_board_specific, apply_net_class_exclusions,
};
use crate::settings::{ResolveContext, RouterSettings, SettingsPatch, Source, defaults, resolve};

/// Which rules file a job uses.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub enum RulesChoice {
    /// No rules file, not even an adjacent one.
    None,
    /// A rules file given on the command line (`-dr`).
    Explicit(PathBuf),
    /// `<design name>.rules` next to the DSN file, if it exists.
    #[default]
    Adjacent,
}

/// What a job loads.
#[derive(Clone, Debug)]
pub struct JobInput {
    /// The bytes of the DSN file.
    pub dsn: Vec<u8>,
    /// The design name: the file name without its last extension.
    pub design_name: String,
    /// The directory of the DSN file, where [`RulesChoice::Adjacent`] looks.
    pub dsn_dir: Option<PathBuf>,
    /// A session file whose wires and vias are imported last.
    pub ses: Option<Vec<u8>>,
    /// The file name of the session, for the log.
    pub ses_name: Option<String>,
    pub rules: RulesChoice,
    /// The patch of the config file (`freerouting.json`).
    pub config_file: Option<SettingsPatch>,
    /// The patch of the environment variables.
    pub environment: Option<SettingsPatch>,
    /// The patch of the command line.
    pub cli: Option<SettingsPatch>,
    /// Further sources, for example `(Source::Testing, java_fixture_test())`.
    pub extra_sources: Vec<(Source, SettingsPatch)>,
}

impl JobInput {
    /// An input with only the DSN: no session, the adjacent rules file, the
    /// default settings.
    pub fn new(dsn: Vec<u8>, design_name: impl Into<String>) -> JobInput {
        JobInput {
            dsn,
            design_name: design_name.into(),
            dsn_dir: None,
            ses: None,
            ses_name: None,
            rules: RulesChoice::Adjacent,
            config_file: None,
            environment: None,
            cli: None,
            extra_sources: Vec::new(),
        }
    }

    /// The design name of a file name: `getFilenameWithoutExtension`, the
    /// name up to its last `.` (all of it without a `.`).
    pub fn design_name_of(file_name: &str) -> String {
        match file_name.rfind('.') {
            Some(index) => file_name[..index].to_string(),
            None => file_name.to_string(),
        }
    }
}

/// Why a job could not be prepared. The text is Java's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobError {
    /// `There was a parse error while reading board file at '<loc>': <detail>`.
    Parse { location: String, detail: String },
    /// The DSN file has no usable board outline, so no board was built.
    NoBoard,
}

impl fmt::Display for JobError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JobError::Parse { location, detail } => write!(
                f,
                "There was a parse error while reading board file at '{location}': {detail}"
            ),
            JobError::NoBoard => f.write_str("The board file has no usable outline"),
        }
    }
}

impl std::error::Error for JobError {}

/// A board ready to route, with the settings that apply to it.
#[derive(Debug)]
pub struct PreparedJob {
    pub board: RoutingBoard,
    /// The final settings with Java's nulls (the tuned `layers` and cost
    /// arrays included).
    pub patch: SettingsPatch,
    /// The final settings read through Java's getter fallbacks.
    pub settings: RouterSettings,
    /// The router's read-only view of `settings`.
    pub board_settings: BoardRouterSettings,
    /// For each setting, the last source that set it.
    pub provenance: BTreeMap<&'static str, Source>,
    /// Java's log lines of the load, in order.
    pub log: Vec<LogLine>,
    /// The result of the session import, if a session was given.
    pub ses_summary: Option<SesImportSummary>,
    /// The rules file that was found and skipped (C04 reads it).
    pub rules_path: Option<PathBuf>,
}

fn push(log: &mut Vec<LogLine>, level: LogLevel, text: impl Into<String>) {
    log.push(LogLine {
        level,
        text: text.into(),
    });
}

/// The rules file of the job, if one is to be read (Java's chain in
/// `RoutingJobScheduler`: the explicit file if it exists, else the adjacent
/// file).
fn find_rules_file(input: &JobInput, log: &mut Vec<LogLine>) -> Option<PathBuf> {
    match &input.rules {
        RulesChoice::None => None,
        RulesChoice::Explicit(path) => {
            if path.exists() {
                Some(path.clone())
            } else {
                push(
                    log,
                    LogLevel::Warn,
                    format!(
                        "RULES file '{}' does not exist; ignoring it",
                        path.display()
                    ),
                );
                None
            }
        }
        RulesChoice::Adjacent => {
            let adjacent = input
                .dsn_dir
                .as_ref()?
                .join(format!("{}.rules", input.design_name));
            adjacent.exists().then_some(adjacent)
        }
    }
}

/// Step 5, the rules hook: the rules file onto the board and its
/// `autoroute_settings` onto the settings. Until C04 lands it logs the DRC
/// runner's text and skips the file.
fn apply_rules_file(
    path: &std::path::Path,
    _board: &mut RoutingBoard,
    _patch: &mut SettingsPatch,
    log: &mut Vec<LogLine>,
) {
    push(
        log,
        LogLevel::Warn,
        format!(
            "RULES files are not supported yet; ignoring '{}'",
            path.display()
        ),
    );
}

/// Loads a job the way Java does before it routes. See the module docs for
/// the sequence.
pub fn prepare_job(input: &JobInput, ctx: &ResolveContext) -> Result<PreparedJob, JobError> {
    let mut log = Vec::new();
    let text = freerouting_dsn::decode(&input.dsn);
    let pcb = Pcb::parse(&text).ok();

    // 1. The first merge.
    let mut sources = vec![(Source::Defaults, defaults(ctx))];
    if let Some(patch) = &input.config_file {
        sources.push((Source::ConfigFile, patch.clone()));
    }
    if let Some(pcb) = &pcb {
        sources.push((Source::Dsn, patch_from_dsn(pcb)));
    }
    if let Some(patch) = &input.environment {
        sources.push((Source::Environment, patch.clone()));
    }
    if let Some(patch) = &input.cli {
        sources.push((Source::Cli, patch.clone()));
    }
    sources.extend(input.extra_sources.iter().cloned());
    let merged = resolve(&sources, ctx);
    for warning in &merged.warnings {
        push(&mut log, LogLevel::Warn, warning.clone());
    }
    let provenance = merged.provenance;
    let mut patch = merged.patch;

    // 2. Read the board.
    let result = match &pcb {
        Some(pcb) => build_board(pcb),
        None => read_board_from_str(&text),
    };
    let (mut board, warnings) = match result {
        BoardReadResult::Success { board, warnings }
        | BoardReadResult::OutlineMissing {
            board: Some(board),
            warnings,
        } => (*board, warnings),
        BoardReadResult::OutlineMissing { board: None, .. } => return Err(JobError::NoBoard),
        BoardReadResult::ParseError { location, detail } => {
            return Err(JobError::Parse { location, detail });
        }
    };
    for warning in warnings {
        push(&mut log, LogLevel::Warn, warning);
    }

    // 3. Tuning, exclusions and the five overrides, on the unexpanded box.
    if patch.layer_count() != board.layer_count() {
        patch.set_layer_count(board.layer_count());
    }
    apply_board_specific(&mut patch, &board);
    for warning in apply_net_class_exclusions(&patch, &mut board) {
        push(&mut log, LogLevel::Warn, warning);
    }
    let mut state = OverrideState::default();
    let settings = RouterSettings::from_patch(&patch, ctx.cores);
    log.extend(overrides::apply_all(&mut board, &settings, &mut state));

    // 4. Expand the bounding box and reduce the nets.
    board.expand_bounding_box_to_include_all_items();
    board.reduce_nets_of_route_items();

    // 5. The rules file.
    let rules_path = find_rules_file(input, &mut log);
    if let Some(path) = &rules_path {
        apply_rules_file(path, &mut board, &mut patch, &mut log);
    }

    // 6. The tuning and the exclusions again (the tuning is a no-op unless the
    // rules file reset the flag).
    apply_board_specific(&mut patch, &board);
    for warning in apply_net_class_exclusions(&patch, &mut board) {
        push(&mut log, LogLevel::Warn, warning);
    }

    // 7. The session.
    let mut ses_summary = None;
    if let Some(ses) = &input.ses {
        let name = input.ses_name.as_deref().unwrap_or("");
        push(
            &mut log,
            LogLevel::Info,
            format!("Loading SES session data: {name}"),
        );
        match crate::io::read_session(&mut board, &freerouting_dsn::decode(ses)) {
            Ok(summary) => {
                let errors = if summary.errors_encountered > 0 {
                    format!(" ({} errors)", summary.errors_encountered)
                } else {
                    String::new()
                };
                push(
                    &mut log,
                    LogLevel::Info,
                    format!(
                        "SES session loaded: {} wires, {} vias imported{errors}",
                        summary.wires_imported, summary.vias_imported
                    ),
                );
                ses_summary = Some(summary);
            }
            Err(e) => push(
                &mut log,
                LogLevel::Error,
                format!("Failed to load session data: {e}"),
            ),
        }
    }

    let settings = RouterSettings::from_patch(&patch, ctx.cores);
    let board_settings = BoardRouterSettings::from_settings(&settings);
    Ok(PreparedJob {
        board,
        patch,
        settings,
        board_settings,
        provenance,
        log,
        ses_summary,
        rules_path,
    })
}

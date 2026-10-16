//! Running the design rules check on design files.
//!
//! Port of `cli.DrcRunner` without the command line parsing, and of the
//! board loading it uses (`HeadlessBoardManager.loadFromSpecctraDsn` with
//! default router settings).

use std::path::{Path, PathBuf};

use super::checker::DesignRulesChecker;
use super::report::DrcReport;
use crate::board::RoutingBoard;
use crate::io::specctra::{BoardReadResult, SesImportSummary, read_board, read_session};

/// What to check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrcRunOptions {
    /// The DSN design file.
    pub design_file: PathBuf,
    /// A session file whose wires and vias are added before the check.
    pub session_file: Option<PathBuf>,
    /// A rules file (Java applies it before the session; not supported
    /// yet, see [`DrcRun::messages`]).
    pub rules_file: Option<PathBuf>,
    /// The unit of the report coordinates.
    pub coordinate_unit: String,
}

impl DrcRunOptions {
    /// Options to check `design_file` with coordinates in millimeters.
    pub fn new(design_file: impl Into<PathBuf>) -> Self {
        DrcRunOptions {
            design_file: design_file.into(),
            session_file: None,
            rules_file: None,
            coordinate_unit: "mm".into(),
        }
    }
}

/// The result of [`run_drc`].
#[derive(Debug)]
pub struct DrcRun {
    pub board: RoutingBoard,
    pub report: DrcReport,
    pub session_summary: Option<SesImportSummary>,
    /// Log messages (Java's `FRLogger.info` and `warn` output of the run).
    pub messages: Vec<String>,
}

/// Loads a DSN design for checking or routing: reads the board, extends
/// its bounding box to all items and reduces the nets of traces and vias at
/// tie pins (`HeadlessBoardManager.applyImmediatePostLoadProcessing`).
pub fn load_design(bytes: &[u8]) -> Result<RoutingBoard, String> {
    let mut board = match read_board(bytes) {
        BoardReadResult::Success { board, .. }
        | BoardReadResult::OutlineMissing {
            board: Some(board), ..
        } => *board,
        BoardReadResult::OutlineMissing { board: None, .. } => {
            return Err("The board file has no usable outline".into());
        }
        BoardReadResult::ParseError { location, detail } => {
            return Err(format!(
                "There was a parse error while reading board file at '{location}': {detail}"
            ));
        }
    };
    board.expand_bounding_box_to_include_all_items();
    board.reduce_nets_of_route_items();
    Ok(board)
}

/// Port of `DrcRunner.initializeDrc`: loads the design (and session), runs
/// the checker and returns the report with the V2 quality and optimizer
/// scores.
pub fn run_drc(options: &DrcRunOptions) -> Result<DrcRun, String> {
    let mut messages = Vec::new();
    let design = &options.design_file;
    messages.push(format!("Loading DSN file for DRC: {}", design.display()));
    let bytes = std::fs::read(design)
        .map_err(|e| format!("Couldn't load the input file '{}': {e}", design.display()))?;
    let mut board =
        load_design(&bytes).map_err(|e| format!("Failed to load board for DRC check: {e}"))?;

    if let Some(rules) = &options.rules_file {
        messages.push(format!(
            "RULES files are not supported yet; ignoring '{}'",
            rules.display()
        ));
    }

    let mut session_summary = None;
    if let Some(session) = &options.session_file {
        if session.exists() {
            messages.push(format!("Loading SES file for DRC: {}", session.display()));
            match std::fs::read(session) {
                Ok(bytes) => match read_session(&mut board, &freerouting_dsn::decode(&bytes)) {
                    Ok(summary) => {
                        let errors = if summary.errors_encountered > 0 {
                            format!(" ({} errors)", summary.errors_encountered)
                        } else {
                            String::new()
                        };
                        messages.push(format!(
                            "SES file loaded for DRC: {} wires, {} vias imported{errors}",
                            summary.wires_imported, summary.vias_imported
                        ));
                        session_summary = Some(summary);
                    }
                    Err(e) => messages.push(format!("Failed to load session file for DRC: {e}")),
                },
                Err(e) => messages.push(format!("Failed to load session file for DRC: {e}")),
            }
        } else {
            messages.push(format!(
                "Session file for DRC not found: {}",
                session.display()
            ));
        }
    }

    let source = file_name(design);
    let report = DesignRulesChecker::new(&board).generate_report(&source, &options.coordinate_unit);
    Ok(DrcRun {
        board,
        report,
        session_summary,
        messages,
    })
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

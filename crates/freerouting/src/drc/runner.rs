//! Running the design rules check on design files.
//!
//! Port of `cli.DrcRunner` without the command line parsing, and of the
//! board loading it uses (`HeadlessBoardManager.loadFromSpecctraDsn` with
//! default router settings).

use std::path::{Path, PathBuf};

use super::checker::DesignRulesChecker;
use super::kicad_report::ReportRules;
use super::kicad_rules::KicadReportInputs;
use super::report::DrcReport;
use crate::board::RoutingBoard;
use crate::io::kicad::{DesignKind, design_kind, read_design};
use crate::io::specctra::{
    BoardReadResult, SesImportSummary, read_board, read_rules, read_session,
};

/// What to check.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DrcRunOptions {
    /// The design file: a DSN, or a `.kicad_pcb` (K07.1).
    pub design_file: PathBuf,
    /// A session file whose wires and vias are added before the check.
    pub session_file: Option<PathBuf>,
    /// A rules file (`-dr`), applied before the session. Its
    /// `autoroute_settings` are ignored: the check does not route.
    pub rules_file: Option<PathBuf>,
    /// The unit of the report coordinates.
    pub coordinate_unit: String,
    /// How the report counts clearance and dangling items: as KiCad does
    /// (the default) or as Java does (K15.5, [`ReportRules`]).
    pub report_rules: ReportRules,
    /// K15.17: a `.kicad_dru` for a DSN input (`--kicad-dru`). The report
    /// counts with its unconditional clearance and its hole clearance, as a
    /// routing job's DRC does with the same file; the board checked is
    /// unchanged (the rules are evaluated on a copy of the DSN's tree). A
    /// `.kicad_pcb` reads the `.kicad_dru` beside it (K15.11) and ignores
    /// this, with a warning.
    pub kicad_dru: Option<PathBuf>,
}

impl DrcRunOptions {
    /// Options to check `design_file` with coordinates in millimeters.
    pub fn new(design_file: impl Into<PathBuf>) -> Self {
        DrcRunOptions {
            design_file: design_file.into(),
            session_file: None,
            rules_file: None,
            coordinate_unit: "mm".into(),
            report_rules: ReportRules::default(),
            kicad_dru: None,
        }
    }
}

/// The result of [`run_drc`].
#[derive(Debug)]
pub struct DrcRun {
    pub board: RoutingBoard,
    pub report: DrcReport,
    pub session_summary: Option<SesImportSummary>,
    /// Log messages (Java's `FRLogger.info` output of the run).
    pub messages: Vec<String>,
    /// Warnings of the run (K15.17: a `--kicad-dru` file that could not be
    /// read as rules, or that a `.kicad_pcb` input does not take).
    pub warnings: Vec<String>,
}

/// Loads a DSN design for checking or routing: reads the board, extends
/// its bounding box to all items and reduces the nets of traces and vias at
/// tie pins (`HeadlessBoardManager.applyImmediatePostLoadProcessing`).
pub fn load_design(bytes: &[u8]) -> Result<RoutingBoard, String> {
    board_of_read(read_board(bytes)).map(|(board, _)| board)
}

/// The board of a read design, ready for checking (see [`load_design`]), and
/// the read's warnings.
fn board_of_read(result: BoardReadResult) -> Result<(RoutingBoard, Vec<String>), String> {
    let (mut board, mut warnings) = match result {
        BoardReadResult::Success { board, warnings }
        | BoardReadResult::OutlineMissing {
            board: Some(board),
            warnings,
        } => (*board, warnings),
        BoardReadResult::OutlineMissing { board: None, .. } => {
            return Err("The board file has no usable outline".into());
        }
        BoardReadResult::ParseError { location, detail } => {
            return Err(format!(
                "There was a parse error while reading board file at '{location}': {detail}"
            ));
        }
    };
    warnings.extend(board.expand_bounding_box_to_include_all_items());
    board.reduce_nets_of_route_items();
    Ok((board, warnings))
}

/// `DrcRunner.initializeDrc`'s rules step. Java ignores the result of the
/// read: it logs "loaded successfully" after a file that failed to parse too.
fn apply_rules(
    board: RoutingBoard,
    rules: &Path,
    design: &Path,
    messages: &mut Vec<String>,
) -> RoutingBoard {
    if !rules.exists() {
        messages.push(format!("RULES file for DRC not found: {}", rules.display()));
        return board;
    }
    messages.push(format!("Loading RULES file for DRC: {}", rules.display()));
    let board = match std::fs::read(rules) {
        Ok(bytes) => {
            let design_name = design_name(design);
            let (board, result) = read_rules(board, &freerouting_dsn::decode(&bytes), &design_name);
            match result {
                Ok(outcome) => messages.extend(outcome.log.into_iter().map(|l| l.text)),
                Err(e) => messages.push(e.to_string()),
            }
            board
        }
        Err(e) => {
            messages.push(format!("Failed to load RULES file for DRC: {e}"));
            return board;
        }
    };
    messages.push("RULES file loaded for DRC successfully".into());
    board
}

/// The job name Java gives a design file: its name up to the last dot.
fn design_name(design: &Path) -> String {
    let name = file_name(design);
    match name.rfind('.') {
        Some(index) => name[..index].to_string(),
        None => name,
    }
}

/// Port of `DrcRunner.initializeDrc`: loads the design (and session), runs
/// the checker and returns the report with the V2 quality and optimizer
/// scores. The report counts as [`DrcRunOptions::report_rules`] says (KiCad's
/// by default, K15.5). The check reads no router settings: the board is the
/// design's, with its rules file and session.
pub fn run_drc(options: &DrcRunOptions) -> Result<DrcRun, String> {
    let mut messages = Vec::new();
    let mut warnings = Vec::new();
    let design = &options.design_file;
    // `read_design` picks the reader by extension: `.kicad_pcb` is read
    // directly (K07.1), everything else as a DSN.
    let kicad = design_kind(design) == DesignKind::KicadPcb;
    messages.push(format!(
        "Loading {} file for DRC: {}",
        if kicad { "KiCad board" } else { "DSN" },
        design.display()
    ));
    let load_scope = crate::run_stats::stage("load", 0);
    let read = read_design(design)?;
    if let Some(input) = &read.kicad {
        messages.extend(input.report_lines());
        messages.extend(input.warnings.iter().map(|w| format!("KiCad reader: {w}")));
    }
    let (mut board, _) = board_of_read(read.result)
        .map_err(|e| format!("Failed to load board for DRC check: {e}"))?;
    // K15.11: the KiCad rules the report counts with (an unconditional
    // `.kicad_dru` clearance, the hole clearance, the pads' holes).
    if let Some(input) = &read.kicad {
        let rules = KicadReportInputs::of_kicad(input).on_board(&board);
        board.set_kicad_report_rules(rules);
        if options.kicad_dru.is_some() {
            warnings.push(KICAD_DRU_ON_PCB.into());
        }
    } else if let Some(file) = &options.kicad_dru {
        let rules = kicad_dru_rules(design, file, &mut messages, &mut warnings)?;
        let on_board = rules.on_board(&board);
        board.set_kicad_report_rules(on_board);
    }

    if let Some(rules) = &options.rules_file {
        board = apply_rules(board, rules, design, &mut messages);
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

    load_scope.finish();
    let source = file_name(design);
    let drc_scope = crate::run_stats::stage("drc", 0);
    let mut checker = DesignRulesChecker::new(&board);
    checker.set_report_rules(options.report_rules);
    let report = checker.generate_report(&source, &options.coordinate_unit);
    drc_scope.finish();
    Ok(DrcRun {
        board,
        report,
        session_summary,
        messages,
        warnings,
    })
}

/// The warning for `--kicad-dru` on a `.kicad_pcb` input, which reads the
/// `.kicad_dru` beside it (the routing job warns the same).
const KICAD_DRU_ON_PCB: &str =
    "--kicad-dru only applies to a DSN input; a .kicad_pcb reads the .kicad_dru next to it";

/// K15.17: the report rules of a `.kicad_dru` given for a DSN input. The rules
/// are evaluated on a copy of the DSN's tree, as a routing job applies them
/// ([`apply_to_pcb`](freerouting_kicad::dru::apply_to_pcb)); only the report
/// takes the result (`KicadReportInputs::apply_rules`), so the board `-drc`
/// checks is the DSN's. A file that does not parse is a warning with the
/// routing job's text and leaves the report without rules; one that cannot be
/// read is an error.
fn kicad_dru_rules(
    design: &Path,
    file: &Path,
    messages: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> Result<KicadReportInputs, String> {
    let rules = std::fs::read(file).map_err(|e| {
        format!(
            "Couldn't read the --kicad-dru file '{}': {e}",
            file.display()
        )
    })?;
    let rules = String::from_utf8_lossy(&rules);
    messages.push(format!("KiCad design rules file: {}", file.display()));
    let mut inputs = KicadReportInputs::default();
    let dsn = std::fs::read(design)
        .map_err(|e| format!("Couldn't load the input file '{}': {e}", design.display()))?;
    let Ok(mut tree) = freerouting_dsn::ast::Pcb::parse(&freerouting_dsn::decode(&dsn)) else {
        warnings.push("the design could not be read again for the --kicad-dru file".into());
        return Ok(inputs);
    };
    match freerouting_kicad::dru::apply_to_pcb(&mut tree, &rules) {
        Ok(report) => {
            inputs.apply_rules(&report, None);
            if let Some(um) = inputs.clearance_um {
                messages.push(format!("the report's clearance is the rules' {um} um"));
            }
            if let Some(um) = inputs.hole_clearance_um {
                messages.push(format!("the report's hole clearance is the rules' {um} um"));
            }
        }
        Err(e) => warnings.push(format!("the .kicad_dru file could not be read: {e}")),
    }
    Ok(inputs)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

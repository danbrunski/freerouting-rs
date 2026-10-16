//! Direct `.kicad_pcb` input (K07.1).
//!
//! [`freerouting_kicad`] turns a KiCad board file into a Specctra syntax
//! tree in KiCad's own DSN frame; this module reads the files next to the
//! board and hands the tree to [`build_board`], the only board builder, so a
//! `.kicad_pcb` and the DSN KiCad exports from it end in the same kind of
//! board (K07.3 compares them).
//!
//! # One dispatch point
//!
//! [`read_design`] picks the reader by extension (case ignored): `.kicad_pcb`
//! is read with the files next to it, `<stem>.kicad_pro` (net classes and the
//! board minimums; KiCad's defaults when it is missing) and
//! `<stem>.kicad_dru` (custom rules, K07.5); every other name is read as a
//! DSN, as before. The DRC runner, the CLI job and the GUI session call it
//! (or [`read_kicad_input`] when they need the tree), so they cannot drift.
//!
//! # What reaches the caller
//!
//! The reader's warnings (each with its `.kicad_pcb` line, for example
//! `line 1204: wire_keepout ...`) go in front of the board builder's own
//! warnings in [`BoardReadResult`], so every front-end that prints a read's
//! warnings prints them. The read report (rule areas, planes, arcs, the
//! `.kicad_dru` outcome) is [`KicadInput::report_lines`]. A `.kicad_pcb` the
//! reader refuses (KiCad 5 or older, not a board, broken syntax) is a
//! [`BoardReadResult::ParseError`] whose detail is the reader's one-line
//! message.
//!
//! The `.kicad_dru` rules are applied only with
//! [`ReadOptions::apply_design_rules`]; the default is off, because KiCad's
//! DSN export ignores that file and applying it changes routing (K00
//! question 7).

use std::path::{Path, PathBuf};

pub use freerouting_kicad as reader;
pub use freerouting_kicad::dru::DruReport;
pub use freerouting_kicad::{Extras, KicadError, ReadOptions, ReadReport};

use super::specctra::{BoardReadResult, build_board, read_board};
use crate::dsn::ast::Pcb;

/// Which reader a design file name selects.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DesignKind {
    /// A Specctra DSN (every name that is not `.kicad_pcb`).
    Dsn,
    /// A KiCad board, read directly.
    KicadPcb,
}

/// `.kicad_pcb` (any case) is [`DesignKind::KicadPcb`], everything else
/// [`DesignKind::Dsn`].
pub fn design_kind(path: &Path) -> DesignKind {
    match path.extension().and_then(|e| e.to_str()) {
        Some(e) if e.eq_ignore_ascii_case("kicad_pcb") => DesignKind::KicadPcb,
        _ => DesignKind::Dsn,
    }
}

/// What the reader made of a `.kicad_pcb` and the files next to it.
#[derive(Clone, Debug)]
pub struct KicadInput {
    /// The tree [`build_board`] consumes.
    pub pcb: Pcb,
    /// The reader's warnings, each prefixed with its `.kicad_pcb` line
    /// (`line 12: ...`; no prefix when the line is unknown).
    pub warnings: Vec<String>,
    /// Rule areas, planes, arcs and dropped zones.
    pub report: ReadReport,
    /// The `.kicad_dru` rules and what became of each.
    pub rules: DruReport,
    pub extras: Extras,
    /// The `<stem>.kicad_pro` that was read, if the file exists.
    pub project_file: Option<PathBuf>,
    /// The `<stem>.kicad_dru` that was read, if the file exists.
    pub rules_file: Option<PathBuf>,
}

impl KicadInput {
    /// The read report as log lines: the rule areas, the counts, then one
    /// line per `.kicad_dru` rule constraint.
    pub fn report_lines(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        out.push(match &self.project_file {
            Some(p) => format!("KiCad project file: {}", p.display()),
            None => "No .kicad_pro next to the board: KiCad's default net class".to_owned(),
        });
        out.extend(self.report.to_string().lines().map(str::to_owned));
        if let Some(p) = &self.rules_file {
            out.push(format!("KiCad design rules file: {}", p.display()));
        }
        out.extend(self.rules.lines());
        out
    }
}

/// Why a `.kicad_pcb` could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum KicadReadError {
    /// The board file could not be read from disk.
    Io(String),
    /// The reader refused the board; the message is its one line.
    Refused(String),
}

impl std::fmt::Display for KicadReadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KicadReadError::Io(m) | KicadReadError::Refused(m) => f.write_str(m),
        }
    }
}

impl std::error::Error for KicadReadError {}

fn sibling(path: &Path, extension: &str) -> PathBuf {
    path.with_extension(extension)
}

fn read_optional(path: &Path) -> Option<String> {
    std::fs::read(path)
        .ok()
        .map(|b| String::from_utf8_lossy(&b).into_owned())
}

/// Reads a `.kicad_pcb`, its `.kicad_pro` and its `.kicad_dru`.
///
/// A missing `.kicad_pro` or `.kicad_dru` is not an error. The `(pcb ...)`
/// name of the tree is the board's file name with `.dsn` in place of
/// `.kicad_pcb`, as KiCad names its export.
pub fn read_kicad_input(path: &Path, options: ReadOptions) -> Result<KicadInput, KicadReadError> {
    let bytes = std::fs::read(path).map_err(|e| {
        KicadReadError::Io(format!(
            "Couldn't load the input file '{}': {e}",
            path.display()
        ))
    })?;
    let text = freerouting_kicad::sexpr::decode(&bytes)
        .map_err(|e| KicadReadError::Refused(e.to_string()))?;
    let pro_path = sibling(path, "kicad_pro");
    let dru_path = sibling(path, "kicad_dru");
    let pro = read_optional(&pro_path);
    let dru = read_optional(&dru_path);
    let name = sibling(path, "dsn")
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "board.dsn".to_owned());
    let read =
        freerouting_kicad::read_with_rules(&name, text, pro.as_deref(), dru.as_deref(), options)
            .map_err(|e| KicadReadError::Refused(e.to_string()))?;
    Ok(KicadInput {
        pcb: read.pcb,
        warnings: read.warnings.iter().map(|w| w.to_string()).collect(),
        report: read.report,
        rules: read.rules,
        extras: read.extras,
        project_file: pro.is_some().then_some(pro_path),
        rules_file: dru.is_some().then_some(dru_path),
    })
}

/// The board of a `.kicad_pcb` as [`BoardReadResult`]: the reader's
/// warnings, then the board builder's. A file that cannot be read from disk
/// is a [`BoardReadResult::ParseError`] here; [`read_design`] tells the two
/// apart.
pub fn read_kicad_board(path: &Path) -> BoardReadResult {
    match read_kicad_input(path, ReadOptions::default()) {
        Ok(input) => board_of(&input),
        Err(e) => kicad_parse_error(path, &e),
    }
}

/// Builds the board of a read `.kicad_pcb`, with the reader's warnings in
/// front of the builder's.
pub fn board_of(input: &KicadInput) -> BoardReadResult {
    match build_board(&input.pcb) {
        BoardReadResult::Success {
            board,
            warnings: built,
        } => BoardReadResult::Success {
            board,
            warnings: merged(&input.warnings, built),
        },
        BoardReadResult::OutlineMissing {
            board,
            warnings: built,
        } => BoardReadResult::OutlineMissing {
            board,
            warnings: merged(&input.warnings, built),
        },
        error @ BoardReadResult::ParseError { .. } => error,
    }
}

fn merged(reader: &[String], built: Vec<String>) -> Vec<String> {
    reader.iter().cloned().chain(built).collect()
}

fn kicad_parse_error(path: &Path, e: &KicadReadError) -> BoardReadResult {
    let location = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    BoardReadResult::ParseError {
        location,
        detail: e.to_string(),
    }
}

/// A design read by [`read_design`].
#[derive(Debug)]
pub struct DesignRead {
    pub result: BoardReadResult,
    /// What the KiCad reader found; `None` for a DSN.
    pub kicad: Option<KicadInput>,
}

/// Reads a design file, choosing the reader by extension ([`design_kind`]).
///
/// `Err` is a file that could not be read from disk; a file that was read
/// but refused is a [`BoardReadResult::ParseError`].
pub fn read_design(path: &Path) -> Result<DesignRead, String> {
    match design_kind(path) {
        DesignKind::Dsn => {
            let bytes = std::fs::read(path)
                .map_err(|e| format!("Couldn't load the input file '{}': {e}", path.display()))?;
            Ok(DesignRead {
                result: read_board(&bytes),
                kicad: None,
            })
        }
        DesignKind::KicadPcb => match read_kicad_input(path, ReadOptions::default()) {
            Ok(input) => Ok(DesignRead {
                result: board_of(&input),
                kicad: Some(input),
            }),
            Err(KicadReadError::Io(m)) => Err(m),
            Err(e) => Ok(DesignRead {
                result: kicad_parse_error(path, &e),
                kicad: None,
            }),
        },
    }
}

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
//! [`ReadOptions::apply_design_rules`]. The reader's default is off, because
//! KiCad's DSN export ignores that file (K00 question 7), and so is every
//! caller here ([`read_design`], [`read_kicad_board`]: the GUI, the DRC
//! runner). A route from the command line turns it on (K13.1, decided
//! 2026-10-20: the board's own rules reach the router on both inputs;
//! `--no-design-rules` turns it off), as a DSN route gets a board's
//! `.kicad_dru` with `--kicad-dru`
//! ([`crate::job::prepare::JobInput::kicad_dru`], Q22.5), which the plugin
//! passes for every DSN route.
//!
//! # Writing the routed copper back (K08)
//!
//! [`write_kicad_board`] writes a routed board into a copy of the
//! `.kicad_pcb` it was read from, without `ImportSpecctraSES`: it reads the
//! input again ([`read_kicad_bytes`]: the copper index of
//! [`freerouting_kicad::write`]), maps the board's traces and vias to KiCad
//! layers, nets and via sizes ([`final_copper`]; `SYSTEM_FIXED` traces left
//! out, as the session leaves them out, so the fixed wires of K14.4's
//! `fixed_pour_classes` never become tracks), diffs them against the
//! file's copper by exact geometry in board units, cuts out what the board no
//! longer has and adds the rest before the final `)`, with deterministic
//! UUIDs. Every other byte stays as it was; zones are not refilled. It refuses
//! to overwrite its input ([`same_file`]) and, with the hash recorded at read
//! time ([`KicadInput::source`], [`write_kicad_board_checked`]), a file that
//! changed since. The CLI's `-do routed.kicad_pcb` goes through
//! [`render_kicad_board`].

use std::path::{Path, PathBuf};

use freerouting_geometry::jmath::round_to_i32;
pub use freerouting_kicad as reader;
pub use freerouting_kicad::dru::DruReport;
use freerouting_kicad::legacy::Pt;
pub use freerouting_kicad::write::WriteSummary;
use freerouting_kicad::write::{
    BPt, BoardFrame, CopperIndex, FinalItem, FinalTrace, FinalVia, fnv1a128,
};
pub use freerouting_kicad::{Extras, KicadError, ReadOptions, ReadReport};

use super::specctra::{BoardReadResult, build_board, read_board};
use crate::board::BasicBoard;
use crate::board::items::ItemKind;
use crate::board::structure::FixedState;
use crate::dsn::ast::Pcb;
use crate::io::CoordinateTransform;

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
    /// The board file that was read.
    pub board_file: PathBuf,
    /// FNV-1a 128 of the board file's bytes as read; the writer refuses a
    /// file that changed since (K08.4).
    pub source_hash: u128,
    /// The file's tracks and vias with their byte spans (K08.1).
    pub copper_index: reader::write::CopperIndex,
}

impl KicadInput {
    /// The board file and its hash, for [`write_kicad_board_checked`].
    pub fn source(&self) -> KicadSource {
        KicadSource {
            path: self.board_file.clone(),
            hash: self.source_hash,
        }
    }

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
    read_kicad_bytes(path, &bytes, options)
}

/// [`read_kicad_input`] on bytes already read from `path` (the files next to
/// it are read from disk).
pub fn read_kicad_bytes(
    path: &Path,
    bytes: &[u8],
    options: ReadOptions,
) -> Result<KicadInput, KicadReadError> {
    let text = freerouting_kicad::sexpr::decode(bytes)
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
        board_file: path.to_path_buf(),
        source_hash: reader::write::fnv1a128(bytes),
        copper_index: read.copper_index,
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

// ---- writing routed copper back (K08.4) ------------------------------------

/// The `.kicad_pcb` a board was built from and the FNV-1a 128 of its bytes
/// at that time ([`KicadInput::source`]).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KicadSource {
    pub path: PathBuf,
    pub hash: u128,
}

/// K14.4: the zones and fills of the `.kicad_pcb` a board is read from
/// ([`freerouting_kicad::fixups::pour_fills`]), for
/// `router.input_fixups.fixed_pour_classes`. The tree keeps no fills, so the file is
/// read again; a file that changed since it was read is refused, as
/// [`render_kicad_board`] refuses it.
pub fn read_pour_fills(
    source: &KicadSource,
) -> Result<Vec<freerouting_kicad::fixups::PourFill>, String> {
    let bytes = std::fs::read(&source.path).map_err(|e| {
        format!(
            "couldn't read the zone fills of '{}': {e}",
            source.path.display()
        )
    })?;
    if fnv1a128(&bytes) != source.hash {
        return Err(format!(
            "'{}' changed since the board was read; read it again",
            source.path.display()
        ));
    }
    let text = freerouting_kicad::sexpr::decode(&bytes).map_err(|e| e.to_string())?;
    freerouting_kicad::fixups::pour_fills(text).map_err(|e| e.to_string())
}

/// The board's coordinate transform as a [`BoardFrame`]: nanometres to board
/// units exactly as the board builder maps the reader's tree (`um`, Y
/// negated, then the transform and `round`), and back on the grid.
struct TransformFrame {
    transform: CoordinateTransform,
    nm_per_unit: i64,
}

impl TransformFrame {
    fn of(board: &BasicBoard) -> Result<TransformFrame, String> {
        let transform = board.communication.coordinate_transform;
        let scale = transform.scale_factor();
        let nm_per_unit = (1000.0 / scale).round();
        let origin = transform.dsn_to_board_point(0.0, 0.0);
        if !(nm_per_unit >= 1.0 && nm_per_unit * scale == 1000.0)
            || origin.x != 0.0
            || origin.y != 0.0
        {
            return Err(format!(
                "the board's resolution ({scale} units per DSN unit) is not the KiCad reader's \
                 whole-nanometre grid"
            ));
        }
        Ok(TransformFrame {
            transform,
            nm_per_unit: nm_per_unit as i64,
        })
    }
}

impl BoardFrame for TransformFrame {
    fn to_board(&self, p: Pt) -> BPt {
        let d = freerouting_kicad::units::map_pt(i64::from(p.0), i64::from(p.1));
        let b = self.transform.dsn_to_board_point(d.x, d.y).round();
        (i64::from(b.x), i64::from(b.y))
    }
    fn half_width(&self, width_nm: i32) -> i64 {
        let um = freerouting_kicad::units::um(i64::from(width_nm));
        i64::from(round_to_i32(self.transform.dsn_to_board(um / 2.0)))
    }
    fn to_nm(&self, b: BPt) -> Pt {
        (
            (b.0 * self.nm_per_unit) as i32,
            (-b.1 * self.nm_per_unit) as i32,
        )
    }
    fn length_nm(&self, units: i64) -> i64 {
        units * self.nm_per_unit
    }
}

/// The traces and vias of `board`, in its item order, in the terms of the
/// file's copper index (KiCad layer positions, net codes, via sizes).
///
/// `SYSTEM_FIXED` traces are left out, as the session writer leaves them out
/// (`ses_writer::session_nets`), so the `.kicad_pcb` output and the session
/// carry the same copper. On a `.kicad_pcb` input such a trace is a locked
/// track of the file, which the diff keeps anyway, or a fixed wire
/// `router.input_fixups.fixed_pour_classes` laid over a pour's fill (K14.4),
/// which is not copper of the board.
pub fn final_copper(board: &BasicBoard, index: &CopperIndex) -> Result<Vec<FinalItem>, String> {
    let layer_of = |l: i32| -> Result<usize, String> {
        let name = usize::try_from(l)
            .ok()
            .and_then(|l| board.layer_structure.layers.get(l))
            .map(|l| l.name.as_str())
            .unwrap_or("");
        index
            .layer_of(name)
            .ok_or_else(|| format!("board layer '{name}' is not a copper layer of the file"))
    };
    let codes: std::collections::BTreeMap<&str, u32> = index
        .dsn_net_names
        .iter()
        .enumerate()
        .rev()
        .filter_map(|(code, name)| Some((name.as_deref()?, code as u32)))
        .collect();
    let mut out = Vec::new();
    for item in board.items() {
        if item.is_trace() && item.fixed_state() == FixedState::SystemFixed {
            continue;
        }
        let Some(&net_number) = item.net_numbers().first() else {
            continue;
        };
        let name = board
            .rules
            .nets
            .get(net_number)
            .map(|n| n.name.as_str())
            .unwrap_or("");
        let net = match &item.kind {
            ItemKind::Trace(_) | ItemKind::Via(_) => codes
                .get(name)
                .copied()
                .ok_or_else(|| format!("net '{name}' is not a net of the file"))?,
            _ => continue,
        };
        match &item.kind {
            ItemKind::Trace(trace) => {
                let corners = trace
                    .polyline
                    .corners()
                    .iter()
                    .map(|c| match c.as_int() {
                        Some(p) => (i64::from(p.x), i64::from(p.y)),
                        None => {
                            let f = c.to_float();
                            (i64::from(round_to_i32(f.x)), i64::from(round_to_i32(f.y)))
                        }
                    })
                    .collect();
                out.push(FinalItem::Trace(FinalTrace {
                    layer: layer_of(trace.layer)?,
                    net,
                    half_width: i64::from(trace.half_width),
                    corners,
                }));
            }
            ItemKind::Via(via) => {
                let padstack = board
                    .library
                    .padstacks
                    .get(via.drill.padstack)
                    .map(|p| p.name.as_str())
                    .unwrap_or("");
                let def = index.via_def(padstack).ok_or_else(|| {
                    format!("via padstack '{padstack}' is not one the KiCad reader made")
                })?;
                out.push(FinalItem::Via(FinalVia {
                    at: (i64::from(via.drill.center.x), i64::from(via.drill.center.y)),
                    net,
                    top: def.top,
                    bottom: def.bottom,
                    diameter: def.diameter,
                    drill: def.drill,
                }));
            }
            _ => {}
        }
    }
    Ok(out)
}

/// Whether two paths name the same file (after canonicalization; a path that
/// does not exist yet is compared by its canonical directory and name).
pub fn same_file(a: &Path, b: &Path) -> bool {
    fn canonical(p: &Path) -> Option<PathBuf> {
        if let Ok(c) = std::fs::canonicalize(p) {
            return Some(c);
        }
        let name = p.file_name()?;
        let dir = match p.parent() {
            Some(d) if !d.as_os_str().is_empty() => d,
            _ => Path::new("."),
        };
        Some(std::fs::canonicalize(dir).ok()?.join(name))
    }
    match (canonical(a), canonical(b)) {
        (Some(x), Some(y)) => x == y,
        _ => a == b,
    }
}

/// The text of `input_path` with the copper of `board` written into it
/// (K08): the input is read again, its copper diffed against the board's,
/// the removed blocks cut out and the new ones added before the final `)`.
/// With `expected_hash`, a file whose bytes no longer hash to it is refused
/// (it changed since the board was built).
pub fn render_kicad_board(
    input_path: &Path,
    board: &BasicBoard,
    expected_hash: Option<u128>,
) -> Result<(String, WriteSummary), String> {
    let bytes = std::fs::read(input_path).map_err(|e| {
        format!(
            "Couldn't load the input file '{}': {e}",
            input_path.display()
        )
    })?;
    let hash = fnv1a128(&bytes);
    if expected_hash.is_some_and(|h| h != hash) {
        return Err(format!(
            "'{}' changed since the board was read; read it again before writing",
            input_path.display()
        ));
    }
    let input =
        read_kicad_bytes(input_path, &bytes, ReadOptions::default()).map_err(|e| e.to_string())?;
    let index = &input.copper_index;
    let frame = TransformFrame::of(board)?;
    let items = final_copper(board, index)?;
    let diff = freerouting_kicad::write::diff(index, &items, &frame);
    let text = freerouting_kicad::sexpr::decode(&bytes).map_err(|e| e.to_string())?;
    freerouting_kicad::write::render(text, index, &diff, hash).map_err(|e| e.to_string())
}

/// [`write_kicad_board_checked`] without the change check.
pub fn write_kicad_board(
    input_path: &Path,
    board: &BasicBoard,
    out_path: &Path,
) -> Result<WriteSummary, String> {
    write_kicad_board_checked(input_path, board, out_path, None)
}

/// Writes the copper of `board` into a copy of `input_path` at `out_path`
/// ([`render_kicad_board`]). Refuses an `out_path` that is the input; writes
/// `.<name>.<pid>.tmp` next to `out_path` and renames it into place.
pub fn write_kicad_board_checked(
    input_path: &Path,
    board: &BasicBoard,
    out_path: &Path,
    expected_hash: Option<u128>,
) -> Result<WriteSummary, String> {
    if same_file(input_path, out_path) {
        return Err(format!(
            "refusing to overwrite the input board '{}'; write to another file",
            input_path.display()
        ));
    }
    let (text, summary) = render_kicad_board(input_path, board, expected_hash)?;
    let name = out_path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| "board.kicad_pcb".to_owned());
    let temp = out_path.with_file_name(format!(".{name}.{}.tmp", std::process::id()));
    let io = |e: std::io::Error| {
        format!(
            "Couldn't save the output file '{}': {e}",
            out_path.display()
        )
    };
    std::fs::write(&temp, text.as_bytes()).map_err(io)?;
    if let Err(e) = std::fs::rename(&temp, out_path) {
        let _ = std::fs::remove_file(&temp);
        return Err(io(e));
    }
    Ok(summary)
}

//! Building a board from a DSN design file.
//!
//! Port of `io.specctra.DsnReader.readBoard` and `io.BoardReadResult`.

use freerouting_dsn::Pcb;

use super::parser::ReadContext;
use super::parser::dsn_file::adjust_plane_autoroute_settings;
use super::parser::library::read_library;
use super::parser::network::{insert_components, insert_logical_parts, read_network};
use super::parser::structure::read_structure;
use super::parser::wiring::read_wiring;
use crate::board::RoutingBoard;
use crate::board::state::WriteResolution;
use crate::board::structure::Unit;

/// The location reported for every DSN read failure.
const PCB_LOCATION: &str = "(pcb";

/// The result of reading a DSN file.
///
/// Port of `io.BoardReadResult` without the `IoError` case (the reader
/// works on text that is already in memory).
#[derive(Debug)]
pub enum BoardReadResult {
    /// The board was read. `warnings` lists what was skipped.
    Success {
        board: Box<RoutingBoard>,
        warnings: Vec<String>,
    },
    /// The structure scope has no usable board outline. Java returns no
    /// board in this case either.
    OutlineMissing {
        board: Option<Box<RoutingBoard>>,
        warnings: Vec<String>,
    },
    /// The file is not a DSN design or could not be read.
    ParseError { location: String, detail: String },
}

impl BoardReadResult {
    fn parse_error(detail: &str) -> Self {
        BoardReadResult::ParseError {
            location: PCB_LOCATION.into(),
            detail: detail.into(),
        }
    }

    /// The board, if one was built.
    pub fn board(&self) -> Option<&RoutingBoard> {
        match self {
            BoardReadResult::Success { board, .. } => Some(board),
            BoardReadResult::OutlineMissing { board, .. } => board.as_deref(),
            BoardReadResult::ParseError { .. } => None,
        }
    }
}

const NOT_A_DSN_FILE: &str = "Not a Specctra DSN file: expected '(pcb <name>' header";
const STRUCTURE_FAILED: &str = "DSN structure parsing failed";

/// Java's header check: an opening bracket followed by the keyword `pcb`.
fn has_pcb_header(text: &str) -> bool {
    let rest = text.trim_start();
    let Some(rest) = rest.strip_prefix('(') else {
        return false;
    };
    let rest = rest.trim_start();
    let keyword: String = rest
        .chars()
        .take_while(|c| !c.is_whitespace() && *c != '(' && *c != ')')
        .collect();
    keyword.eq_ignore_ascii_case("pcb")
}

/// Reads a DSN file and builds its board.
///
/// The bytes are decoded as UTF-8, or Latin-1 if they are not valid UTF-8.
pub fn read_board(bytes: &[u8]) -> BoardReadResult {
    read_board_from_str(&freerouting_dsn::decode(bytes))
}

/// Reads DSN text and builds its board.
pub fn read_board_from_str(text: &str) -> BoardReadResult {
    if !has_pcb_header(text) {
        return BoardReadResult::parse_error(NOT_A_DSN_FILE);
    }
    match Pcb::parse(text) {
        Ok(pcb) => build_board(&pcb),
        Err(_) => BoardReadResult::parse_error(STRUCTURE_FAILED),
    }
}

/// Builds the board of a parsed DSN design, as Java's reader does while it
/// reads the scopes.
///
/// The syntax tree merges repeated top-level scopes, so they are applied in
/// the order exporters write them: parser, resolution, structure,
/// placement, library, part library, network, wiring.
pub fn build_board(pcb: &Pcb) -> BoardReadResult {
    let mut ctx = ReadContext::new();
    let read_ok = read_scopes(&mut ctx, pcb);
    super::parser::set_last_scope_identifier(&ctx.scope_identifier);
    let warnings = std::mem::take(&mut ctx.warnings);
    if read_ok {
        let Some(mut board) = ctx.board.take() else {
            // Java returns a success without a board when the file has no
            // structure scope; the port reports it as a failed read.
            return BoardReadResult::parse_error(STRUCTURE_FAILED);
        };
        if !ctx.autoroute_settings_read {
            adjust_plane_autoroute_settings(&mut board);
        }
        if !warnings.is_empty() {
            board.log.push(format!(
                "DSN file '{}' was loaded with {} warning(s).",
                if pcb.name.trim().is_empty() {
                    "unknown"
                } else {
                    pcb.name.as_str()
                },
                warnings.len()
            ));
        }
        BoardReadResult::Success {
            board: Box::new(board),
            warnings,
        }
    } else if !ctx.board_outline_ok {
        BoardReadResult::OutlineMissing {
            board: ctx.board.take().map(Box::new),
            warnings,
        }
    } else {
        BoardReadResult::parse_error(STRUCTURE_FAILED)
    }
}

fn read_scopes(ctx: &mut ReadContext, pcb: &Pcb) -> bool {
    if let Some(parser) = &pcb.parser {
        if let Some(quote) = &parser.string_quote {
            ctx.string_quote = quote.clone();
        }
        ctx.host_cad = parser.host_cad.clone();
        ctx.host_version = parser.host_version.clone();
        ctx.constants = parser.constants.clone();
        ctx.write_resolution = parser.write_resolution.as_ref().map(|w| WriteResolution {
            char_name: w.name.clone(),
            positive_int: w.value,
        });
        if parser.generated_by_freerouting {
            ctx.dsn_file_generated_by_host = false;
        }
    }
    if let Some(resolution) = pcb.resolution {
        ctx.unit = Unit::from(resolution.unit);
        ctx.resolution = resolution.value;
    }
    if let Some(structure) = &pcb.structure
        && !read_structure(ctx, structure)
    {
        return false;
    }
    if ctx.board.is_none() {
        // Every later scope needs the board; Java fails with an exception.
        return pcb.placement.is_none()
            && pcb.library.is_none()
            && pcb.network.is_none()
            && pcb.wiring.is_none();
    }
    if pcb
        .placement
        .as_ref()
        .and_then(|p| p.flip_style_rotate_first)
        .unwrap_or(false)
    {
        ctx.board().components.set_flip_style_rotate_first(true);
    }
    if let Some(library) = &pcb.library
        && !read_library(ctx, library)
    {
        return false;
    }
    if let Some(network) = &pcb.network {
        if !read_network(ctx, network) {
            return false;
        }
        insert_components(ctx, pcb.placement.as_ref());
        insert_logical_parts(ctx, pcb.part_library.as_ref());
    }
    if let Some(wiring) = &pcb.wiring
        && !read_wiring(ctx, wiring)
    {
        return false;
    }
    true
}

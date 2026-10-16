//! The file header and the version gate.
//!
//! KiCad 6 and later start with `(kicad_pcb (version N) (generator ...)
//! (generator_version ...))`. KiCad 5 has `(host pcbnew ...)` instead of
//! `generator`, and its `version` is below `20201115` (`(version 4)` for the
//! first s-expression format, `20171130` for KiCad 5). Those files use
//! `(module ...)` blocks and bare net names, and the reader refuses them with
//! one line instead of reading half of them (KiCadRoutingTools'
//! `UnsupportedBoardFormat`).
//!
//! A `kicad_pcb` without a `(version)` list is read as `20201115`, as KiCad's
//! `parseHeader` does.

use crate::error::{ErrorKind, KicadError};
use crate::sexpr::{NodeRef, Tree};

/// First file version the reader accepts: "module -> footprint and change
/// fill syntax" (KiCad 6 development), `SEXPR_BOARD_FILE_VERSION 20201115`.
pub const FIRST_SUPPORTED_VERSION: u32 = 20201115;

/// Newest file version KiCad 10.0.0 writes (`SEXPR_BOARD_FILE_VERSION`). Newer
/// files are still read; callers may warn ([`Header::is_newer_than_known`]).
pub const LATEST_KNOWN_VERSION: u32 = 20260206;

/// What the header says about the file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub version: u32,
    /// The `(generator ...)` value, empty when absent.
    pub generator: String,
    pub generator_version: Option<String>,
}

impl Header {
    /// True when the file is newer than the KiCad this reader was written
    /// against.
    pub fn is_newer_than_known(&self) -> bool {
        self.version > LATEST_KNOWN_VERSION
    }
}

/// The text of the refusal for a file before KiCad 6.
pub fn unsupported_version_message(version: &str) -> String {
    format!(
        "unsupported KiCad board file version {version} (KiCad 5 or older); \
         open and save the board in KiCad 6 or later"
    )
}

/// Finds the `kicad_pcb` list of a parsed file.
///
/// The first top-level item must be a list headed `kicad_pcb`, otherwise the
/// file is [`ErrorKind::NotABoard`]. Anything after it is a syntax error.
pub fn board<'a>(tree: &'a Tree<'a>) -> Result<NodeRef<'a>, KicadError> {
    let root = tree.root();
    let Some(first) = root.item(0) else {
        return Err(KicadError::new(ErrorKind::NotABoard, "the file is empty"));
    };
    if !first.is_list() || first.head() != Some("kicad_pcb") {
        let what = match (first.is_list(), first.head()) {
            (true, Some(h)) => {
                format!("not a KiCad board: the top-level list is `{h}`, not `kicad_pcb`")
            }
            (true, None) => "not a KiCad board: the top-level list has no name".to_owned(),
            (false, _) => "not a KiCad board: the file does not start with a list".to_owned(),
        };
        return Err(first.error(ErrorKind::NotABoard, what));
    }
    if let Some(extra) = root.item(1) {
        return Err(extra.error(
            ErrorKind::Syntax,
            "unexpected data after the closing parenthesis of the board",
        ));
    }
    Ok(first)
}

/// Reads and checks the header of the board in `tree`.
///
/// Errors: [`ErrorKind::NotABoard`] when the top-level list is not a
/// `kicad_pcb`; [`ErrorKind::UnsupportedVersion`] when `version <
/// 20201115`; [`ErrorKind::Syntax`] when the version is not a number.
pub fn read_header(tree: &Tree<'_>) -> Result<Header, KicadError> {
    let board = board(tree)?;
    let version_node = board.child("version");
    let version = match version_node {
        None => FIRST_SUPPORTED_VERSION,
        Some(v) => {
            let text = v.atom(1).unwrap_or("");
            match text.parse::<i64>() {
                // A negative or oversized number is a version this reader cannot use.
                Ok(n) => u32::try_from(n).unwrap_or(0),
                Err(_) => {
                    return Err(v.error(
                        ErrorKind::Syntax,
                        format!("`(version {text})` is not a number"),
                    ));
                }
            }
        }
    };
    if version < FIRST_SUPPORTED_VERSION {
        let text = version_node.and_then(|v| v.atom(1)).unwrap_or("0");
        let message = unsupported_version_message(text);
        return Err(match version_node {
            Some(v) => v.error(ErrorKind::UnsupportedVersion, message),
            None => KicadError::new(ErrorKind::UnsupportedVersion, message),
        });
    }
    let generator = board
        .child("generator")
        .and_then(|g| g.str_value(1))
        .map(|s| s.into_owned())
        .unwrap_or_default();
    let generator_version = board
        .child("generator_version")
        .and_then(|g| g.str_value(1))
        .map(|s| s.into_owned());
    Ok(Header {
        version,
        generator,
        generator_version,
    })
}

/// Parses `src` and checks its header. Convenience for callers that only
/// need to know whether the reader accepts a file.
pub fn check(src: &str) -> Result<Header, KicadError> {
    let tree = Tree::parse(src)?;
    read_header(&tree)
}

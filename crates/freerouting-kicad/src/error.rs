//! Errors and warnings of the `.kicad_pcb` reader.
//!
//! Positions are 1-based; the column counts characters, not bytes. A
//! position of line 0 means "unknown" (a node the reader synthesised).

use std::fmt;

/// What went wrong.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// Not valid s-expression text (unterminated string, stray `)`, missing
    /// `)` at the end of the file, bytes that are not UTF-8).
    Syntax,
    /// A KiCad file generation the reader refuses (before KiCad 6).
    UnsupportedVersion,
    /// Valid s-expressions, but the top-level list is not a `kicad_pcb`.
    NotABoard,
    /// A well-formed board with content the reader cannot use.
    Semantic,
}

/// A fatal reader error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KicadError {
    pub kind: ErrorKind,
    pub line: u32,
    pub column: u32,
    pub message: String,
}

impl KicadError {
    /// An error without a source position (line 0).
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        KicadError {
            kind,
            line: 0,
            column: 0,
            message: message.into(),
        }
    }

    /// An error at byte `offset` of `src`. The line and column are counted
    /// here, on the error path only.
    pub fn at(kind: ErrorKind, src: &str, offset: usize, message: impl Into<String>) -> Self {
        let (line, column) = line_col(src, offset);
        KicadError {
            kind,
            line,
            column,
            message: message.into(),
        }
    }
}

impl fmt::Display for KicadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(
                f,
                "line {}, column {}: {}",
                self.line, self.column, self.message
            )
        }
    }
}

impl std::error::Error for KicadError {}

/// A recoverable problem: the reader skipped or defaulted something.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub line: u32,
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.line == 0 {
            write!(f, "{}", self.message)
        } else {
            write!(f, "line {}: {}", self.line, self.message)
        }
    }
}

/// Line and column (both 1-based, column in characters) of byte `offset`.
/// Counts from the start of `src`, so call it on error paths only. An offset
/// past the end, or inside a character, is clamped to the previous boundary.
pub fn line_col(src: &str, offset: usize) -> (u32, u32) {
    let mut end = offset.min(src.len());
    while !src.is_char_boundary(end) {
        end -= 1;
    }
    let head = &src.as_bytes()[..end];
    let line = head.iter().filter(|&&b| b == b'\n').count() + 1;
    let line_start = head.iter().rposition(|&b| b == b'\n').map_or(0, |p| p + 1);
    let column = src[line_start..end].chars().count() + 1;
    (line as u32, column as u32)
}

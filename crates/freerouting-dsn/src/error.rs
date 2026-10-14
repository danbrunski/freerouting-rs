//! Error and warning types. Every error and warning carries the line and column
//! where it was detected.

use std::fmt;

/// A position in the source text. Lines and columns start at 1; columns count
/// characters, not bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Pos {
    pub line: u32,
    pub column: u32,
}

impl Pos {
    pub const START: Pos = Pos { line: 1, column: 1 };
}

impl fmt::Display for Pos {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "line {}, column {}", self.line, self.column)
    }
}

/// A fatal parse error.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub pos: Pos,
    pub message: String,
}

impl Error {
    pub(crate) fn new(pos: Pos, message: impl Into<String>) -> Self {
        Error {
            pos,
            message: message.into(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pos, self.message)
    }
}

impl std::error::Error for Error {}

/// A recoverable problem. The parser skipped or defaulted something, the same
/// way the Java reader logs a warning and carries on.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    pub pos: Pos,
    pub message: String,
}

impl fmt::Display for Warning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.pos, self.message)
    }
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

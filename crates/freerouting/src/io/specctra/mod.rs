//! The Specctra DSN format: building a board from a design file.
//!
//! Port of `io.specctra.DsnReader` and the board-building parts of
//! `io.specctra.parser`. The file is read into a syntax tree by
//! `freerouting-dsn`; this module applies it to a new board the way the
//! Java scope readers do.

mod dsn_reader;
pub(crate) mod parser;

pub use dsn_reader::{BoardReadResult, build_board, read_board, read_board_from_str};

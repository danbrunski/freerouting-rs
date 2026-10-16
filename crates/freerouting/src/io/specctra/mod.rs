//! The Specctra DSN format: building a board from a design file and
//! importing session files.
//!
//! Port of `io.specctra.DsnReader`, `io.specctra.SesReader` and the
//! board-building parts of `io.specctra.parser`. The file is read into a syntax tree by
//! `freerouting-dsn`; this module applies it to a new board the way the
//! Java scope readers do.

mod dsn_reader;
pub(crate) mod parser;
pub mod rules_reader;
pub mod rules_writer;
mod ses_reader;
pub mod ses_writer;

pub use dsn_reader::{BoardReadResult, build_board, read_board, read_board_from_str};
pub use rules_reader::{RulesError, RulesOutcome, apply_rules_file, read_rules};
pub use rules_writer::{rules_from_board, write_rules};
pub use ses_reader::{SesImportSummary, import_session, read_session};
pub use ses_writer::{SesStyle, session_from_board, snapped_endpoint, write_ses, write_ses_to};

pub mod dsn_writer;
pub use dsn_writer::{pcb_from_board, write_dsn, write_dsn_to};

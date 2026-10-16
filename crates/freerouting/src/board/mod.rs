//! The board model: structure, items, search trees and the board facade.
//!
//! Port of `board.model`, `board.searchtree`, `board.state`, `board.trace`
//! and `board.facade`.

pub mod autoroute_log;
pub mod basic_board;
pub mod items;
pub mod journal;
pub mod routing_board;
pub mod search;
pub mod side_table;
pub mod snapshot;
pub mod state;
pub mod state_hash;
pub mod structure;
mod trace_ops;

pub use autoroute_log::AutorouteChange;
pub use basic_board::BasicBoard;
pub use journal::BoardChanges;
pub use routing_board::RoutingBoard;
pub use side_table::ItemSideTable;
pub use snapshot::BoardSnapshot;
pub use trace_ops::{ChangeOutcome, LineReuse};

// B04: read-only checks, picks and item helpers (in their own files so they
// run beside B02's work on basic_board.rs).
pub mod item_queries;
pub mod queries;
pub mod selection_filter;

pub use selection_filter::{ItemFilter, SelectableChoice};

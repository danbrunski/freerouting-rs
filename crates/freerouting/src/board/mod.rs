//! The board model: structure, items, search trees and the board facade.
//!
//! Port of `board.model`, `board.searchtree`, `board.state`, `board.trace`
//! and `board.facade`.

pub mod basic_board;
pub mod items;
pub mod journal;
pub mod routing_board;
pub mod search;
pub mod snapshot;
pub mod state;
pub mod state_hash;
pub mod structure;
mod trace_ops;

pub use basic_board::BasicBoard;
pub use journal::BoardChanges;
pub use routing_board::RoutingBoard;
pub use snapshot::BoardSnapshot;

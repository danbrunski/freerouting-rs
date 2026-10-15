//! The board model: structure, items, search trees and the board facade.
//!
//! Port of `board.model`, `board.searchtree`, `board.state`, `board.trace`
//! and `board.facade`.

pub mod basic_board;
pub mod items;
pub mod routing_board;
mod trace_ops;
pub mod search;
pub mod state;
pub mod structure;

pub use basic_board::BasicBoard;
pub use routing_board::RoutingBoard;

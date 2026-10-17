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

// B05: item operations, pin exits and pin-connection fixes (in their own
// files, beside the other lanes' work on basic_board.rs).
pub mod item_ops;
pub mod pin_exits;
mod trace_pin_connection;

pub use item_ops::ItemTransform;
pub use pin_exits::TraceExitRestriction;

// B08: the engine API of the GUI (drawable geometry, item info, interactive
// leftovers), in their own files.
pub mod item_geometry;

pub use item_geometry::{GeometryShape, ItemGeometry, LayerShape, geometry_of_item, tile_geometry};
pub mod gui_ops;

pub use gui_ops::DetachError;
pub mod item_info;

pub use item_info::{InfoField, ItemInfo};

// B09: board services for algorithm stages (in their own files).
pub mod transaction;

pub use transaction::{CheckPolicy, TransactionResult};
pub mod obstacle_view;

pub use obstacle_view::ObstacleShape;

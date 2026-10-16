//! The geometric room types of room completion (B03).
//!
//! Java's `IncompleteFreeSpaceExpansionRoom` carries a shape, a layer and
//! the shape the room must contain; the autorouter's arenas (A02) wrap it
//! with doors and costs. Only the geometry lives here, so room completion
//! does not depend on the autorouter. A complete room in a search tree is a
//! [`TreeObject::Room`](super::TreeObject) with its [`RoomId`]
//! (B02.1).

use freerouting_geometry::TileShape;

pub use super::tree_object::RoomId;

/// An expansion room whose shape is not yet complete (Java
/// `IncompleteFreeSpaceExpansionRoom`, without its doors).
#[derive(Clone, Debug)]
pub struct IncompleteRoom {
    /// The shape the room may grow to; `None` is the whole plane.
    pub shape: Option<TileShape>,
    /// The layer of the room.
    pub layer: i32,
    /// The shape the completed room must contain. Java allows `null` and
    /// warns; room completion then returns no rooms.
    pub contained: Option<TileShape>,
}

impl IncompleteRoom {
    /// A room with `shape` on `layer` that must contain `contained`.
    pub fn new(shape: Option<TileShape>, layer: i32, contained: Option<TileShape>) -> Self {
        IncompleteRoom {
            shape,
            layer,
            contained,
        }
    }
}

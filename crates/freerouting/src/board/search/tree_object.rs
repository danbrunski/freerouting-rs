//! The keys of the search-tree leaves: board items and the autorouter's
//! complete free-space rooms.
//!
//! Java's trees hold `SearchTreeObject`s, and `ShapeTree.Leaf.compareTo`
//! (`ShapeTree.java:273-279`) orders leaves by object, then by shape index.
//! A room compares before every item (`CompleteFreeSpaceExpansionRoom.compareTo`
//! returns -1 against a non-room, `Item.compareTo` returns 1), and rooms and
//! items each order by id descending. [`TreeObject`]'s derived `Ord` is that
//! order exactly: `Room` is declared first, and [`RoomId`] and [`ItemId`]
//! order by id descending.

use std::cmp::Ordering;
use std::fmt;

use freerouting_geometry::TileShape;

use crate::board::items::ItemId;
use crate::datastructures::LeafId;

/// The id of a complete free-space expansion room. The A series' session
/// hands them out (B03.6); a tree only stores them.
///
/// Orders by id descending, as `CompleteFreeSpaceExpansionRoom.compareTo`
/// (`other.id - this.id`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct RoomId(pub i32);

impl Ord for RoomId {
    fn cmp(&self, other: &Self) -> Ordering {
        other.0.cmp(&self.0)
    }
}

impl PartialOrd for RoomId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl fmt::Display for RoomId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// The object a search-tree leaf belongs to (Java `SearchTreeObject`).
///
/// The derived `Ord` is Java's `Leaf.compareTo` on the objects: rooms before
/// items, each by id descending.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TreeObject {
    Room(RoomId),
    Item(ItemId),
}

impl TreeObject {
    /// The item, if this is one.
    pub fn item(self) -> Option<ItemId> {
        match self {
            TreeObject::Item(id) => Some(id),
            TreeObject::Room(_) => None,
        }
    }

    /// The room, if this is one.
    pub fn room(self) -> Option<RoomId> {
        match self {
            TreeObject::Room(id) => Some(id),
            TreeObject::Item(_) => None,
        }
    }

    /// `R` or `I` and the id, as the tree walk writes them.
    pub(crate) fn walk_label(self) -> (char, i32) {
        match self {
            TreeObject::Room(id) => ('R', id.0),
            TreeObject::Item(id) => ('I', id.0),
        }
    }
}

impl From<ItemId> for TreeObject {
    fn from(id: ItemId) -> Self {
        TreeObject::Item(id)
    }
}

impl From<RoomId> for TreeObject {
    fn from(id: RoomId) -> Self {
        TreeObject::Room(id)
    }
}

/// One tile shape of a tree object in a search tree.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ObjectEntry {
    pub object: TreeObject,
    pub shape_index: usize,
}

/// A room in a search tree: Java's `CompleteFreeSpaceExpansionRoom` as a
/// `SearchTreeObject`. It has one tree shape, its shape
/// (`getTreeShape`), on its layer (`shapeLayer`), and is an obstacle for
/// every net (`isObstacle`, `isTraceObstacle`,
/// `CompleteFreeSpaceExpansionRoom.java:57-84`).
#[derive(Clone, Debug)]
pub struct RoomEntry {
    pub shape: TileShape,
    pub layer: i32,
    /// `None` when the shape has no bounding shape (Java's `insert` returns
    /// a null leaf then).
    pub leaf: Option<LeafId>,
}

//! Generic containers used by the board model.
//!
//! Port of the parts of `app.freerouting.datastructures` the board needs:
//! [`UndoableObjects`], [`ShapeTree`] (with the `MinAreaTree` insertion
//! strategy), [`IdGenerator`] and [`PlanarDelaunayTriangulation`]. Math helpers (`Signum`, `BigIntAux`) live in
//! `freerouting-geometry`.

mod id_generator;
mod planar_delaunay_triangulation;
mod shape_tree;
mod undoable_objects;

pub use id_generator::{IdGenerator, ItemIdGenerator};
pub use planar_delaunay_triangulation::{PlanarDelaunayTriangulation, ResultEdge};
pub use shape_tree::{BestFirstHeap, LeafId, ShapeTree};
pub(crate) use undoable_objects::StateRecord;
pub use undoable_objects::{DenseKey, UndoableObjects};

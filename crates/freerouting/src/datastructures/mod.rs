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
pub use shape_tree::{LeafId, ShapeTree};
pub use undoable_objects::UndoableObjects;

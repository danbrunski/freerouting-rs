//! Generic containers used by the board model.
//!
//! Port of the parts of `app.freerouting.datastructures` the board needs:
//! [`UndoableObjects`], [`ShapeTree`] (with the `MinAreaTree` insertion
//! strategy) and [`IdGenerator`]. Math helpers (`Signum`, `BigIntAux`) live in
//! `freerouting-geometry`.

mod id_generator;
mod shape_tree;
mod undoable_objects;

pub use id_generator::{IdGenerator, ItemIdGenerator};
pub use shape_tree::{LeafId, ShapeTree};
pub use undoable_objects::UndoableObjects;

//! Push and shove (O03, O04).
//!
//! Port of the check half of Java's push-and-shove stack (at `a773a70ec`):
//! `board.model.structure.ShapeEntrySide` and `ShapeAndEntrySide`
//! ([`entry_side`]), `board.searchtree.ShapeTraceEntries` ([`trace_entries`],
//! its cutouts in [`cutout`]), `board.optimize.TraceShover.check` and
//! `springOver` ([`trace_check`], [`spring`]), and the checks of
//! `board.actions.DrillItemMover` and `ForcedPadRouter` ([`drill_check`]),
//! of `ForcedViaInserter` ([`via_check`]) and
//! `RoutingBoard.checkForcedTracePolyline` ([`forced`]).
//! The maze search asks these checks whether a
//! trace or via could go somewhere if the traces in the way were shoved
//! aside, even with ripup off.
//!
//! The checks change no item, but they are not free of side effects, and
//! both side effects are Java's: building a temporary substitute trace burns
//! an item id ([`BasicBoard::burn_item_id`](crate::board::BasicBoard)), and a
//! failed check records the shove-failing obstacle and layer on the
//! [`RoutingBoard`](crate::board::RoutingBoard).
//!
//! One file per concern, not per Java class (O00). The entry lists of
//! `ShapeTraceEntries`, a circular linked list with an anchor in Java, are an
//! indexed `Vec` with explicit `next` indices here; the visiting order is
//! Java's.

pub mod cutout;
pub mod drill_check;
pub mod entry_side;
pub mod forced;
pub mod spring;
pub mod trace_check;
pub mod trace_entries;
pub mod via_check;

pub use cutout::CutoutResult;
pub use drill_check::CheckDrillResult;
pub use entry_side::{ShapeAndEntrySide, ShapeEntrySide};
pub use spring::SpringOver;
pub use trace_check::{FULL_OK_LENGTH, TraceShover};
pub use trace_entries::ShapeTraceEntries;
pub use via_check::ShoveDepths;

//! Path location (A05): from a found connection to trace corners.
//!
//! Port of `autoroute/path/FoundConnectionLocator.java` and
//! `FoundConnectionLocator45Degree.java` at `a773a70ec`. After the maze
//! search reaches a destination, [`locate`] walks the backtrack chain from
//! the destination door to the start ([`backtrack`]), collects the items the
//! search decided to rip, and turns the chain of door sections into one
//! polyline per layer ([`Located`]).
//!
//! - [`backtrack`]: `FoundConnectionLocator.backtrack`, the chain and the
//!   ripped items with their costs in Java's order;
//! - [`locate`](mod@locate): the driver (`FoundConnectionLocator`'s
//!   constructor, `getInstance`, `calculateNextTrace`, `adjustStartCorner`),
//!   the result and the errors;
//! - `locate45.rs`: the 45 degree corner rules
//!   (`FoundConnectionLocator45Degree`), also used at 90 degrees;
//! - [`insert`](mod@insert): the insertion of a located connection
//!   (`FoundConnectionInserter`, A06).
//!
//! Decisions (A05):
//! - Java's identity comparisons of points (`FoundConnectionLocator.java:415,
//!   432`) are the [`Corner`] enum, never value comparisons.
//! - The ripped set is a `BTreeSet<ItemId>` (descending id, as Java's
//!   `TreeSet<Item>`); the costs a [`RipupCosts`] list with Java's
//!   `LinkedHashMap.put` semantics. Both are filled during backtracking,
//!   before any acceptance check, so a rejected result leaves them filled.
//! - Where Java throws, the locator returns a [`LocateError`]; Java's
//!   `autorouteConnection` catches the exception and returns `FAILED`.
//!   Where Java loops forever (a warning branch of
//!   `calculateNextTraceCorners` that returns corners without advancing),
//!   the locator returns [`LocateError::EndlessLoop`] (`DIVERGENCES.md`,
//!   "Path location").
//! - `locate_any.rs`: the any-angle corner rules
//!   (`FoundConnectionLocatorAnyAngle`, A07.5), for boards without an angle
//!   restriction.
//! - [`Located`] also carries the backtrack chain, for the `locate` probe
//!   and for diagnostics (Java's `emitDiagnostics` reads the same array).

pub mod backtrack;
pub mod insert;
pub mod locate;
mod locate45;
mod locate_any;

pub use backtrack::{BacktrackElement, RipupCosts, backtrack, put_cost};
pub use insert::{InsertError, Inserted, insert_located, insert_segments};
pub use locate::{Corner, LocateError, Located, ResultItem, locate};

#[cfg(test)]
mod tests;

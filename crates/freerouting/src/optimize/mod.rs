//! Route optimization: pull-tight (O01), later the via optimizer (O05) and
//! the batch optimizer (O06).
//!
//! Port of `board.optimize.TraceTightener*` and the board entry points of
//! `board.facade.RoutingBoardOperations` at `a773a70ec`. The tightener is one
//! struct, [`tighten::Tightener`], whose [`tighten::AngleMode`] selects the
//! 45°, 90° or any-angle steps (decision of O01: one struct, not three
//! classes). The board entry points are `RoutingBoard` methods in
//! `pull_tight`.

pub mod params;
mod pull_tight;
pub mod tighten;

pub use params::{Clip, PullTightParams};
pub use pull_tight::OptChangedAreaRun;
pub use tighten::{AngleMode, LineOrigin, Outcome, Tightener, TrackedPolyline};

// O05: the via optimizer.
pub mod via;

pub use via::OPT_VIA_RECURSION_DEPTH;

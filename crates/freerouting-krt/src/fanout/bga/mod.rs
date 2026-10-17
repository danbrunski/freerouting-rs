//! The dense-BGA escape planner as a fanout stage (X12).
//!
//! - [`array`] (X12.1): the ball array of a component in its package frame
//!   (X10.1's [`crate::fanout::frame::PartFrame`]), or why it is none.
//! - [`channels`] (X12.2): band capacity and layer balancing.
//! - [`vias`] (X12.3): dog-bone via sites and padstacks.
//! - [`emit`] (X12.4): the escapes through the board API.
//!
//! The design follows KiCadRoutingTools' `py_router/bga_fanout` at
//! `0486e799` (channel escapes, layer balancing, the via sized to the
//! pitch, plane-ball drops, parts solved in their own frame); KRT's
//! under-pad engine, coupled pair escapes, via-in-pad fallback (D8),
//! negotiation and rerouting are not ported.
//!
//! **Selection.** `router.fanout.algorithm = "krt-bga-escape"` (with
//! `fanout.enabled`) runs [`KrtBgaEscape`] in the fanout slot: the escapes
//! of every array, then Freerouting's fanout (`BatchFanout`, A12), with its
//! passes, budgets and order unchanged, on every other component, including
//! an array where the planner placed no copper at all (a 0.5 mm array where
//! no via fits, [`EscapeReport::escaped_parts`]), which it fans out pin by
//! pin as without this stage. An array with any planned copper is not
//! handed over: measured on pocketsdr-v3-0's 0.8 mm U7 (X12.6 dry run),
//! Freerouting's fanout on its left-over balls ripped up planned escapes
//! and cut the fanned balls from 64 to 43.
//! Unselected, nothing here runs and sessions stay Freerouting's.

use std::collections::BTreeSet;

use freerouting::autoroute::batch::fanout::{FanoutRun, NoFanoutObserver};
use freerouting::autoroute::stages::{FanoutStage, FanoutSummary, StageCtx, StageRegistry};
use freerouting::board::RoutingBoard;

pub mod array;
pub mod channels;
pub mod emit;
pub mod vias;

pub use array::{ArrayKind, BallArray, Rejection, analyze_array};
pub use emit::{BallOutcome, BallResult, EscapeReport, Skip, escape};

/// The stage id.
pub const BGA_ESCAPE_ID: &str = "krt-bga-escape";

/// The `krt-bga-escape` fanout stage (module docs).
#[derive(Clone, Debug, Default)]
pub struct KrtBgaEscape {
    /// The report of the last run.
    pub last_report: Option<EscapeReport>,
}

impl FanoutStage for KrtBgaEscape {
    fn id(&self) -> &'static str {
        BGA_ESCAPE_ID
    }

    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> FanoutSummary {
        let report = escape(board, ctx.board_settings, ctx.cancel);
        let parts: BTreeSet<i32> = report.escaped_parts().iter().map(|c| c.0).collect();
        self.last_report = Some(report);
        let mut run = FanoutRun::new(board, ctx);
        run.retain_components(|c| !parts.contains(&c.component));
        run.run(board, ctx, &mut NoFanoutObserver)
    }
}

/// Registers `krt-bga-escape`.
pub(crate) fn register(registry: &mut StageRegistry) {
    registry
        .fanout
        .insert(BGA_ESCAPE_ID, |_| Box::new(KrtBgaEscape::default()));
}

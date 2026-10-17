//! Escape planners as fanout stages (X10, later X12).
//!
//! - [`frame`] (X10.1): the part frame of a component and the sides of an
//!   edge-row part (QFN, QFP, SOIC, SOP).
//! - [`qfn`] (X10.2): the escape stubs of those parts.
//!
//! **Selection.** `router.fanout.algorithm = "krt-qfn-escape"` (with
//! `fanout.enabled`) runs [`KrtQfnEscape`] in the fanout slot: stubs on
//! every qualifying part, then Freerouting's fanout (`BatchFanout`, A12)
//! on the other components only, with its passes, budgets and order
//! unchanged. Unselected, nothing here runs and sessions stay
//! Freerouting's.

use std::collections::BTreeSet;

use freerouting::autoroute::batch::fanout::{FanoutRun, NoFanoutObserver};
use freerouting::autoroute::stages::{FanoutStage, FanoutSummary, StageCtx, StageRegistry};
use freerouting::board::RoutingBoard;

pub mod frame;
pub mod qfn;

pub use frame::{PartFrame, PartLayout, Side, analyze, sides};
pub use qfn::{EscapeReport, PinResult, Skip, StubKind, escape};

/// The stage id.
pub const QFN_ESCAPE_ID: &str = "krt-qfn-escape";

/// The `krt-qfn-escape` fanout stage (module docs).
#[derive(Clone, Debug, Default)]
pub struct KrtQfnEscape {
    /// The report of the last run.
    pub last_report: Option<EscapeReport>,
}

impl FanoutStage for KrtQfnEscape {
    fn id(&self) -> &'static str {
        QFN_ESCAPE_ID
    }

    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> FanoutSummary {
        let report = escape(board, ctx.board_settings, ctx.cancel);
        let parts: BTreeSet<i32> = report.parts.iter().map(|c| c.0).collect();
        self.last_report = Some(report);
        let mut run = FanoutRun::new(board, ctx);
        run.retain_components(|c| !parts.contains(&c.component));
        run.run(board, ctx, &mut NoFanoutObserver)
    }
}

/// Registers `krt-qfn-escape`.
pub(crate) fn register(registry: &mut StageRegistry) {
    registry
        .fanout
        .insert(QFN_ESCAPE_ID, |_| Box::new(KrtQfnEscape::default()));
}

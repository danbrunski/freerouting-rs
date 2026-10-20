//! Escape planners as fanout stages (X10, X12).
//!
//! - [`frame`] (X10.1): the part frame of a component and the sides of an
//!   edge-row part (QFN, QFP, SOIC, SOP).
//! - [`qfn`] (X10.2): the escape stubs of those parts.
//! - [`rules`]: the net rules and the via-in-pad check both planners read.
//! - [`bga`] (X12): the dense-BGA escape planner, stage `krt-bga-escape`.
//!
//! **Selection.** `router.fanout.algorithm = "krt-qfn-escape"` (with
//! `fanout.enabled`) runs [`KrtQfnEscape`] in the fanout slot: stubs on
//! every qualifying part, then Freerouting's fanout (`BatchFanout`, A12)
//! on the other components only, with its passes, budgets and order
//! unchanged. Unselected, nothing here runs and sessions stay
//! Freerouting's.
//!
//! **Pin escape (Q21).** `router.fanout.pin_escape = "straight"` runs
//! [`KrtPinEscape`] (board stage [`PIN_ESCAPE_STAGE`], before the fanout):
//! the same stubs ([`escape`]) on every qualifying part and nothing else,
//! whether the fanout runs or not. Its floor is the board minimum
//! ([`min_trace_half_width`](freerouting::autoroute::control::min_trace_half_width),
//! which includes `router.board_minimums.track_width_um`), so no stub is
//! narrower than the board allows, and a stub that doesn't keep its
//! clearance at that width is not inserted. Unset or `off`, nothing runs.

use std::collections::BTreeSet;

use freerouting::autoroute::batch::fanout::{FanoutRun, NoFanoutObserver};
pub use freerouting::autoroute::stages::pin_escape::PIN_ESCAPE_STAGE;
use freerouting::autoroute::stages::{
    BoardStage, FanoutStage, FanoutSummary, StageCtx, StagePhase, StageRegistry, StageReport,
};
use freerouting::board::RoutingBoard;

pub mod bga;
pub mod frame;
pub mod qfn;
pub mod rules;

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

/// The `krt-pin-escape` board stage (Q21, module docs): the stubs of
/// [`escape`] before the fanout, selected by `router.fanout.pin_escape`.
#[derive(Clone, Debug, Default)]
pub struct KrtPinEscape {
    /// The report of the last run.
    pub last_report: Option<EscapeReport>,
}

impl BoardStage for KrtPinEscape {
    fn id(&self) -> &'static str {
        PIN_ESCAPE_STAGE
    }

    fn phase(&self) -> StagePhase {
        StagePhase::BeforeFanout
    }

    fn run(&mut self, board: &mut RoutingBoard, ctx: &mut StageCtx) -> StageReport {
        let report = escape(board, ctx.board_settings, ctx.cancel);
        let log = report.log(board);
        self.last_report = Some(report);
        StageReport { log }
    }
}

/// Registers `krt-qfn-escape` and `krt-pin-escape`.
pub(crate) fn register(registry: &mut StageRegistry) {
    registry
        .fanout
        .insert(QFN_ESCAPE_ID, |_| Box::new(KrtQfnEscape::default()));
    registry
        .board_stages
        .insert(PIN_ESCAPE_STAGE, |_| Box::new(KrtPinEscape::default()));
    bga::register(registry);
}

//! Q02: neck-down through pin fields
//! (`docs/roadmap/tasks/Q02-neck-down-through-pin-fields.md`).
//!
//! - [`width_report`] (Q02.1): the width each wide net actually got;
//! - [`ledger`] (Q02.2): every shipped narrowing and the strict-sizes rule;
//! - [`bounded`] (Q02.3): the distance bound of a neck and the neck width,
//!   as pure functions;
//! - [`bounded_neck`] (Q17.2): the `bounded-neck` neck-down stage, a width
//!   policy in the maze search ([`WidthPolicy`](crate::autoroute::width_policy::WidthPolicy))
//!   built from them;
//! - [`widen_back`] (Q17.3): the piecewise widen-back with tapers after a
//!   neck-down retry (`router.neck_down.widen_back`).
//!
//! The `widths` diagnostic (`router.special.diagnostics=widths`, also
//! added by `router.neck_down.strict`) computes the report and the ledger
//! on the final board and puts them in [`RunSummary::widths`]; the CLI's
//! result JSON carries them under `widths`. **Reports change nothing:** the
//! diagnostic reads the boards by shared reference and writes only the
//! summary, so the SES is byte-identical with it on and off
//! (`tests/quality_widths.rs`).

pub mod bounded;
pub mod bounded_neck;
pub mod ledger;
pub mod widen_back;
pub mod width_report;

use crate::autoroute::AttemptState;
use crate::autoroute::events::RunSummary;
use crate::autoroute::stages::{
    ConnectionOutcome, ConnectionParams, Diagnostics, RouteTask, StageCtx,
};
use crate::board::RoutingBoard;
use crate::drc::json::JsonValue;
use crate::settings::RouterSettings;

use ledger::{CallMark, NarrowingLedger, StageMarks};
use width_report::WidthReport;

/// The registry id of the diagnostic.
pub const DIAGNOSTIC_ID: &str = "widths";

/// What the `widths` diagnostic reports.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct NeckdownReport {
    /// The delivered widths of the wide nets.
    pub widths: WidthReport,
    /// Every shipped narrowing of those nets.
    pub ledger: NarrowingLedger,
    /// `router.neck_down.strict`.
    pub strict: bool,
}

impl NeckdownReport {
    /// The report as `freerouting-widths/1` JSON: the width report's
    /// members, then `strict` and `narrowings`.
    pub fn to_json(&self) -> JsonValue {
        let mut members = self.widths.json_members();
        members.push(("strict".into(), JsonValue::Bool(self.strict)));
        members.push((
            "narrowings".into(),
            self.ledger.to_json(self.widths.units_per_mm),
        ));
        JsonValue::Object(members)
    }
}

/// The `widths` diagnostic.
#[derive(Debug)]
pub struct WidthDiagnostics {
    marks: StageMarks,
    strict: bool,
}

impl WidthDiagnostics {
    /// The diagnostic of a job with `settings`.
    pub fn from_settings(settings: &RouterSettings) -> WidthDiagnostics {
        WidthDiagnostics {
            marks: StageMarks::from_settings(settings),
            strict: ledger::is_strict(settings),
        }
    }
}

fn max_item_id(board: &RoutingBoard) -> i32 {
    board.items().map(|i| i.id().0).max().unwrap_or(0)
}

impl Diagnostics for WidthDiagnostics {
    fn id(&self) -> &'static str {
        DIAGNOSTIC_ID
    }

    fn on_input(&mut self, board: &RoutingBoard, ctx: &StageCtx) {
        self.marks.input_max = Some(max_item_id(board));
        self.marks.neck_width_um = ctx.board_settings.neck_width_um();
        self.marks.min_trace_half_width =
            crate::autoroute::control::min_trace_half_width(board, ctx.board_settings);
    }

    fn after_connection(
        &mut self,
        board: &RoutingBoard,
        _task: &RouteTask,
        _p: &ConnectionParams,
        outcome: &ConnectionOutcome,
        _ctx: &StageCtx,
    ) {
        if matches!(
            outcome.state,
            AttemptState::Routed | AttemptState::Failed | AttemptState::InsertError
        ) {
            self.marks.calls.push(CallMark {
                first_new_id: outcome.first_new_id,
                max_after: max_item_id(board),
            });
        }
    }

    fn finish(&mut self, board: &RoutingBoard, _ctx: &StageCtx, summary: &mut RunSummary) {
        let widths = width_report::report(board);
        let ledger = ledger::ledger(board, &widths, &self.marks);
        summary.widths = Some(NeckdownReport {
            widths,
            ledger,
            strict: self.strict,
        });
    }
}

//! The progress panel of a running autorouter (G07.3): the status line and
//! the counters Java shows in its footer (`ScreenMessages`), from the newest
//! [`Progress`] event.
//!
//! Java's templates are in `text.rs` and filled the way
//! `ScreenMessages.setBatchAutorouteInfo` and `setBoardScore` fill them:
//! the batch router shows "To route / routed" and "Ripped up / failed", the
//! fanout "To route / routed" and "Failed / extra vias", and the score line
//! follows every event. The status line is Java's `autorouter_started` with
//! the pass number (`GuiRoutingJobWorker`'s task-state listener), or
//! `optimizer_started`; the fanout has its own message.

use eframe::egui;
use freerouting::autoroute::events::{CompletionStatus, Phase, Progress, RunSummary};
use freerouting::job::log::decimal_format_0_00;

use crate::commands::Command;
use crate::text;

/// The four lines of the panel for one event.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProgressTexts {
    /// What the router is doing.
    pub status: String,
    /// "To route: n; routed: n".
    pub add: String,
    /// "Ripped up: n; failed: n" (fanout: "Failed: n; extra vias: +n").
    pub layer: String,
    /// "Score: ... (n incomplete connections, n clearance violations)".
    pub score: String,
}

/// The lines for `progress`, as Java would fill them with its counters.
pub fn texts(progress: &Progress) -> ProgressTexts {
    let n = |v: usize| v.to_string();
    let score = text::fill(
        text::SCORE,
        &[
            &decimal_format_0_00(f64::from(progress.score)),
            &n(progress.incomplete),
            &n(progress.violations),
        ],
    );
    match progress.phase {
        Phase::Fanout => ProgressTexts {
            status: format!(
                "Fanout pass #{} (routed {}, failed {}, +{} vias)",
                progress.pass, progress.routed, progress.failed, progress.extra_vias
            ),
            add: text::fill(
                text::BATCH_AUTOROUTE_ADD,
                &[&n(progress.items_to_go), &n(progress.routed)],
            ),
            layer: text::fill(
                text::BATCH_FANOUT_LAYER,
                &[&n(progress.failed), &n(progress.extra_vias)],
            ),
            score,
        },
        Phase::Autoroute => ProgressTexts {
            status: text::fill(text::AUTOROUTER_STARTED, &[&progress.pass.to_string()]),
            add: text::fill(
                text::BATCH_AUTOROUTE_ADD,
                &[&n(progress.items_to_go), &n(progress.routed)],
            ),
            layer: text::fill(
                text::BATCH_AUTOROUTE_LAYER,
                &[&n(progress.ripped), &n(progress.failed)],
            ),
            score,
        },
        Phase::Optimize => ProgressTexts {
            status: text::fill(text::OPTIMIZER_STARTED, &[&progress.pass.to_string()]),
            add: String::new(),
            layer: String::new(),
            score,
        },
    }
}

/// Java's `autoroute_end_message` for a finished job ("Autoroute completed;
/// 3 connections remain unrouted.").
pub fn end_message(summary: &RunSummary) -> String {
    let status = if summary.status == CompletionStatus::Completed {
        text::COMPLETED
    } else {
        text::INTERRUPTED
    };
    text::fill(
        text::AUTOROUTE_END_MESSAGE,
        &[status, &summary.incomplete.to_string()],
    )
}

/// The panel: the status line, the counters and the Cancel button.
pub fn ui(ui: &mut egui::Ui, progress: Option<&Progress>, stopping: bool) -> Vec<Command> {
    let mut out = Vec::new();
    ui.heading(text::MENU_AUTOROUTER);
    if stopping {
        ui.label(text::ROUTING_STOPPING);
    }
    match progress {
        None => {
            ui.label(text::BATCH_AUTOROUTER_START_MESSAGE);
        }
        Some(progress) => {
            let lines = texts(progress);
            ui.label(lines.status);
            if !lines.add.is_empty() {
                ui.label(lines.add);
            }
            if !lines.layer.is_empty() {
                ui.label(lines.layer);
            }
            ui.label(lines.score);
        }
    }
    if ui
        .add_enabled(!stopping, egui::Button::new(text::CANCEL_BUTTON))
        .on_hover_text(text::CANCEL_BUTTON_TOOLTIP)
        .clicked()
    {
        out.push(Command::StopRouting);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn progress(phase: Phase) -> Progress {
        Progress {
            phase,
            pass: 2,
            items_to_go: 7,
            routed: 5,
            failed: 1,
            ripped: 3,
            extra_vias: 4,
            incomplete: 9,
            violations: 2,
            score: 912.345,
            ..Progress::default()
        }
    }

    #[test]
    fn autoroute_lines_follow_java() {
        let t = texts(&progress(Phase::Autoroute));
        assert_eq!(t.add, "To route: 7; routed: 5");
        assert_eq!(t.layer, "Ripped up: 3; failed: 1");
        assert_eq!(
            t.score,
            "Score: 912.34 (9 incomplete connections, 2 clearance violations)"
        );
        assert!(t.status.ends_with("Autorouter pass #2"), "{}", t.status);
    }

    #[test]
    fn fanout_lines_follow_java() {
        let t = texts(&progress(Phase::Fanout));
        assert_eq!(t.add, "To route: 7; routed: 5");
        assert_eq!(t.layer, "Failed: 1; extra vias: +4");
        assert_eq!(t.status, "Fanout pass #2 (routed 5, failed 1, +4 vias)");
    }

    #[test]
    fn end_messages() {
        let mut summary = RunSummary {
            incomplete: 3,
            ..RunSummary::default()
        };
        assert_eq!(
            end_message(&summary),
            "Autoroute completed; 3 connections remain unrouted."
        );
        summary.status = CompletionStatus::Interrupted;
        assert_eq!(
            end_message(&summary),
            "Autoroute interrupted; 3 connections remain unrouted."
        );
    }
}

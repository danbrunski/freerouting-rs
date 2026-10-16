//! The selection toolbar (G05.4): Java's `BoardToolbarInspectedItem`, shown
//! while items are selected.
//!
//! Java's toolbar at the pin has ten buttons: Cancel, Info, the four
//! "extend selection" buttons, Violations and the three zoom buttons. The
//! other edits its bundle still has texts for (Fix, Unfix, Delete, New Net,
//! New Component, Cutout, Assign Clearance Class, Autoroute, Pull Tight) are
//! inert there. Decision G00 question 6 adds Delete, Fix and Unfix, drawn
//! last and apart from Java's ten; the rest are left out.
//!
//! It floats over the top of the canvas instead of taking a row of the
//! window, so that selecting something does not resize the canvas (which
//! would fit the view again).

use eframe::egui;
use freerouting::board::items::ItemId;

use crate::commands::{Command, ExtendKind};
use crate::text;

/// One button.
#[derive(Clone, Debug, PartialEq)]
pub struct Button {
    pub label: &'static str,
    pub tooltip: &'static str,
    pub command: Command,
}

fn button(label: &'static str, tooltip: &'static str, command: Command) -> Button {
    Button {
        label,
        tooltip,
        command,
    }
}

/// The buttons in groups, as Java's toolbar separates them; the last group
/// holds the three added by decision.
pub fn groups(selection: &[ItemId]) -> Vec<Vec<Button>> {
    vec![
        vec![
            button(
                text::TB_CANCEL,
                text::TB_CANCEL_TIP,
                Command::ClearSelection,
            ),
            button(
                text::TB_INFO,
                text::TB_INFO_TIP,
                Command::ShowItemInfo(selection.to_vec()),
            ),
        ],
        vec![
            button(
                text::TB_NETS,
                text::TB_NETS_TIP,
                Command::ExtendSelection(ExtendKind::Nets),
            ),
            button(
                text::TB_CONN_SETS,
                text::TB_CONN_SETS_TIP,
                Command::ExtendSelection(ExtendKind::ConnectedSets),
            ),
            button(
                text::TB_CONNECTIONS,
                text::TB_CONNECTIONS_TIP,
                Command::ExtendSelection(ExtendKind::Connections),
            ),
            button(
                text::TB_COMPONENTS,
                text::TB_COMPONENTS_TIP,
                Command::ExtendSelection(ExtendKind::Components),
            ),
        ],
        vec![button(
            text::TB_VIOLATIONS,
            text::TB_VIOLATIONS_TIP,
            Command::ToggleSelectionViolations,
        )],
        vec![
            button(
                text::TB_ZOOM_SELECTION,
                text::TB_ZOOM_SELECTION_TIP,
                Command::ZoomSelection,
            ),
            button(text::TB_ZOOM_ALL, text::TB_ZOOM_ALL_TIP, Command::FitBoard),
            button(
                text::TB_ZOOM_REGION,
                text::TB_ZOOM_REGION_TIP,
                Command::ZoomToRectangle,
            ),
        ],
        vec![
            button(
                text::TB_DELETE,
                text::TB_DELETE_TIP,
                Command::DeleteSelection,
            ),
            button(text::TB_FIX, text::TB_FIX_TIP, Command::FixSelection),
            button(text::TB_UNFIX, text::TB_UNFIX_TIP, Command::UnfixSelection),
        ],
    ]
}

/// The labels of all buttons, in order.
pub fn labels() -> Vec<&'static str> {
    groups(&[]).into_iter().flatten().map(|b| b.label).collect()
}

/// Java's ten reachable buttons, in Java's order.
pub const JAVA_LABELS: [&str; 10] = [
    text::TB_CANCEL,
    text::TB_INFO,
    text::TB_NETS,
    text::TB_CONN_SETS,
    text::TB_CONNECTIONS,
    text::TB_COMPONENTS,
    text::TB_VIOLATIONS,
    text::TB_ZOOM_SELECTION,
    text::TB_ZOOM_ALL,
    text::TB_ZOOM_REGION,
];

/// The three buttons added by decision (G00 question 6).
pub const ADDED_LABELS: [&str; 3] = [text::TB_DELETE, text::TB_FIX, text::TB_UNFIX];

/// Draws the toolbar over the top of `canvas` while `selection` is not
/// empty and returns the clicked commands.
pub fn show(ctx: &egui::Context, canvas: egui::Rect, selection: &[ItemId]) -> Vec<Command> {
    let mut out = Vec::new();
    if selection.is_empty() {
        return out;
    }
    egui::Area::new(egui::Id::new("selection-toolbar"))
        .order(egui::Order::Foreground)
        .pivot(egui::Align2::CENTER_TOP)
        .fixed_pos(canvas.center_top() + egui::vec2(0.0, 6.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(text::TB_ACCESSIBLE_NAME).weak());
                    for (n, group) in groups(selection).into_iter().enumerate() {
                        if n > 0 {
                            ui.separator();
                        }
                        for b in group {
                            if ui.button(b.label).on_hover_text(b.tooltip).clicked() {
                                out.push(b.command);
                            }
                        }
                    }
                });
            });
        });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Java `BoardMenuToolbarA11yTest.inspectToolbarActionsAreGroupedSeparatorsAndAccessible`:
    /// the ten actions in order, in four groups (three separators), each
    /// with a distinct accessible name. The Hungarian-locale test is not
    /// ported (English only, G00 question 3).
    #[test]
    fn javas_toolbar_is_four_groups_with_distinct_names() {
        let groups = groups(&[ItemId(1)]);
        let sizes: Vec<usize> = groups.iter().map(Vec::len).collect();
        assert_eq!(sizes[..4], [2, 4, 1, 3], "Java's four groups");
        assert_eq!(sizes[4], 3, "the added edits form a fifth group");
        let java: Vec<&Button> = groups[..4].iter().flatten().collect();
        assert_eq!(
            java.iter().map(|b| b.label).collect::<Vec<_>>(),
            JAVA_LABELS
        );
        let mut names: Vec<&str> = groups.iter().flatten().map(|b| b.label).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 13, "unique sibling names");
        assert!(groups.iter().flatten().all(|b| !b.tooltip.is_empty()));
    }

    #[test]
    fn javas_ten_come_first_then_the_three_added() {
        let labels = labels();
        assert_eq!(labels[..10], JAVA_LABELS);
        assert_eq!(labels[10..], ADDED_LABELS);
    }
}

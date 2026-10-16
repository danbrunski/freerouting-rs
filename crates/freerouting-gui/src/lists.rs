//! The Info list windows and the analysis results they are built from.

use eframe::egui;

use std::collections::BTreeMap;

use freerouting::board::structure::Unit;

use crate::analysis::{Analysis, Tag};
use crate::commands::{Command, ListKind};
use crate::panels::object_list::{ListSpec, ListWindow, Row};
use crate::panels::{
    components, incompletes, islands, length, library, nets, unconnected, violations,
};
use crate::session::Session;
use crate::text;

/// All Info list windows.
#[derive(Default)]
pub struct Lists {
    incompletes: ListWindow,
    length: ListWindow,
    clearance: ListWindow,
    unconnected: ListWindow,
    islands: ListWindow,
    // G04: lists over the board's own data. Their rows are built when the
    // window opens and on Recalculate, and again when the board or the unit
    // changes, never every frame.
    nets: ListWindow,
    components: ListWindow,
    packages: ListWindow,
    padstacks: ListWindow,
    /// The nets window's "Incomplete connections only" box.
    nets_incomplete_only: bool,
    /// The unit the rows were built in.
    unit: Option<Unit>,
}

impl Lists {
    pub fn window(&mut self, kind: ListKind) -> &mut ListWindow {
        match kind {
            ListKind::Incompletes => &mut self.incompletes,
            ListKind::LengthViolations => &mut self.length,
            ListKind::ClearanceViolations => &mut self.clearance,
            ListKind::Unconnected => &mut self.unconnected,
            ListKind::ZoneIslands => &mut self.islands,
            ListKind::Nets => &mut self.nets,
            ListKind::Components => &mut self.components,
            ListKind::Packages => &mut self.packages,
            ListKind::Padstacks => &mut self.padstacks,
        }
    }

    /// The rows last built for a list, whatever state they were built for.
    pub fn built_rows(&self, kind: ListKind) -> Option<&[Row]> {
        self.get(kind).built_rows()
    }

    /// The rows of a list that pass its filter, for tests and the like.
    pub fn visible_rows(&self, kind: ListKind) -> Vec<Row> {
        let window = self.get(kind);
        window
            .built_rows()
            .map(|rows| window.visible_rows(rows).into_iter().cloned().collect())
            .unwrap_or_default()
    }

    fn get(&self, kind: ListKind) -> &ListWindow {
        match kind {
            ListKind::Incompletes => &self.incompletes,
            ListKind::LengthViolations => &self.length,
            ListKind::ClearanceViolations => &self.clearance,
            ListKind::Unconnected => &self.unconnected,
            ListKind::ZoneIslands => &self.islands,
            ListKind::Nets => &self.nets,
            ListKind::Components => &self.components,
            ListKind::Packages => &self.packages,
            ListKind::Padstacks => &self.padstacks,
        }
    }

    pub fn is_open(&self, kind: ListKind) -> bool {
        self.get(kind).open
    }

    /// The nets window's "Incomplete connections only" setting.
    pub fn nets_incomplete_only(&self) -> bool {
        self.nets_incomplete_only
    }

    /// Sets "Incomplete connections only" and rebuilds the rows (Java
    /// recalculates when the box is ticked).
    pub fn set_nets_incomplete_only(&mut self, on: bool) {
        if self.nets_incomplete_only != on {
            self.nets_incomplete_only = on;
            self.nets.invalidate();
        }
    }

    /// The rows of a list for the session's current state, if the analysis
    /// has finished the part they need.
    pub fn build_rows(kind: ListKind, session: &Session, analysis: &Analysis) -> Option<Vec<Row>> {
        let board = session.board.basic();
        match kind {
            ListKind::Incompletes => {
                let r = analysis.ratsnest(session)?;
                Some(incompletes::rows(board, &r.airlines))
            }
            ListKind::LengthViolations => {
                let r = analysis.ratsnest(session)?;
                Some(length::rows(board, &r.length_violations))
            }
            ListKind::ClearanceViolations => {
                let d = analysis.drc(session)?;
                Some(violations::rows(board, &d.clearance))
            }
            ListKind::Unconnected => {
                let d = analysis.drc(session)?;
                Some(unconnected::rows(board, &d.unconnected))
            }
            ListKind::ZoneIslands => {
                let d = analysis.drc(session)?;
                Some(islands::rows(board, &d.islands))
            }
            ListKind::Nets => Some(nets::rows(board, Unit::Mm, false, &BTreeMap::new())),
            ListKind::Components => Some(components::rows(board)),
            ListKind::Packages => Some(library::package_rows(board)),
            ListKind::Padstacks => Some(library::padstack_rows(board)),
        }
    }

    /// The rows of the lists over the board's own data, in `unit`. The nets
    /// list needs the ratsnest only when it shows the incomplete nets.
    fn build_board_rows(
        &self,
        kind: ListKind,
        session: &Session,
        analysis: &Analysis,
        unit: Unit,
    ) -> Option<Vec<Row>> {
        let board = session.board.basic();
        match kind {
            ListKind::Nets => {
                let counts = if self.nets_incomplete_only {
                    nets::incomplete_counts(analysis.ratsnest(session)?)
                } else {
                    BTreeMap::new()
                };
                Some(nets::rows(board, unit, self.nets_incomplete_only, &counts))
            }
            other => Self::build_rows(other, session, analysis),
        }
    }

    /// Draws the open windows and returns their commands.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        analysis: &Analysis,
        unit: Unit,
    ) -> Vec<Command> {
        let tag: Tag = analysis.tag_of(session);
        let mut out = Vec::new();
        if self.unit != Some(unit) {
            self.unit = Some(unit);
            for kind in BOARD_LISTS {
                self.window(kind).invalidate();
            }
        }
        out.extend(self.show_board_lists(ctx, session, analysis, tag, unit));
        for (kind, title, empty) in [
            (
                ListKind::Incompletes,
                text::INCOMPLETES_TITLE,
                text::INCOMPLETES_EMPTY,
            ),
            (
                ListKind::LengthViolations,
                text::LENGTH_TITLE,
                text::LENGTH_EMPTY,
            ),
            (
                ListKind::ClearanceViolations,
                text::CLEARANCE_TITLE,
                text::CLEARANCE_EMPTY,
            ),
            (
                ListKind::Unconnected,
                text::UNCONNECTED_TITLE,
                text::UNCONNECTED_EMPTY,
            ),
            (
                ListKind::ZoneIslands,
                text::ISLANDS_TITLE,
                text::ISLANDS_EMPTY,
            ),
        ] {
            let window = self.window(kind);
            if !window.open {
                continue;
            }
            if !window.has_rows(tag)
                && let Some(rows) = Self::build_rows(kind, session, analysis)
            {
                window.set_rows(tag, rows);
            }
            out.extend(window.show(ctx, kind, title, empty, tag));
        }
        out
    }
}

/// The lists over the board's own data.
const BOARD_LISTS: [ListKind; 4] = [
    ListKind::Nets,
    ListKind::Components,
    ListKind::Packages,
    ListKind::Padstacks,
];

impl Lists {
    /// Draws the open lists over the board's own data (G04.1, G04.2).
    fn show_board_lists(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        analysis: &Analysis,
        tag: Tag,
        unit: Unit,
    ) -> Vec<Command> {
        let mut out = Vec::new();
        for kind in BOARD_LISTS {
            if !self.get(kind).open {
                continue;
            }
            if !self.get(kind).has_rows(tag)
                && let Some(rows) = self.build_board_rows(kind, session, analysis, unit)
            {
                self.window(kind).set_rows(tag, rows);
            }
            let (title, select_label, select_tip) = match kind {
                ListKind::Nets => (text::NETS_TITLE, text::NETS_SELECT, text::NETS_SELECT_TIP),
                ListKind::Components => {
                    (text::COMPONENTS_TITLE, text::SELECT, text::SELECT_TOOLTIP)
                }
                ListKind::Packages => (text::PACKAGES_TITLE, text::SELECT, text::SELECT_TOOLTIP),
                _ => (text::PADSTACKS_TITLE, text::SELECT, text::SELECT_TOOLTIP),
            };
            let spec = ListSpec {
                title,
                empty: text::LIST_EMPTY,
                select_label,
                select_tooltip: select_tip,
                // Java hides the nets window's Info, Invert and Recalculate
                // buttons; its filter box rebuilds the list instead.
                show_info: kind != ListKind::Nets,
                show_invert: kind != ListKind::Nets,
                show_recalculate: kind != ListKind::Nets,
                recalculate: Command::RecalculateList(kind),
            };
            if kind == ListKind::Nets {
                let mut only = self.nets_incomplete_only;
                let count = nets::count_line(session.board.basic());
                out.extend(self.nets.show_with(ctx, kind, &spec, tag, |ui| {
                    ui.label(text::NET_EXPLANATION);
                    ui.strong(&count);
                    ui.checkbox(&mut only, text::NETS_INCOMPLETE_ONLY)
                        .on_hover_text(text::NETS_INCOMPLETE_ONLY_TIP);
                    Vec::new()
                }));
                self.set_nets_incomplete_only(only);
            } else {
                out.extend(
                    self.window(kind)
                        .show_with(ctx, kind, &spec, tag, |_| Vec::new()),
                );
            }
        }
        out
    }
}

//! The Info list windows and the analysis results they are built from.

use eframe::egui;

use crate::analysis::{Analysis, Tag};
use crate::commands::{Command, ListKind};
use crate::panels::object_list::{ListWindow, Row};
use crate::panels::{incompletes, islands, length, unconnected, violations};
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
}

impl Lists {
    pub fn window(&mut self, kind: ListKind) -> &mut ListWindow {
        match kind {
            ListKind::Incompletes => &mut self.incompletes,
            ListKind::LengthViolations => &mut self.length,
            ListKind::ClearanceViolations => &mut self.clearance,
            ListKind::Unconnected => &mut self.unconnected,
            ListKind::ZoneIslands => &mut self.islands,
        }
    }

    /// The rows last built for a list, whatever state they were built for.
    pub fn built_rows(&self, kind: ListKind) -> Option<&[Row]> {
        self.get(kind).built_rows()
    }

    fn get(&self, kind: ListKind) -> &ListWindow {
        match kind {
            ListKind::Incompletes => &self.incompletes,
            ListKind::LengthViolations => &self.length,
            ListKind::ClearanceViolations => &self.clearance,
            ListKind::Unconnected => &self.unconnected,
            ListKind::ZoneIslands => &self.islands,
        }
    }

    pub fn is_open(&self, kind: ListKind) -> bool {
        match kind {
            ListKind::Incompletes => self.incompletes.open,
            ListKind::LengthViolations => self.length.open,
            ListKind::ClearanceViolations => self.clearance.open,
            ListKind::Unconnected => self.unconnected.open,
            ListKind::ZoneIslands => self.islands.open,
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
        }
    }

    /// Draws the open windows and returns their commands.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        session: &Session,
        analysis: &Analysis,
    ) -> Vec<Command> {
        let tag: Tag = analysis.tag_of(session);
        let mut out = Vec::new();
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

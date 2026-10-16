//! The log window and the lines it shows (G04.8).
//!
//! Java prints these to a console and counts them in the status bar. Here the
//! log collects, oldest first and newest last:
//!
//! 1. what the DSN and SES readers and the board builder skipped
//!    (`Session::warnings`, as warnings);
//! 2. what the engine logged while it built the board
//!    (`BasicBoard::log_messages`, as information);
//! 3. what the app did since the board was opened: loads, design rules
//!    checks, exports (`Log::push`).
//!
//! The status bar counts the errors and the warnings and opens this window.

use eframe::egui;

use crate::session::Session;
use crate::text;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Level {
    Error,
    Warning,
    Info,
}

impl Level {
    pub fn label(self) -> &'static str {
        match self {
            Level::Error => text::LOG_LEVEL_ERROR,
            Level::Warning => text::LOG_LEVEL_WARNING,
            Level::Info => text::LOG_LEVEL_INFO,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub level: Level,
    pub text: String,
}

/// The events of the app since the board was opened.
#[derive(Clone, Debug, Default)]
pub struct Log {
    events: Vec<Entry>,
}

impl Log {
    pub fn push(&mut self, level: Level, text: impl Into<String>) {
        self.events.push(Entry {
            level,
            text: text.into(),
        });
    }

    /// Forgets the events of the board that was open.
    pub fn clear(&mut self) {
        self.events.clear();
    }

    /// Every line, oldest first: the loader warnings, the engine's messages
    /// and the events.
    pub fn entries(&self, session: Option<&Session>) -> Vec<Entry> {
        let mut out = Vec::new();
        if let Some(s) = session {
            out.extend(s.warnings.iter().map(|w| Entry {
                level: Level::Warning,
                text: w.clone(),
            }));
            if let Some(summary) = s.ses_summary.filter(|x| x.errors_encountered > 0) {
                out.push(Entry {
                    level: Level::Warning,
                    text: format!(
                        "Session import: {} wires and {} vias imported, {} errors \
                         (nets, wires or vias that could not be imported).",
                        summary.wires_imported, summary.vias_imported, summary.errors_encountered
                    ),
                });
            }
            out.extend(s.board.basic().log_messages().iter().map(|m| Entry {
                level: Level::Info,
                text: m.clone(),
            }));
        }
        out.extend(self.events.iter().cloned());
        out
    }

    /// `(errors, warnings)` for the status bar.
    pub fn counts(&self, session: Option<&Session>) -> (usize, usize) {
        let entries = self.entries(session);
        let count = |level| entries.iter().filter(|e| e.level == level).count();
        (count(Level::Error), count(Level::Warning))
    }
}

/// The window's content: one line per entry, newest last, scrolled to the
/// end.
pub fn ui(ui: &mut egui::Ui, entries: &[Entry]) {
    if entries.is_empty() {
        ui.label(text::LOG_EMPTY);
        return;
    }
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            for entry in entries {
                let color = match entry.level {
                    Level::Error => ui.visuals().error_fg_color,
                    Level::Warning => ui.visuals().warn_fg_color,
                    Level::Info => ui.visuals().text_color(),
                };
                ui.horizontal_wrapped(|ui| {
                    ui.colored_label(color, format!("[{}]", entry.level.label()));
                    ui.label(&entry.text);
                });
            }
        });
}

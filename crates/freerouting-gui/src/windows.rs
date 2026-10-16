//! The display windows that are plain flags: Visibility, Display Options,
//! the Color Manager, the log and About (G04.4, G04.6, G04.7, G04.8).
//!
//! The list windows are in [`crate::lists`]. A window here is open or not;
//! its content is a function in [`crate::panels`] that returns commands, so
//! a test can run one alone.

use eframe::egui;

use crate::commands::{Command, WindowKind};
use crate::panels::log::Entry;
use crate::panels::{about, colors, display, layers, log};
use crate::text;
use crate::viewport::ViewState;

/// Which of the flag windows are open.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Windows {
    visibility: bool,
    display: bool,
    colors: bool,
    log: bool,
    about: bool,
}

impl Windows {
    fn flag(&mut self, kind: WindowKind) -> &mut bool {
        match kind {
            WindowKind::Visibility => &mut self.visibility,
            WindowKind::DisplayMisc => &mut self.display,
            WindowKind::Colors => &mut self.colors,
            WindowKind::Log => &mut self.log,
            WindowKind::About => &mut self.about,
        }
    }

    pub fn is_open(&self, kind: WindowKind) -> bool {
        let mut copy = *self;
        *copy.flag(kind)
    }

    pub fn set(&mut self, kind: WindowKind, open: bool) {
        *self.flag(kind) = open;
    }

    /// Draws the open windows and returns their commands.
    pub fn show(
        &mut self,
        ctx: &egui::Context,
        view: &ViewState,
        layer_names: &[String],
        log_entries: &[Entry],
    ) -> Vec<Command> {
        let mut out = Vec::new();
        let mut window =
            |kind: WindowKind, title: &str, size: [f32; 2], add: &mut dyn FnMut(&mut egui::Ui)| {
                let mut open = *self.flag(kind);
                if !open {
                    return;
                }
                egui::Window::new(title)
                    .id(egui::Id::new(("window", kind)))
                    .open(&mut open)
                    .default_size(size)
                    .show(ctx, |ui| add(ui));
                *self.flag(kind) = open;
            };
        window(
            WindowKind::Visibility,
            text::VISIBILITY_TITLE,
            [440.0, 520.0],
            &mut |ui| out.extend(layers::visibility_ui(ui, &view.look, layer_names)),
        );
        window(
            WindowKind::DisplayMisc,
            text::DISPLAY_TITLE,
            [320.0, 300.0],
            &mut |ui| out.extend(display::ui(ui, view)),
        );
        window(
            WindowKind::Colors,
            text::COLORS_TITLE,
            [720.0, 360.0],
            &mut |ui| out.extend(colors::ui(ui, &view.look, layer_names)),
        );
        window(
            WindowKind::Log,
            text::LOG_TITLE,
            [640.0, 280.0],
            &mut |ui| log::ui(ui, log_entries),
        );
        window(
            WindowKind::About,
            text::ABOUT_TITLE,
            [420.0, 200.0],
            &mut |ui| about::ui(ui),
        );
        out
    }
}

//! Laying out one frame: menu, status bar, side panel and canvas. The
//! panels return commands; [`App::update`](eframe::App::update) runs them.

use eframe::egui;

use crate::app::App;
use crate::commands::Command;
use crate::menu;
use crate::panels::{board_stats, layers};
use crate::viewport::{self, CanvasOut};

/// The side panel's width before the user drags it. This is the width the
/// panel had when it followed its contents, so the default look is unchanged.
const DEFAULT_PANEL_WIDTH: f32 = 203.0;

impl App {
    pub(crate) fn panels(&mut self, ctx: &egui::Context) -> (Vec<Command>, CanvasOut) {
        let mut commands = Vec::new();
        egui::TopBottomPanel::top("menu").show(ctx, |ui| {
            commands.extend(menu::ui(
                ui,
                self.loaded.as_ref(),
                &self.settings.recent_files,
            ));
        });
        let status_id = egui::Id::new("status");
        let status: Option<String> = ctx
            .data(|d| d.get_temp::<Option<String>>(status_id))
            .flatten();
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            ui.horizontal(|ui| match (&self.error, &self.loading) {
                (Some(e), _) => drop(ui.colored_label(ui.visuals().error_fg_color, e)),
                (None, Some((paths, _))) => {
                    ui.spinner();
                    ui.label(format!("Reading {}", paths[0].display()));
                }
                _ => drop(ui.label(status.unwrap_or_default())),
            });
        });
        let width = self
            .settings
            .side_panel_width
            .unwrap_or(DEFAULT_PANEL_WIDTH);
        let side = egui::SidePanel::left("layers")
            .resizable(true)
            .default_width(width)
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| match &self.loaded {
                        None => drop(ui.weak("No board loaded.")),
                        Some(b) => {
                            commands.extend(layers::ui(ui, &b.scene, &self.view));
                            ui.add_space(8.0);
                            commands.extend(board_stats::ui(ui, b));
                        }
                    });
            });
        // Only a real change counts: a stored width that comes back equal
        // must not look like an edit.
        let now = side.response.rect.width();
        if self
            .settings
            .side_panel_width
            .is_none_or(|w| (w - now).abs() > 0.5)
        {
            self.settings.side_panel_width = Some(now);
        }
        let message = match &self.loading {
            Some((paths, _)) => format!("Reading {}…", paths[0].display()),
            None => "Open a .dsn board with File > Open, or drop one on the window.".into(),
        };
        self.scripted
            .before_canvas(&mut self.view, self.loaded.as_ref());
        let mut out = CanvasOut::default();
        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                out = viewport::canvas(ui, &mut self.view, self.loaded.as_ref(), &message);
                // The status bar is laid out before the canvas, so a new
                // hover text shows on the next frame.
                if ctx.data(|d| d.get_temp::<Option<String>>(status_id)) != Some(out.status.clone())
                {
                    ctx.request_repaint();
                }
                ctx.data_mut(|d| d.insert_temp(status_id, out.status.clone()));
            });
        (commands, out)
    }
}

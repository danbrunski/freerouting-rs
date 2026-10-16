//! Laying out one frame: menu, status bar, side panel and canvas. The
//! panels return commands; [`App::update`](eframe::App::update) runs them.

use eframe::egui;

use crate::app::App;
use crate::commands::Command;
use crate::menu;
use crate::overlays;
use crate::panels::{board_stats, layers, selection_toolbar, status};
use crate::toolbar;
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
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            commands.extend(toolbar::ui(
                ui,
                self.overlay,
                self.loaded.is_some(),
                self.view.unit,
            ));
        });
        // The status bar is laid out before the canvas, so the hover of the
        // last frame is what it shows.
        let hover = viewport::hover_of(ctx);
        let (errors, warnings) = self.log.counts(self.loaded.as_ref().map(|b| &b.session));
        egui::TopBottomPanel::bottom("status").show(ctx, |ui| {
            let layer = self
                .loaded
                .as_ref()
                .and_then(|b| b.scene.layers.get(self.view.active_layer))
                .map(|l| l.name.as_str());
            commands.extend(status::ui(
                ui,
                &status::Status {
                    errors,
                    warnings,
                    error: self.error.as_deref(),
                    loading: self
                        .loading
                        .as_ref()
                        .map(|(paths, _)| format!("Reading {}", paths[0].display())),
                    // While a region is dragged, Java says how it ends.
                    notice: if self.view.tool_drag.is_some() {
                        Some(crate::text::DRAG_TO_SELECT_REGION)
                    } else {
                        self.notice.as_deref()
                    },
                    hover: &hover,
                    layer,
                    unit: self.view.unit,
                },
            ));
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
                            ui.add_space(8.0);
                            commands.extend(board_stats::checks(
                                ui,
                                &board_stats::Checks::of(&b.session, &self.analysis),
                            ));
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
            None => "Open a .dsn or .kicad_pcb board with File > Open, or drop one on the window."
                .into(),
        };
        self.scripted
            .before_canvas(&mut self.view, self.loaded.as_ref());
        let mut out = CanvasOut::default();
        let central = egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show(ctx, |ui| {
                out = viewport::canvas(ui, &mut self.view, self.loaded.as_ref(), &message);
                self.overlay_counts = Default::default();
                if let (Some(b), Some(camera)) = (&self.loaded, self.view.camera) {
                    // The Net Classes window's "Filter Incomplete Connections"
                    // keeps the airlines of some nets only.
                    let filtered = self.dialogs.incompletes_filter().and_then(|keep| {
                        let all = self.analysis.ratsnest(&b.session)?;
                        Some(crate::dialogs::net_classes::filter_ratsnest(all, keep))
                    });
                    let inputs = overlays::Inputs {
                        look: &self.view.look,
                        board: &b.session.board,
                        ratsnest: filtered
                            .as_ref()
                            .or(self.analysis.ratsnest(&b.session).map(|r| &**r)),
                        drc: self.analysis.drc(&b.session).map(|d| &**d),
                        selection: &self.selection,
                        state: self.overlay,
                    };
                    let painter = ui.painter_at(self.view.canvas);
                    self.overlay_counts =
                        overlays::draw(&painter, self.view.canvas, &camera, &inputs);
                }
                // A new hover shows on the next frame.
                if viewport::store_hover(ctx, &out.hover) {
                    ctx.request_repaint();
                }
            });
        // G05: what the tools draw on the canvas, and the selection toolbar
        // over its top edge.
        if let Some(camera) = self.view.camera {
            let painter = ctx
                .layer_painter(central.response.layer_id)
                .with_clip_rect(self.view.canvas);
            self.overlay_counts.tools = self.draw_tools(&painter, &camera);
        }
        commands.extend(std::mem::take(&mut out.commands));
        commands.extend(selection_toolbar::show(
            ctx,
            self.view.canvas,
            &self.selection.ids(),
        ));
        (commands, out)
    }
}

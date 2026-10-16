//! Layer list and the visibility of each kind of item.

use eframe::egui::{self, RichText, Sense};

use crate::commands::Command;
use crate::scene::{Kind, Scene};
use crate::view;
use crate::viewport::ViewState;

pub fn ui(ui: &mut egui::Ui, scene: &Scene, state: &ViewState) -> Vec<Command> {
    let mut out = Vec::new();
    let n = scene.layers.len();
    ui.heading("Layers");
    ui.weak("Click a name to bring it to the front.");
    for (i, layer) in scene.layers.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut visible = state.layer_visible.get(i).copied().unwrap_or(true);
            if ui.checkbox(&mut visible, "").changed() {
                out.push(Command::ToggleLayer(i));
            }
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
            ui.painter().rect_filled(rect, 2.0, view::layer_color(i, n));
            let mut text = RichText::new(&layer.name);
            if !layer.signal {
                text = text.italics();
            }
            let shortcut = if i < 9 {
                format!("  {}", i + 1)
            } else {
                String::new()
            };
            let kind = if layer.signal { "Signal" } else { "Plane" };
            if ui
                .selectable_label(state.active_layer == i, text)
                .on_hover_text(format!("{kind} layer{shortcut}"))
                .clicked()
            {
                out.push(Command::SetActiveLayer(i));
            }
        });
    }
    ui.horizontal(|ui| {
        if ui.small_button("All").clicked() {
            out.push(Command::ShowAllLayers);
        }
        if ui.small_button("Active only").clicked() {
            out.push(Command::ActiveLayerOnly);
        }
    });
    ui.add_space(8.0);
    ui.heading("Show");
    for kind in Kind::ALL {
        let mut on = state.kind_on(kind);
        if ui.checkbox(&mut on, kind.label()).changed() {
            out.push(Command::ToggleKind(kind));
        }
    }
    let mut labels = state.show_labels;
    if ui.checkbox(&mut labels, "Reference designators").changed() {
        out.push(Command::ToggleLabels);
    }
    out
}

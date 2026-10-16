//! Layer list and the visibility of each kind of item.

use eframe::egui::{self, RichText, Sense};

use crate::commands::Command;
use crate::look::{
    ItemColor, LEVEL_STEP, Look, MAX_LEVEL, Object, VIRTUAL_LAYERS, virtual_name, virtual_tooltip,
};
use crate::scene::{Kind, Scene};
use crate::text;
use crate::viewport::ViewState;

pub fn ui(ui: &mut egui::Ui, scene: &Scene, state: &ViewState) -> Vec<Command> {
    let mut out = Vec::new();
    ui.heading("Layers");
    ui.weak("Click a name to bring it to the front.");
    for (i, layer) in scene.layers.iter().enumerate() {
        ui.horizontal(|ui| {
            let mut visible = state.layer_visible.get(i).copied().unwrap_or(true);
            if ui.checkbox(&mut visible, "").changed() {
                out.push(Command::ToggleLayer(i));
            }
            let (rect, _) = ui.allocate_exact_size(egui::vec2(12.0, 12.0), Sense::hover());
            ui.painter()
                .rect_filled(rect, 2.0, state.look.item_color(i, ItemColor::Trace));
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
    ui.heading("Virtual layers");
    for i in 0..VIRTUAL_LAYERS {
        let mut on = state.look.virtual_on(i);
        if ui
            .checkbox(&mut on, virtual_name(i))
            .on_hover_text(virtual_tooltip(i))
            .changed()
        {
            out.push(Command::ToggleVirtualLayer(i));
        }
    }
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

/// One slider row: the label, a slider in steps of 10 and the value. Returns
/// the new value when the user moved it.
fn level_slider(ui: &mut egui::Ui, label: &str, tip: &str, level: u8) -> Option<u8> {
    let mut value = level;
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([160.0, 18.0], egui::Label::new(label))
            .on_hover_text(tip);
        let slider = egui::Slider::new(&mut value, 0..=MAX_LEVEL)
            .step_by(f64::from(LEVEL_STEP))
            .suffix(" %");
        changed = ui.add(slider).on_hover_text(tip).changed();
    });
    changed.then_some(value)
}

/// Display > Layer and Object Visibility: Java's `WindowVisibility` (G04.4).
/// A slider from 0 to 100 % per copper layer and per kind of object, in steps
/// of 10; 0 hides. Reset restores 100 % everywhere and the default dimming.
pub fn visibility_ui(ui: &mut egui::Ui, look: &Look, layer_names: &[String]) -> Vec<Command> {
    let mut out = Vec::new();
    ui.vertical_centered(|ui| ui.label(text::VISIBILITY_HEADER));
    ui.separator();
    egui::ScrollArea::vertical()
        .max_height(480.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            ui.strong(text::VISIBILITY_LAYER_SECTION)
                .on_hover_text(text::VISIBILITY_LAYER_TIP);
            for (i, name) in layer_names.iter().enumerate() {
                if let Some(v) =
                    level_slider(ui, name, text::VISIBILITY_LAYER_TIP, look.layer_level(i))
                {
                    out.push(Command::SetLayerLevel(i, v));
                }
            }
            ui.separator();
            ui.strong(text::VISIBILITY_OBJECT_SECTION)
                .on_hover_text(text::VISIBILITY_OBJECT_TIP);
            for object in Object::ALL {
                if let Some(v) = level_slider(
                    ui,
                    object.label(),
                    text::VISIBILITY_OBJECT_TIP,
                    look.object_level(object),
                ) {
                    out.push(Command::SetObjectLevel(object, v));
                }
            }
        });
    ui.separator();
    ui.vertical_centered(|ui| {
        if ui
            .button(text::VISIBILITY_RESET)
            .on_hover_text(text::VISIBILITY_RESET_TIP)
            .clicked()
        {
            out.push(Command::ResetLevels);
        }
    });
    out
}

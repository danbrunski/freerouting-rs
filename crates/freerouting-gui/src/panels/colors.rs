//! Display > Color Manager: Java's `ColorManager` (G04.7).
//!
//! A table of the colour of each kind of item on each copper layer
//! (`ItemColorTableModel`) and a list of the other colours
//! (`OtherColorTableModel`), each a colour button, and a button that restores
//! the defaults. The buttons produce [`Command::SetItemColor`] and
//! [`Command::SetOtherColor`]; the colours are kept in [`Look`] and stored
//! with the settings.

use eframe::egui;

use crate::commands::Command;
use crate::look::{ItemColor, Look, OtherColor};
use crate::text;

pub fn ui(ui: &mut egui::Ui, look: &Look, layer_names: &[String]) -> Vec<Command> {
    let mut out = Vec::new();
    egui::ScrollArea::both()
        .auto_shrink([true, true])
        .show(ui, |ui| {
            egui::Grid::new("item-colors").striped(true).show(ui, |ui| {
                ui.strong(text::COLOR_LAYER);
                for column in ItemColor::ALL {
                    ui.strong(column.label());
                }
                ui.end_row();
                for (layer, name) in layer_names.iter().enumerate() {
                    ui.label(name);
                    for column in ItemColor::ALL {
                        let c = look.item_color(layer, column);
                        let mut rgb = [c.r(), c.g(), c.b()];
                        if ui.color_edit_button_srgb(&mut rgb).changed() {
                            out.push(Command::SetItemColor(layer, column, rgb));
                        }
                    }
                    ui.end_row();
                }
            });
            ui.separator();
            egui::Grid::new("other-colors").show(ui, |ui| {
                for color in OtherColor::ALL {
                    ui.label(color.label());
                    let c = look.other_color(color);
                    let mut rgb = [c.r(), c.g(), c.b()];
                    if ui.color_edit_button_srgb(&mut rgb).changed() {
                        out.push(Command::SetOtherColor(color, rgb));
                    }
                    ui.end_row();
                }
            });
        });
    ui.separator();
    if ui
        .button(text::COLORS_RESET)
        .on_hover_text(text::COLORS_RESET_TIP)
        .clicked()
    {
        out.push(Command::ResetColors);
    }
    out
}

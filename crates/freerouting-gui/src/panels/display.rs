//! Display > Miscellaneous: Java's `WindowDisplayMisc` (G04.6).
//!
//! The crosshair (small is the system cursor, large is the full-canvas
//! cross; `,` toggles), the board rotation (none, 90, 180, -90 degrees), the
//! board mirroring (none, left to right, top to bottom) and the slider of the
//! automatic layer dimming. Rotation and mirroring only turn the map from the
//! board to the screen ([`crate::view::Orientation`]); no board coordinate
//! changes.

use eframe::egui;

use crate::commands::Command;
use crate::look::MAX_LEVEL;
use crate::text;
use crate::view::Mirror;
use crate::viewport::ViewState;

pub fn ui(ui: &mut egui::Ui, view: &ViewState) -> Vec<Command> {
    let mut out = Vec::new();
    ui.horizontal(|ui| {
        ui.label(text::CROSSHAIR);
        for (label, large) in [
            (text::CROSSHAIR_SMALL, false),
            (text::CROSSHAIR_LARGE, true),
        ] {
            if ui
                .radio(view.crosshair == large, label)
                .on_hover_text(text::CROSSHAIR_TIP)
                .clicked()
            {
                out.push(Command::SetCrosshair(large));
            }
        }
    });
    ui.separator();
    ui.label(text::ROTATION);
    for (turns, label) in [
        (0u8, text::ROTATION_NONE),
        (1, text::ROTATION_90),
        (2, text::ROTATION_180),
        (3, text::ROTATION_270),
    ] {
        if ui
            .radio(view.orientation.quarter_turns == turns, label)
            .clicked()
        {
            out.push(Command::SetRotation(turns));
        }
    }
    ui.separator();
    ui.label(text::MIRRORING);
    for (mirror, label) in [
        (Mirror::None, text::MIRROR_NONE),
        (Mirror::LeftRight, text::MIRROR_LEFT_RIGHT),
        (Mirror::TopBottom, text::MIRROR_TOP_BOTTOM),
    ] {
        if ui.radio(view.orientation.mirror == mirror, label).clicked() {
            out.push(Command::SetMirror(mirror));
        }
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(text::LAYER_DIMMING)
            .on_hover_text(text::LAYER_DIMMING_TIP);
        let mut dim = view.look.dim();
        let slider = egui::Slider::new(&mut dim, 0..=MAX_LEVEL)
            .step_by(f64::from(crate::look::LEVEL_STEP))
            .suffix(" %");
        if ui.add(slider).changed() {
            out.push(Command::SetDim(dim));
        }
    });
    out
}

//! The profile selector and the "Algorithms" section of the autoroute
//! settings (N01.3, N01.4).
//!
//! - **Profile** (Legacy / Enhanced): the starting point of every value.
//!   Switching it moves the values that still have the old profile's value
//!   to the new one's ([`freerouting::settings::profile::rebase`]); values the
//!   user or the design set stay.
//! - **Algorithms**: one row per stage slot of the engine's catalog
//!   ([`freerouting::autoroute::stages::catalog`]): the registered ids to
//!   pick from, then the slot's options. A value that differs from the
//!   profile's is marked, and its Reset button returns it to the profile's
//!   value.
//!
//! Options and stage ids that failed a registered gate but meet N02's keep
//! rule are marked "(experimental)" with their help text on hover, and
//! listed again, with the board class they win on, under an "Experimental"
//! header at the end of the section (N02.4).
//!
//! The section edits the same [`SettingsPatch`] as the rest of the dialog;
//! the paths it covers are [`algorithm_paths`].

use eframe::egui;
use freerouting::autoroute::stages::catalog::{
    EXPERIMENTAL, ExperimentalKind, SLOTS, SlotArity, StageSlot, catalog,
};
use freerouting::settings::profile::rebase;
use freerouting::settings::{FieldKind, FieldValue, Profile, SettingsPatch, field_by_path};

use super::Draft;

/// The text of the mark on a value that differs from the profile.
pub const DIFFERS_MARK: &str = "changed";

/// Every settings path the section has a control for: the slot keys and
/// their options.
pub fn algorithm_paths() -> Vec<&'static str> {
    SLOTS
        .iter()
        .flat_map(|slot| std::iter::once(slot.key).chain(slot.options.iter().copied()))
        .collect()
}

impl Draft {
    /// The profile the draft names (unset is `enhanced`; an unknown name,
    /// which only a hand-edited file can give, reads as the default).
    pub fn profile(&self) -> Profile {
        Profile::of_patch(&self.patch).unwrap_or_default()
    }

    /// Switches the profile; see the module docs.
    pub fn set_profile(&mut self, profile: Profile) {
        rebase(&mut self.patch, profile, self.cores);
    }

    /// The value of `path` under the draft's profile.
    pub fn profile_value(&self, path: &str) -> Option<FieldValue> {
        let field = field_by_path(path)?;
        (field.get)(&self.profile().defaults(self.cores))
    }

    /// True if `path` differs from the profile's value.
    pub fn differs_from_profile(&self, path: &str) -> bool {
        path != "profile" && self.get(path) != self.profile_value(path)
    }

    /// Returns `path` to the profile's value.
    pub fn reset_to_profile(&mut self, path: &str) {
        let Some(field) = field_by_path(path) else {
            return;
        };
        match self.profile_value(path) {
            Some(value) => (field.set)(&mut self.patch, value),
            None => (field.clear)(&mut self.patch),
        }
    }

    /// The stage slots with the draft profile's defaults.
    pub fn stage_slots(&self) -> Vec<StageSlot> {
        catalog(&freerouting_stages::registry(), self.profile(), self.cores)
    }

    /// The ids a slot runs now: its key's value, else the profile's choice.
    pub fn stage_ids(&self, slot: &StageSlot) -> Vec<String> {
        match self.get(slot.desc.key) {
            Some(FieldValue::Str(id)) => vec![id],
            Some(FieldValue::StrList(ids)) => ids,
            _ => slot.default.clone(),
        }
    }

    /// Picks `ids` for a slot. The profile's own choice clears an unset key
    /// again, so picking the default does not count as a change.
    pub fn set_stage_ids(&mut self, slot: &StageSlot, ids: Vec<String>) {
        if self.profile_value(slot.desc.key).is_none() && ids == slot.default {
            if let Some(field) = field_by_path(slot.desc.key) {
                (field.clear)(&mut self.patch);
            }
            return;
        }
        let value = match slot.desc.arity {
            SlotArity::List => FieldValue::StrList(ids),
            SlotArity::One | SlotArity::Optional => match ids.into_iter().next() {
                Some(id) => FieldValue::Str(id),
                None => {
                    if let Some(field) = field_by_path(slot.desc.key) {
                        (field.clear)(&mut self.patch);
                    }
                    return;
                }
            },
        };
        self.set(slot.desc.key, value);
    }

    /// Sets an option from its text; blank clears an optional field. A text
    /// that does not parse is ignored.
    pub fn set_option_text(&mut self, path: &str, text: &str) {
        let Some(field) = field_by_path(path) else {
            return;
        };
        if text.trim().is_empty() && field.optional {
            (field.clear)(&mut self.patch);
            return;
        }
        let mut patch: SettingsPatch = self.patch.clone();
        if patch.set_path(path, text).is_ok() {
            self.patch = patch;
        }
    }
}

/// The profile selector.
pub fn profile_selector(ui: &mut egui::Ui, draft: &mut Draft) {
    let current = draft.profile();
    let mut chosen = current;
    ui.horizontal(|ui| {
        ui.label("Profile:")
            .on_hover_text("The starting point of every setting below.");
        egui::ComboBox::from_id_salt("autoroute_profile")
            .selected_text(current.label())
            .show_ui(ui, |ui| {
                for profile in Profile::ALL {
                    ui.selectable_value(&mut chosen, profile, profile.label())
                        .on_hover_text(profile.description());
                }
            })
            .response
            .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, "Profile"));
    });
    ui.weak(current.description());
    if chosen != current {
        draft.set_profile(chosen);
    }
}

/// The mark and the Reset button of a row that differs from the profile.
fn reset_cell(ui: &mut egui::Ui, draft: &mut Draft, path: &str, label: &str) {
    if draft.differs_from_profile(path) {
        ui.horizontal(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, DIFFERS_MARK)
                .on_hover_text(format!(
                    "Differs from the {} profile",
                    draft.profile().label()
                ));
            let reset = ui
                .small_button("Reset")
                .on_hover_text("Return to the profile's value");
            reset.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, format!("Reset {label}"))
            });
            if reset.clicked() {
                draft.reset_to_profile(path);
            }
        });
    } else {
        ui.label("");
    }
}

/// The text of a stage id: marked when it is experimental.
fn stage_text(slot: &StageSlot, id: &str) -> String {
    if slot.experimental_ids.iter().any(|e| e.key == id) {
        format!("{id} (experimental)")
    } else {
        id.to_string()
    }
}

/// The help text of an experimental stage id.
fn experimental_help(slot: &StageSlot, id: &str) -> Option<&'static str> {
    slot.experimental_ids
        .iter()
        .find(|e| e.key == id)
        .map(|e| e.help)
}

fn stage_choice(ui: &mut egui::Ui, draft: &mut Draft, slot: &StageSlot) {
    let label = slot.desc.label;
    let current = draft.stage_ids(slot);
    match slot.desc.arity {
        SlotArity::List => {
            let mut ids = current.clone();
            ui.vertical(|ui| {
                // A grid cell is narrow; an id must not wrap letter by letter.
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                if slot.ids.is_empty() {
                    ui.weak("(none registered)");
                }
                for id in &slot.ids {
                    let mut on = ids.iter().any(|i| i == id);
                    let response = ui.checkbox(&mut on, stage_text(slot, id));
                    let response = match experimental_help(slot, id) {
                        Some(help) => response.on_hover_text(help),
                        None => response,
                    };
                    response.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Checkbox,
                            true,
                            on,
                            format!("{label}: {id}"),
                        )
                    });
                    if response.changed() {
                        if on {
                            ids.push(id.to_string());
                        } else {
                            ids.retain(|i| i != id);
                        }
                    }
                }
            });
            if ids != current {
                draft.set_stage_ids(slot, ids);
            }
        }
        SlotArity::One | SlotArity::Optional => {
            let mut chosen = current.first().cloned().unwrap_or_default();
            let none = "(none)";
            egui::ComboBox::from_id_salt(("autoroute_stage", slot.desc.key))
                .selected_text(if chosen.is_empty() {
                    none
                } else {
                    chosen.as_str()
                })
                .show_ui(ui, |ui| {
                    if slot.desc.arity == SlotArity::Optional {
                        ui.selectable_value(&mut chosen, String::new(), none);
                    }
                    for id in &slot.ids {
                        let response =
                            ui.selectable_value(&mut chosen, id.to_string(), stage_text(slot, id));
                        if let Some(help) = experimental_help(slot, id) {
                            response.on_hover_text(help);
                        }
                    }
                })
                .response
                .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, true, label));
            let ids: Vec<String> = Some(chosen).filter(|c| !c.is_empty()).into_iter().collect();
            if ids != current {
                draft.set_stage_ids(slot, ids);
            }
        }
    }
}

fn option_value(ui: &mut egui::Ui, draft: &mut Draft, path: &'static str) {
    let Some(field) = field_by_path(path) else {
        return;
    };
    match (field.kind, draft.get(path)) {
        (FieldKind::Bool, value) => {
            let mut on = matches!(value, Some(FieldValue::Bool(true)));
            let response = ui.checkbox(&mut on, "");
            response.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Checkbox, true, on, path)
            });
            if response.changed() {
                draft.set(path, FieldValue::Bool(on));
            }
        }
        (_, value) => {
            let before = value.map(|v| v.to_string()).unwrap_or_default();
            let mut text = before.clone();
            let response = ui.add(egui::TextEdit::singleline(&mut text).desired_width(140.0));
            response.widget_info(|| {
                egui::WidgetInfo::text_edit(true, before.clone(), text.clone(), path)
            });
            if text != before {
                draft.set_option_text(path, &text);
            }
        }
    }
}

/// The "Algorithms" section.
pub fn algorithms(ui: &mut egui::Ui, draft: &mut Draft) {
    egui::CollapsingHeader::new("Algorithms")
        .id_salt("autoroute_algorithms")
        .show(ui, |ui| {
            ui.weak(format!(
                "Each stage and option starts at the {} profile's value; \"{DIFFERS_MARK}\" \
                 marks the ones that differ.",
                draft.profile().label()
            ));
            for slot in draft.stage_slots() {
                ui.separator();
                egui::Grid::new(("autoroute_slot", slot.desc.key))
                    .num_columns(3)
                    .show(ui, |ui| {
                        ui.strong(slot.desc.label)
                            .on_hover_text(format!("router.{}", slot.desc.key));
                        stage_choice(ui, draft, &slot);
                        reset_cell(ui, draft, slot.desc.key, slot.desc.label);
                        ui.end_row();
                        for option in &slot.options {
                            match option.experimental {
                                Some(e) => ui
                                    .label(format!("{} (experimental)", option.path))
                                    .on_hover_text(format!(
                                        "{}\n\nExperimental: {}",
                                        option.doc, e.help
                                    )),
                                None => ui.label(option.path).on_hover_text(option.doc),
                            };
                            option_value(ui, draft, option.path);
                            reset_cell(ui, draft, option.path, option.path);
                            ui.end_row();
                        }
                    });
            }
            ui.separator();
            experimental_section(ui);
        });
}

/// The "Experimental" header: every opt-in that failed a gate but meets the
/// keep rule, with the board class it wins on (N02.4).
fn experimental_section(ui: &mut egui::Ui) {
    egui::CollapsingHeader::new("Experimental")
        .id_salt("autoroute_experimental")
        .show(ui, |ui| {
            ui.weak("Off in every profile. Each wins on a board class, or is kept pending a measurement.");
            for e in EXPERIMENTAL {
                let key = match e.kind {
                    ExperimentalKind::Option => format!("router.{}", e.key),
                    ExperimentalKind::Stage => format!("{} (stage)", e.key),
                };
                ui.strong(key);
                ui.label(format!("Board class: {}", e.class));
                ui.label(e.help);
                ui.add_space(4.0);
            }
        });
}

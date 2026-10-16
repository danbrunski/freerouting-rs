//! The "General Settings" window of the selection (G05.2): Java's
//! `WindowSelectParameter`. It edits [`SelectionSettings`], which the inspect
//! tool reads (`tools::inspect`) and `settings.rs` persists.
//!
//! The choices are the engine's: [`SelectableChoice`] is B04's enum and
//! [`ItemFilter`] its filter. The GUI keeps the user's choices and builds the
//! filter from them; it never tests an item's class itself.

use std::collections::BTreeSet;

use eframe::egui;
use freerouting::board::{ItemFilter, SelectableChoice};

use crate::commands::Command;
use crate::text;

/// Which layers a click or a region selects on (Java
/// `WorkspaceSettings.selectOnAllVisibleLayers`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum SelectionLayers {
    /// Every visible layer; the current layer is preferred.
    #[default]
    AllVisible,
    /// Only the current layer.
    CurrentOnly,
}

impl SelectionLayers {
    /// The name stored in the settings.
    pub fn name(self) -> &'static str {
        match self {
            SelectionLayers::AllVisible => "all_visible",
            SelectionLayers::CurrentOnly => "current_only",
        }
    }

    pub fn from_name(name: &str) -> Option<SelectionLayers> {
        match name {
            "all_visible" => Some(SelectionLayers::AllVisible),
            "current_only" => Some(SelectionLayers::CurrentOnly),
            _ => None,
        }
    }
}

/// What the user allows to be selected (Java `WorkspaceSettings`:
/// `selectOnAllVisibleLayers` and the `ItemSelectionFilter`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SelectionSettings {
    pub layers: SelectionLayers,
    pub selectable: BTreeSet<SelectableChoice>,
}

impl Default for SelectionSettings {
    /// Java's defaults: all visible layers, and the choices of
    /// `new ItemSelectionFilter()` (everything but the keepouts, the
    /// conduction areas and the board outline).
    fn default() -> Self {
        SelectionSettings {
            layers: SelectionLayers::AllVisible,
            selectable: choices_of(&ItemFilter::default()),
        }
    }
}

fn choices_of(filter: &ItemFilter) -> BTreeSet<SelectableChoice> {
    SelectableChoice::ALL
        .into_iter()
        .filter(|c| filter.is_selected(*c))
        .collect()
}

impl SelectionSettings {
    /// The engine's filter for these choices.
    pub fn filter(&self) -> ItemFilter {
        self.selectable
            .iter()
            .fold(ItemFilter::none(), |f, c| f.with(*c, true))
    }

    pub fn set(&mut self, choice: SelectableChoice, on: bool) {
        if on {
            self.selectable.insert(choice);
        } else {
            self.selectable.remove(&choice);
        }
    }

    pub fn is_on(&self, choice: SelectableChoice) -> bool {
        self.selectable.contains(&choice)
    }

    /// Only `choice`, plus the fixed states (Java
    /// `new ItemSelectionFilter(choice)`).
    pub fn only(choice: SelectableChoice) -> SelectionSettings {
        SelectionSettings {
            layers: SelectionLayers::AllVisible,
            selectable: choices_of(&ItemFilter::only(choice)),
        }
    }

    /// The choices as stored: Java's constant names, in Java's order.
    pub fn stored_choices(&self) -> Vec<String> {
        SelectableChoice::ALL
            .into_iter()
            .filter(|c| self.is_on(*c))
            .map(|c| c.java_name().to_string())
            .collect()
    }

    /// Settings from the stored names. A name this version does not know is
    /// ignored.
    pub fn from_stored(layers: Option<&str>, choices: Option<&[String]>) -> SelectionSettings {
        let mut settings = SelectionSettings::default();
        if let Some(layers) = layers.and_then(SelectionLayers::from_name) {
            settings.layers = layers;
        }
        if let Some(choices) = choices {
            settings.selectable = SelectableChoice::ALL
                .into_iter()
                .filter(|c| choices.iter().any(|name| name == c.java_name()))
                .collect();
        }
        settings
    }
}

/// The label of a choice's check box (`WindowSelectParameter_en.properties`).
pub fn choice_label(choice: SelectableChoice) -> &'static str {
    match choice {
        SelectableChoice::Traces => text::SELECT_TRACES,
        SelectableChoice::Vias => text::SELECT_VIAS,
        SelectableChoice::Pins => text::SELECT_PINS,
        SelectableChoice::Conduction => text::SELECT_CONDUCTION,
        SelectableChoice::Keepout => text::SELECT_KEEPOUT,
        SelectableChoice::ViaKeepout => text::SELECT_VIA_KEEPOUT,
        SelectableChoice::ComponentKeepout => text::SELECT_COMPONENT_KEEPOUT,
        SelectableChoice::BoardOutline => text::SELECT_BOARD_OUTLINE,
        SelectableChoice::Fixed => text::SELECT_FIXED,
        SelectableChoice::Unfixed => text::SELECT_UNFIXED,
    }
}

fn choice_tooltip(choice: SelectableChoice) -> Option<&'static str> {
    match choice {
        SelectableChoice::Conduction => Some(text::SELECT_CONDUCTION_TIP),
        SelectableChoice::Keepout => Some(text::SELECT_KEEPOUT_TIP),
        SelectableChoice::ViaKeepout => Some(text::SELECT_VIA_KEEPOUT_TIP),
        SelectableChoice::ComponentKeepout => Some(text::SELECT_COMPONENT_KEEPOUT_TIP),
        SelectableChoice::BoardOutline => Some(text::SELECT_BOARD_OUTLINE_TIP),
        SelectableChoice::Fixed => Some(text::SELECT_FIXED_TIP),
        SelectableChoice::Unfixed => Some(text::SELECT_UNFIXED_TIP),
        _ => None,
    }
}

/// The window's content. `layer` is the name of the current layer.
pub fn ui(ui: &mut egui::Ui, settings: &SelectionSettings, layer: Option<&str>) -> Vec<Command> {
    let mut out = Vec::new();
    ui.strong(text::SELECT_LAYERS);
    for (value, label, tip) in [
        (
            SelectionLayers::AllVisible,
            text::SELECT_ALL_VISIBLE,
            text::SELECT_ALL_VISIBLE_TIP,
        ),
        (
            SelectionLayers::CurrentOnly,
            text::SELECT_CURRENT_ONLY,
            text::SELECT_CURRENT_ONLY_TIP,
        ),
    ] {
        if ui
            .radio(settings.layers == value, label)
            .on_hover_text(tip)
            .clicked()
        {
            out.push(Command::SetSelectionLayers(value));
        }
    }
    ui.add_space(6.0);
    ui.strong(text::SELECT_ITEMS);
    for choice in SelectableChoice::ALL {
        let mut on = settings.is_on(choice);
        let response = ui.checkbox(&mut on, choice_label(choice));
        let response = match choice_tooltip(choice) {
            Some(tip) => response.on_hover_text(tip),
            None => response,
        };
        if response.changed() {
            out.push(Command::SetSelectable(choice, on));
        }
    }
    if let Some(layer) = layer {
        ui.add_space(6.0);
        ui.label(format!("{} {layer}", text::SELECT_CURRENT_LAYER))
            .on_hover_text(text::SELECT_CURRENT_LAYER_TIP);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_javas() {
        let s = SelectionSettings::default();
        assert_eq!(s.layers, SelectionLayers::AllVisible);
        assert_eq!(s.filter(), ItemFilter::default());
        assert!(s.is_on(SelectableChoice::Traces) && !s.is_on(SelectableChoice::Keepout));
    }

    #[test]
    fn stored_round_trip_and_unknown_names() {
        let mut s = SelectionSettings::default();
        s.set(SelectableChoice::Keepout, true);
        s.set(SelectableChoice::Vias, false);
        s.layers = SelectionLayers::CurrentOnly;
        let back = SelectionSettings::from_stored(Some(s.layers.name()), Some(&s.stored_choices()));
        assert_eq!(back, s);
        let odd = SelectionSettings::from_stored(Some("nonsense"), Some(&["NOPE".to_string()]));
        assert_eq!(odd.layers, SelectionLayers::AllVisible);
        assert!(odd.selectable.is_empty());
        assert_eq!(
            SelectionSettings::only(SelectableChoice::Pins).filter(),
            ItemFilter::only(SelectableChoice::Pins)
        );
    }
}

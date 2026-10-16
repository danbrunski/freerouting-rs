//! Rules > Net Classes: Java's `WindowNetClasses` (G06.1).
//!
//! One row per net class with the columns `NAME`, `VIA_RULE`,
//! `CLEARANCE_CLASS`, `TRACE_WIDTH`, `ON_LAYER`, `SHOVE_FIXED`,
//! `CYCLES_WITH_AREAS`, `MIN_TRACE_LENGTH`, `MAX_TRACE_LENGTH` and
//! `IGNORED_BY_AUTOROUTER`, and the buttons Add, Remove, Assign, Select, Show
//! Nets and Filter Incomplete Connections. Each cell edit sends one
//! [`RulesEdit`]. A row is chosen by clicking its name (Ctrl or Cmd adds to
//! the choice); a double click on a name (not the default class's) renames
//! the class.
//!
//! Where this differs from Java (`docs/DIVERGENCES.md`, "Rules dialogs"):
//! Remove checks every net for the class, where Java's loop skips the last
//! net; Select selects the items of any chosen class, where Java compares
//! with `selectedClassArr[i]` (the item's net index) instead of `[j]`.

use std::collections::BTreeSet;

use eframe::egui::{self, Id};
use freerouting::board::BasicBoard;
use freerouting::geometry::jmath::java_round;
use freerouting::rules::NetClassId;

use super::RulesDialog;
use super::input::{Input, apply_trace_width, can_remove_net_class, parse_float_cell};
use super::rules_edit::RulesEdit;
use super::{RuleView, name_combo, text_cell};
use crate::commands::Command;
use crate::overlays::Frame;
use crate::text;

/// The "Layer Routing Rules" dialog being edited (`LayerRulesDialog`).
#[derive(Clone, Debug, PartialEq)]
pub struct LayerRules {
    pub class: NetClassId,
    /// `(board layer, name, checked)` of the signal layers.
    pub layers: Vec<(usize, String, bool)>,
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    /// The chosen rows, as the table shows them (ascending class order).
    pub selected: BTreeSet<NetClassId>,
    /// The class whose name is being typed, and whether the field has had focus.
    renaming: Option<(NetClassId, bool)>,
    pub layer_rules: Option<LayerRules>,
    /// The classes of the "Contained Nets" window, while it is open.
    pub contained: Option<Vec<NetClassId>>,
    /// The nets that keep their airlines after "Filter Incomplete
    /// Connections" (`setIncompletesFilter`); `None` shows all.
    pub incompletes_filter: Option<BTreeSet<i32>>,
}

/// `getLayerSummary`.
pub fn layer_summary(board: &BasicBoard, class: NetClassId) -> String {
    let net_class = board.rules.net_classes.get(class);
    let layers = &board.layer_structure.layers;
    let all: Vec<usize> = (0..layers.len()).filter(|&i| layers[i].is_signal).collect();
    let active: Vec<usize> = all
        .iter()
        .copied()
        .filter(|&i| net_class.is_active_routing_layer(i as i32))
        .collect();
    if active.is_empty() {
        return text::NC_LAYERS_NONE.to_string();
    }
    if active.len() == all.len() {
        return text::NC_LAYERS_ALL.to_string();
    }
    if active.len() == 2 && active[0] == all[0] && active[1] == all[all.len() - 1] {
        return text::NC_LAYERS_OUTER.to_string();
    }
    if active.len() == all.len() - 2
        && !active.contains(&all[0])
        && !active.contains(&all[all.len() - 1])
    {
        return text::NC_LAYERS_INNER.to_string();
    }
    if active.len() > 3 {
        return text::fill(text::NC_LAYERS_CUSTOM, &[&active.len().to_string()]);
    }
    active
        .iter()
        .map(|&i| layers[i].name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// `getTraceWidthSummary`: the common width of the active signal layers in
/// the display unit with four fraction digits, "Multiple widths..." when they
/// differ, "0" when no layer is active.
pub fn trace_width_summary(view: &RuleView<'_>, class: NetClassId) -> String {
    let board = view.board;
    let net_class = board.rules.net_classes.get(class);
    let mut common: Option<i32> = None;
    for (i, layer) in board.layer_structure.layers.iter().enumerate() {
        if layer.is_signal && net_class.is_active_routing_layer(i as i32) {
            let width = net_class.trace_half_width(i as i32);
            match common {
                None => common = Some(width),
                Some(c) if c != width => return text::NC_WIDTH_MULTIPLE.to_string(),
                Some(_) => {}
            }
        }
    }
    match common {
        None => "0".to_string(),
        Some(half) => format!("{:.4}", view.units.to_display(f64::from(half) * 2.0)),
    }
}

/// The text of the minimum or maximum length cell: the length in the display
/// unit as a Java `float`, a minimum below 0 as 0, a maximum of 0 or less as
/// -1 (`NetClassTableModel.setValues`).
pub fn length_text(view: &RuleView<'_>, board_length: f64, minimum: bool) -> String {
    let mut value = view.units.to_display(board_length);
    if value <= 0.0 {
        value = if minimum { 0.0 } else { -1.0 };
    }
    text::java_float_to_string(value as f32)
}

/// The edit for a typed length: `Float.parseFloat` (0 when it fails), a value
/// of 0 or less is 0, then `Math.round(userToBoard(value))`.
pub fn length_edit_value(view: &RuleView<'_>, typed: &str) -> f64 {
    let mut value = parse_float_cell(typed);
    if value <= 0.0 {
        value = 0.0;
    }
    java_round(view.units.to_board(f64::from(value))) as f64
}

/// The items of the nets of the chosen classes (`SelectClassesListener`).
pub fn items_of_classes(
    board: &BasicBoard,
    classes: &BTreeSet<NetClassId>,
) -> Vec<freerouting::board::items::ItemId> {
    board
        .items()
        .filter(|item| {
            item.net_numbers().iter().any(|&n| {
                board
                    .rules
                    .nets
                    .get(n)
                    .is_some_and(|net| classes.contains(&net.net_class()))
            })
        })
        .map(freerouting::board::items::Item::id)
        .collect()
}

/// The numbers of the nets whose class is among `classes`.
pub fn nets_of_classes(board: &BasicBoard, classes: &BTreeSet<NetClassId>) -> BTreeSet<i32> {
    (1..=board.rules.nets.max_net_number())
        .filter(|&n| {
            board
                .rules
                .nets
                .get(n)
                .is_some_and(|net| classes.contains(&net.net_class()))
        })
        .collect()
}

/// The ratsnest with only the airlines of the nets in `keep` (Java's
/// `setIncompletesFilter`: the nets of the chosen classes stay visible).
pub fn filter_ratsnest(
    ratsnest: &crate::analysis::Ratsnest,
    keep: &BTreeSet<i32>,
) -> crate::analysis::Ratsnest {
    crate::analysis::Ratsnest {
        airlines: ratsnest
            .airlines
            .iter()
            .filter(|a| keep.contains(&a.net_number))
            .cloned()
            .collect(),
        ..ratsnest.clone()
    }
}

pub fn show(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    // Classes that were removed leave the choice.
    let ids: Vec<NetClassId> = view
        .board
        .rules
        .net_classes
        .iter()
        .map(|(id, _)| id)
        .collect();
    state.selected.retain(|c| ids.contains(c));
    if state.renaming.is_some_and(|(c, _)| !ids.contains(&c)) {
        state.renaming = None;
    }
    if state.open {
        let mut open = true;
        egui::Window::new(text::NC_TITLE)
            .id(Id::new(("rules", RulesDialog::NetClasses)))
            .open(&mut open)
            .default_size([980.0, 300.0])
            .show(ctx, |ui| out.extend(table_ui(ui, state, view, &ids)));
        state.open = open;
    }
    out.extend(layer_rules_window(ctx, state));
    contained_nets_window(ctx, state, view);
    out
}

/// Scroll by the bars and the wheel, not by dragging the table: a dragged
/// content area takes the clicks of the cells (egui 0.32).
pub(crate) fn no_drag() -> egui::scroll_area::ScrollSource {
    egui::scroll_area::ScrollSource {
        drag: false,
        ..egui::scroll_area::ScrollSource::ALL
    }
}

fn table_ui(
    ui: &mut egui::Ui,
    state: &mut State,
    view: &RuleView<'_>,
    ids: &[NetClassId],
) -> Vec<Command> {
    let mut out = Vec::new();
    let board = view.board;
    let via_rules: Vec<String> = board
        .rules
        .via_rules
        .iter()
        .map(|(_, r)| r.name.clone())
        .collect();
    let clearance_names = view.clearance_class_names();
    egui::ScrollArea::vertical()
        .max_height(360.0)
        .show(ui, |ui| {
            egui::Grid::new("net_class_table")
                .striped(true)
                .min_col_width(60.0)
                .show(ui, |ui| {
                    for (i, title) in text::NC_COLUMNS.iter().enumerate() {
                        let heading = ui.strong(*title);
                        if i > 0 {
                            heading.on_hover_text(text::NC_COLUMN_TIPS[i - 1]);
                        }
                    }
                    ui.end_row();
                    for (row, &class) in ids.iter().enumerate() {
                        out.extend(class_row(
                            ui,
                            state,
                            view,
                            class,
                            row == 0,
                            &via_rules,
                            &clearance_names,
                        ));
                        ui.end_row();
                    }
                });
        });
    ui.separator();
    out.extend(buttons(ui, state, view, ids));
    out
}

#[allow(clippy::too_many_arguments)]
fn class_row(
    ui: &mut egui::Ui,
    state: &mut State,
    view: &RuleView<'_>,
    class: NetClassId,
    is_default: bool,
    via_rules: &[String],
    clearance_names: &[String],
) -> Vec<Command> {
    let mut out = Vec::new();
    let board = view.board;
    let net_class = board.rules.net_classes.get(class);
    let id = |column: &str| Id::new(("net_class_cell", class, column));
    let edit = |out: &mut Vec<Command>, e: RulesEdit| out.push(Command::EditRules(e));

    // NAME
    if let Some((renamed, started)) = state.renaming.filter(|(c, _)| *c == class && !is_default) {
        let cell = id("name");
        if let Some(name) = text_cell(ui, cell, net_class.name(), 110.0) {
            // Java returns silently when the name exists (`rename_net_class`).
            edit(
                &mut out,
                RulesEdit::RenameNetClass {
                    class: renamed,
                    name,
                },
            );
        }
        if !started {
            ui.memory_mut(|m| m.request_focus(cell));
            state.renaming = Some((renamed, true));
        } else if !ui.memory(|m| m.has_focus(cell)) {
            state.renaming = None;
        }
    } else {
        let selected = state.selected.contains(&class);
        let response = ui.selectable_label(selected, net_class.name());
        if response.clicked() {
            let additive = ui.input(|i| i.modifiers.command);
            if !additive {
                state.selected.clear();
            }
            if additive && selected {
                state.selected.remove(&class);
            } else {
                state.selected.insert(class);
            }
        }
        if response.double_clicked() && !is_default {
            state.renaming = Some((class, false));
        }
    }
    // VIA_RULE
    let rule_name = net_class
        .via_rule()
        .map(|r| board.rules.via_rules.get(r).name.clone())
        .unwrap_or_default();
    if let Some(rule) = name_combo(ui, id("via_rule"), &rule_name, via_rules) {
        edit(&mut out, RulesEdit::SetNetClassViaRule { class, rule });
    }
    // CLEARANCE_CLASS
    let clearance = board
        .rules
        .clearance_matrix
        .get_name(net_class.trace_clearance_class())
        .unwrap_or_default()
        .to_string();
    if let Some(name) = name_combo(ui, id("clearance"), &clearance, clearance_names) {
        edit(
            &mut out,
            RulesEdit::SetNetClassClearanceClass { class, name },
        );
    }
    // TRACE_WIDTH
    let width = trace_width_summary(view, class);
    if let Some(typed) = text_cell(ui, id("width"), &width, 90.0) {
        let to_board = |v: f64| view.units.to_board(v);
        apply_trace_width(
            |half_width| {
                edit(
                    &mut out,
                    RulesEdit::SetNetClassHalfWidth { class, half_width },
                )
            },
            Input::Text(&typed),
            Some(&to_board),
        );
    }
    // ON_LAYER
    if ui.button(layer_summary(board, class)).clicked() {
        state.layer_rules = Some(open_layer_rules(board, class));
    }
    // SHOVE_FIXED
    let mut shove = net_class.is_shove_fixed() || !net_class.pull_tight();
    if ui.checkbox(&mut shove, "").changed() {
        edit(
            &mut out,
            RulesEdit::SetNetClassShoveFixed {
                class,
                value: shove,
            },
        );
    }
    // CYCLES_WITH_AREAS
    let mut cycles = net_class.ignore_cycles_with_areas();
    if ui.checkbox(&mut cycles, "").changed() {
        edit(
            &mut out,
            RulesEdit::SetNetClassIgnoreCyclesWithAreas {
                class,
                value: cycles,
            },
        );
    }
    // MIN_TRACE_LENGTH, MAX_TRACE_LENGTH
    let min = length_text(view, net_class.minimum_trace_length(), true);
    if let Some(typed) = text_cell(ui, id("min"), &min, 70.0) {
        let length = length_edit_value(view, &typed);
        edit(&mut out, RulesEdit::SetNetClassMinLength { class, length });
    }
    let max = length_text(view, net_class.maximum_trace_length(), false);
    if let Some(typed) = text_cell(ui, id("max"), &max, 70.0) {
        let length = length_edit_value(view, &typed);
        edit(&mut out, RulesEdit::SetNetClassMaxLength { class, length });
    }
    // IGNORED_BY_AUTOROUTER
    let mut ignored = net_class.is_ignored_by_autorouter;
    if ui.checkbox(&mut ignored, "").changed() {
        edit(
            &mut out,
            RulesEdit::SetNetClassIgnoredByAutorouter {
                class,
                value: ignored,
            },
        );
    }
    out
}

fn open_layer_rules(board: &BasicBoard, class: NetClassId) -> LayerRules {
    let net_class = board.rules.net_classes.get(class);
    LayerRules {
        class,
        layers: board
            .layer_structure
            .layers
            .iter()
            .enumerate()
            .filter(|(_, l)| l.is_signal)
            .map(|(i, l)| {
                (
                    i,
                    l.name.clone(),
                    net_class.is_active_routing_layer(i as i32),
                )
            })
            .collect(),
    }
}

/// The buttons under the table.
fn buttons(
    ui: &mut egui::Ui,
    state: &mut State,
    view: &RuleView<'_>,
    ids: &[NetClassId],
) -> Vec<Command> {
    let mut out = Vec::new();
    let board = view.board;
    ui.horizontal(|ui| {
        if ui
            .button(text::NC_ADD)
            .on_hover_text(text::NC_ADD_TIP)
            .clicked()
        {
            out.push(Command::EditRules(RulesEdit::AddNetClass));
        }
        if ui
            .button(text::NC_REMOVE)
            .on_hover_text(text::NC_REMOVE_TIP)
            .clicked()
        {
            out.extend(remove_selected(state, ids));
        }
        if ui
            .button(text::NC_ASSIGN)
            .on_hover_text(text::NC_ASSIGN_TIP)
            .clicked()
        {
            out.push(Command::ShowRulesDialog(RulesDialog::AssignNetClass));
        }
        let has_choice = !state.selected.is_empty();
        if ui
            .add_enabled(has_choice, egui::Button::new(text::NC_SELECT))
            .on_hover_text(text::NC_SELECT_TIP)
            .clicked()
        {
            out.push(Command::Select(items_of_classes(board, &state.selected)));
            out.push(Command::ZoomSelection);
        }
        if ui
            .add_enabled(has_choice, egui::Button::new(text::NC_SHOW_NETS))
            .on_hover_text(text::NC_SHOW_NETS_TIP)
            .clicked()
        {
            state.contained = Some(state.selected.iter().copied().collect());
        }
        if ui
            .add_enabled(has_choice, egui::Button::new(text::NC_FILTER_INCOMPLETES))
            .on_hover_text(text::NC_FILTER_INCOMPLETES_TIP)
            .clicked()
        {
            state.incompletes_filter = Some(nets_of_classes(board, &state.selected));
        }
        if state.incompletes_filter.is_some() && ui.button("Show all connections").clicked() {
            state.incompletes_filter = None;
        }
    });
    out
}

/// `RemoveNetClassListener`: nothing without a chosen row, the message for
/// the last class; otherwise the edit, which refuses a class a net uses.
fn remove_selected(state: &State, ids: &[NetClassId]) -> Vec<Command> {
    let selected_row = state
        .selected
        .iter()
        .next()
        .and_then(|c| ids.iter().position(|i| i == c))
        .map_or(-1, |p| p as i32);
    if !can_remove_net_class(ids.len(), selected_row) {
        if ids.len() <= 1 {
            return vec![Command::Notice(text::NC_DEFAULT_NOT_REMOVED.to_string())];
        }
        return Vec::new();
    }
    vec![Command::EditRules(RulesEdit::RemoveNetClass(
        ids[selected_row as usize],
    ))]
}

fn layer_rules_window(ctx: &egui::Context, state: &mut State) -> Vec<Command> {
    let mut out = Vec::new();
    let Some(rules) = state.layer_rules.as_mut() else {
        return out;
    };
    let mut done = false;
    let mut cancel = false;
    egui::Window::new(text::NC_LAYER_TITLE)
        .id(Id::new("net_class_layer_rules"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            let last = rules.layers.len().saturating_sub(1);
            ui.horizontal(|ui| {
                if ui.button(text::NC_ALL).clicked() {
                    rules.layers.iter_mut().for_each(|l| l.2 = true);
                }
                if ui.button(text::NC_OUTER).clicked() {
                    for (n, l) in rules.layers.iter_mut().enumerate() {
                        l.2 = n == 0 || n == last;
                    }
                }
                if ui.button(text::NC_INNER).clicked() {
                    let many = rules.layers.len() >= 2;
                    for (n, l) in rules.layers.iter_mut().enumerate() {
                        l.2 = !(many && (n == 0 || n == last));
                    }
                }
                if ui.button(text::NC_CLEAR).clicked() {
                    rules.layers.iter_mut().for_each(|l| l.2 = false);
                }
            });
            egui::Grid::new("net_class_layer_grid").show(ui, |ui| {
                for (n, (_, name, checked)) in rules.layers.iter_mut().enumerate() {
                    ui.checkbox(checked, name.as_str());
                    if n % 2 == 1 {
                        ui.end_row();
                    }
                }
            });
            ui.horizontal(|ui| {
                if ui.button(text::OK).clicked() {
                    done = true;
                }
                if ui.button(text::CANCEL).clicked() {
                    cancel = true;
                }
            });
        });
    if done {
        out.push(Command::EditRules(RulesEdit::SetNetClassLayers {
            class: rules.class,
            layers: rules.layers.iter().map(|(i, _, on)| (*i, *on)).collect(),
        }));
    }
    if done || cancel {
        state.layer_rules = None;
    }
    out
}

fn contained_nets_window(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) {
    let Some(classes) = &state.contained else {
        return;
    };
    let board = view.board;
    let frame = Frame::new(board);
    let set: BTreeSet<NetClassId> = classes.iter().copied().collect();
    let mut open = true;
    egui::Window::new(text::NC_CONTAINED_NETS)
        .id(Id::new("net_class_contained_nets"))
        .open(&mut open)
        .default_size([520.0, 260.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for net in nets_of_classes(board, &set) {
                    ui.label(crate::panels::nets::net_info(
                        board,
                        &frame,
                        view.units.display,
                        net,
                    ));
                }
            });
        });
    if !open {
        state.contained = None;
    }
}

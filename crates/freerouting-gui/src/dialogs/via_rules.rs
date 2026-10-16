//! Rules > Vias: Java's `WindowVia` and `WindowViaRule` (G06.5).
//!
//! The Via Rules window has three parts. *Via padstacks*: Info, Create (a
//! name, a start and an end layer and a circle radius per layer;
//! `AddPadstackListener`) and Remove (`RemovePadstackListener`, refused while
//! a via uses the padstack). *Vias*: Info and Edit (opens the Edit Vias
//! window, [`super::vias`]). *Via rules*: Info, Create, Edit and Remove. Edit
//! opens a rule editor (`WindowViaRule`) with Append, Remove, Move Up and
//! Move Down; **the order of a rule's vias is the routing preference order**,
//! so Move Up and Move Down send exactly the swap the user asked for
//! ([`RulesEdit::SwapViasInRule`], `ViaRule::swap`).

use eframe::egui::{self, Id};
use freerouting::geometry::jmath::round_to_i32;
use freerouting::library::PadstackId;
use freerouting::rules::{ViaInfoId, ViaRuleId};

use super::rules_edit::RulesEdit;
use super::{RuleView, RulesDialog, confirm_window, prompt_window};
use crate::commands::Command;
use crate::text;

/// The form of `AddPadstackListener`: name, layers and radii.
#[derive(Clone, Debug, PartialEq)]
pub struct PadstackForm {
    pub name: String,
    /// Board layer numbers of the first and the last layer.
    pub start: usize,
    pub end: usize,
    /// The default radius as typed, in the display unit.
    pub default_radius: String,
    /// The radius typed for each layer from `start`; `None` follows the
    /// default radius.
    pub radii: Vec<Option<String>>,
}

/// A rule editor window (`WindowViaRule`).
#[derive(Clone, Debug, PartialEq)]
pub struct RuleEditor {
    pub rule: ViaRuleId,
    pub selected: Option<ViaInfoId>,
    /// "Choose a via to add": the vias the rule lacks, and the chosen one.
    pub appending: Option<Option<ViaInfoId>>,
    pub removing: Option<ViaInfoId>,
}

#[derive(Default)]
pub struct State {
    pub open: bool,
    pub selected_rule: Option<ViaRuleId>,
    pub padstack_form: Option<PadstackForm>,
    /// The padstack chosen in the Remove dialog.
    pub removing_padstack: Option<Option<PadstackId>>,
    /// The name typed for a new via rule.
    pub new_rule_name: Option<String>,
    pub removing_rule: Option<ViaRuleId>,
    pub editors: Vec<RuleEditor>,
    /// An Info window: its title and its lines.
    pub info: Option<(String, Vec<String>)>,
}

/// The radius a layer gets: its own text or the default, a text that is not a
/// number counting as the default (`input instanceof Number`), and the default
/// being 100 design units when it is not a number either.
pub fn radius_value(own: Option<&str>, default_text: &str, view: &RuleView<'_>) -> f64 {
    let parse = |t: &str| super::input::parse_trace_width_value(super::Input::Text(t));
    let default = parse(default_text).unwrap_or_else(|| {
        freerouting::board::structure::Unit::scale(100.0, view.units.design, view.units.display)
    });
    own.and_then(parse).unwrap_or(default)
}

/// The edit for a filled form: a circle of each layer's radius in board units.
pub fn padstack_edit(form: &PadstackForm, view: &RuleView<'_>) -> RulesEdit {
    let radii = (form.start..=form.end)
        .map(|layer| {
            let own = form
                .radii
                .get(layer - form.start)
                .and_then(|r| r.as_deref());
            let radius = radius_value(own, &form.default_radius, view);
            round_to_i32(view.units.to_board(radius))
        })
        .collect();
    RulesEdit::CreateViaPadstack {
        name: form.name.clone(),
        first_layer: form.start,
        radii,
    }
}

/// A new form for a board, with Java's defaults: the first layer to the last.
fn new_form(view: &RuleView<'_>) -> Option<PadstackForm> {
    let layers = view.board.layer_structure.count();
    if layers <= 1 {
        return None;
    }
    let default =
        freerouting::board::structure::Unit::scale(100.0, view.units.design, view.units.display);
    Some(PadstackForm {
        name: String::new(),
        start: 0,
        end: layers - 1,
        default_radius: super::route_settings::plain_number(default, 4),
        radii: vec![None; layers],
    })
}

pub fn show(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    // Rules that were removed leave the choice and the editors.
    let rules: Vec<ViaRuleId> = view
        .board
        .rules
        .via_rules
        .iter()
        .map(|(id, _)| id)
        .collect();
    if state.selected_rule.is_some_and(|r| !rules.contains(&r)) {
        state.selected_rule = None;
    }
    state.editors.retain(|e| rules.contains(&e.rule));
    if state.open {
        let mut open = true;
        egui::Window::new(text::VIA_TITLE)
            .id(Id::new(("rules", RulesDialog::ViaRules)))
            .open(&mut open)
            .default_size([420.0, 420.0])
            .show(ctx, |ui| out.extend(main_ui(ui, state, view, &rules)));
        state.open = open;
    }
    out.extend(padstack_form_window(ctx, state, view));
    out.extend(remove_padstack_window(ctx, state, view));
    out.extend(new_rule_window(ctx, state));
    out.extend(remove_rule_window(ctx, state, view));
    out.extend(rule_editors(ctx, state, view));
    info_window(ctx, state);
    out
}

fn main_ui(
    ui: &mut egui::Ui,
    state: &mut State,
    view: &RuleView<'_>,
    rules: &[ViaRuleId],
) -> Vec<Command> {
    let mut out = Vec::new();
    let board = view.board;
    ui.label(text::VIA_PADSTACKS);
    ui.horizontal(|ui| {
        if ui
            .button(text::VIA_INFO)
            .on_hover_text(text::VIA_INFO_TIP_PADSTACKS)
            .clicked()
        {
            state.info = Some((text::VIA_PADSTACKS.to_string(), padstack_lines(view)));
        }
        if ui
            .button(text::VIA_CREATE)
            .on_hover_text(text::VIA_CREATE_TIP_PADSTACK)
            .clicked()
        {
            state.padstack_form = new_form(view);
        }
        if ui
            .button(text::VIA_REMOVE)
            .on_hover_text(text::VIA_REMOVE_TIP_PADSTACK)
            .clicked()
            && board.library.via_padstack_count() > 0
        {
            state.removing_padstack = Some(board.library.via_padstack_ids().first().copied());
        }
    });
    ui.separator();
    ui.label(text::VIA_VIAS);
    ui.horizontal(|ui| {
        if ui
            .button(text::VIA_INFO)
            .on_hover_text(text::VIA_INFO_TIP_VIAS)
            .clicked()
        {
            state.info = Some((text::VIA_VIAS.to_string(), via_lines(view)));
        }
        if ui
            .button(text::VIA_EDIT)
            .on_hover_text(text::VIA_EDIT_TIP_VIAS)
            .clicked()
        {
            out.push(Command::ShowRulesDialog(RulesDialog::EditVias));
        }
    });
    ui.separator();
    ui.label(text::VIA_RULES);
    egui::ScrollArea::vertical()
        .max_height(140.0)
        .show(ui, |ui| {
            for &rule in rules {
                let name = &board.rules.via_rules.get(rule).name;
                if ui
                    .selectable_label(state.selected_rule == Some(rule), name)
                    .clicked()
                {
                    state.selected_rule = Some(rule);
                }
            }
        });
    ui.horizontal(|ui| {
        let chosen = state.selected_rule;
        if ui
            .add_enabled(chosen.is_some(), egui::Button::new(text::VIA_INFO))
            .on_hover_text(text::VIA_INFO_TIP_RULE)
            .clicked()
            && let Some(rule) = chosen
        {
            state.info = Some((text::VIA_SELECTED_RULE.to_string(), rule_lines(view, rule)));
        }
        if ui
            .button(text::VIA_CREATE)
            .on_hover_text(text::VIA_CREATE_TIP_RULE)
            .clicked()
        {
            state.new_rule_name = Some(String::new());
        }
        if ui
            .add_enabled(chosen.is_some(), egui::Button::new(text::VIA_EDIT))
            .on_hover_text(text::VIA_EDIT_TIP_RULE)
            .clicked()
            && let Some(rule) = chosen
            && !state.editors.iter().any(|e| e.rule == rule)
        {
            state.editors.push(RuleEditor {
                rule,
                selected: None,
                appending: None,
                removing: None,
            });
        }
        if ui
            .add_enabled(chosen.is_some(), egui::Button::new(text::VIA_REMOVE))
            .on_hover_text(text::VIA_REMOVE_TIP_RULE)
            .clicked()
        {
            state.removing_rule = chosen;
        }
    });
    out
}

/// What `Padstack.printInfo` shows, as lines: the name and its layer range.
fn padstack_lines(view: &RuleView<'_>) -> Vec<String> {
    let board = view.board;
    (0..board.library.via_padstack_count())
        .filter_map(|n| board.library.get_via_padstack(n))
        .map(|p| {
            let layers: Vec<&str> = board
                .layer_structure
                .layers
                .iter()
                .enumerate()
                .filter(|(i, _)| p.get_shape(*i as i32).is_some())
                .map(|(_, l)| l.name.as_str())
                .collect();
            format!("{}: {}", p.name, layers.join(", "))
        })
        .collect()
}

/// What `ViaInfo.printInfo` shows, as lines.
fn via_lines(view: &RuleView<'_>) -> Vec<String> {
    let board = view.board;
    board
        .rules
        .via_infos
        .iter()
        .map(|(_, v)| via_line(view, v))
        .collect()
}

fn via_line(view: &RuleView<'_>, via: &freerouting::rules::ViaInfo) -> String {
    let board = view.board;
    let padstack = board
        .library
        .padstacks
        .get(via.padstack)
        .map(|p| p.name.as_str())
        .unwrap_or_default();
    let class = board
        .rules
        .clearance_matrix
        .get_name(via.clearance_class)
        .unwrap_or_default();
    format!(
        "{}: padstack {padstack}, clearance class {class}, attach SMD {}",
        via.name,
        if via.attach_smd_allowed { "yes" } else { "no" }
    )
}

fn rule_lines(view: &RuleView<'_>, rule: ViaRuleId) -> Vec<String> {
    let board = view.board;
    let rule = board.rules.via_rules.get(rule);
    let mut lines = vec![rule.name.clone()];
    lines.extend(
        rule.vias()
            .iter()
            .map(|&v| via_line(view, board.rules.via_infos.get(v))),
    );
    lines
}

fn info_window(ctx: &egui::Context, state: &mut State) {
    let Some((title, lines)) = &state.info else {
        return;
    };
    let mut open = true;
    egui::Window::new(title)
        .id(Id::new("via_info_window"))
        .open(&mut open)
        .default_size([480.0, 200.0])
        .show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                for line in lines {
                    ui.label(line);
                }
            });
        });
    if !open {
        state.info = None;
    }
}

/// `AddPadstackListener` as one form: the name (refused while it exists), the
/// start and end layer, the default radius and a radius for each layer.
fn padstack_form_window(
    ctx: &egui::Context,
    state: &mut State,
    view: &RuleView<'_>,
) -> Vec<Command> {
    let Some(form) = state.padstack_form.as_mut() else {
        return Vec::new();
    };
    let board = view.board;
    let layers = &board.layer_structure.layers;
    let mut done = false;
    let mut cancel = false;
    let exists = board.library.padstacks.get_by_name(&form.name).is_some();
    egui::Window::new(text::VIA_CREATE_TIP_PADSTACK)
        .id(Id::new("via_padstack_form"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(text::VIA_PROMPT_PADSTACK_NAME);
            ui.add(egui::TextEdit::singleline(&mut form.name).desired_width(220.0));
            if exists {
                ui.colored_label(egui::Color32::LIGHT_RED, text::VIA_NAME_EXISTS);
            }
            if layers.len() > 2 {
                ui.label(text::VIA_SELECT_START_LAYER);
                egui::ComboBox::from_id_salt("padstack_start")
                    .selected_text(&layers[form.start].name)
                    .show_ui(ui, |ui| {
                        for (i, layer) in layers.iter().enumerate().take(layers.len() - 1) {
                            if ui.selectable_label(form.start == i, &layer.name).clicked() {
                                form.start = i;
                            }
                        }
                    });
                if form.end <= form.start {
                    form.end = layers.len() - 1;
                }
                ui.label(text::VIA_SELECT_END_LAYER);
                egui::ComboBox::from_id_salt("padstack_end")
                    .selected_text(&layers[form.end].name)
                    .show_ui(ui, |ui| {
                        for (i, layer) in layers.iter().enumerate().skip(form.start + 1) {
                            if ui.selectable_label(form.end == i, &layer.name).clicked() {
                                form.end = i;
                            }
                        }
                    });
            }
            ui.label(text::VIA_PROMPT_RADIUS);
            ui.add(egui::TextEdit::singleline(&mut form.default_radius).desired_width(90.0));
            ui.label(text::VIA_ADJUST_CIRCLES);
            for (layer, slot) in layers[form.start..=form.end]
                .iter()
                .zip(form.radii.iter_mut())
            {
                ui.horizontal(|ui| {
                    ui.label(text::fill(text::VIA_RADIUS_ON_LAYER, &[&layer.name]));
                    let mut shown = slot.clone().unwrap_or_else(|| form.default_radius.clone());
                    if ui
                        .add(egui::TextEdit::singleline(&mut shown).desired_width(90.0))
                        .changed()
                    {
                        *slot = Some(shown);
                    }
                });
            }
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(
                        !exists && !form.name.is_empty(),
                        egui::Button::new(text::OK),
                    )
                    .clicked()
                {
                    done = true;
                }
                if ui.button(text::CANCEL).clicked() {
                    cancel = true;
                }
            });
        });
    let mut out = Vec::new();
    if done {
        out.push(Command::EditRules(padstack_edit(form, view)));
    }
    if done || cancel {
        state.padstack_form = None;
    }
    out
}

/// `RemovePadstackListener`: a choice of the via padstacks.
fn remove_padstack_window(
    ctx: &egui::Context,
    state: &mut State,
    view: &RuleView<'_>,
) -> Vec<Command> {
    let Some(choice) = state.removing_padstack.as_mut() else {
        return Vec::new();
    };
    let board = view.board;
    let mut done = false;
    let mut cancel = false;
    egui::Window::new(text::VIA_REMOVE_PADSTACK)
        .id(Id::new("via_remove_padstack"))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(text::VIA_CHOOSE_TO_REMOVE);
            let shown = choice
                .and_then(|id| board.library.padstacks.get(id))
                .map_or("", |p| p.name.as_str());
            let combo = egui::ComboBox::from_id_salt("remove_padstack_choice")
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for &id in board.library.via_padstack_ids() {
                        if let Some(p) = board.library.padstacks.get(id)
                            && ui.selectable_label(*choice == Some(id), &p.name).clicked()
                        {
                            *choice = Some(id);
                        }
                    }
                });
            super::name_drop_down(ui, &combo.response, shown);
            ui.horizontal(|ui| {
                if ui.button(text::OK).clicked() {
                    done = true;
                }
                if ui.button(text::CANCEL).clicked() {
                    cancel = true;
                }
            });
        });
    let mut out = Vec::new();
    if done && let Some(id) = *choice {
        out.push(Command::EditRules(RulesEdit::RemoveViaPadstack(id)));
    }
    if done || cancel {
        state.removing_padstack = None;
    }
    out
}

/// `AddViaRuleListener`: the name is trimmed and an empty one does nothing.
fn new_rule_window(ctx: &egui::Context, state: &mut State) -> Vec<Command> {
    let Some(buffer) = state.new_rule_name.as_mut() else {
        return Vec::new();
    };
    let Some(answer) = prompt_window(
        ctx,
        "via_new_rule",
        text::VIA_RULES,
        text::VIA_PROMPT_RULE_NAME,
        buffer,
    ) else {
        return Vec::new();
    };
    state.new_rule_name = None;
    match answer {
        Some(name) if !name.trim().is_empty() => vec![Command::EditRules(RulesEdit::AddViaRule {
            name: name.trim().to_string(),
        })],
        _ => Vec::new(),
    }
}

fn remove_rule_window(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let Some(rule) = state.removing_rule else {
        return Vec::new();
    };
    let name = view.board.rules.via_rules.get(rule).name.clone();
    let question = text::fill(text::VIA_REMOVE_RULE_CONFIRM, &[&name]);
    match confirm_window(
        ctx,
        "via_remove_rule",
        text::VIA_RULES,
        &question,
        text::YES,
        text::NO,
    ) {
        Some(yes) => {
            state.removing_rule = None;
            if yes {
                vec![Command::EditRules(RulesEdit::RemoveViaRule(rule))]
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    }
}

/// The vias of `rule` and the position of `via` in them.
fn position(view: &RuleView<'_>, rule: ViaRuleId, via: ViaInfoId) -> Option<usize> {
    view.board
        .rules
        .via_rules
        .get(rule)
        .vias()
        .iter()
        .position(|&v| v == via)
}

/// Move Up (`MoveUpListener`): swaps the chosen via with the one before it.
/// Move Down (`MoveDownListener`): with the one after it.
pub fn move_edit(
    view: &RuleView<'_>,
    rule: ViaRuleId,
    via: ViaInfoId,
    up: bool,
) -> Option<RulesEdit> {
    let vias = view.board.rules.via_rules.get(rule).vias();
    let at = position(view, rule, via)?;
    let other = if up {
        at.checked_sub(1)?
    } else {
        (at + 1 < vias.len()).then_some(at + 1)?
    };
    let (first, second) = if up {
        (vias[other], vias[at])
    } else {
        (vias[at], vias[other])
    };
    Some(RulesEdit::SwapViasInRule {
        rule,
        first,
        second,
    })
}

fn rule_editors(ctx: &egui::Context, state: &mut State, view: &RuleView<'_>) -> Vec<Command> {
    let mut out = Vec::new();
    let board = view.board;
    let mut closed = Vec::new();
    for (n, editor) in state.editors.iter_mut().enumerate() {
        let rule = editor.rule;
        let rule_name = board.rules.via_rules.get(rule).name.clone();
        let vias: Vec<ViaInfoId> = board.rules.via_rules.get(rule).vias().to_vec();
        if editor.selected.is_some_and(|s| !vias.contains(&s)) {
            editor.selected = None;
        }
        let mut open = true;
        egui::Window::new(format!("{} {rule_name}", text::VR_TITLE))
            .id(Id::new(("via_rule_editor", rule)))
            .open(&mut open)
            .default_size([300.0, 250.0])
            .show(ctx, |ui| {
                for &via in &vias {
                    let name = &board.rules.via_infos.get(via).name;
                    if ui
                        .selectable_label(editor.selected == Some(via), name)
                        .clicked()
                    {
                        editor.selected = Some(via);
                    }
                }
                ui.separator();
                ui.horizontal(|ui| {
                    if ui
                        .button(text::VR_APPEND)
                        .on_hover_text(text::VR_APPEND_TIP)
                        .clicked()
                    {
                        let free = board
                            .rules
                            .via_infos
                            .iter()
                            .any(|(v, _)| !vias.contains(&v));
                        if free {
                            editor.appending = Some(None);
                        }
                    }
                    if ui
                        .add_enabled(
                            editor.selected.is_some(),
                            egui::Button::new(text::VR_REMOVE),
                        )
                        .on_hover_text(text::VR_REMOVE_TIP)
                        .clicked()
                    {
                        editor.removing = editor.selected;
                    }
                    for (label, tip, up) in [
                        (text::VR_MOVE_UP, text::VR_MOVE_UP_TIP, true),
                        (text::VR_MOVE_DOWN, text::VR_MOVE_DOWN_TIP, false),
                    ] {
                        if ui.button(label).on_hover_text(tip).clicked()
                            && let Some(via) = editor.selected
                            && let Some(edit) = move_edit(view, rule, via, up)
                        {
                            out.push(Command::EditRules(edit));
                        }
                    }
                });
            });
        out.extend(append_window(ctx, editor, view, &vias));
        out.extend(remove_via_window(ctx, editor, view));
        if !open {
            closed.push(n);
        }
    }
    for n in closed.into_iter().rev() {
        state.editors.remove(n);
    }
    out
}

/// `AppendListener`: a choice among the vias the rule lacks.
fn append_window(
    ctx: &egui::Context,
    editor: &mut RuleEditor,
    view: &RuleView<'_>,
    vias: &[ViaInfoId],
) -> Vec<Command> {
    let Some(choice) = editor.appending.as_mut() else {
        return Vec::new();
    };
    let board = view.board;
    let candidates: Vec<ViaInfoId> = board
        .rules
        .via_infos
        .iter()
        .map(|(v, _)| v)
        .filter(|v| !vias.contains(v))
        .collect();
    if choice.is_none() {
        *choice = candidates.first().copied();
    }
    let mut done = false;
    let mut cancel = false;
    egui::Window::new(text::VR_APPEND_TITLE)
        .id(Id::new(("via_rule_append", editor.rule)))
        .collapsible(false)
        .resizable(false)
        .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
        .show(ctx, |ui| {
            ui.label(text::VR_CHOOSE_TO_APPEND);
            let shown = choice.map_or("", |v| board.rules.via_infos.get(v).name.as_str());
            let combo = egui::ComboBox::from_id_salt(("append_choice", editor.rule))
                .selected_text(shown)
                .show_ui(ui, |ui| {
                    for &v in &candidates {
                        if ui
                            .selectable_label(
                                *choice == Some(v),
                                &board.rules.via_infos.get(v).name,
                            )
                            .clicked()
                        {
                            *choice = Some(v);
                        }
                    }
                });
            super::name_drop_down(ui, &combo.response, shown);
            ui.horizontal(|ui| {
                if ui.button(text::OK).clicked() {
                    done = true;
                }
                if ui.button(text::CANCEL).clicked() {
                    cancel = true;
                }
            });
        });
    let mut out = Vec::new();
    if done && let Some(via) = *choice {
        out.push(Command::EditRules(RulesEdit::AppendViaToRule {
            rule: editor.rule,
            via,
        }));
    }
    if done || cancel {
        editor.appending = None;
    }
    out
}

fn remove_via_window(
    ctx: &egui::Context,
    editor: &mut RuleEditor,
    view: &RuleView<'_>,
) -> Vec<Command> {
    let Some(via) = editor.removing else {
        return Vec::new();
    };
    let board = view.board;
    let question = text::fill(
        text::VR_REMOVE_CONFIRM,
        &[
            &board.rules.via_infos.get(via).name,
            &board.rules.via_rules.get(editor.rule).name,
        ],
    );
    match confirm_window(
        ctx,
        "via_rule_remove",
        text::VR_TITLE,
        &question,
        text::YES,
        text::NO,
    ) {
        Some(yes) => {
            editor.removing = None;
            if yes {
                vec![Command::EditRules(RulesEdit::RemoveViaFromRule {
                    rule: editor.rule,
                    via,
                })]
            } else {
                Vec::new()
            }
        }
        None => Vec::new(),
    }
}

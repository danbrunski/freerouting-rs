//! `RulesEdit`: the one message the rules dialogs send for a user change
//! (`Command::EditRules`), and [`apply`], which runs it on the engine board
//! through B08's rules-edit facade (`freerouting::rules::edit`).
//!
//! The dialogs only read the board and build these values; every change to
//! board rules, library or search trees happens in [`apply`]. Each variant
//! names the Java dialog action it replays. Refusals return the English text
//! of the Java bundle as the outcome's message (Java shows them in the status
//! line).

use freerouting::board::RoutingBoard;
use freerouting::board::items::ItemId;
use freerouting::board::structure::{AngleRestriction, FixedState};
use freerouting::geometry::{ConvexShape, IntPoint, shapes::Circle};
use freerouting::library::PadstackId;
use freerouting::rules::edit::{ClearanceLayer, NetClassRemoveError, ViaEditError};
use freerouting::rules::{NetClassId, ViaInfoId, ViaRuleId};

use crate::text;

/// One user change to the board rules.
#[derive(Clone, Debug, PartialEq)]
pub enum RulesEdit {
    // WindowNetClasses
    /// `AddNetClassListener`: `BoardRules.appendNetClass`.
    AddNetClass,
    /// `RemoveNetClassListener`.
    RemoveNetClass(NetClassId),
    RenameNetClass {
        class: NetClassId,
        name: String,
    },
    SetNetClassViaRule {
        class: NetClassId,
        rule: String,
    },
    SetNetClassClearanceClass {
        class: NetClassId,
        name: String,
    },
    /// `applyTraceWidth`, already converted: the half width in board units.
    SetNetClassHalfWidth {
        class: NetClassId,
        half_width: i32,
    },
    /// `applyShoveFixedSelection`.
    SetNetClassShoveFixed {
        class: NetClassId,
        value: bool,
    },
    SetNetClassIgnoreCyclesWithAreas {
        class: NetClassId,
        value: bool,
    },
    /// Minimum trace length in board units (`Math.round(userToBoard(..))`).
    SetNetClassMinLength {
        class: NetClassId,
        length: f64,
    },
    SetNetClassMaxLength {
        class: NetClassId,
        length: f64,
    },
    /// `applyAutorouterIgnoreSelection`.
    SetNetClassIgnoredByAutorouter {
        class: NetClassId,
        value: bool,
    },
    /// `LayerRulesDialog.save`: `(board layer, active)` for each signal layer.
    SetNetClassLayers {
        class: NetClassId,
        layers: Vec<(usize, bool)>,
    },
    // WindowAssignNetClass
    /// `Net.setClass`.
    SetNetClass {
        net: i32,
        class: NetClassId,
    },
    // WindowClearanceMatrix
    /// `applyClearanceValue`: symmetric, in board units.
    SetClearance {
        row: usize,
        column: usize,
        layer: ClearanceLayer,
        board_value: i32,
    },
    /// `addClass`.
    AddClearanceClass {
        name: String,
    },
    /// One confirmed step of `pruneClearanceMatrix`: the items of class
    /// `from` move to `to`, class `from` is removed.
    MergeClearanceClass {
        from: i32,
        to: i32,
    },
    // WindowEditVias
    /// `AddViaListener`.
    AddViaInfo,
    RemoveViaInfo(ViaInfoId),
    RenameViaInfo {
        via: ViaInfoId,
        name: String,
    },
    SetViaInfoPadstack {
        via: ViaInfoId,
        padstack: String,
    },
    SetViaInfoClearanceClass {
        via: ViaInfoId,
        name: String,
    },
    SetViaInfoAttachSmd {
        via: ViaInfoId,
        allowed: bool,
    },
    // WindowVia
    /// `AddPadstackListener`: circles on `first_layer..=last_layer`, one
    /// radius in board units per layer (0 or less: no shape on that layer).
    CreateViaPadstack {
        name: String,
        first_layer: usize,
        radii: Vec<i32>,
    },
    RemoveViaPadstack(PadstackId),
    AddViaRule {
        name: String,
    },
    RemoveViaRule(ViaRuleId),
    // WindowViaRule
    AppendViaToRule {
        rule: ViaRuleId,
        via: ViaInfoId,
    },
    RemoveViaFromRule {
        rule: ViaRuleId,
        via: ViaInfoId,
    },
    /// Move up and down: swaps two neighbours of the rule's via list.
    SwapViasInRule {
        rule: ViaRuleId,
        first: ViaInfoId,
        second: ViaInfoId,
    },
    // WindowRouteParameter
    /// `setCurrentSnapAngle`.
    SetSnapAngle(AngleRestriction),
    /// `setIgnoreConduction`.
    SetIgnoreConduction(bool),
    /// `setPinEdgeToTurnDist`, in board units.
    SetPinEdgeToTurnDist(f64),
    /// `setClearanceCompensation`.
    SetClearanceCompensation(bool),
    /// `OutLineKeepoutListener`.
    SetKeepoutOutsideOutline(bool),
}

impl RulesEdit {
    /// True if the edit changes board items, so the scene is built again. The
    /// other edits change rules only. Neither kind is part of the board
    /// history (`undo`), as in Java: the history holds items and components,
    /// and these edits change an item in place without a saved version.
    pub fn touches_items(&self) -> bool {
        matches!(
            self,
            RulesEdit::SetIgnoreConduction(_)
                | RulesEdit::SetPinEdgeToTurnDist(_)
                | RulesEdit::SetKeepoutOutsideOutline(_)
        )
    }
}

/// What [`apply`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditOutcome {
    /// False if the edit was refused or changed nothing.
    pub changed: bool,
    /// The status line text (Java's `setStatusMessage`), if the edit has one.
    pub message: Option<String>,
}

impl EditOutcome {
    fn done() -> Self {
        EditOutcome {
            changed: true,
            message: None,
        }
    }

    fn said(message: String) -> Self {
        EditOutcome {
            changed: true,
            message: Some(message),
        }
    }

    fn refused(message: String) -> Self {
        EditOutcome {
            changed: false,
            message: Some(message),
        }
    }

    fn nothing() -> Self {
        EditOutcome::default()
    }

    fn when(changed: bool) -> Self {
        EditOutcome {
            changed,
            message: None,
        }
    }
}

/// Runs `edit` on `board`.
pub fn apply(board: &mut RoutingBoard, edit: &RulesEdit) -> EditOutcome {
    match edit {
        RulesEdit::SetIgnoreConduction(value) => {
            // Java `changePlaneAsObstacle(!value)`.
            board.change_plane_as_obstacle(!*value);
            EditOutcome::done()
        }
        _ => apply_basic(board, edit),
    }
}

fn apply_basic(board: &mut RoutingBoard, edit: &RulesEdit) -> EditOutcome {
    let b: &mut freerouting::board::BasicBoard = board;
    match edit {
        RulesEdit::AddNetClass => {
            b.add_net_class();
            EditOutcome::done()
        }
        RulesEdit::RemoveNetClass(class) => remove_net_class(b, *class),
        RulesEdit::RenameNetClass { class, name } => {
            EditOutcome::when(b.rename_net_class(*class, name))
        }
        RulesEdit::SetNetClassViaRule { class, rule } => {
            EditOutcome::when(b.set_net_class_via_rule(*class, rule))
        }
        RulesEdit::SetNetClassClearanceClass { class, name } => {
            EditOutcome::when(b.set_net_class_clearance_class(*class, name))
        }
        RulesEdit::SetNetClassHalfWidth { class, half_width } => {
            EditOutcome::when(b.set_net_class_trace_half_width(*class, *half_width))
        }
        RulesEdit::SetNetClassShoveFixed { class, value } => {
            b.set_net_class_shove_fixed(*class, *value);
            EditOutcome::done()
        }
        RulesEdit::SetNetClassIgnoreCyclesWithAreas { class, value } => {
            b.set_net_class_ignore_cycles_with_areas(*class, *value);
            EditOutcome::done()
        }
        RulesEdit::SetNetClassMinLength { class, length } => {
            b.set_net_class_minimum_trace_length(*class, *length);
            EditOutcome::done()
        }
        RulesEdit::SetNetClassMaxLength { class, length } => {
            b.set_net_class_maximum_trace_length(*class, *length);
            EditOutcome::done()
        }
        RulesEdit::SetNetClassIgnoredByAutorouter { class, value } => {
            b.set_net_class_ignored_by_autorouter(*class, *value);
            EditOutcome::done()
        }
        RulesEdit::SetNetClassLayers { class, layers } => {
            let net_class = b.rules.net_classes.get_mut(*class);
            for &(layer, active) in layers {
                net_class.set_active_routing_layer(layer as i32, active);
            }
            EditOutcome::done()
        }
        RulesEdit::SetNetClass { net, class } => {
            EditOutcome::when(b.assign_net_class(*net, *class))
        }
        RulesEdit::SetClearance {
            row,
            column,
            layer,
            board_value,
        } => {
            b.apply_clearance_value(*row, *column, *layer, *board_value);
            EditOutcome::done()
        }
        RulesEdit::AddClearanceClass { name } => EditOutcome::when(b.add_clearance_class(name)),
        RulesEdit::MergeClearanceClass { from, to } => merge_clearance_class(b, *from, *to),
        RulesEdit::AddViaInfo => add_via_info(b),
        RulesEdit::RemoveViaInfo(via) => remove_via_info(b, *via),
        RulesEdit::RenameViaInfo { via, name } => {
            EditOutcome::when(b.rename_via_info(*via, name).is_ok())
        }
        RulesEdit::SetViaInfoPadstack { via, padstack } => {
            EditOutcome::when(b.set_via_info_padstack(*via, padstack))
        }
        RulesEdit::SetViaInfoClearanceClass { via, name } => {
            EditOutcome::when(b.set_via_info_clearance_class(*via, name))
        }
        RulesEdit::SetViaInfoAttachSmd { via, allowed } => {
            b.set_via_info_attach_smd(*via, *allowed);
            EditOutcome::done()
        }
        RulesEdit::CreateViaPadstack {
            name,
            first_layer,
            radii,
        } => create_via_padstack(b, name, *first_layer, radii),
        RulesEdit::RemoveViaPadstack(id) => match b.remove_via_padstack(*id) {
            Ok(()) => EditOutcome::done(),
            Err(ViaEditError::PadstackInUse(via)) => {
                EditOutcome::refused(text::fill(text::VIA_PADSTACK_IN_USE, &[&via]))
            }
            Err(_) => EditOutcome::nothing(),
        },
        RulesEdit::AddViaRule { name } => EditOutcome::when(b.add_via_rule(name).is_ok()),
        RulesEdit::RemoveViaRule(rule) => EditOutcome::when(b.remove_via_rule(*rule)),
        RulesEdit::AppendViaToRule { rule, via } => {
            b.append_via_to_rule(*rule, *via);
            EditOutcome::done()
        }
        RulesEdit::RemoveViaFromRule { rule, via } => {
            EditOutcome::when(b.remove_via_from_rule(*rule, *via))
        }
        RulesEdit::SwapViasInRule {
            rule,
            first,
            second,
        } => EditOutcome::when(b.swap_vias_in_rule(*rule, *first, *second)),
        RulesEdit::SetSnapAngle(angle) => {
            b.rules.set_trace_angle_restriction(*angle);
            EditOutcome::done()
        }
        RulesEdit::SetPinEdgeToTurnDist(value) => {
            set_pin_edge_to_turn_dist(b, *value);
            EditOutcome::done()
        }
        RulesEdit::SetClearanceCompensation(value) => {
            b.set_clearance_compensation_used(*value);
            EditOutcome::done()
        }
        RulesEdit::SetKeepoutOutsideOutline(value) => {
            // Java: only if the board has an outline.
            b.generate_keepout_outside_outline(*value);
            EditOutcome::done()
        }
        RulesEdit::SetIgnoreConduction(_) => EditOutcome::nothing(),
    }
}

/// The first net, in ascending net number, whose class is `class`. Java's
/// loop (`for i = 1; i < maxNetNumber`) stops one net short and so lets the
/// last net's class be removed; all nets are checked here
/// (`docs/DIVERGENCES.md`, "Rules dialogs").
pub fn first_net_using(
    board: &freerouting::board::BasicBoard,
    class: NetClassId,
) -> Option<String> {
    (1..=board.rules.nets.max_net_number())
        .filter_map(|n| board.rules.nets.get(n))
        .find(|net| net.net_class() == class)
        .map(|net| net.name.clone())
}

fn remove_net_class(b: &mut freerouting::board::BasicBoard, class: NetClassId) -> EditOutcome {
    let name = b.rules.net_classes.get(class).name().to_string();
    if b.rules.net_classes.count() <= 1 {
        return EditOutcome::refused(text::NC_DEFAULT_NOT_REMOVED.to_string());
    }
    if let Some(net) = first_net_using(b, class) {
        return EditOutcome::refused(text::fill(text::NC_IN_USE, &[&net]));
    }
    match b.remove_net_class(class) {
        Ok(()) => EditOutcome::said(text::fill(text::NC_REMOVED, &[&name])),
        Err(NetClassRemoveError::LastClass) => {
            EditOutcome::refused(text::NC_DEFAULT_NOT_REMOVED.to_string())
        }
        Err(NetClassRemoveError::InUse(net)) => {
            EditOutcome::refused(text::fill(text::NC_IN_USE, &[&net]))
        }
        Err(NetClassRemoveError::NotFound) => EditOutcome::nothing(),
    }
}

/// `pruneClearanceMatrix`, one confirmed step: `changeClearanceClassIndex`,
/// `removeClearanceClass` (which stops the prune with a warning when it
/// fails), `clearanceClassRemoved`.
fn merge_clearance_class(
    b: &mut freerouting::board::BasicBoard,
    from: i32,
    to: i32,
) -> EditOutcome {
    b.change_clearance_class_index(from, to);
    if !b.remove_clearance_class(from) {
        // Java logs a warning and stops the prune.
        return EditOutcome::refused(
            "WindowClearanceMatrix.prune_clearance_matrix error removing clearance class".into(),
        );
    }
    b.clearance_class_removed(from);
    EditOutcome::done()
}

/// `AddViaListener` names the via `new_via_<n>` for the smallest free `n`
/// (`tm.getText("newVia")` finds `new_via=new_via_` through the snake case
/// fallback of Java's `TextManager`). B08's `add_via_info` names it
/// `newVia<n>`, so the new via is renamed here.
fn add_via_info(b: &mut freerouting::board::BasicBoard) -> EditOutcome {
    let Ok(id) = b.add_via_info() else {
        return EditOutcome::nothing();
    };
    let name = (1..)
        .map(|n| format!("{}{n}", text::EV_NEW_VIA))
        .find(|name| !b.rules.via_infos.name_exists(name))
        .expect("an unused number");
    // The rename can only fail on a name that exists, and this one doesn't.
    let _ = b.rename_via_info(id, &name);
    EditOutcome::done()
}

fn remove_via_info(b: &mut freerouting::board::BasicBoard, via: ViaInfoId) -> EditOutcome {
    let name = b.rules.via_infos.get(via).name.clone();
    match b.remove_via_info(via) {
        Ok(()) => EditOutcome::said(text::fill(text::EV_REMOVED, &[&name])),
        Err(ViaEditError::LastVia) => EditOutcome::refused(text::EV_LAST_NOT_REMOVED.to_string()),
        Err(ViaEditError::InRule(rule)) => {
            EditOutcome::refused(text::fill(text::EV_IN_RULE, &[&rule]))
        }
        Err(_) => EditOutcome::nothing(),
    }
}

/// `AddPadstackListener`'s last part: circles of the given radii on the
/// layers from `first_layer`, none when no radius is above 0.
fn create_via_padstack(
    b: &mut freerouting::board::BasicBoard,
    name: &str,
    first_layer: usize,
    radii: &[i32],
) -> EditOutcome {
    if b.library.padstacks.get_by_name(name).is_some() {
        return EditOutcome::refused(text::VIA_NAME_EXISTS.to_string());
    }
    let layer_count = b.layer_structure.count();
    let mut shapes: Vec<Option<ConvexShape>> = vec![None; layer_count];
    let mut shape_exists = false;
    for (i, &radius) in radii.iter().enumerate() {
        if radius > 0 && first_layer + i < layer_count {
            shapes[first_layer + i] =
                Some(ConvexShape::Circle(Circle::new(IntPoint::ZERO, radius)));
            shape_exists = true;
        }
    }
    if !shape_exists {
        return EditOutcome::nothing();
    }
    let id = b
        .library
        .padstacks
        .add(name, shapes, Vec::new(), true, true);
    EditOutcome::when(b.add_via_padstack(id))
}

/// `GuiBoardRoutingSettings.setPinEdgeToTurnDist` after the conversion to
/// board units: when the distance changes, the two-corner traces that touch
/// a pin with exit restrictions and are shove fixed become unfixed, then the
/// rule is set.
fn set_pin_edge_to_turn_dist(b: &mut freerouting::board::BasicBoard, value: f64) {
    if value != b.rules.pin_edge_to_turn_dist() {
        let mut unfix: Vec<ItemId> = Vec::new();
        for pin in b.pins() {
            if !b.has_trace_exit_restrictions(pin) {
                continue;
            }
            for contact in b.normal_contacts(pin) {
                let Some(item) = b.item(contact) else {
                    continue;
                };
                if item.as_trace().is_some_and(|t| t.corner_count() == 2)
                    && item.fixed_state() == FixedState::ShoveFixed
                {
                    unfix.push(contact);
                }
            }
        }
        for id in unfix {
            b.set_fixed_state(id, FixedState::Unfixed);
        }
    }
    b.rules.set_pin_edge_to_turn_dist(value);
}

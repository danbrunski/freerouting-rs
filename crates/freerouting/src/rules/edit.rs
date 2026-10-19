//! The rule dialogs' edits (B08.4): one function per dialog action, on the
//! board, replaying the calls Java's dialogs make so that the rules, the
//! items and the search trees end up as in Java.
//!
//! # The call lists (Java at `a773a70ec`, `gui/windows/routing/`)
//!
//! `WindowClearanceMatrix`:
//! - set a value (`ClearanceTableModel.setValueAt`, `:379-447`):
//!   [`BasicBoard::clearance_items_assigned`] is the dialog's check (items in
//!   both classes: the GUI asks for confirmation). Then
//!   `applyClearanceValue` (`:143-155`): `setValue(row, col, v)` and
//!   `setValue(col, row, v)` on all layers, `setInnerValue` on the inner
//!   layers, or `setValue(.., layer, v)` on one layer, and, only if items were
//!   assigned to both classes, `searchTreeManager.clearanceValueChanged()`
//!   ([`BasicBoard::apply_clearance_value`]);
//! - add a class (`addClass`, `:225-257`): `clearanceMatrix.appendClass(name)`,
//!   then `searchTreeManager.setClearanceCompensationUsed(false)` (compensation
//!   is only used with the classes `null` and `default`)
//!   ([`BasicBoard::add_clearance_class`]);
//! - prune (`pruneClearanceMatrix`, `:261-292`): for class `i` from the last
//!   down to 2 and class `j` from the last down to 0, `j != i`, the first `j`
//!   with `isEqual(i, j)` (the loop over `j` stops there, confirmed or not): after
//!   the confirmation `rules.changeClearanceClassIndex(i, j, items)`, then
//!   `rules.removeClearanceClass(i, items)` (on failure the whole prune stops),
//!   then `searchTreeManager.clearanceClassRemoved(i)`
//!   ([`BasicBoard::prune_clearance_matrix`]).
//!
//! `BoardRules.changeClearanceClassIndex` (`:266-293`) and
//! `removeClearanceClass` (`:298-345`) walk the items (highest id first) and
//! renumber them with `Item.setClearanceClassIndex` (`Item.java:1001-1008`),
//! then the net classes and via infos
//! ([`BasicBoard::change_clearance_class_index`],
//! [`BasicBoard::remove_clearance_class`]).
//!
//! **Java leaves the trees stale here, and so does this module.**
//! `setClearanceClassIndex` only changes the number; unlike
//! `changeClearanceClassIndex` (`Item.java:1010-1022`, the port's
//! `BasicBoard::change_clearance_class`) it neither clears the item's derived
//! data nor reinserts it into a compensated tree. After a prune the autoroute
//! trees therefore still hold the shapes computed with the old class numbers
//! until `clearanceClassRemoved` drops the trees of the removed class; the
//! default tree stays as it is. A fix would be a measured divergence, so it is
//! not made. The renumbering does not save the items for undo (Java mutates
//! the shared object), and it is not recorded as an autoroute change.
//!
//! `WindowNetClasses` (`:155-198`, `:381-420`, `:662-780`) only changes the net
//! class (no item, no tree): append (`BoardRules.appendNetClass`), remove (not the
//! last class; refused if a net `1..maxNetNumber-1` uses it: Java's loop
//! `for i = 1; i < maxNetNumber` skips the last net, kept), rename (refused if
//! the name exists), via rule by name, shove-fixed (`setShoveFixed(v)` and
//! `setPullTight(!v)`), ignore cycles with areas, minimum and maximum trace
//! length, ignored by the autorouter, trace clearance class by name, trace
//! width. The width and the lengths are board units here; the dialog converts
//! with `Math.round(userToBoard(..))`.
//!
//! `WindowAssignNetClass` (`:176`): `Net.setClass`.
//!
//! `WindowVia` (`:276-300`, `:360-462`, `:530-570`): add a via padstack
//! (`padstacks.add(name, shapes, true, true)` and `library.addViaPadstack`),
//! remove one (refused if a via info uses it; then `removeViaPadstack`), add
//! and remove a via rule. `WindowViaRule` (`:135-185`): `swap`, `appendVia`,
//! `removeVia`. `WindowEditVias` (`AddViaListener`, `RemoveViaListener`,
//! `setValueAt`): a new via info `newVia<n>` with the first via padstack, the
//! default net class's via clearance class and no SMD attach; remove one
//! (refused for the last, or if a via rule contains it); name (refused if it
//! exists), padstack by via padstack name, clearance class by name, attach SMD.
//! None of them touches an item or a tree. `WindowPadstacks` only lists.

use crate::board::basic_board::BasicBoard;
use crate::board::items::ItemId;
use crate::board::search::ShapeContext;
use crate::library::PadstackId;
use crate::rules::{ItemClass, NetClassId, ViaInfo, ViaInfoId, ViaRuleId};

/// The layer choice of the clearance matrix window.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClearanceLayer {
    /// `ComboBoxLayer.ALL_LAYER_INDEX`.
    All,
    /// `ComboBoxLayer.INNER_LAYER_INDEX`.
    Inner,
    /// One board layer.
    Layer(usize),
}

/// How [`BasicBoard::prune_clearance_matrix`] ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PruneOutcome {
    /// The names of the classes that were removed, in removal order.
    pub removed: Vec<String>,
    /// True if `removeClearanceClass` failed and the prune stopped there.
    pub failed: bool,
}

/// Why a net class could not be removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NetClassRemoveError {
    /// Only one net class is left (`canRemoveNetClass`).
    LastClass,
    /// A net uses the class; its name.
    InUse(String),
    NotFound,
}

/// Why a via padstack, via info or via rule edit was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ViaEditError {
    /// The padstack is used by the via info with this name.
    PadstackInUse(String),
    /// A via rule with this name contains the via info.
    InRule(String),
    /// The last via info cannot be removed.
    LastVia,
    /// The name exists already (or is empty where a name is needed).
    BadName,
    /// An argument does not name anything.
    NotFound,
}

impl BasicBoard {
    // ---------------------------------------------------------------
    // WindowClearanceMatrix
    // ---------------------------------------------------------------

    /// The dialog's check before setting a value (`setValueAt`, `:389-407`):
    /// true if some item is in clearance class `row` and some item in class
    /// `column`. Then the dialog asks for confirmation.
    pub fn clearance_items_assigned(&self, row: i32, column: i32) -> bool {
        let row_used = self.items().any(|i| i.clearance_class() == row);
        let column_used = self.items().any(|i| i.clearance_class() == column);
        row_used && column_used
    }

    /// `applyClearanceValue` for the classes `row` and `column` (symmetric),
    /// then `clearanceValueChanged` if items are assigned to both. Does
    /// nothing for a class out of range.
    pub fn apply_clearance_value(
        &mut self,
        row: usize,
        column: usize,
        layer: ClearanceLayer,
        board_value: i32,
    ) {
        let classes = self.rules.clearance_matrix.class_count();
        if row >= classes || column >= classes {
            return;
        }
        let items_assigned = self.clearance_items_assigned(row as i32, column as i32);
        let matrix = &mut self.rules.clearance_matrix;
        match layer {
            ClearanceLayer::All => {
                matrix.set_value(row, column, board_value);
                matrix.set_value(column, row, board_value);
            }
            ClearanceLayer::Inner => {
                matrix.set_inner_value(row, column, board_value);
                matrix.set_inner_value(column, row, board_value);
            }
            ClearanceLayer::Layer(l) => {
                if l >= matrix.layer_count() {
                    return;
                }
                matrix.set_value_on_layer(row, column, l, board_value);
                matrix.set_value_on_layer(column, row, l, board_value);
            }
        }
        if items_assigned {
            self.clearance_value_changed();
        }
    }

    /// `searchTreeManager.clearanceValueChanged()`: drops the compensated
    /// autoroute trees; with clearance compensation the default tree is
    /// rebuilt from the items.
    pub fn clearance_value_changed(&mut self) {
        let ctx = ShapeContext {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            communication: &self.communication,
            layer_count: self.layer_structure.count(),
            split_quirks: self.java_quirks.split(),
        };
        self.search_trees.clearance_value_changed(&self.items, &ctx);
    }

    /// `searchTreeManager.clearanceClassRemoved(no)`: drops the autoroute
    /// trees compensated for class `no` (not the default tree).
    pub fn clearance_class_removed(&mut self, class: i32) {
        self.search_trees.clearance_class_removed(class);
    }

    /// `addClass`: appends clearance class `name` (a copy of the default
    /// class's values) and turns clearance compensation off. False if a class
    /// with that name exists (any case; the dialog's own check compares
    /// case-sensitively, `appendClass` ignores case).
    pub fn add_clearance_class(&mut self, name: &str) -> bool {
        if !self.rules.clearance_matrix.append_class(name) {
            return false;
        }
        self.set_clearance_compensation_used(false);
        true
    }

    /// `pruneClearanceMatrix`. `confirm` gets the name of the class that is
    /// equal to another and returns whether to remove it.
    pub fn prune_clearance_matrix(
        &mut self,
        confirm: &mut dyn FnMut(&str) -> bool,
    ) -> PruneOutcome {
        let mut outcome = PruneOutcome {
            removed: Vec::new(),
            failed: false,
        };
        let mut i = self.rules.clearance_matrix.class_count() as i32 - 1;
        while i >= 2 {
            let mut j = self.rules.clearance_matrix.class_count() as i32 - 1;
            while j >= 0 {
                if i != j && self.rules.clearance_matrix.is_equal(i, j) {
                    let name = self
                        .rules
                        .clearance_matrix
                        .get_name(i)
                        .unwrap_or_default()
                        .to_string();
                    if confirm(&name) {
                        self.change_clearance_class_index(i, j);
                        if !self.remove_clearance_class(i) {
                            outcome.failed = true;
                            return outcome;
                        }
                        self.clearance_class_removed(i);
                        outcome.removed.push(name);
                    }
                    break;
                }
                j -= 1;
            }
            i -= 1;
        }
        outcome
    }

    /// `BoardRules.changeClearanceClassIndex(from, to, items)`: items with
    /// class `from` get class `to` (`Item.setClearanceClassIndex`: the trees
    /// are not told, see the module docs), then the net classes and via
    /// infos.
    pub fn change_clearance_class_index(&mut self, from: i32, to: i32) {
        for id in self.item_ids() {
            if self.item(id).is_some_and(|i| i.clearance_class() == from) {
                self.set_clearance_class_stale(id, to);
            }
        }
        self.rules.change_clearance_class_index(from, to);
    }

    /// `BoardRules.removeClearanceClass(index, items)`: false if an item, a
    /// net class or a via info still uses the class. Otherwise the higher
    /// classes move down in the items (not in the trees), net classes and via
    /// infos, and the class leaves the matrix.
    pub fn remove_clearance_class(&mut self, index: i32) -> bool {
        if self.items().any(|i| i.clearance_class() == index) {
            return false;
        }
        if self.rules.clearance_class_used_by_rules(index) {
            return false;
        }
        for id in self.item_ids() {
            let class = self.item(id).map(|i| i.clearance_class());
            if let Some(class) = class.filter(|c| *c > index) {
                self.set_clearance_class_stale(id, class - 1);
            }
        }
        self.rules.remove_clearance_class_from_rules(index);
        true
    }

    /// `Item.setClearanceClassIndex` (`Item.java:1001-1008`): the number
    /// only. Out of range is logged and ignored.
    fn set_clearance_class_stale(&mut self, id: ItemId, class: i32) {
        if class < 0 || class >= self.rules.clearance_matrix.class_count() as i32 {
            self.log
                .push("Item.set_clearance_class_no: index out of range".into());
            return;
        }
        if let Some(item) = self.items.get_mut(&id) {
            item.clearance_class = class;
            self.note_changed(id);
        }
    }

    // ---------------------------------------------------------------
    // WindowNetClasses, WindowAssignNetClass
    // ---------------------------------------------------------------

    /// `BoardRules.appendNetClass`: a new class with the default class's via
    /// rule, width and clearance class.
    pub fn add_net_class(&mut self) -> NetClassId {
        self.rules.append_net_class()
    }

    /// `RemoveNetClassListener`: refuses to remove the last class or one a net
    /// uses.
    pub fn remove_net_class(&mut self, class: NetClassId) -> Result<(), NetClassRemoveError> {
        if self.rules.net_classes.count() <= 1 {
            return Err(NetClassRemoveError::LastClass);
        }
        // Java: `for (int i = 1; i < nets.maxNetNumber(); i++)`.
        for number in 1..self.rules.nets.max_net_number() {
            if let Some(net) = self.rules.nets.get(number)
                && net.net_class() == class
            {
                return Err(NetClassRemoveError::InUse(net.name.clone()));
            }
        }
        if self.rules.net_classes.remove(class) {
            Ok(())
        } else {
            Err(NetClassRemoveError::NotFound)
        }
    }

    /// Renames a net class; false if the name exists (the dialog returns
    /// silently).
    pub fn rename_net_class(&mut self, class: NetClassId, name: &str) -> bool {
        if self.rules.net_classes.get_by_name(name).is_some() {
            return false;
        }
        self.rules.net_classes.get_mut(class).set_name(name);
        true
    }

    /// Sets the via rule of a net class by name; false if there is none.
    pub fn set_net_class_via_rule(&mut self, class: NetClassId, via_rule_name: &str) -> bool {
        let Some(rule) = self.rules.via_rule_by_name(via_rule_name) else {
            return false;
        };
        self.rules
            .net_classes
            .get_mut(class)
            .set_via_rule(Some(rule));
        true
    }

    /// `applyShoveFixedSelection`: shove fixed, and pull tight the other way
    /// round.
    pub fn set_net_class_shove_fixed(&mut self, class: NetClassId, shove_fixed: bool) {
        let net_class = self.rules.net_classes.get_mut(class);
        net_class.set_shove_fixed(shove_fixed);
        net_class.set_pull_tight(!shove_fixed);
    }

    pub fn set_net_class_ignore_cycles_with_areas(&mut self, class: NetClassId, value: bool) {
        self.rules
            .net_classes
            .get_mut(class)
            .set_ignore_cycles_with_areas(value);
    }

    /// The minimum trace length of a net class, in board units.
    pub fn set_net_class_minimum_trace_length(&mut self, class: NetClassId, length: f64) {
        self.rules
            .net_classes
            .get_mut(class)
            .set_minimum_trace_length(length);
    }

    /// The maximum trace length of a net class, in board units.
    pub fn set_net_class_maximum_trace_length(&mut self, class: NetClassId, length: f64) {
        self.rules
            .net_classes
            .get_mut(class)
            .set_maximum_trace_length(length);
    }

    /// `applyAutorouterIgnoreSelection`.
    pub fn set_net_class_ignored_by_autorouter(&mut self, class: NetClassId, value: bool) {
        self.rules
            .net_classes
            .get_mut(class)
            .is_ignored_by_autorouter = value;
    }

    /// Sets the trace clearance class of a net class by name; false if the
    /// matrix has no such class.
    pub fn set_net_class_clearance_class(&mut self, class: NetClassId, name: &str) -> bool {
        let index = self.rules.clearance_matrix.get_no(name);
        if index < 0 {
            return false;
        }
        self.rules
            .net_classes
            .get_mut(class)
            .set_trace_clearance_class(index);
        true
    }

    /// `applyTraceWidth`: a half width above 0, in board units, on all layers.
    pub fn set_net_class_trace_half_width(&mut self, class: NetClassId, half_width: i32) -> bool {
        if half_width <= 0 {
            return false;
        }
        self.rules
            .net_classes
            .get_mut(class)
            .set_trace_half_width(half_width);
        true
    }

    /// `WindowAssignNetClass`: `Net.setClass`.
    pub fn assign_net_class(&mut self, net_number: i32, class: NetClassId) -> bool {
        match self.rules.nets.get_mut(net_number) {
            Some(net) => {
                net.set_class(class);
                true
            }
            None => false,
        }
    }

    // ---------------------------------------------------------------
    // WindowVia, WindowViaRule, WindowEditVias
    // ---------------------------------------------------------------

    /// `AddPadstackListener`: registers padstack `id` (already added to the
    /// library) as a via padstack. False if a via padstack of that name exists.
    pub fn add_via_padstack(&mut self, id: PadstackId) -> bool {
        self.library.add_via_padstack(id)
    }

    /// `RemovePadstackListener`: refused if a via info uses the padstack.
    pub fn remove_via_padstack(&mut self, id: PadstackId) -> Result<(), ViaEditError> {
        if let Some((_, info)) = self.rules.via_infos.iter().find(|(_, v)| v.padstack == id) {
            return Err(ViaEditError::PadstackInUse(info.name.clone()));
        }
        if self.library.remove_via_padstack(id) {
            Ok(())
        } else {
            Err(ViaEditError::NotFound)
        }
    }

    /// `AddViaRuleListener`: a via rule named `name.trim()`; refused when empty.
    pub fn add_via_rule(&mut self, name: &str) -> Result<ViaRuleId, ViaEditError> {
        let name = name.trim();
        if name.is_empty() {
            return Err(ViaEditError::BadName);
        }
        Ok(self.rules.via_rules.add(crate::rules::ViaRule::new(name)))
    }

    /// `RemoveViaRuleListener` (after the confirmation).
    pub fn remove_via_rule(&mut self, rule: ViaRuleId) -> bool {
        self.rules.via_rules.remove(rule)
    }

    /// `WindowViaRule`'s append: adds via info `via` to `rule`.
    pub fn append_via_to_rule(&mut self, rule: ViaRuleId, via: ViaInfoId) {
        self.rules.via_rules.get_mut(rule).append_via(via);
    }

    /// `WindowViaRule`'s delete.
    pub fn remove_via_from_rule(&mut self, rule: ViaRuleId, via: ViaInfoId) -> bool {
        self.rules.via_rules.get_mut(rule).remove_via(via)
    }

    /// `WindowViaRule`'s move up and down: swaps two vias of a rule.
    pub fn swap_vias_in_rule(
        &mut self,
        rule: ViaRuleId,
        first: ViaInfoId,
        second: ViaInfoId,
    ) -> bool {
        self.rules.via_rules.get_mut(rule).swap(first, second)
    }

    /// `AddViaListener`: a via info `newVia<n>` (the first free `n`) with the
    /// first via padstack, the default net class's via clearance class and no
    /// SMD attach.
    pub fn add_via_info(&mut self) -> Result<ViaInfoId, ViaEditError> {
        let mut no = 1;
        let name = loop {
            let name = format!("newVia{no}");
            if !self.rules.via_infos.name_exists(&name) {
                break name;
            }
            no += 1;
        };
        let padstack = *self
            .library
            .via_padstack_ids()
            .first()
            .ok_or(ViaEditError::NotFound)?;
        let default_class = self.rules.default_net_class();
        let clearance_class = self
            .rules
            .net_classes
            .get(default_class)
            .default_item_clearance_classes
            .get(ItemClass::Via);
        self.rules
            .via_infos
            .add(ViaInfo {
                name,
                padstack,
                clearance_class,
                attach_smd_allowed: false,
            })
            .ok_or(ViaEditError::BadName)
    }

    /// `RemoveViaListener`: refused for the last via info, and for one a via
    /// rule contains.
    pub fn remove_via_info(&mut self, via: ViaInfoId) -> Result<(), ViaEditError> {
        if self.rules.via_infos.count() <= 1 {
            return Err(ViaEditError::LastVia);
        }
        if let Some((_, rule)) = self.rules.via_rules.iter().find(|(_, r)| r.contains(via)) {
            return Err(ViaEditError::InRule(rule.name.clone()));
        }
        if self.rules.via_infos.remove(via) {
            Ok(())
        } else {
            Err(ViaEditError::NotFound)
        }
    }

    /// `setValueAt`, name column: refused if the name exists.
    pub fn rename_via_info(&mut self, via: ViaInfoId, name: &str) -> Result<(), ViaEditError> {
        if self.rules.via_infos.name_exists(name) {
            return Err(ViaEditError::BadName);
        }
        self.rules.via_infos.get_mut(via).name = name.to_string();
        Ok(())
    }

    /// `setValueAt`, padstack column: by via padstack name.
    pub fn set_via_info_padstack(&mut self, via: ViaInfoId, padstack_name: &str) -> bool {
        let Some(id) = self
            .library
            .get_via_padstack_by_name(padstack_name)
            .map(|p| p.id)
        else {
            return false;
        };
        self.rules.via_infos.get_mut(via).padstack = id;
        true
    }

    /// `setValueAt`, clearance class column: by name.
    pub fn set_via_info_clearance_class(&mut self, via: ViaInfoId, name: &str) -> bool {
        let index = self.rules.clearance_matrix.get_no(name);
        if index < 0 {
            return false;
        }
        self.rules.via_infos.get_mut(via).clearance_class = index;
        true
    }

    /// `setValueAt`, attach SMD column.
    pub fn set_via_info_attach_smd(&mut self, via: ViaInfoId, allowed: bool) {
        self.rules.via_infos.get_mut(via).attach_smd_allowed = allowed;
    }
}

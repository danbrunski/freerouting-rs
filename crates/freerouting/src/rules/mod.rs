//! Design rules: nets, net classes, the clearance matrix, vias and via rules.
//!
//! Port of `app.freerouting.rules`. Java shares rule objects by reference (a
//! net class points to its via rule, a via rule to its via infos); here they
//! live in arenas with stable ids ([`NetClassId`], [`ViaRuleId`],
//! [`ViaInfoId`]). Removing an entry only drops it from the ordered list, so
//! ids held elsewhere stay valid, as Java references to a removed object do.

mod clearance_matrix;
pub mod edit;

pub use clearance_matrix::{CLEARANCE_SAFETY_MARGIN, ClearanceMatrix, ClearanceRow};

use crate::board::structure::{AngleRestriction, LayerStructure};
use crate::library::{PadstackId, Padstacks};
use crate::util::equals_ignore_case;

/// An arena with a separate order list. Removed entries stay in the arena.
#[derive(Clone, Debug)]
struct OrderedArena<T> {
    items: Vec<T>,
    order: Vec<usize>,
}

impl<T> Default for OrderedArena<T> {
    fn default() -> Self {
        OrderedArena {
            items: Vec::new(),
            order: Vec::new(),
        }
    }
}

impl<T> OrderedArena<T> {
    fn push(&mut self, item: T) -> usize {
        self.items.push(item);
        let id = self.items.len() - 1;
        self.order.push(id);
        id
    }

    fn remove(&mut self, id: usize) -> bool {
        match self.order.iter().position(|i| *i == id) {
            Some(pos) => {
                self.order.remove(pos);
                true
            }
            None => false,
        }
    }

    fn count(&self) -> usize {
        self.order.len()
    }

    fn nth(&self, index: usize) -> Option<usize> {
        self.order.get(index).copied()
    }

    fn iter(&self) -> impl Iterator<Item = (usize, &T)> {
        self.order.iter().map(|&i| (i, &self.items[i]))
    }
}

/// The item classes that have a default clearance class in a net class.
///
/// Port of `rules.DefaultItemClearanceClasses.ItemClass`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ItemClass {
    None,
    Trace,
    Via,
    Pin,
    Smd,
    Area,
}

impl ItemClass {
    pub const ALL: [ItemClass; 6] = [
        ItemClass::None,
        ItemClass::Trace,
        ItemClass::Via,
        ItemClass::Pin,
        ItemClass::Smd,
        ItemClass::Area,
    ];
}

/// The clearance class an item of each [`ItemClass`] gets by default.
///
/// Port of `rules.DefaultItemClearanceClasses`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DefaultItemClearanceClasses {
    classes: [i32; 6],
}

impl Default for DefaultItemClearanceClasses {
    fn default() -> Self {
        let mut result = DefaultItemClearanceClasses { classes: [0; 6] };
        result.set_all(1);
        result
    }
}

impl DefaultItemClearanceClasses {
    pub fn get(&self, item_class: ItemClass) -> i32 {
        self.classes[item_class as usize]
    }

    pub fn set(&mut self, item_class: ItemClass, index: i32) {
        self.classes[item_class as usize] = index;
    }

    /// Sets all classes except [`ItemClass::None`].
    pub fn set_all(&mut self, index: i32) {
        for value in &mut self.classes[1..] {
            *value = index;
        }
    }
}

/// A minimum and maximum trace length. 0 means no bound.
///
/// Port of the record `rules.NetLengthConstraint`.
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct NetLengthConstraint {
    pub min_length: f64,
    pub max_length: f64,
}

impl NetLengthConstraint {
    pub const UNCONSTRAINED: NetLengthConstraint = NetLengthConstraint {
        min_length: 0.0,
        max_length: 0.0,
    };

    pub fn new(min_length: f64, max_length: f64) -> Self {
        NetLengthConstraint {
            min_length,
            max_length,
        }
    }

    pub fn has_min(&self) -> bool {
        self.min_length > 0.0
    }

    pub fn has_max(&self) -> bool {
        self.max_length > 0.0
    }

    pub fn is_constrained(&self) -> bool {
        self.has_min() || self.has_max()
    }

    /// The middle of the bounds, or the one bound there is, or 0.
    pub fn target_length(&self) -> f64 {
        match (self.has_min(), self.has_max()) {
            (true, true) => (self.min_length + self.max_length) / 2.0,
            (true, false) => self.min_length,
            (false, true) => self.max_length,
            (false, false) => 0.0,
        }
    }

    /// True if `current_length` is within the bounds.
    pub fn is_satisfied(&self, current_length: f64) -> bool {
        !(self.has_min() && current_length < self.min_length
            || self.has_max() && current_length > self.max_length)
    }
}

/// The id of a [`ViaInfo`] in [`ViaInfos`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViaInfoId(pub usize);

/// A via type: a padstack with a clearance class.
///
/// Port of `rules.ViaInfo`.
#[derive(Clone, Debug)]
pub struct ViaInfo {
    pub name: String,
    pub padstack: PadstackId,
    pub clearance_class: i32,
    /// Whether the via may be placed on an SMD pad.
    pub attach_smd_allowed: bool,
}

/// The via types of a board.
///
/// Port of `rules.ViaInfos`.
#[derive(Clone, Debug, Default)]
pub struct ViaInfos {
    arena: OrderedArena<ViaInfo>,
}

impl ViaInfos {
    /// Adds `info` unless a via with the same name exists.
    pub fn add(&mut self, info: ViaInfo) -> Option<ViaInfoId> {
        if self.name_exists(&info.name) {
            return None;
        }
        Some(ViaInfoId(self.arena.push(info)))
    }

    pub fn count(&self) -> usize {
        self.arena.count()
    }

    /// The id of the `index`-th via info.
    pub fn nth(&self, index: usize) -> Option<ViaInfoId> {
        self.arena.nth(index).map(ViaInfoId)
    }

    pub fn get(&self, id: ViaInfoId) -> &ViaInfo {
        &self.arena.items[id.0]
    }

    pub fn get_mut(&mut self, id: ViaInfoId) -> &mut ViaInfo {
        &mut self.arena.items[id.0]
    }

    /// The via info named `name` (exact match).
    pub fn get_by_name(&self, name: &str) -> Option<ViaInfoId> {
        self.arena
            .iter()
            .find(|(_, info)| info.name == name)
            .map(|(id, _)| ViaInfoId(id))
    }

    pub fn name_exists(&self, name: &str) -> bool {
        self.get_by_name(name).is_some()
    }

    pub fn remove(&mut self, id: ViaInfoId) -> bool {
        self.arena.remove(id.0)
    }

    /// The via infos in order.
    pub fn iter(&self) -> impl Iterator<Item = (ViaInfoId, &ViaInfo)> {
        self.arena.iter().map(|(id, info)| (ViaInfoId(id), info))
    }
}

/// The id of a [`ViaRule`] in [`ViaRules`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ViaRuleId(pub usize);

/// An ordered list of via types the router may use for a net class.
///
/// Port of `rules.ViaRule`.
#[derive(Clone, Debug)]
pub struct ViaRule {
    pub name: String,
    vias: Vec<ViaInfoId>,
}

impl ViaRule {
    pub fn new(name: impl Into<String>) -> Self {
        ViaRule {
            name: name.into(),
            vias: Vec::new(),
        }
    }

    pub fn append_via(&mut self, via: ViaInfoId) {
        self.vias.push(via);
    }

    pub fn remove_via(&mut self, via: ViaInfoId) -> bool {
        match self.vias.iter().position(|v| *v == via) {
            Some(pos) => {
                self.vias.remove(pos);
                true
            }
            None => false,
        }
    }

    pub fn via_count(&self) -> usize {
        self.vias.len()
    }

    pub fn get_via(&self, index: usize) -> Option<ViaInfoId> {
        self.vias.get(index).copied()
    }

    pub fn vias(&self) -> &[ViaInfoId] {
        &self.vias
    }

    pub fn contains(&self, via: ViaInfoId) -> bool {
        self.vias.contains(&via)
    }

    /// True if a via of this rule uses `padstack`.
    pub fn contains_padstack(&self, padstack: PadstackId, infos: &ViaInfos) -> bool {
        self.vias.iter().any(|v| infos.get(*v).padstack == padstack)
    }

    /// The first via spanning exactly the layers `from_layer..=to_layer`.
    pub fn get_layer_range(
        &self,
        from_layer: i32,
        to_layer: i32,
        infos: &ViaInfos,
        padstacks: &Padstacks,
    ) -> Option<ViaInfoId> {
        self.vias.iter().copied().find(|v| {
            padstacks
                .get(infos.get(*v).padstack)
                .is_some_and(|p| p.from_layer() == from_layer && p.to_layer() == to_layer)
        })
    }

    /// Swaps two vias of the rule. Returns false if one is missing.
    pub fn swap(&mut self, first: ViaInfoId, second: ViaInfoId) -> bool {
        let (Some(i1), Some(i2)) = (
            self.vias.iter().position(|v| *v == first),
            self.vias.iter().position(|v| *v == second),
        ) else {
            return false;
        };
        self.vias.swap(i1, i2);
        true
    }
}

/// The via rules of a board.
///
/// Java keeps them in a `Vector<ViaRule>` on `BoardRules`.
#[derive(Clone, Debug, Default)]
pub struct ViaRules {
    arena: OrderedArena<ViaRule>,
}

impl ViaRules {
    pub fn add(&mut self, rule: ViaRule) -> ViaRuleId {
        ViaRuleId(self.arena.push(rule))
    }

    pub fn remove(&mut self, id: ViaRuleId) -> bool {
        self.arena.remove(id.0)
    }

    pub fn count(&self) -> usize {
        self.arena.count()
    }

    pub fn is_empty(&self) -> bool {
        self.count() == 0
    }

    pub fn nth(&self, index: usize) -> Option<ViaRuleId> {
        self.arena.nth(index).map(ViaRuleId)
    }

    pub fn get(&self, id: ViaRuleId) -> &ViaRule {
        &self.arena.items[id.0]
    }

    pub fn get_mut(&mut self, id: ViaRuleId) -> &mut ViaRule {
        &mut self.arena.items[id.0]
    }

    /// The rule named `name` (exact match).
    pub fn get_by_name(&self, name: &str) -> Option<ViaRuleId> {
        self.arena
            .iter()
            .find(|(_, rule)| rule.name == name)
            .map(|(id, _)| ViaRuleId(id))
    }

    pub fn iter(&self) -> impl Iterator<Item = (ViaRuleId, &ViaRule)> {
        self.arena.iter().map(|(id, rule)| (ViaRuleId(id), rule))
    }
}

/// The id of a [`NetClass`] in [`NetClasses`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct NetClassId(pub usize);

/// Routing rules shared by a group of nets.
///
/// Port of `rules.NetClass`.
#[derive(Clone, Debug)]
pub struct NetClass {
    name: String,
    trace_half_widths: Vec<i32>,
    active_routing_layers: Vec<bool>,
    signal_layers: Vec<bool>,
    pub default_item_clearance_classes: DefaultItemClearanceClasses,
    pub is_ignored_by_autorouter: bool,
    via_rule: Option<ViaRuleId>,
    trace_clearance_class: i32,
    shove_fixed: bool,
    pull_tight: bool,
    ignore_cycles_with_areas: bool,
    minimum_trace_length: f64,
    maximum_trace_length: f64,
}

impl NetClass {
    /// A class with trace width 0 and all signal layers active.
    pub fn new(
        name: impl Into<String>,
        layers: &LayerStructure,
        ignored_by_autorouter: bool,
    ) -> Self {
        let signal_layers: Vec<bool> = layers.layers.iter().map(|l| l.is_signal).collect();
        NetClass {
            name: name.into(),
            trace_half_widths: vec![0; layers.count()],
            active_routing_layers: signal_layers.clone(),
            signal_layers,
            default_item_clearance_classes: DefaultItemClearanceClasses::default(),
            is_ignored_by_autorouter: ignored_by_autorouter,
            via_rule: None,
            trace_clearance_class: 0,
            shove_fixed: false,
            pull_tight: true,
            ignore_cycles_with_areas: false,
            minimum_trace_length: 0.0,
            maximum_trace_length: 0.0,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn set_name(&mut self, name: impl Into<String>) {
        self.name = name.into();
    }

    /// Sets the trace half width on all layers.
    pub fn set_trace_half_width(&mut self, value: i32) {
        self.trace_half_widths.fill(value);
    }

    pub fn set_trace_half_width_on_layer(&mut self, layer: usize, value: i32) {
        if let Some(w) = self.trace_half_widths.get_mut(layer) {
            *w = value;
        }
    }

    /// Sets the trace half width on the inner layers.
    pub fn set_trace_half_width_on_inner(&mut self, value: i32) {
        let n = self.trace_half_widths.len();
        for i in 1..n.saturating_sub(1) {
            self.trace_half_widths[i] = value;
        }
    }

    pub fn layer_count(&self) -> usize {
        self.trace_half_widths.len()
    }

    /// The trace half width on `layer`, 0 if out of range.
    pub fn trace_half_width(&self, layer: i32) -> i32 {
        if layer < 0 {
            return 0;
        }
        self.trace_half_widths
            .get(layer as usize)
            .copied()
            .unwrap_or(0)
    }

    pub fn trace_clearance_class(&self) -> i32 {
        self.trace_clearance_class
    }

    pub fn set_trace_clearance_class(&mut self, class: i32) {
        self.trace_clearance_class = class;
    }

    pub fn via_rule(&self) -> Option<ViaRuleId> {
        self.via_rule
    }

    pub fn set_via_rule(&mut self, rule: Option<ViaRuleId>) {
        self.via_rule = rule;
    }

    pub fn is_shove_fixed(&self) -> bool {
        self.shove_fixed
    }

    pub fn set_shove_fixed(&mut self, value: bool) {
        self.shove_fixed = value;
    }

    pub fn pull_tight(&self) -> bool {
        self.pull_tight
    }

    pub fn set_pull_tight(&mut self, value: bool) {
        self.pull_tight = value;
    }

    pub fn ignore_cycles_with_areas(&self) -> bool {
        self.ignore_cycles_with_areas
    }

    pub fn set_ignore_cycles_with_areas(&mut self, value: bool) {
        self.ignore_cycles_with_areas = value;
    }

    pub fn minimum_trace_length(&self) -> f64 {
        self.minimum_trace_length
    }

    pub fn set_minimum_trace_length(&mut self, value: f64) {
        self.minimum_trace_length = value;
    }

    pub fn maximum_trace_length(&self) -> f64 {
        self.maximum_trace_length
    }

    pub fn set_maximum_trace_length(&mut self, value: f64) {
        self.maximum_trace_length = value;
    }

    pub fn length_constraint(&self) -> NetLengthConstraint {
        NetLengthConstraint::new(self.minimum_trace_length, self.maximum_trace_length)
    }

    /// Sets both length bounds; `None` clears them.
    pub fn set_length_constraint(&mut self, constraint: Option<NetLengthConstraint>) {
        let c = constraint.unwrap_or(NetLengthConstraint::UNCONSTRAINED);
        self.minimum_trace_length = c.min_length;
        self.maximum_trace_length = c.max_length;
    }

    pub fn is_active_routing_layer(&self, layer: i32) -> bool {
        layer >= 0
            && self
                .active_routing_layers
                .get(layer as usize)
                .copied()
                .unwrap_or(false)
    }

    pub fn set_active_routing_layer(&mut self, layer: i32, active: bool) {
        if layer >= 0 {
            if let Some(a) = self.active_routing_layers.get_mut(layer as usize) {
                *a = active;
            }
        }
    }

    pub fn set_all_layers_active(&mut self, value: bool) {
        self.active_routing_layers.fill(value);
    }

    pub fn set_all_inner_layers_active(&mut self, value: bool) {
        let n = self.trace_half_widths.len();
        for i in 1..n.saturating_sub(1) {
            self.active_routing_layers[i] = value;
        }
    }

    /// True if the trace width differs between signal layers.
    pub fn trace_width_is_layer_dependent(&self) -> bool {
        let compare = self.trace_half_widths[0];
        (1..self.trace_half_widths.len())
            .any(|i| self.signal_layers[i] && self.trace_half_widths[i] != compare)
    }

    /// True if the trace width differs between inner signal layers.
    pub fn trace_width_is_inner_layer_dependent(&self) -> bool {
        let n = self.trace_half_widths.len();
        if n <= 3 {
            return false;
        }
        let mut first_inner = 1;
        while !self.signal_layers[first_inner] {
            first_inner += 1;
            if first_inner >= n {
                return false;
            }
        }
        if first_inner >= n - 1 {
            return false;
        }
        let compare = self.trace_half_widths[first_inner];
        (first_inner + 1..n - 1)
            .any(|i| self.signal_layers[i] && self.trace_half_widths[i] != compare)
    }
}

/// The net classes of a board. The first class is the default class.
///
/// Port of `rules.NetClasses`.
#[derive(Clone, Debug, Default)]
pub struct NetClasses {
    arena: OrderedArena<NetClass>,
}

impl NetClasses {
    pub fn count(&self) -> usize {
        self.arena.count()
    }

    /// The id of the `index`-th class.
    pub fn nth(&self, index: usize) -> Option<NetClassId> {
        self.arena.nth(index).map(NetClassId)
    }

    pub fn get(&self, id: NetClassId) -> &NetClass {
        &self.arena.items[id.0]
    }

    pub fn get_mut(&mut self, id: NetClassId) -> &mut NetClass {
        &mut self.arena.items[id.0]
    }

    /// The class named `name` (exact match).
    pub fn get_by_name(&self, name: &str) -> Option<NetClassId> {
        self.arena
            .iter()
            .find(|(_, class)| class.name == name)
            .map(|(id, _)| NetClassId(id))
    }

    /// Appends a class named `name`.
    pub fn append(
        &mut self,
        name: impl Into<String>,
        layers: &LayerStructure,
        ignored_by_autorouter: bool,
    ) -> NetClassId {
        NetClassId(
            self.arena
                .push(NetClass::new(name, layers, ignored_by_autorouter)),
        )
    }

    /// Appends a class named `class<n>` with the first unused `n`.
    pub fn append_unnamed(&mut self, layers: &LayerStructure) -> NetClassId {
        let mut index = 0;
        let name = loop {
            index += 1;
            let name = format!("class{index}");
            if self.get_by_name(&name).is_none() {
                break name;
            }
        };
        self.append(name, layers, false)
    }

    /// The first class with the given clearance class and via rule and
    /// `trace_half_width` on all layers.
    pub fn find(
        &self,
        trace_half_width: i32,
        trace_clearance_class: i32,
        via_rule: Option<ViaRuleId>,
    ) -> Option<NetClassId> {
        self.arena
            .iter()
            .find(|(_, c)| {
                c.trace_clearance_class == trace_clearance_class
                    && c.via_rule == via_rule
                    && c.trace_half_widths.iter().all(|w| *w == trace_half_width)
            })
            .map(|(id, _)| NetClassId(id))
    }

    /// The first class with the given clearance class, via rule and trace half
    /// widths per layer.
    pub fn find_layer_widths(
        &self,
        trace_half_widths: &[i32],
        trace_clearance_class: i32,
        via_rule: Option<ViaRuleId>,
    ) -> Option<NetClassId> {
        self.arena
            .iter()
            .find(|(_, c)| {
                c.trace_clearance_class == trace_clearance_class
                    && c.via_rule == via_rule
                    && c.trace_half_widths == trace_half_widths
            })
            .map(|(id, _)| NetClassId(id))
    }

    pub fn remove(&mut self, id: NetClassId) -> bool {
        self.arena.remove(id.0)
    }

    pub fn iter(&self) -> impl Iterator<Item = (NetClassId, &NetClass)> {
        self.arena.iter().map(|(id, c)| (NetClassId(id), c))
    }
}

/// A net (or subnet) of the board.
///
/// Port of `rules.Net`. The queries that walk the board items
/// (`getTerminalItems`, `getPins`, `getItems`, `getTraceLength`,
/// `getViaCount`) are on `BasicBoard`.
#[derive(Clone, Debug)]
pub struct Net {
    pub name: String,
    pub subnet_number: i32,
    pub net_number: i32,
    contains_plane: bool,
    net_class: NetClassId,
    length_constraint: Option<NetLengthConstraint>,
}

impl Net {
    pub fn net_class(&self) -> NetClassId {
        self.net_class
    }

    pub fn set_class(&mut self, class: NetClassId) {
        self.net_class = class;
    }

    pub fn contains_plane(&self) -> bool {
        self.contains_plane
    }

    pub fn set_contains_plane(&mut self, value: bool) {
        self.contains_plane = value;
    }

    /// The explicit minimum length, else the one of the net class.
    pub fn minimum_trace_length(&self, classes: &NetClasses) -> f64 {
        match self.length_constraint {
            Some(c) if c.has_min() => c.min_length,
            _ => classes.get(self.net_class).minimum_trace_length,
        }
    }

    /// The explicit maximum length, else the one of the net class.
    pub fn maximum_trace_length(&self, classes: &NetClasses) -> f64 {
        match self.length_constraint {
            Some(c) if c.has_max() => c.max_length,
            _ => classes.get(self.net_class).maximum_trace_length,
        }
    }

    /// Sets an explicit minimum length, keeping the explicit maximum.
    pub fn set_minimum_trace_length(&mut self, value: f64) {
        let max = self.length_constraint.map_or(0.0, |c| c.max_length);
        self.length_constraint = Some(NetLengthConstraint::new(value, max));
    }

    /// Sets an explicit maximum length, keeping the explicit minimum.
    pub fn set_maximum_trace_length(&mut self, value: f64) {
        let min = self.length_constraint.map_or(0.0, |c| c.min_length);
        self.length_constraint = Some(NetLengthConstraint::new(min, value));
    }

    /// The effective length bounds.
    pub fn length_constraint(&self, classes: &NetClasses) -> NetLengthConstraint {
        NetLengthConstraint::new(
            self.minimum_trace_length(classes),
            self.maximum_trace_length(classes),
        )
    }

    pub fn set_length_constraint(&mut self, constraint: Option<NetLengthConstraint>) {
        self.length_constraint = constraint;
    }

    pub fn has_explicit_length_constraint(&self) -> bool {
        self.length_constraint.is_some_and(|c| c.is_constrained())
    }

    pub fn explicit_length_constraint(&self) -> Option<NetLengthConstraint> {
        self.length_constraint
    }
}

/// The nets of a board, numbered from 1.
///
/// Port of `rules.Nets`.
#[derive(Clone, Debug, Default)]
pub struct Nets {
    nets: Vec<Net>,
}

impl Nets {
    /// The largest net number of a normal net.
    pub const MAX_LEGAL_NET_NUMBER: i32 = 9_999_999;
    /// The net number of items that belong to no net but must not connect to
    /// other items.
    pub const HIDDEN_NET_NUMBER: i32 = 10_000_001;

    /// True for the numbers of ordinary nets.
    pub fn is_normal_net_number(net_number: i32) -> bool {
        net_number > 0 && net_number <= Self::MAX_LEGAL_NET_NUMBER
    }

    /// The highest net number.
    pub fn max_net_number(&self) -> i32 {
        self.nets.len() as i32
    }

    /// The net named `name` (any case) with subnet `subnet_number`.
    pub fn get_by_name(&self, name: &str, subnet_number: i32) -> Option<&Net> {
        self.nets
            .iter()
            .find(|n| equals_ignore_case(&n.name, name) && n.subnet_number == subnet_number)
    }

    /// All subnets of the net named `name` (any case).
    pub fn get_all_by_name(&self, name: &str) -> Vec<&Net> {
        self.nets
            .iter()
            .filter(|n| equals_ignore_case(&n.name, name))
            .collect()
    }

    /// The net with number `net_number`.
    pub fn get(&self, net_number: i32) -> Option<&Net> {
        if net_number < 1 {
            return None;
        }
        self.nets.get(net_number as usize - 1)
    }

    pub fn get_mut(&mut self, net_number: i32) -> Option<&mut Net> {
        if net_number < 1 {
            return None;
        }
        self.nets.get_mut(net_number as usize - 1)
    }

    /// Appends a net in class `net_class` and returns its number.
    pub fn add(
        &mut self,
        name: impl Into<String>,
        subnet_number: i32,
        contains_plane: bool,
        net_class: NetClassId,
    ) -> i32 {
        let number = self.nets.len() as i32 + 1;
        self.nets.push(Net {
            name: name.into(),
            subnet_number,
            net_number: number,
            contains_plane,
            net_class,
            length_constraint: None,
        });
        number
    }

    /// Java `Nets.newNet(locale)`: a net named `net#<count + 1>` in class
    /// `default_class` (Java's `Net` constructor takes the board's default
    /// class), subnet 1, without a plane. Returns the net number.
    ///
    /// Java looks the prefix up as `TextManager.getText("net#")` in the bundle
    /// of `NetClasses` and `Common`, which have no such key at the pin, so
    /// `getText` returns the key and the name is `net#<n>` in every locale.
    pub fn new_net(&mut self, default_class: NetClassId) -> i32 {
        let name = format!("net#{}", self.nets.len() + 1);
        self.add(name, 1, false, default_class)
    }

    /// All nets in number order.
    pub fn iter(&self) -> impl Iterator<Item = &Net> {
        self.nets.iter()
    }
}

/// All design rules of a board.
///
/// Port of `rules.BoardRules`.
#[derive(Clone, Debug)]
pub struct BoardRules {
    pub clearance_matrix: ClearanceMatrix,
    pub nets: Nets,
    pub via_infos: ViaInfos,
    pub via_rules: ViaRules,
    pub net_classes: NetClasses,
    layer_structure: LayerStructure,
    trace_angle_restriction: AngleRestriction,
    ignore_conduction: bool,
    min_trace_half_width: i32,
    max_trace_half_width: i32,
    pin_edge_to_turn_dist: f64,
    use_slow_autoroute_algorithm: bool,
    hole_clearance: i32,
    /// Clearance violations smaller than this are tolerated (micrometers).
    pub clearance_tolerance_um: f64,
}

impl BoardRules {
    pub fn new(layer_structure: LayerStructure, clearance_matrix: ClearanceMatrix) -> Self {
        BoardRules {
            clearance_matrix,
            nets: Nets::default(),
            via_infos: ViaInfos::default(),
            via_rules: ViaRules::default(),
            net_classes: NetClasses::default(),
            layer_structure,
            trace_angle_restriction: AngleRestriction::FortyfiveDegree,
            ignore_conduction: true,
            min_trace_half_width: 100_000,
            max_trace_half_width: 100,
            pin_edge_to_turn_dist: 0.0,
            use_slow_autoroute_algorithm: false,
            hole_clearance: 0,
            clearance_tolerance_um: 1.0,
        }
    }

    /// The default clearance class (1).
    pub fn default_clearance_class() -> i32 {
        1
    }

    /// The clearance class of items without clearance (0).
    pub fn clearance_class_none() -> i32 {
        0
    }

    pub fn layer_structure(&self) -> &LayerStructure {
        &self.layer_structure
    }

    /// The trace half width of net `net_number` on `layer`.
    pub fn trace_half_width(&self, net_number: i32, layer: i32) -> i32 {
        self.nets.get(net_number).map_or(0, |net| {
            self.net_classes.get(net.net_class).trace_half_width(layer)
        })
    }

    /// True if the trace width of net `net_number` differs between layers.
    pub fn trace_widths_are_layer_dependent(&self, net_number: i32) -> bool {
        let compare = self.trace_half_width(net_number, 0);
        (1..self.layer_structure.count() as i32)
            .any(|i| self.trace_half_width(net_number, i) != compare)
    }

    pub fn min_trace_half_width(&self) -> i32 {
        self.min_trace_half_width
    }

    pub fn max_trace_half_width(&self) -> i32 {
        self.max_trace_half_width
    }

    pub fn hole_clearance(&self) -> i32 {
        self.hole_clearance
    }

    pub fn set_hole_clearance(&mut self, value: i32) {
        self.hole_clearance = value.max(0);
    }

    /// Sets the trace half width of the default class on `layer`.
    pub fn set_default_trace_half_width(&mut self, layer: usize, value: i32) {
        let default = self.default_net_class();
        self.net_classes
            .get_mut(default)
            .set_trace_half_width_on_layer(layer, value);
        self.min_trace_half_width = self.min_trace_half_width.min(value);
        self.max_trace_half_width = self.max_trace_half_width.max(value);
    }

    /// The trace half width of the default class on `layer`.
    pub fn default_trace_half_width(&mut self, layer: i32) -> i32 {
        let default = self.default_net_class();
        self.net_classes.get(default).trace_half_width(layer)
    }

    /// Sets the trace half width of the default class on all layers. Values
    /// that are not positive are ignored.
    pub fn set_default_trace_half_widths(&mut self, value: i32) {
        if value <= 0 {
            return;
        }
        let default = self.default_net_class();
        self.net_classes
            .get_mut(default)
            .set_trace_half_width(value);
        self.min_trace_half_width = self.min_trace_half_width.min(value);
        self.max_trace_half_width = self.max_trace_half_width.max(value);
    }

    /// The default net class, created on first use.
    pub fn default_net_class(&mut self) -> NetClassId {
        if self.net_classes.count() == 0 {
            self.create_default_net_class();
        }
        self.net_classes.nth(0).expect("default net class")
    }

    /// The default net class if it exists.
    pub fn default_net_class_id(&self) -> Option<NetClassId> {
        self.net_classes.nth(0)
    }

    /// Appends a class named `class<n>` with the trace settings of the
    /// default class and the default via rule.
    pub fn get_new_net_class(&mut self) -> NetClassId {
        let result = self.net_classes.append_unnamed(&self.layer_structure);
        self.init_new_class(result);
        result
    }

    /// Appends a class named `name` with the trace settings of the default
    /// class and the default via rule.
    pub fn get_new_named_net_class(&mut self, name: &str) -> NetClassId {
        let result = self.net_classes.append(name, &self.layer_structure, false);
        self.init_new_class(result);
        result
    }

    fn init_new_class(&mut self, class: NetClassId) {
        let default = self.default_net_class();
        let clearance = self.net_classes.get(default).trace_clearance_class;
        let width = self.net_classes.get(default).trace_half_width(0);
        let via_rule = self.default_via_rule();
        let new_class = self.net_classes.get_mut(class);
        new_class.set_trace_clearance_class(clearance);
        new_class.set_via_rule(via_rule);
        new_class.set_trace_half_width(width);
    }

    /// Creates a via rule named `name` from the via infos with the via
    /// clearance class of `net_class`, keeping for each layer range the via
    /// with the smallest pad, and assigns it to `net_class`.
    pub fn create_default_via_rule(
        &mut self,
        net_class: NetClassId,
        name: &str,
        padstacks: &Padstacks,
    ) {
        if self.via_infos.count() == 0 {
            return;
        }
        let mut rule = ViaRule::new(name);
        let via_class = self
            .net_classes
            .get(net_class)
            .default_item_clearance_classes
            .get(ItemClass::Via);
        let infos: Vec<ViaInfoId> = self.via_infos.iter().map(|(id, _)| id).collect();
        for id in infos {
            let info = self.via_infos.get(id);
            if info.clearance_class != via_class {
                continue;
            }
            let Some(padstack) = padstacks.get(info.padstack) else {
                continue;
            };
            let (from, to) = (padstack.from_layer(), padstack.to_layer());
            match rule.get_layer_range(from, to, &self.via_infos, padstacks) {
                Some(existing) => {
                    let new_width = padstack.get_shape(from).map_or(0.0, |s| s.max_width());
                    let existing_width = padstacks
                        .get(self.via_infos.get(existing).padstack)
                        .and_then(|p| p.get_shape(from))
                        .map_or(0.0, |s| s.max_width());
                    if new_width < existing_width {
                        rule.remove_via(existing);
                        rule.append_via(id);
                    }
                }
                None => rule.append_via(id),
            }
        }
        let rule_id = self.via_rules.add(rule);
        self.net_classes
            .get_mut(net_class)
            .set_via_rule(Some(rule_id));
    }

    /// Creates the default class `"default"` with trace half width 1500 and
    /// clearance class 1.
    pub fn create_default_net_class(&mut self) {
        let id = self
            .net_classes
            .append("default", &self.layer_structure, false);
        let class = self.net_classes.get_mut(id);
        class.set_trace_half_width(1500);
        class.set_trace_clearance_class(1);
    }

    /// Appends an unnamed class with the settings of the default class.
    pub fn append_net_class(&mut self) -> NetClassId {
        let id = self.net_classes.append_unnamed(&self.layer_structure);
        let default = self.net_classes.nth(0).expect("default net class");
        let (via_rule, width, clearance) = {
            let d = self.net_classes.get(default);
            (d.via_rule, d.trace_half_width(0), d.trace_clearance_class)
        };
        let class = self.net_classes.get_mut(id);
        class.set_via_rule(via_rule);
        class.set_trace_half_width(width);
        class.set_trace_clearance_class(clearance);
        id
    }

    /// The class named `name`, or a new one with the settings of the default
    /// class (including its default item clearance classes).
    pub fn append_named_net_class(&mut self, name: &str) -> NetClassId {
        if let Some(found) = self.net_classes.get_by_name(name) {
            return found;
        }
        let id = self.net_classes.append(name, &self.layer_structure, false);
        let default = self.net_classes.nth(0).expect("default net class");
        let d = self.net_classes.get(default).clone();
        let class = self.net_classes.get_mut(id);
        class.default_item_clearance_classes = d.default_item_clearance_classes;
        class.set_via_rule(d.via_rule);
        class.set_trace_half_width(d.trace_half_width(0));
        class.set_trace_clearance_class(d.trace_clearance_class);
        id
    }

    /// The first via rule.
    pub fn default_via_rule(&self) -> Option<ViaRuleId> {
        self.via_rules.nth(0)
    }

    /// The via rule named `name` (exact match).
    pub fn via_rule_by_name(&self, name: &str) -> Option<ViaRuleId> {
        self.via_rules.get_by_name(name)
    }

    /// Adds a net in the default class and returns its number.
    pub fn add_net(&mut self, name: &str, subnet_number: i32, contains_plane: bool) -> i32 {
        let class = self.default_net_class();
        self.nets.add(name, subnet_number, contains_plane, class)
    }

    /// The net class of net `net_number`.
    pub fn net_class_of(&self, net_number: i32) -> Option<&NetClass> {
        self.nets
            .get(net_number)
            .map(|n| self.net_classes.get(n.net_class))
    }

    /// Replaces clearance class `from_index` by `to_index` in the classes,
    /// via infos and the given item clearance classes.
    pub fn change_clearance_class_index(&mut self, from_index: i32, to_index: i32) {
        for id in self.net_classes.arena.order.clone() {
            let class = &mut self.net_classes.arena.items[id];
            if class.trace_clearance_class == from_index {
                class.trace_clearance_class = to_index;
            }
            for item_class in ItemClass::ALL {
                if class.default_item_clearance_classes.get(item_class) == from_index {
                    class
                        .default_item_clearance_classes
                        .set(item_class, to_index);
                }
            }
        }
        for id in self.via_infos.arena.order.clone() {
            let info = &mut self.via_infos.arena.items[id];
            if info.clearance_class == from_index {
                info.clearance_class = to_index;
            }
        }
    }

    /// True if clearance class `index` is used by a net class or via info.
    pub fn clearance_class_used_by_rules(&self, index: i32) -> bool {
        self.net_classes.iter().any(|(_, c)| {
            c.trace_clearance_class == index
                || ItemClass::ALL
                    .iter()
                    .any(|ic| c.default_item_clearance_classes.get(*ic) == index)
        }) || self
            .via_infos
            .iter()
            .any(|(_, v)| v.clearance_class == index)
    }

    /// Removes clearance class `index` from the matrix and renumbers the
    /// higher classes in the net classes and via infos. The caller checks
    /// that no item or rule uses it and renumbers the items.
    pub fn remove_clearance_class_from_rules(&mut self, index: i32) {
        for id in self.net_classes.arena.order.clone() {
            let class = &mut self.net_classes.arena.items[id];
            if class.trace_clearance_class > index {
                class.trace_clearance_class -= 1;
            }
            for item_class in ItemClass::ALL {
                let current = class.default_item_clearance_classes.get(item_class);
                if current > index {
                    class
                        .default_item_clearance_classes
                        .set(item_class, current - 1);
                }
            }
        }
        for id in self.via_infos.arena.order.clone() {
            let info = &mut self.via_infos.arena.items[id];
            if info.clearance_class > index {
                info.clearance_class -= 1;
            }
        }
        self.clearance_matrix.remove_class(index as usize);
    }

    pub fn pin_edge_to_turn_dist(&self) -> f64 {
        self.pin_edge_to_turn_dist
    }

    pub fn set_pin_edge_to_turn_dist(&mut self, value: f64) {
        self.pin_edge_to_turn_dist = value;
    }

    pub fn ignore_conduction(&self) -> bool {
        self.ignore_conduction
    }

    pub fn set_ignore_conduction(&mut self, value: bool) {
        self.ignore_conduction = value;
    }

    pub fn trace_angle_restriction(&self) -> AngleRestriction {
        self.trace_angle_restriction
    }

    pub fn set_trace_angle_restriction(&mut self, restriction: AngleRestriction) {
        self.trace_angle_restriction = restriction;
    }

    pub fn use_slow_autoroute_algorithm(&self) -> bool {
        self.use_slow_autoroute_algorithm
    }

    pub fn set_use_slow_autoroute_algorithm(&mut self, value: bool) {
        self.use_slow_autoroute_algorithm = value;
    }

    /// The larger pad diameter of the first via of the default via rule, or 0.
    pub fn default_via_diameter(&self, padstacks: &Padstacks) -> f64 {
        let Some(rule) = self.default_via_rule() else {
            return 0.0;
        };
        let Some(via) = self.via_rules.get(rule).get_via(0) else {
            return 0.0;
        };
        let Some(padstack) = padstacks.get(self.via_infos.get(via).padstack) else {
            return 0.0;
        };
        let from = padstack
            .get_shape(padstack.from_layer())
            .map_or(0.0, |s| s.max_width());
        let to = padstack
            .get_shape(padstack.to_layer())
            .map_or(0.0, |s| s.max_width());
        from.max(to)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::structure::Layer;

    fn layers() -> LayerStructure {
        LayerStructure::new(vec![
            Layer::new("Top", true),
            Layer::new("GND", false),
            Layer::new("Bottom", true),
        ])
    }

    #[test]
    fn default_net_class_settings() {
        let mut rules = BoardRules::new(layers(), ClearanceMatrix::default_instance(3, 100));
        let default = rules.default_net_class();
        let class = rules.net_classes.get(default);
        assert_eq!(class.name(), "default");
        assert_eq!(class.trace_half_width(2), 1500);
        assert_eq!(class.trace_clearance_class(), 1);
        assert!(class.is_active_routing_layer(0));
        assert!(!class.is_active_routing_layer(1));
        assert!(class.pull_tight());
        assert_eq!(class.default_item_clearance_classes.get(ItemClass::None), 0);
        assert_eq!(class.default_item_clearance_classes.get(ItemClass::Smd), 1);
    }

    #[test]
    fn new_classes_copy_the_default_class() {
        let mut rules = BoardRules::new(layers(), ClearanceMatrix::default_instance(3, 100));
        rules.set_default_trace_half_widths(250);
        let a = rules.get_new_net_class();
        let b = rules.append_net_class();
        assert_eq!(rules.net_classes.get(a).name(), "class1");
        assert_eq!(rules.net_classes.get(b).name(), "class2");
        assert_eq!(rules.net_classes.get(a).trace_half_width(0), 250);
        assert_eq!(
            rules.net_classes.find(250, 1, None),
            Some(rules.default_net_class())
        );
        let named = rules.append_named_net_class("Power");
        assert_eq!(rules.append_named_net_class("Power"), named);
        assert_eq!(rules.net_classes.count(), 4);
        assert_eq!(rules.min_trace_half_width(), 250);
        assert_eq!(rules.max_trace_half_width(), 250);
    }

    #[test]
    fn nets_are_numbered_from_one_and_found_without_case() {
        let mut rules = BoardRules::new(layers(), ClearanceMatrix::default_instance(3, 100));
        assert_eq!(rules.add_net("GND", 1, false), 1);
        assert_eq!(rules.add_net("VCC", 1, true), 2);
        assert_eq!(rules.add_net("GND", 2, false), 3);
        assert_eq!(rules.nets.get_by_name("gnd", 2).unwrap().net_number, 3);
        assert_eq!(rules.nets.get_all_by_name("Gnd").len(), 2);
        assert!(rules.nets.get(4).is_none());
        assert!(rules.nets.get(2).unwrap().contains_plane());
        assert_eq!(rules.nets.max_net_number(), 3);
        assert!(Nets::is_normal_net_number(1));
        assert!(!Nets::is_normal_net_number(Nets::HIDDEN_NET_NUMBER));
    }

    #[test]
    fn via_rules_keep_removed_rules_alive_for_holders() {
        let mut rules = BoardRules::new(layers(), ClearanceMatrix::default_instance(3, 100));
        let first = rules.via_rules.add(ViaRule::new("default"));
        let second = rules.via_rules.add(ViaRule::new("power"));
        assert_eq!(rules.default_via_rule(), Some(first));
        assert!(rules.via_rules.remove(first));
        assert_eq!(rules.default_via_rule(), Some(second));
        assert_eq!(rules.via_rules.get(first).name, "default");
        assert_eq!(rules.via_rule_by_name("default"), None);
    }
}

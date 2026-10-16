//! The default rule of the structure scope and of the layers (trace width, the clearance matrix
//! and `smd_to_turn_gap`), and which net classes recreate their clearance class themselves.
//!
//! Port of `Rule.writeDefaultRule`, `writeNamedClearanceRules` and `Structure.writeLayers`.
//!
//! # The clearance matrix
//!
//! Java writes the default clearance and one `(clearance v (type name))` per named class. Its
//! reader skips a type list that is one name without `_`, so the matrix did not survive a write
//! and read. This writer writes the pairs `(clearance v (type a b))`, and uses a copy of the
//! matrix to emit exactly the pairs the reader needs to rebuild the target.
//!
//! The reader processes the structure rule on all layers first: the default clearance sets
//! every pair of classes, and the first mention of a class appends it with the values of the
//! default class. A layer's rule then sets the default and the pairs on that layer alone. The
//! plan follows the same steps on a copy of the matrix and writes a pair only where the copy
//! differs from the board.
//!
//! # Net classes that create their own clearance class
//!
//! A net class with `(rule (clearance v))` makes the reader create a clearance class named like
//! the net class (unless one exists), set the class's clearance class to it, and make it the
//! default clearance class of every item kind of the net class (`set_all`). 708 of the 715 net
//! classes of the fixtures are like that, and a class that exists already does none of it. So
//! such a class is left out of the structure rules and written with its `(clearance v)` rule
//! ("owned"), and the plan checks on the copy that the reader ends with the board's matrix. If
//! it does not, no class is owned, and the default clearance classes of the net classes are
//! not reproduced.

use std::collections::{HashMap, HashSet};

use freerouting_dsn::ast::Rule;
use freerouting_geometry::jmath::round_to_i32;

use super::Cx;
use crate::rules::{ClearanceMatrix, ItemClass, NetClassId, ViaInfoId, ViaRule, ViaRuleId};

/// The rules of the structure scope and of each layer (layer 0 has none of its own), and the
/// net classes that create their clearance class when they are read.
pub(super) struct RulePlan {
    pub global: Vec<Rule>,
    pub layers: Vec<Vec<Rule>>,
    pub owned: HashSet<NetClassId>,
    pub via: Option<ViaPlan>,
}

/// How the via rules are written so that the reader makes them again.
///
/// The reader makes a via rule for every net class with a `use_via` list (named like the class,
/// from the via infos of the class's via clearance class with these padstacks), and appends the
/// `use_via` names to the via padstack list of the structure scope. Boards read from KiCad
/// files have this everywhere: the board's via padstack list is the structure's list followed by
/// the rules' padstack names in class order. The plan gives each class with a rule of its own
/// its `use_via`, drops those names from the structure's list and leaves out all via rules,
/// when the reader would end with the same rules and list (checked on the board).
pub(super) struct ViaPlan {
    /// How many of the board's via padstacks the structure scope lists.
    pub structure_len: usize,
    /// The `use_via` names of the classes that have a rule of their own.
    pub use_via: HashMap<NetClassId, Vec<String>>,
}

struct MatrixRules {
    global: Vec<Rule>,
    layers: Vec<Vec<Rule>>,
    /// The matrix as the reader leaves it after the structure scope.
    sim: ClearanceMatrix,
}

fn pair(value: f64, first: &str, second: &str) -> Rule {
    Rule::Clearance {
        value,
        types: vec![first.to_string(), second.to_string()],
    }
}

pub(super) fn plan(cx: &Cx) -> RulePlan {
    let all: Vec<i32> = (0..cx.board.rules.clearance_matrix.class_count() as i32).collect();
    let via = via_plan(cx);
    let owned = owned_classes(cx, via.as_ref());
    if !owned.is_empty() {
        let owned_indices: HashSet<i32> = owned.iter().map(|(_, index)| *index).collect();
        let kept: Vec<i32> = all
            .iter()
            .copied()
            .filter(|i| !owned_indices.contains(i))
            .collect();
        let rules = matrix_rules(cx, &kept);
        if reader_rebuilds(cx, &rules.sim, &owned) {
            return RulePlan {
                global: rules.global,
                layers: rules.layers,
                owned: owned.iter().map(|(id, _)| *id).collect(),
                via,
            };
        }
    }
    let rules = matrix_rules(cx, &all);
    RulePlan {
        global: rules.global,
        layers: rules.layers,
        owned: HashSet::new(),
        via,
    }
}

/// The value of a clearance as the reader reads it back.
fn through_file(cx: &Cx, value: i32) -> i32 {
    round_to_i32(
        cx.transform
            .dsn_to_board(cx.transform.board_to_dsn(f64::from(value))),
    )
}

/// The net classes (with their clearance class index, in net class order) that the reader would
/// build the same way: all item kinds and the trace use the class named like the net class, and
/// the via rule is the one the reader makes from the via infos of that class.
fn owned_classes(cx: &Cx, via: Option<&ViaPlan>) -> Vec<(NetClassId, i32)> {
    let board = cx.board;
    let rules = &board.rules;
    let mut result = Vec::new();
    if rules.via_infos.count() == 0 {
        return result;
    }
    for (id, class) in rules.net_classes.iter().skip(1) {
        let index = rules.clearance_matrix.get_no(class.name());
        if index < 2
            || class.trace_clearance_class() != index
            || ItemClass::ALL[1..]
                .iter()
                .any(|k| class.default_item_clearance_classes.get(*k) != index)
        {
            continue;
        }
        match via {
            // The reader makes the rule from the class's `use_via`.
            Some(via) if !via.use_via.contains_key(&id) => continue,
            Some(_) => {}
            None => {
                let Some(rule) = class.via_rule().map(|r| rules.via_rules.get(r)) else {
                    continue;
                };
                if rule.name != class.name() || rule.vias() != auto_via_rule(cx, index).as_slice() {
                    continue;
                }
            }
        }
        result.push((id, index));
    }
    result
}

/// The vias `BoardRules::create_default_via_rule` picks for the via clearance class
/// `via_class`: for each layer range the via with the smallest pad.
fn auto_via_rule(cx: &Cx, via_class: i32) -> Vec<ViaInfoId> {
    let board = cx.board;
    let rules = &board.rules;
    let padstacks = &board.library.padstacks;
    let mut rule = ViaRule::new("");
    for (id, info) in rules.via_infos.iter() {
        if info.clearance_class != via_class {
            continue;
        }
        let Some(padstack) = padstacks.get(info.padstack) else {
            continue;
        };
        let (from, to) = (padstack.from_layer(), padstack.to_layer());
        match rule.get_layer_range(from, to, &rules.via_infos, padstacks) {
            Some(existing) => {
                let width =
                    |p: &crate::library::Padstack| p.get_shape(from).map_or(0.0, |s| s.max_width());
                let existing_width = padstacks
                    .get(rules.via_infos.get(existing).padstack)
                    .map_or(0.0, width);
                if width(padstack) < existing_width {
                    rule.remove_via(existing);
                    rule.append_via(id);
                }
            }
            None => rule.append_via(id),
        }
    }
    rule.vias().to_vec()
}

/// Follows the reader through the net classes: each owned class appends its clearance class
/// with the default class's values, raises its clearances to the others to its own value
/// (`Network.addClearanceRule`) and sets its own on all layers. True if the board's matrix is
/// the result.
fn reader_rebuilds(cx: &Cx, structure: &ClearanceMatrix, owned: &[(NetClassId, i32)]) -> bool {
    let target = &cx.board.rules.clearance_matrix;
    let mut sim = structure.clone();
    for (_, index) in owned {
        let name = cx.class_name(*index);
        let value = through_file(cx, target.get_value(*index, *index, 0, false));
        sim.append_class(&name);
        let class_no = sim.class_count() - 1;
        for i in 1..sim.class_count() {
            for layer in 0..sim.layer_count() {
                let current = sim
                    .get_value(class_no as i32, i as i32, layer as i32, false)
                    .max(value);
                sim.set_value_on_layer(class_no, i, layer, current);
                sim.set_value_on_layer(i, class_no, layer, current);
            }
        }
        sim.set_value(class_no, class_no, value);
    }
    let n = target.class_count();
    sim.class_count() == n
        && sim.layer_count() == target.layer_count()
        && (0..n as i32).all(|i| {
            sim.get_no(&cx.class_name(i)) == i
                && (0..n as i32).all(|j| {
                    (0..target.layer_count() as i32)
                        .all(|l| sim.get_value(i, j, l, false) == target.get_value(i, j, l, false))
                })
        })
}

/// The structure rule and the layer rules for the classes `kept` (board indices, ascending,
/// starting with 0 and 1).
fn matrix_rules(cx: &Cx, kept: &[i32]) -> MatrixRules {
    let board = cx.board;
    let matrix = &board.rules.clearance_matrix;
    let layer_count = board.layer_count();
    let transform = &cx.transform;
    let dsn = |value: i32| transform.board_to_dsn(f64::from(value));
    let smd_to_turn_gap = Rule::Clearance {
        value: transform.board_to_dsn(board.rules.pin_edge_to_turn_dist()),
        types: vec!["smd_to_turn_gap".into()],
    };
    let half_width = |layer: usize| {
        board.rules.default_net_class_id().map_or(0, |id| {
            board
                .rules
                .net_classes
                .get(id)
                .trace_half_width(layer as i32)
        })
    };
    let width =
        |layer: usize| Rule::Width(2.0 * transform.board_to_dsn(f64::from(half_width(layer))));

    // Layer 0 is written in the structure scope, and the reader applies it to every layer.
    let mut sim = ClearanceMatrix::default_instance(layer_count.max(1), 0);
    let default_value = matrix.get_value(1, 1, 0, false);
    sim.set_default_value(default_value);
    let mut global = vec![
        width(0),
        Rule::Clearance {
            value: dsn(default_value),
            types: Vec::new(),
        },
        smd_to_turn_gap.clone(),
    ];
    for (k, &i) in kept.iter().enumerate().skip(2) {
        let name = cx.class_name(i);
        sim.append_class(&name);
        let diagonal = matrix.get_value(i, i, 0, false);
        global.push(pair(dsn(diagonal), &name, &name));
        sim.set_value(k, k, diagonal);
        for (kj, &j) in kept.iter().enumerate().take(k) {
            let target = matrix.get_value(i, j, 0, false);
            let back = matrix.get_value(j, i, 0, false);
            if sim.get_value(k as i32, kj as i32, 0, false) != target
                || sim.get_value(kj as i32, k as i32, 0, false) != back
            {
                global.push(pair(dsn(target), &name, &cx.class_name(j)));
                sim.set_value(k, kj, target);
                sim.set_value(kj, k, target);
            }
        }
    }

    let mut layers = vec![Vec::new(); layer_count];
    for (layer, layer_rules) in layers.iter_mut().enumerate().skip(1) {
        let l = layer as i32;
        let differs = half_width(layer) != half_width(0)
            || kept.iter().skip(1).any(|&i| {
                kept.iter()
                    .skip(1)
                    .any(|&j| matrix.get_value(i, j, l, false) != matrix.get_value(i, j, 0, false))
            });
        if !differs {
            continue;
        }
        let mut sim_layer = sim.clone();
        let default_value = matrix.get_value(1, 1, l, false);
        sim_layer.set_default_value_on_layer(layer, default_value);
        layer_rules.push(width(layer));
        layer_rules.push(Rule::Clearance {
            value: dsn(default_value),
            types: Vec::new(),
        });
        layer_rules.push(smd_to_turn_gap.clone());
        for (k, &i) in kept.iter().enumerate().skip(1) {
            for (kj, &j) in kept.iter().enumerate().skip(1).take(k) {
                if (k, kj) == (1, 1) {
                    continue;
                }
                let target = matrix.get_value(i, j, l, false);
                let back = matrix.get_value(j, i, l, false);
                if sim_layer.get_value(k as i32, kj as i32, l, false) != target
                    || sim_layer.get_value(kj as i32, k as i32, l, false) != back
                {
                    layer_rules.push(pair(dsn(target), &cx.class_name(i), &cx.class_name(j)));
                    sim_layer.set_value_on_layer(k, kj, layer, target);
                    sim_layer.set_value_on_layer(kj, k, layer, target);
                }
            }
        }
    }
    MatrixRules {
        global,
        layers,
        sim,
    }
}

/// The via padstack names of a via rule.
fn rule_padstacks(cx: &Cx, rule: &ViaRule) -> Vec<String> {
    rule.vias()
        .iter()
        .filter_map(|v| {
            cx.board
                .library
                .padstacks
                .get(cx.board.rules.via_infos.get(*v).padstack)
        })
        .map(|p| p.name.clone())
        .collect()
}

/// `Network.createViaRule` on the names of a class: the vias with these padstacks and the
/// class's via clearance class.
fn rule_from_names(cx: &Cx, via_class: i32, names: &[String]) -> Vec<ViaInfoId> {
    let rules = &cx.board.rules;
    let mut vias = Vec::new();
    for name in names {
        for (id, info) in rules.via_infos.iter() {
            if info.clearance_class == via_class
                && cx
                    .board
                    .library
                    .padstacks
                    .get(info.padstack)
                    .is_some_and(|p| p.name == *name)
            {
                vias.push(id);
            }
        }
    }
    vias
}

/// The via plan, if the reader would rebuild the board's via rules and via padstack list from
/// it: the rules are the default rule (made by the reader when the file has none) and one rule
/// per class with its own, each what `use_via` builds, and the list ends with their padstacks.
fn via_plan(cx: &Cx) -> Option<ViaPlan> {
    let board = cx.board;
    let rules = &board.rules;
    if rules.via_infos.count() == 0 {
        return None;
    }
    let first = rules.via_rules.nth(0)?;
    let default_class = rules.default_net_class_id()?;
    let default_via_class = rules
        .net_classes
        .get(default_class)
        .default_item_clearance_classes
        .get(ItemClass::Via);
    let first_rule = rules.via_rules.get(first);
    if first_rule.name != "default" || first_rule.vias() != auto_via_rule(cx, default_via_class) {
        return None;
    }
    let mut use_via = HashMap::new();
    let mut own_rules = vec![first];
    let mut all_names: Vec<String> = Vec::new();
    for (id, class) in rules.net_classes.iter() {
        let Some(rule_id) = class.via_rule() else {
            continue;
        };
        let rule = rules.via_rules.get(rule_id);
        if rule_id == first || rule.name != class.name() {
            continue;
        }
        let names = rule_padstacks(cx, rule);
        let via_class = class.default_item_clearance_classes.get(ItemClass::Via);
        if names.is_empty() || rule_from_names(cx, via_class, &names) != rule.vias() {
            return None;
        }
        all_names.extend(names.iter().cloned());
        own_rules.push(rule_id);
        use_via.insert(id, names);
    }
    let listed: Vec<ViaRuleId> = rules.via_rules.iter().map(|(id, _)| id).collect();
    if listed != own_rules {
        return None;
    }
    let library = &board.library;
    let list: Vec<&str> = (0..library.via_padstack_count())
        .filter_map(|i| library.get_via_padstack(i))
        .map(|p| p.name.as_str())
        .collect();
    let structure_len = list.len().checked_sub(all_names.len())?;
    list[structure_len..]
        .iter()
        .zip(&all_names)
        .all(|(a, b)| a == b)
        .then_some(ViaPlan {
            structure_len,
            use_via,
        })
}

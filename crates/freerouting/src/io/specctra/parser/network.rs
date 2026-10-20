//! The network scope: nets, via types, via rules, net classes, and the
//! insertion of the placed components and logical parts.
//!
//! Port of `io.specctra.parser.Network.readScope` and its helpers.

use crate::util::HashMap;

use freerouting_dsn::ast;
use freerouting_geometry::jmath::round_to_i32;
use freerouting_geometry::{Area, IntPoint, Point, PolylineArea, Shape, Vector};

use super::library::find_padstack;
use super::structure::contains_wire_clearance_pair;
use super::wiring::NetNames;
use super::{NetKey, PinKey, ReadContext};
use crate::board::RoutingBoard;
use crate::board::items::AreaKind;
use crate::board::structure::{ComponentId, FixedState};
use crate::io::CoordinateTransform;
use crate::library::{PadstackId, PartPin};
use crate::rules::{
    BoardRules, ClearanceMatrix, ItemClass, NetClassId, NetLengthConstraint, ViaInfo, ViaRule,
};
use crate::util::{equals_ignore_case, java_split, remove_dot_numbers};

/// The class separator in generated clearance class and via names (Java
/// `DsnFile.CLASS_CLEARANCE_SEPARATOR`).
const CLASS_CLEARANCE_SEPARATOR: char = '-';

/// True for the names KiCad gives its default net class.
///
/// Port of `io.KiCadNetClassNames.isKiCadDefaultNetClassName`.
pub(crate) fn is_kicad_default_net_class_name(name: &str) -> bool {
    !name.is_empty()
        && (equals_ignore_case(name, "default") || equals_ignore_case(name, "kicad_default"))
}

/// Port of `io.KiCadNetClassNames.resolveNetClass`.
fn resolve_net_class(rules: &mut BoardRules, name: &str) -> Option<NetClassId> {
    if is_kicad_default_net_class_name(name) {
        return Some(rules.default_net_class());
    }
    rules.net_classes.get_by_name(name)
}

/// A net class as read from the DSN file, after Java's `NetClass.readScope`.
struct DsnNetClass<'a> {
    class: &'a ast::NetClass,
    use_via: Vec<String>,
    use_layer: Vec<String>,
    min_trace_length: f64,
    max_trace_length: f64,
}

/// Java `Circuit.readScope`'s result for an optional circuit scope:
/// `(max, min, use_via, use_layer)`.
fn circuit_lengths(circuit: Option<&ast::Circuit>) -> (f64, f64) {
    match circuit.and_then(|c| c.length) {
        Some(length) => (length.max, length.min.unwrap_or(0.0)),
        None => (0.0, 0.0),
    }
}

/// Java `NetClass.readScope`'s result with its circuit lists.
fn dsn_net_class(class: &ast::NetClass) -> DsnNetClass<'_> {
    let (max, min) = circuit_lengths(class.circuit.as_ref());
    DsnNetClass {
        class,
        use_via: class
            .circuit
            .as_ref()
            .map(|c| c.use_via.clone())
            .unwrap_or_default(),
        use_layer: class
            .circuit
            .as_ref()
            .map(|c| c.use_layer.clone())
            .unwrap_or_default(),
        min_trace_length: min,
        max_trace_length: max,
    }
}

/// `Network.insertNetClass` for a class of a rules file (the rules reader
/// calls it with `viaAtSmdAllowed = false`, which is `ctx`'s default).
pub(crate) fn insert_rules_net_class(ctx: &mut ReadContext, class: &ast::NetClass) {
    insert_net_class(ctx, &dsn_net_class(class));
}

/// `Network.readViaInfo` for a via of a rules file. `None` when Java returns
/// `null` (the padstack is unknown).
pub(crate) fn read_rules_via_info(ctx: &mut ReadContext, via: &ast::NetworkVia) -> Option<ViaInfo> {
    read_via_info(ctx, via)
}

/// `RulesReader.applyViaInfo`: an info with the name of an existing one
/// replaces it at the end of the list.
pub(crate) fn replace_via_info(board: &mut RoutingBoard, info: ViaInfo) {
    if let Some(existing) = board.rules.via_infos.get_by_name(&info.name) {
        board.rules.via_infos.remove(existing);
    }
    board.rules.via_infos.add(info);
}

/// `Network.addViaRule` for a via rule of a rules file.
pub(crate) fn add_rules_via_rule(board: &mut RoutingBoard, rule: &ast::ViaRule) -> bool {
    add_via_rule(board, rule)
}

/// Reads the network scope. Returns false where Java's reader does.
pub(crate) fn read_network(ctx: &mut ReadContext, network: &ast::Network) -> bool {
    for net in &network.nets {
        read_net(ctx, net);
    }
    let mut via_infos = Vec::new();
    for via in &network.vias {
        match read_via_info(ctx, via) {
            Some(info) => via_infos.push(info),
            None => return false,
        }
    }
    let mut classes: Vec<DsnNetClass> = network.classes.iter().map(dsn_net_class).collect();

    // Java appends the `use_via` lists of the classes to the via padstack
    // names. Without a structure via scope it takes the first class's list
    // itself, so the later classes' vias also end up in that class.
    if ctx.via_padstack_names.is_none() && classes.iter().skip(1).any(|c| !c.use_via.is_empty()) {
        crate::run_stats::record_n06(crate::run_stats::Counter::DsnViaAlias);
    }
    let mut aliased_class: Option<usize> = None;
    for i in 0..classes.len() {
        let use_via = classes[i].use_via.clone();
        match (&mut ctx.via_padstack_names, aliased_class) {
            (Some(names), alias) => {
                names.extend(use_via.iter().cloned());
                if let Some(k) = alias {
                    classes[k].use_via.extend(use_via);
                }
            }
            (None, _) => {
                ctx.via_padstack_names = Some(use_via);
                // PARITY(quirk, java_quirks.dsn_via_alias): Network.java:1328-1335
                // shares the first class's list; `false` copies it.
                aliased_class = ctx.java_quirks.dsn_via_alias.then_some(i);
            }
        }
    }
    if let Some(names) = ctx.via_padstack_names.clone() {
        let board = ctx.board();
        let mut ids: Vec<PadstackId> = Vec::new();
        for name in &names {
            match find_padstack(&board.library.padstacks, name) {
                Some(padstack) => ids.push(padstack.id),
                None => board.log.push(format!(
                    "Library.read_scope: via padstack with name '{name} not found"
                )),
            }
        }
        board.library.set_via_padstacks(ids);
    }
    insert_via_infos(ctx, via_infos);
    insert_via_rules(ctx.board(), &network.via_rules);
    for class in &classes {
        insert_net_class(ctx, class);
    }
    insert_class_pairs(ctx, &network.class_classes);
    true
}

/// Java `Network.readNetScope`.
fn read_net(ctx: &mut ReadContext, net: &ast::Net) {
    ctx.scope_identifier = net.name.clone();
    let subnet_pin_lists: Vec<Vec<&ast::PinRef>> = if !net.fromtos.is_empty() {
        net.fromtos.iter().map(|f| f.iter().collect()).collect()
    } else if net.ordered {
        net.pins.windows(2).map(|w| vec![&w[0], &w[1]]).collect()
    } else {
        vec![net.pins.iter().collect()]
    };
    let transform = ctx.coordinate_transform;
    let (circuit_max, circuit_min) = circuit_lengths(net.circuit.as_ref());
    for (subnet_number, pins) in (net.subnet.unwrap_or(1)..).zip(subnet_pin_lists) {
        let key = NetKey::new(&net.name, subnet_number);
        if ctx.netlist.add_net(key.clone()) {
            let contains_plane = ctx.layer_structure.contains_plane(&net.name);
            ctx.board()
                .rules
                .add_net(&net.name, subnet_number, contains_plane);
        }
        ctx.netlist
            .set_pins(&key, pins.iter().map(|p| PinKey::new(&p.component, &p.pin)));
        let board = ctx.board();
        let board_net = board
            .rules
            .nets
            .get_by_name(&net.name, subnet_number)
            .map(|n| n.net_number);
        if let Some(net_number) = board_net
            && net.circuit.is_some()
        {
            let max = if circuit_max > 0.0 {
                transform.dsn_to_board(circuit_max)
            } else {
                0.0
            };
            let min = if circuit_min > 0.0 {
                transform.dsn_to_board(circuit_min)
            } else {
                0.0
            };
            if max > 0.0 || min > 0.0 {
                board
                    .rules
                    .nets
                    .get_mut(net_number)
                    .expect("net")
                    .set_length_constraint(Some(NetLengthConstraint::new(min, max)));
            }
        }
        if !net.rules.is_empty() {
            let Some(net_number) = board_net else {
                board.log.push(format!(
                    "Network.read_net_scope: board net not found at '{}'",
                    net.name
                ));
                return;
            };
            for rule in &net.rules {
                match rule {
                    ast::Rule::Width(width) => {
                        let half_width = round_to_i32(transform.dsn_to_board(*width) / 2.0);
                        let rules = &mut board.rules;
                        let default = rules.default_net_class();
                        let (clearance, via_rule) = {
                            let d = rules.net_classes.get(default);
                            (d.trace_clearance_class(), d.via_rule())
                        };
                        let class = match rules.net_classes.find(half_width, clearance, via_rule) {
                            Some(class) => class,
                            None => rules.get_new_net_class(),
                        };
                        rules
                            .net_classes
                            .get_mut(class)
                            .set_trace_half_width(half_width);
                        rules
                            .nets
                            .get_mut(net_number)
                            .expect("net")
                            .set_class(class);
                    }
                    ast::Rule::Length(length) => {
                        let max = if length.max > 0.0 {
                            transform.dsn_to_board(length.max)
                        } else {
                            0.0
                        };
                        let min = length.min.unwrap_or(0.0);
                        let min = if min > 0.0 {
                            transform.dsn_to_board(min)
                        } else {
                            0.0
                        };
                        if max > 0.0 || min > 0.0 {
                            board
                                .rules
                                .nets
                                .get_mut(net_number)
                                .expect("net")
                                .set_length_constraint(Some(NetLengthConstraint::new(min, max)));
                        }
                    }
                    ast::Rule::Clearance { .. } => board.log.push(format!(
                        "Network.read_net_scope: Rule not yet implemented at '{}'",
                        net.name
                    )),
                }
            }
        }
    }
}

/// Java `Network.readViaInfo`. `None` makes the whole read fail.
fn read_via_info(ctx: &mut ReadContext, via: &ast::NetworkVia) -> Option<ViaInfo> {
    ctx.scope_identifier = via.padstack.clone();
    let board = ctx.board();
    let library = &mut board.library;
    let stripped = remove_dot_numbers(&via.padstack);
    let padstack = match library
        .get_via_padstack_by_name(&via.padstack)
        .or_else(|| library.get_via_padstack_by_name(&stripped))
    {
        Some(p) => p.id,
        None => {
            let Some(id) = find_padstack(&library.padstacks, &via.padstack).map(|p| p.id) else {
                board.log.push(format!(
                    "Network.read_via_info: padstack not found at '{}'",
                    via.padstack
                ));
                return None;
            };
            library.add_via_padstack(id);
            id
        }
    };
    let mut clearance_class = board.rules.clearance_matrix.get_no(&via.clearance_class);
    if clearance_class < 0 {
        clearance_class = BoardRules::default_clearance_class();
    }
    Some(ViaInfo {
        name: via.name.clone(),
        padstack,
        clearance_class,
        attach_smd_allowed: via.attach,
    })
}

/// Java `Network.insertViaInfos`.
fn insert_via_infos(ctx: &mut ReadContext, via_infos: Vec<ViaInfo>) {
    let attach_allowed = ctx.via_at_smd_allowed;
    let board = ctx.board();
    if via_infos.is_empty() {
        let default = board.rules.default_net_class();
        create_default_via_infos(board, default, attach_allowed);
    } else {
        for info in via_infos {
            board.rules.via_infos.add(info);
        }
    }
}

/// Java `Network.createDefaultViaInfos`: a via type for each via padstack,
/// with the via clearance class of `net_class`.
fn create_default_via_infos(board: &mut RoutingBoard, net_class: NetClassId, attach_allowed: bool) {
    let clearance_class = board
        .rules
        .net_classes
        .get(net_class)
        .default_item_clearance_classes
        .get(ItemClass::Via);
    let is_default_class = Some(net_class) == board.rules.default_net_class_id();
    let class_name = board.rules.net_classes.get(net_class).name().to_string();
    let padstack_ids = board.library.via_padstack_ids().to_vec();
    for id in padstack_ids {
        let Some(padstack) = board.library.padstacks.get(id) else {
            continue;
        };
        let name = if is_default_class {
            padstack.name.clone()
        } else {
            format!("{}{CLASS_CLEARANCE_SEPARATOR}{class_name}", padstack.name)
        };
        let info = ViaInfo {
            name,
            padstack: id,
            clearance_class,
            attach_smd_allowed: attach_allowed && padstack.attach_allowed,
        };
        board.rules.via_infos.add(info);
    }
}

/// Java `Network.insertViaRules`.
fn insert_via_rules(board: &mut RoutingBoard, via_rules: &[ast::ViaRule]) {
    let mut rule_found = false;
    for rule in via_rules {
        if rule.vias.is_empty() {
            continue;
        }
        if add_via_rule(board, rule) {
            rule_found = true;
        }
    }
    if !rule_found {
        let default = board.rules.default_net_class();
        let padstacks = board.library.padstacks.clone();
        board
            .rules
            .create_default_via_rule(default, "default", &padstacks);
    }
    let default_rule = board.rules.default_via_rule();
    let classes: Vec<NetClassId> = board.rules.net_classes.iter().map(|(id, _)| id).collect();
    for class in classes {
        board
            .rules
            .net_classes
            .get_mut(class)
            .set_via_rule(default_rule);
    }
}

/// Java `Network.addViaRule`.
fn add_via_rule(board: &mut RoutingBoard, rule: &ast::ViaRule) -> bool {
    let existing = board.rules.via_rule_by_name(&rule.name);
    let mut new_rule = ViaRule::new(rule.name.clone());
    let mut rule_ok = true;
    for via in &rule.vias {
        match board.rules.via_infos.get_by_name(via) {
            Some(info) => new_rule.append_via(info),
            None => {
                board
                    .log
                    .push("Network.insert_via_rules: viaInfo not found".into());
                rule_ok = false;
            }
        }
    }
    if rule_ok {
        if let Some(existing) = existing {
            board.rules.via_rules.remove(existing);
        }
        board.rules.via_rules.add(new_rule);
    }
    rule_ok
}

fn set_value(matrix: &mut ClearanceMatrix, i: i32, j: i32, layer: i32, value: i32) {
    if i < 0 || j < 0 {
        return;
    }
    if layer < 0 {
        matrix.set_value(i as usize, j as usize, value);
    } else {
        matrix.set_value_on_layer(i as usize, j as usize, layer as usize, value);
    }
}

/// Java `Network.getClearanceClass`: the clearance class
/// `<net class>-<item class>`, created from the net class's row if missing.
fn get_clearance_class(
    rules: &mut BoardRules,
    net_class: NetClassId,
    item_class_name: &str,
) -> i32 {
    let net_class_name = rules.net_classes.get(net_class).name().to_string();
    let new_class_name = if item_class_name == "wire" {
        net_class_name.clone()
    } else {
        format!("{net_class_name}{CLASS_CLEARANCE_SEPARATOR}{item_class_name}")
    };
    let found = rules.clearance_matrix.get_no(&new_class_name);
    if found >= 0 {
        return found;
    }
    let matrix = &mut rules.clearance_matrix;
    matrix.append_class(&new_class_name);
    let result = matrix.get_no(&new_class_name);
    let net_class_no = matrix.get_no(&net_class_name);
    if net_class_no < 0 || result < 0 {
        return result;
    }
    for i in 1..matrix.class_count() as i32 {
        for j in 0..matrix.layer_count() as i32 {
            let value = matrix.get_value(net_class_no, i, j, false);
            matrix.set_value_on_layer(result as usize, i as usize, j as usize, value);
            matrix.set_value_on_layer(i as usize, result as usize, j as usize, value);
        }
    }
    let item_class = match item_class_name {
        "via" => Some(ItemClass::Via),
        "pin" => Some(ItemClass::Pin),
        "smd" => Some(ItemClass::Smd),
        "area" => Some(ItemClass::Area),
        _ => None,
    };
    if let Some(item_class) = item_class {
        rules
            .net_classes
            .get_mut(net_class)
            .default_item_clearance_classes
            .set(item_class, result);
    }
    result
}

/// Java `Network.addClearanceRule`: a clearance rule of a net class.
fn add_clearance_rule(
    rules: &mut BoardRules,
    net_class: NetClassId,
    value: f64,
    types: &[String],
    layer: i32,
    transform: &CoordinateTransform,
) {
    let clearance = round_to_i32(transform.dsn_to_board(value));
    let class_name = rules.net_classes.get(net_class).name().to_string();
    let mut class_no = rules.clearance_matrix.get_no(&class_name);
    if class_no < 0 {
        let matrix = &mut rules.clearance_matrix;
        matrix.append_class(&class_name);
        class_no = matrix.get_no(&class_name);
        for i in 1..matrix.class_count() as i32 {
            for j in 0..matrix.layer_count() as i32 {
                let current = matrix.get_value(class_no, i, j, false).max(clearance);
                matrix.set_value_on_layer(class_no as usize, i as usize, j as usize, current);
                matrix.set_value_on_layer(i as usize, class_no as usize, j as usize, current);
            }
        }
        rules
            .net_classes
            .get_mut(net_class)
            .default_item_clearance_classes
            .set_all(class_no);
    }
    rules
        .net_classes
        .get_mut(net_class)
        .set_trace_clearance_class(class_no);
    if types.is_empty() {
        set_value(
            &mut rules.clearance_matrix,
            class_no,
            class_no,
            layer,
            clearance,
        );
        return;
    }
    if contains_wire_clearance_pair(types) {
        for name in ["via", "smd", "pin", "area"] {
            get_clearance_class(rules, net_class, name);
        }
    }
    for current in types {
        let pair = java_split(current, "_");
        if pair.len() != 2 {
            continue;
        }
        let first = get_clearance_class(rules, net_class, &pair[0]);
        let second = get_clearance_class(rules, net_class, &pair[1]);
        set_value(&mut rules.clearance_matrix, first, second, layer, clearance);
        set_value(&mut rules.clearance_matrix, second, first, layer, clearance);
    }
}

/// Java `Network.insertNetClass`.
fn insert_net_class(ctx: &mut ReadContext, dsn_class: &DsnNetClass) {
    let class = dsn_class.class;
    let transform = ctx.coordinate_transform;
    let via_at_smd_allowed = ctx.via_at_smd_allowed;
    let use_layer_nos: Vec<i32> = dsn_class
        .use_layer
        .iter()
        .map(|name| ctx.layer_structure.get_no(name))
        .collect();
    let layer_count = ctx.layer_structure.layers.len();
    let board = ctx.board();
    let net_class = if is_kicad_default_net_class_name(&class.name) {
        board.rules.default_net_class()
    } else {
        board.rules.default_net_class();
        board.rules.append_named_net_class(&class.name)
    };
    // Java's messages name the board's net class, which is the default class
    // (named `default`) for a class called `kicad_default` too.
    let log_name = board.rules.net_classes.get(net_class).name().to_string();
    if let Some(name) = &class.clearance_class {
        let no = board.rules.clearance_matrix.get_no(name);
        if no >= 0 {
            board
                .rules
                .net_classes
                .get_mut(net_class)
                .set_trace_clearance_class(no);
        } else {
            board.log.push(format!(
                "Network.insert_net_class: clearance class not found at '{log_name}'"
            ));
        }
    }
    if let Some(via_rule) = &class.via_rule {
        match board.rules.via_rule_by_name(via_rule) {
            Some(rule) => board
                .rules
                .net_classes
                .get_mut(net_class)
                .set_via_rule(Some(rule)),
            None => board.log.push(format!(
                "Network.insert_net_class: via rule not found at '{log_name}'"
            )),
        }
    }
    if dsn_class.max_trace_length > 0.0 {
        board
            .rules
            .net_classes
            .get_mut(net_class)
            .set_maximum_trace_length(transform.dsn_to_board(dsn_class.max_trace_length));
    }
    if dsn_class.min_trace_length > 0.0 {
        board
            .rules
            .net_classes
            .get_mut(net_class)
            .set_minimum_trace_length(transform.dsn_to_board(dsn_class.min_trace_length));
    }
    for net_name in &class.nets {
        let numbers: Vec<i32> = board
            .rules
            .nets
            .get_all_by_name(net_name)
            .iter()
            .map(|n| n.net_number)
            .collect();
        for number in numbers {
            board
                .rules
                .nets
                .get_mut(number)
                .expect("net")
                .set_class(net_class);
        }
    }
    let mut clearance_rule_found = false;
    for rule in &class.rules {
        match rule {
            ast::Rule::Width(width) => {
                let half_width = round_to_i32(transform.dsn_to_board(width / 2.0));
                board
                    .rules
                    .net_classes
                    .get_mut(net_class)
                    .set_trace_half_width(half_width);
            }
            ast::Rule::Clearance { value, types } => {
                add_clearance_rule(&mut board.rules, net_class, *value, types, -1, &transform);
                clearance_rule_found = true;
            }
            ast::Rule::Length(length) => {
                let class = board.rules.net_classes.get_mut(net_class);
                if length.max > 0.0 {
                    class.set_maximum_trace_length(transform.dsn_to_board(length.max));
                }
                let min = length.min.unwrap_or(0.0);
                if min > 0.0 {
                    class.set_minimum_trace_length(transform.dsn_to_board(min));
                }
            }
        }
    }
    for layer_rule in &class.layer_rules {
        for layer_name in &layer_rule.layers {
            let layer = board.layer_structure.get_no(layer_name);
            if layer < 0 {
                board.log.push(format!(
                    "Network.insert_net_class: layer not found at '{log_name}'"
                ));
                continue;
            }
            for rule in &layer_rule.rules {
                match rule {
                    ast::Rule::Width(width) => {
                        let half_width = round_to_i32(transform.dsn_to_board(width / 2.0));
                        board
                            .rules
                            .net_classes
                            .get_mut(net_class)
                            .set_trace_half_width_on_layer(layer as usize, half_width);
                    }
                    ast::Rule::Clearance { value, types } => {
                        add_clearance_rule(
                            &mut board.rules,
                            net_class,
                            *value,
                            types,
                            layer,
                            &transform,
                        );
                        clearance_rule_found = true;
                    }
                    ast::Rule::Length(_) => board.log.push(format!(
                        "Network.insert_net_class: layer rule type not yet implemented at '{log_name}'"
                    )),
                }
            }
        }
    }
    {
        let c = board.rules.net_classes.get_mut(net_class);
        c.set_pull_tight(class.pull_tight.unwrap_or(true));
        c.set_shove_fixed(class.shove_fixed.unwrap_or(false));
    }
    let is_default = Some(net_class) == board.rules.default_net_class_id();
    let mut via_infos_created = false;
    if clearance_rule_found && !is_default {
        create_default_via_infos(board, net_class, via_at_smd_allowed);
        via_infos_created = true;
    }
    if !dsn_class.use_via.is_empty() {
        create_via_rule(board, &dsn_class.use_via, net_class);
    } else if via_infos_created {
        let name = board.rules.net_classes.get(net_class).name().to_string();
        let padstacks = board.library.padstacks.clone();
        board
            .rules
            .create_default_via_rule(net_class, &name, &padstacks);
    }
    if !dsn_class.use_layer.is_empty() {
        let c = board.rules.net_classes.get_mut(net_class);
        for i in 0..layer_count as i32 {
            c.set_active_routing_layer(i, false);
        }
        for no in &use_layer_nos {
            c.set_active_routing_layer(*no, true);
        }
        for i in 0..layer_count {
            if !c.is_active_routing_layer(i as i32) {
                c.set_trace_half_width_on_layer(i, 0);
            }
        }
    }
}

/// Java `Network.createViaRule`: a via rule with the vias of `use_via` that
/// have the via clearance class of the net class.
fn create_via_rule(board: &mut RoutingBoard, use_via: &[String], net_class: NetClassId) {
    let name = board.rules.net_classes.get(net_class).name().to_string();
    let mut rule = ViaRule::new(name);
    let via_class = board
        .rules
        .net_classes
        .get(net_class)
        .default_item_clearance_classes
        .get(ItemClass::Via);
    for via_name in use_via {
        for (id, info) in board.rules.via_infos.iter() {
            if info.clearance_class == via_class
                && board
                    .library
                    .padstacks
                    .get(info.padstack)
                    .is_some_and(|p| p.name == *via_name)
            {
                rule.append_via(id);
            }
        }
    }
    let rule_id = board.rules.via_rules.add(rule);
    board
        .rules
        .net_classes
        .get_mut(net_class)
        .set_via_rule(Some(rule_id));
}

/// Java `Network.insertClassPairs`. Java walks the remaining names with the
/// outer iterator, so only the pairs of the first known class with the
/// others are used.
///
/// `router.java_quirks.dsn_class_class_pairs = false` (N06) uses every pair.
fn insert_class_pairs(ctx: &mut ReadContext, class_classes: &[ast::ClassClass]) {
    let transform = ctx.coordinate_transform;
    // PARITY(quirk, java_quirks.dsn_class_class_pairs): Network.java:563-593.
    let java_pairs = ctx.java_quirks.dsn_class_class_pairs;
    let board = ctx.board();
    for class_class in class_classes {
        if class_class.classes.len() >= 3 {
            crate::run_stats::record_n06(crate::run_stats::Counter::DsnClassClassPairs);
        }
        if !java_pairs {
            insert_all_class_pairs(board, class_class, &transform);
            continue;
        }
        let mut names = class_class.classes.iter();
        while let Some(first_name) = names.next() {
            let Some(first) = resolve_net_class(&mut board.rules, first_name) else {
                board
                    .log
                    .push("Network.insert_class_pairs: first class not found".into());
                continue;
            };
            for second_name in names.by_ref() {
                let Some(second) = resolve_net_class(&mut board.rules, second_name) else {
                    board
                        .log
                        .push("Network.insert_class_pairs: second class not found".into());
                    continue;
                };
                insert_class_pair_info(board, class_class, first, second, &transform);
            }
        }
    }
}

/// Every pair of the known classes of `class_class`, each once, in list
/// order (N06 `dsn_class_class_pairs = false`).
fn insert_all_class_pairs(
    board: &mut RoutingBoard,
    class_class: &ast::ClassClass,
    transform: &CoordinateTransform,
) {
    let names = &class_class.classes;
    for (i, first_name) in names.iter().enumerate() {
        let Some(first) = resolve_net_class(&mut board.rules, first_name) else {
            board
                .log
                .push("Network.insert_class_pairs: first class not found".into());
            continue;
        };
        for second_name in &names[i + 1..] {
            let Some(second) = resolve_net_class(&mut board.rules, second_name) else {
                board
                    .log
                    .push("Network.insert_class_pairs: second class not found".into());
                continue;
            };
            insert_class_pair_info(board, class_class, first, second, transform);
        }
    }
}

/// Java `Network.insertClassPairInfo`.
fn insert_class_pair_info(
    board: &mut RoutingBoard,
    class_class: &ast::ClassClass,
    first: NetClassId,
    second: NetClassId,
    transform: &CoordinateTransform,
) {
    for rule in &class_class.rules {
        match rule {
            ast::Rule::Clearance { value, types } => {
                add_mixed_clearance_rule(
                    &mut board.rules,
                    first,
                    second,
                    *value,
                    types,
                    -1,
                    transform,
                );
            }
            _ => board
                .log
                .push("Network.insert_class_pair_info: unexpected rule".into()),
        }
    }
    for layer_rule in &class_class.layer_rules {
        for layer_name in &layer_rule.layers {
            let layer = board.layer_structure.get_no(layer_name);
            if layer < 0 {
                board.log.push(format!(
                    "Network.insert_class_pair_info: layer not found at '{layer_name}'"
                ));
                continue;
            }
            for rule in &layer_rule.rules {
                match rule {
                    ast::Rule::Clearance { value, types } => add_mixed_clearance_rule(
                        &mut board.rules,
                        first,
                        second,
                        *value,
                        types,
                        layer,
                        transform,
                    ),
                    _ => board
                        .log
                        .push("Network.insert_class_pair_info: unexpected layer rule type".into()),
                }
            }
        }
    }
}

/// Java `Network.addMixedClearanceRule`.
fn add_mixed_clearance_rule(
    rules: &mut BoardRules,
    first: NetClassId,
    second: NetClassId,
    value: f64,
    types: &[String],
    layer: i32,
    transform: &CoordinateTransform,
) {
    let clearance = round_to_i32(transform.dsn_to_board(value));
    let class_no = |rules: &mut BoardRules, class: NetClassId| {
        let name = rules.net_classes.get(class).name().to_string();
        let mut no = rules.clearance_matrix.get_no(&name);
        if no < 0 {
            rules.clearance_matrix.append_class(&name);
            no = rules.clearance_matrix.get_no(&name);
        }
        no
    };
    let first_no = class_no(rules, first);
    let second_no = class_no(rules, second);
    if types.is_empty() {
        set_value(
            &mut rules.clearance_matrix,
            first_no,
            second_no,
            layer,
            clearance,
        );
        set_value(
            &mut rules.clearance_matrix,
            second_no,
            first_no,
            layer,
            clearance,
        );
        return;
    }
    for current in types {
        let pair = java_split(current, "_");
        if pair.len() != 2 {
            continue;
        }
        for i in 0..2 {
            let (a, b) = if i == 0 {
                (
                    get_clearance_class(rules, first, &pair[0]),
                    get_clearance_class(rules, second, &pair[1]),
                )
            } else {
                (
                    get_clearance_class(rules, second, &pair[0]),
                    get_clearance_class(rules, first, &pair[1]),
                )
            };
            set_value(&mut rules.clearance_matrix, a, b, layer, clearance);
            set_value(&mut rules.clearance_matrix, b, a, layer, clearance);
        }
    }
}

/// Java `Network.insertComponents` and `insertComponent`: adds the placed
/// components with their pins, keepouts and outlines.
pub(crate) fn insert_components(ctx: &mut ReadContext, placement: Option<&ast::Placement>) {
    let Some(placement) = placement else {
        return;
    };
    // Java asks the net list for each pin; the index is built once.
    let nets_by_pin: HashMap<PinKey, Vec<NetKey>> = ctx
        .netlist
        .nets_by_pin()
        .into_iter()
        .map(|(pin, nets)| (pin.clone(), nets.into_iter().cloned().collect()))
        .collect();
    // Java scans the nets for each pin's net; no net is added here.
    let names = NetNames::new(&ctx.board().rules.nets);
    // Placement resolution declares its own unit; its integer is grid
    // precision, not a multiplier on DSN coordinates. Package-local offsets
    // remain in the PCB unit and are not converted here.
    let factor = if ctx.java_quirks.dsn_unit_ignored {
        1.0
    } else {
        placement.resolution.map_or(1.0, |r| {
            crate::board::structure::Unit::scale(
                1.0,
                crate::board::structure::Unit::from(r.unit),
                ctx.unit,
            )
        })
    };
    for component in &placement.components {
        for place in &component.places {
            if factor == 1.0 {
                insert_component(ctx, place, &component.image, &nets_by_pin, &names);
            } else {
                let mut converted = place.clone();
                if let Some(location) = &mut converted.location {
                    location.at.x *= factor;
                    location.at.y *= factor;
                }
                insert_component(ctx, &converted, &component.image, &nets_by_pin, &names);
            }
        }
    }
}

fn insert_component(
    ctx: &mut ReadContext,
    place: &ast::Place,
    image: &str,
    nets_by_pin: &HashMap<PinKey, Vec<NetKey>>,
    names: &NetNames,
) {
    let transform = ctx.coordinate_transform;
    let board = ctx.board();
    let (Some(front), Some(back)) = (
        board
            .library
            .packages
            .get_by_name(image, true)
            .map(|p| p.id),
        board
            .library
            .packages
            .get_by_name(image, false)
            .map(|p| p.id),
    ) else {
        board.log.push(format!(
            "Network.insert_component: component package not found at '{image}'"
        ));
        return;
    };
    let location = place.location;
    let exact_location = location.map(|l| transform.dsn_to_board_point(l.at.x, l.at.y));
    let rotation = location.map_or(0.0, |l| l.rotation);
    let is_front = location.is_none_or(|l| l.side == ast::Side::Front);
    let (position_fixed, part_number) = if location.is_some() {
        (place.position_locked, place.part_number.clone())
    } else {
        (false, None)
    };
    let component_id = board.components.add_precise(
        place.name.clone(),
        exact_location,
        rotation,
        is_front,
        front,
        back,
        position_fixed,
        part_number,
    );
    let component = board.components.get(component_id).expect("component");
    if !component.is_placed() {
        return;
    }
    let component_location = component.location().expect("placed component");
    let translation =
        Point::Int(component_location).difference_by(&Point::Int(IntPoint::new(0, 0)));
    let placed_on_front = component.placed_on_front();
    let package_id = component.package();
    let fixed_state = if position_fixed {
        FixedState::SystemFixed
    } else {
        FixedState::Unfixed
    };
    let pin_infos: HashMap<&str, &str> = place
        .pins
        .iter()
        .map(|p| (p.name.as_str(), p.clearance_class.as_str()))
        .collect();
    let package = board
        .library
        .packages
        .get(package_id)
        .expect("package")
        .clone();
    for (pin_index, package_pin) in package.pins().iter().enumerate() {
        let Some(padstack) = board
            .library
            .padstacks
            .get(package_pin.padstack_id)
            .cloned()
        else {
            board
                .log
                .push("Network.insert_component: pin padstack not found".into());
            return;
        };
        let mut net_numbers: Vec<i32> = Vec::new();
        if let Some(nets) = nets_by_pin.get(&PinKey::new(&place.name, &package_pin.name)) {
            for net in nets {
                match names.get(&board.rules.nets, &net.name.0, net.subnet_number) {
                    Some(board_net) => net_numbers.push(board_net.net_number),
                    None => board
                        .log
                        .push("Network.insert_component: board net not found".into()),
                }
            }
        }
        let net_class = match net_numbers.first().and_then(|n| board.rules.nets.get(*n)) {
            Some(net) => net.net_class(),
            None => board.rules.default_net_class(),
        };
        let mut clearance_class = -1;
        if let Some(name) = pin_infos.get(package_pin.name.as_str()) {
            clearance_class = board.rules.clearance_matrix.get_no(name);
        }
        if clearance_class < 0 {
            let item_class = if padstack.from_layer() == padstack.to_layer() {
                ItemClass::Smd
            } else {
                ItemClass::Pin
            };
            clearance_class = board
                .rules
                .net_classes
                .get(net_class)
                .default_item_clearance_classes
                .get(item_class);
        }
        let pin = board.insert_pin(
            component_id.0,
            pin_index,
            &net_numbers,
            clearance_class,
            fixed_state,
        );
        if pin.is_none() || !padstack.has_auxiliary_shapes() {
            continue;
        }
        let rotate_first = board.components.flip_style_rotate_first();
        let transform_shape = board
            .components
            .get(component_id)
            .expect("component")
            .placement_transform(rotate_first);
        let layer_count = padstack.board_layer_count() as i32;
        for pad_layer in 0..layer_count {
            let Some(aux_shapes) = padstack.get_auxiliary_shapes(pad_layer) else {
                continue;
            };
            if aux_shapes.is_empty() {
                continue;
            }
            let board_layer = if placed_on_front || padstack.placed_absolute {
                pad_layer
            } else {
                layer_count - pad_layer - 1
            };
            if board_layer < 0 || board_layer >= board.layer_count() as i32 {
                continue;
            }
            for aux in aux_shapes {
                let shape = transform_shape.shape(
                    &aux.to_shape(),
                    package_pin.exact_relative_location(),
                    package_pin.rotation_in_degree,
                );
                let area = match shape {
                    Shape::Polyline(p) => Area::PolylineArea(PolylineArea::new(p, Vec::new())),
                    circle => Area::Shape(circle),
                };
                if !area.is_empty() {
                    board.insert_area(
                        AreaKind::Obstacle,
                        area,
                        board_layer,
                        Vector::ZERO,
                        0.0,
                        false,
                        &net_numbers,
                        clearance_class,
                        component_id.0,
                        Some(format!("{}_aux", package_pin.name)),
                        fixed_state,
                    );
                }
            }
        }
    }
    insert_component_keepouts(
        board,
        place,
        &package,
        component_id,
        &translation,
        rotation,
        is_front,
        fixed_state,
    );
    insert_component_outlines(
        board,
        &package,
        component_id,
        &translation,
        rotation,
        is_front,
        fixed_state,
    );
}

#[allow(clippy::too_many_arguments)]
fn insert_component_keepouts(
    board: &mut RoutingBoard,
    place: &ast::Place,
    package: &crate::library::Package,
    component_id: ComponentId,
    translation: &Vector,
    rotation: f64,
    is_front: bool,
    fixed_state: FixedState,
) {
    let layer_count = board.layer_count() as i32;
    for k in 0..3 {
        let (keepouts, infos, kind) = match k {
            0 => (&package.keepouts, &place.keepouts, AreaKind::Obstacle),
            1 => (
                &package.via_keepouts,
                &place.via_keepouts,
                AreaKind::ViaObstacle,
            ),
            _ => (
                &package.place_keepouts,
                &place.place_keepouts,
                AreaKind::ComponentObstacle,
            ),
        };
        let infos: HashMap<&str, &str> = infos
            .iter()
            .map(|i| (i.name.as_str(), i.clearance_class.as_str()))
            .collect();
        for keepout in keepouts {
            let mut layer = keepout.layer;
            if layer >= layer_count {
                board
                    .log
                    .push("Network.insert_component: keepout layer is to big".into());
                continue;
            }
            if layer >= 0 && !is_front {
                layer = layer_count - keepout.layer - 1;
            }
            let default = board.rules.default_net_class();
            let mut clearance_class = board
                .rules
                .net_classes
                .get(default)
                .default_item_clearance_classes
                .get(ItemClass::Area);
            if let Some(name) = infos.get(keepout.name.as_str()) {
                let no = board.rules.clearance_matrix.get_no(name);
                if no > 0 {
                    clearance_class = no;
                }
            }
            let layers: Vec<i32> = if layer >= 0 {
                vec![layer]
            } else {
                (0..layer_count)
                    .filter(|j| board.layer_structure.layers[*j as usize].is_signal)
                    .collect()
            };
            for layer in layers {
                board.insert_area(
                    kind,
                    keepout.area.clone(),
                    layer,
                    translation.clone(),
                    rotation,
                    !is_front,
                    &[],
                    clearance_class,
                    component_id.0,
                    Some(keepout.name.clone()),
                    fixed_state,
                );
            }
        }
    }
}

fn insert_component_outlines(
    board: &mut RoutingBoard,
    package: &crate::library::Package,
    component_id: ComponentId,
    translation: &Vector,
    rotation: f64,
    is_front: bool,
    fixed_state: FixedState,
) {
    let Some(outlines) = &package.outline else {
        return;
    };
    let mut courtyard_index: Option<usize> = None;
    if outlines.len() > 1 {
        let mut max_area = -1.0;
        for (i, outline) in outlines.iter().enumerate() {
            if let Some(shape) = outline {
                let b = shape.bounding_box();
                let area = f64::from(b.width()) * f64::from(b.height());
                if area > max_area {
                    max_area = area;
                    courtyard_index = Some(i);
                }
            }
        }
    }
    let widths = package.outline_widths.as_deref().unwrap_or(&[]);
    let closed = package.outline_is_closed.as_deref().unwrap_or(&[]);
    for (i, outline) in outlines.iter().enumerate() {
        let mut is_courtyard = Some(i) == courtyard_index;
        if widths.get(i).is_some_and(|w| *w == 0.0) {
            is_courtyard = true;
        }
        let is_fabrication = !is_courtyard && widths.get(i).is_some_and(|w| *w <= 110.0);
        let is_closed = closed.get(i).copied().unwrap_or(false);
        let Some(shape) = outline else {
            board
                .log
                .push("BasicBoard.insert_component_outline: area is null".into());
            continue;
        };
        board.insert_component_outline(
            Area::Shape(shape.clone()),
            is_front,
            translation.clone(),
            rotation,
            component_id.0,
            is_courtyard,
            is_fabrication,
            is_closed,
            fixed_state,
        );
    }
}

/// Java `Network.insertLogicalParts`. Stops at the first part it can't
/// resolve, as Java does.
pub(crate) fn insert_logical_parts(ctx: &mut ReadContext, part_library: Option<&ast::PartLibrary>) {
    let Some(part_library) = part_library else {
        return;
    };
    let board = ctx.board();
    for part in &part_library.parts {
        let Some(package_id) = search_lib_package(board, &part.name, &part_library.mappings) else {
            return;
        };
        let package = board.library.packages.get(package_id).expect("package");
        let mut pins = Vec::with_capacity(part.pins.len());
        for pin in &part.pins {
            let Some(pin_index) = package.get_pin_index(&pin.name) else {
                board.log.push(format!(
                    "Network.insert_logical_parts: package pin not found at '{}'",
                    pin.name
                ));
                return;
            };
            pins.push(PartPin {
                pin_index,
                pin_name: pin.name.clone(),
                gate_name: pin.gate.clone(),
                gate_swap_code: pin.gate_swap_code,
                gate_pin_name: pin.gate_pin.clone(),
                gate_pin_swap_code: pin.gate_pin_swap_code,
            });
        }
        board.library.logical_parts.add(part.name.clone(), pins);
    }
    for mapping in &part_library.mappings {
        let part = board
            .library
            .logical_parts
            .get_by_name(&mapping.name)
            .map(|p| p.id);
        if part.is_none() {
            board.log.push(format!(
                "Network.insert_logical_parts: logical part not found at '{}'",
                mapping.name
            ));
        }
        for component_name in &mapping.components {
            let id = board.components.get_by_name(component_name).map(|c| c.id);
            match id {
                Some(id) => board.components.set_logical_part(id, part),
                None => board.log.push(format!(
                    "Network.insert_logical_parts: board component not found at '{component_name}'"
                )),
            }
        }
    }
}

/// Java `Network.searchLibPackage`: the package of the first component
/// mapped to the part.
fn search_lib_package(
    board: &mut RoutingBoard,
    part_name: &str,
    mappings: &[ast::LogicalPartMapping],
) -> Option<crate::library::PackageId> {
    for mapping in mappings {
        if mapping.name != part_name {
            continue;
        }
        let Some(component_name) = mapping.components.first() else {
            board.log.push(format!(
                "Network.search_lib_package: component list empty at '{part_name}'"
            ));
            return None;
        };
        let Some(component) = board.components.get_by_name(component_name) else {
            board.log.push(format!(
                "Network.search_lib_package: component not found at '{component_name}'"
            ));
            return None;
        };
        return Some(component.package());
    }
    board.log.push(format!(
        "Network.search_lib_package: library package '{part_name}' not found"
    ));
    None
}

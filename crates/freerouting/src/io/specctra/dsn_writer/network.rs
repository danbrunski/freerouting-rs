//! The network scope: nets with their pins, via types, via rules and net classes.
//!
//! Port of `Network.writeScope`, `Net.writeScope`, `writeViaInfos`, `writeViaRules`,
//! `writeNetClasses`, `writeNetClass` and `writeCircuit`.

use std::collections::{HashMap, HashSet};

use freerouting_dsn::ast::{
    Circuit, LayerRule, LengthRule, Net, NetClass, Network, NetworkVia, PinRef, Rule, ViaRule,
};

use super::Cx;
use super::rules::ViaPlan;
use crate::board::structure::ComponentId;
use crate::rules::{NetClassId, NetLengthConstraint};

pub(super) fn network(cx: &Cx, owned: &HashSet<NetClassId>, via: Option<&ViaPlan>) -> Network {
    let board = cx.board;
    let max_net = usize::try_from(board.rules.nets.max_net_number()).unwrap_or(0);
    // One pass: the pins of every net in item list order (Java scans all pins per net).
    let mut pins: Vec<Vec<PinRef>> = vec![Vec::new(); max_net + 1];
    for item in &cx.items {
        let Some(pin) = item.as_pin() else {
            continue;
        };
        let Some(component) = board.components.get(ComponentId(item.component_id())) else {
            continue;
        };
        let Some(pin_name) = board
            .library
            .packages
            .get(component.package())
            .and_then(|p| p.get_pin(pin.pin_index))
            .map(|p| p.name.as_str())
        else {
            continue;
        };
        for &net in item.net_numbers() {
            if let Some(list) = usize::try_from(net).ok().and_then(|n| pins.get_mut(n)) {
                list.push(PinRef::new(component.name.as_str(), pin_name));
            }
        }
    }
    let nets = (1..=max_net)
        .filter_map(|number| {
            let net = board.rules.nets.get(number as i32)?;
            let circuit = net
                .explicit_length_constraint()
                .filter(NetLengthConstraint::is_constrained)
                .map(|constraint| Circuit {
                    length: Some(length_rule(
                        cx,
                        constraint.max_length,
                        constraint.min_length,
                    )),
                    use_via: Vec::new(),
                    use_layer: Vec::new(),
                });
            Some(Net {
                name: net.name.clone(),
                subnet: Some(net.subnet_number),
                pins: std::mem::take(&mut pins[number]),
                ordered: false,
                fromtos: Vec::new(),
                rules: Vec::new(),
                circuit,
            })
        })
        .collect();
    Network {
        nets,
        vias: via_infos(cx, owned),
        via_rules: if via.is_some() {
            Vec::new()
        } else {
            via_rules(cx, owned)
        },
        classes: net_classes(cx, owned, via),
        class_classes: Vec::new(),
    }
}

/// `(length max min)`: Java writes -1 for no maximum and 0 for no minimum.
fn length_rule(cx: &Cx, max: f64, min: f64) -> LengthRule {
    LengthRule {
        max: if max <= 0.0 {
            -1.0
        } else {
            cx.transform.board_to_dsn(max)
        },
        min: Some(if min <= 0.0 {
            0.0
        } else {
            cx.transform.board_to_dsn(min)
        }),
    }
}

/// `Network.writeViaInfos`. The reader makes a via type `<padstack>-<class>` of the clearance
/// class of each net class that creates its clearance class (`createDefaultViaInfos`), and
/// those classes do not exist yet when it reads the via types, so these are left out.
fn via_infos(cx: &Cx, owned: &HashSet<NetClassId>) -> Vec<NetworkVia> {
    let board = cx.board;
    let made_by_reader: HashSet<(String, i32)> = owned
        .iter()
        .map(|id| board.rules.net_classes.get(*id))
        .flat_map(|class| {
            let index = class.trace_clearance_class();
            board
                .library
                .via_padstack_ids()
                .iter()
                .filter_map(|id| board.library.padstacks.get(*id))
                .map(move |p| (format!("{}-{}", p.name, class.name()), index))
        })
        .collect();
    board
        .rules
        .via_infos
        .iter()
        .filter(|(_, info)| !made_by_reader.contains(&(info.name.clone(), info.clearance_class)))
        .map(|(_, info)| NetworkVia {
            name: info.name.clone(),
            padstack: board
                .library
                .padstacks
                .get(info.padstack)
                .map(|p| p.name.clone())
                .unwrap_or_default(),
            clearance_class: cx.class_name(info.clearance_class),
            attach: info.attach_smd_allowed,
        })
        .collect()
}

/// `Network.writeViaRules`. The rule of an owned class is left out: the reader makes it again
/// from the via infos when it reads the class's clearance rule.
fn via_rules(cx: &Cx, owned: &HashSet<NetClassId>) -> Vec<ViaRule> {
    let rules = &cx.board.rules;
    let skipped: HashSet<_> = owned
        .iter()
        .filter_map(|id| rules.net_classes.get(*id).via_rule())
        .collect();
    rules
        .via_rules
        .iter()
        .filter(|(id, _)| !skipped.contains(id))
        .map(|(_, rule)| ViaRule {
            name: rule.name.clone(),
            vias: rule
                .vias()
                .iter()
                .map(|id| rules.via_infos.get(*id).name.clone())
                .collect(),
        })
        .collect()
}

/// `Network.writeNetClasses` and `writeNetClass`.
fn net_classes(cx: &Cx, owned: &HashSet<NetClassId>, via: Option<&ViaPlan>) -> Vec<NetClass> {
    let board = cx.board;
    let rules = &board.rules;
    let layer_count = board.layer_count();
    // The nets of each class in net number order.
    let mut members: HashMap<NetClassId, Vec<String>> = HashMap::new();
    for net in rules.nets.iter() {
        members
            .entry(net.net_class())
            .or_default()
            .push(net.name.clone());
    }
    rules
        .net_classes
        .iter()
        .map(|(id, class)| {
            let width = |layer: usize| {
                2.0 * cx
                    .transform
                    .board_to_dsn(f64::from(class.trace_half_width(layer as i32)))
            };
            let default_half_width = class.trace_half_width(0);
            let layer_rules = (1..layer_count)
                .filter(|&l| class.trace_half_width(l as i32) != default_half_width)
                .map(|l| LayerRule {
                    layers: vec![cx.layer_name(l as i32)],
                    rules: vec![Rule::Width(width(l))],
                })
                .collect();
            let (min, max) = (class.minimum_trace_length(), class.maximum_trace_length());
            let is_owned = owned.contains(&id);
            let mut class_rules = vec![Rule::Width(width(0))];
            if is_owned {
                // The reader creates the clearance class of the net class from this rule.
                let index = class.trace_clearance_class();
                class_rules.push(Rule::Clearance {
                    value: cx.transform.board_to_dsn(f64::from(
                        board
                            .rules
                            .clearance_matrix
                            .get_value(index, index, 0, false),
                    )),
                    types: Vec::new(),
                });
            }
            NetClass {
                name: class.name().to_string(),
                nets: members.remove(&id).unwrap_or_default(),
                clearance_class: (!is_owned).then(|| cx.class_name(class.trace_clearance_class())),
                via_rule: if is_owned || via.is_some() {
                    None
                } else {
                    class
                        .via_rule()
                        .map(|rule| rules.via_rules.get(rule).name.clone())
                },
                rules: class_rules,
                layer_rules,
                circuit: Some(Circuit {
                    length: (min > 0.0 || max > 0.0).then(|| length_rule(cx, max, min)),
                    use_via: via
                        .and_then(|v| v.use_via.get(&id))
                        .cloned()
                        .unwrap_or_default(),
                    use_layer: if all_signal_layers_active(cx, class) {
                        Vec::new()
                    } else {
                        (0..layer_count)
                            .filter(|&l| class.is_active_routing_layer(l as i32))
                            .map(|l| cx.layer_name(l as i32))
                            .collect()
                    },
                }),
                shove_fixed: class.is_shove_fixed().then_some(true),
                pull_tight: (!class.pull_tight()).then_some(false),
            }
        })
        .collect()
}

/// True if the class routes on exactly the signal layers, which is what a class has until a
/// `use_layer` says otherwise. The reader sets the trace width of every layer outside a
/// `use_layer` to 0, so Java's `(use_layer ...)` of such a class would zero the width of the
/// power layers; Rust leaves the scope out.
fn all_signal_layers_active(cx: &Cx, class: &crate::rules::NetClass) -> bool {
    cx.board
        .layer_structure
        .layers
        .iter()
        .enumerate()
        .all(|(l, layer)| class.is_active_routing_layer(l as i32) == layer.is_signal)
}

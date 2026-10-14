//! The design file writer.

use super::Writer;
use crate::ast::*;

pub(crate) fn write_pcb(pcb: &Pcb) -> String {
    let mut w = Writer::new();
    w.open("pcb");
    w.name(&pcb.name);
    if let Some(parser) = &pcb.parser {
        w.parser_info(parser);
    }
    if let Some(resolution) = &pcb.resolution {
        w.resolution(resolution);
    }
    if let Some(unit) = pcb.unit {
        w.open("unit");
        w.word(unit.name());
        w.close();
    }
    if let Some(structure) = &pcb.structure {
        w.structure(structure);
    }
    if let Some(placement) = &pcb.placement {
        w.placement(placement);
    }
    if let Some(library) = &pcb.library {
        w.library(library);
    }
    if let Some(part_library) = &pcb.part_library {
        w.part_library(part_library);
    }
    if let Some(network) = &pcb.network {
        w.network(network);
    }
    if let Some(wiring) = &pcb.wiring {
        w.open("wiring");
        for item in &wiring.items {
            w.wiring_item(item);
        }
        w.close();
    }
    w.finish()
}

impl Writer {
    fn structure(&mut self, structure: &Structure) {
        self.open("structure");
        // Layers first: Java builds its layer list when it reaches the first
        // keepout, plane or autoroute_settings scope.
        for layer in &structure.layers {
            self.layer(layer);
        }
        for boundary in &structure.boundaries {
            self.open("boundary");
            for shape in &boundary.shapes {
                self.shape(shape);
            }
            if let Some(class) = &boundary.clearance_class {
                self.name_scope("clearance_class", class);
            }
            self.close();
        }
        if let Some(via) = &structure.via {
            self.open("via");
            self.names(&via.padstacks);
            if !via.spare.is_empty() {
                self.open("spare");
                self.names(&via.spare);
                self.close();
            }
            self.close();
        }
        self.rules(&structure.rules);
        if let Some(angle) = structure.snap_angle {
            self.snap_angle(angle);
        }
        if let Some(control) = &structure.control {
            self.open("control");
            if let Some(via_at_smd) = control.via_at_smd {
                self.on_off("via_at_smd", via_at_smd);
            }
            self.close();
        }
        if let Some(rotate_first) = structure.flip_style_rotate_first {
            self.flip_style(rotate_first);
        }
        // Java reads autoroute_settings only before the first keepout or plane.
        if let Some(settings) = &structure.autoroute_settings {
            self.autoroute_settings(settings);
        }
        for keepout in &structure.keepouts {
            self.keepout(keepout);
        }
        for plane in &structure.planes {
            self.open("plane");
            self.name(&plane.net);
            self.area(&plane.area);
            self.close();
        }
        self.close();
    }

    fn layer(&mut self, layer: &Layer) {
        self.open("layer");
        self.name(&layer.name);
        if let Some(layer_type) = &layer.layer_type {
            self.open("type");
            match layer_type {
                LayerType::Signal => self.word("signal"),
                LayerType::Power => self.word("power"),
                LayerType::Jumper => self.word("jumper"),
                LayerType::Other(other) => self.quoted(other),
            }
            self.close();
        }
        self.rules(&layer.rules);
        if !layer.use_net.is_empty() {
            self.open("use_net");
            self.names(&layer.use_net);
            self.close();
        }
        self.close();
    }

    fn library(&mut self, library: &Library) {
        self.open("library");
        for image in &library.images {
            self.open("image");
            self.name(&image.name);
            if let Some(side) = image.side {
                self.open("side");
                self.word(match side {
                    Side::Front => "front",
                    Side::Back => "back",
                });
                self.close();
            }
            for outline in &image.outlines {
                self.open("outline");
                self.shape(outline);
                self.close();
            }
            for pin in &image.pins {
                self.open("pin");
                self.name(&pin.padstack);
                if let Some(rotation) = pin.rotation {
                    self.open_inline("rotate");
                    self.number(rotation);
                    self.close();
                }
                self.name(&pin.name);
                self.number(pin.at.x);
                self.number(pin.at.y);
                self.close();
            }
            for keepout in &image.keepouts {
                self.keepout(keepout);
            }
            self.close();
        }
        for padstack in &library.padstacks {
            self.padstack(padstack);
        }
        self.close();
    }

    fn part_library(&mut self, library: &PartLibrary) {
        self.open("part_library");
        for mapping in &library.mappings {
            self.open("logical_part_mapping");
            self.name(&mapping.name);
            self.open("comp");
            self.names(&mapping.components);
            self.close();
            self.close();
        }
        for part in &library.parts {
            self.open("logical_part");
            self.name(&part.name);
            for pin in &part.pins {
                self.open("pin");
                self.name(&pin.name);
                self.integer(pin.code);
                self.name(&pin.gate);
                self.integer(pin.gate_swap_code);
                self.name(&pin.gate_pin);
                self.integer(pin.gate_pin_swap_code);
                self.close();
            }
            self.close();
        }
        self.close();
    }

    fn network(&mut self, network: &Network) {
        self.open("network");
        for net in &network.nets {
            self.open("net");
            self.name(&net.name);
            if let Some(subnet) = net.subnet {
                self.integer(subnet);
            }
            if !net.pins.is_empty() || net.ordered {
                self.pin_list(if net.ordered { "order" } else { "pins" }, &net.pins);
            }
            for fromto in &net.fromtos {
                self.pin_list("fromto", fromto);
            }
            self.rules(&net.rules);
            if let Some(circuit) = &net.circuit {
                self.circuit(circuit);
            }
            self.close();
        }
        for via in &network.vias {
            self.network_via(via);
        }
        for rule in &network.via_rules {
            self.via_rule(rule);
        }
        for class in &network.classes {
            self.class(class);
        }
        for class_class in &network.class_classes {
            self.open("class_class");
            self.open("classes");
            self.names(&class_class.classes);
            self.close();
            self.rules(&class_class.rules);
            for layer_rule in &class_class.layer_rules {
                self.layer_rule(layer_rule);
            }
            self.close();
        }
        self.close();
    }
}

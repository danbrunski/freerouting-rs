//! Scopes shared by design, session and rules files.

use super::{POINTS_PER_LINE, Writer, quote};
use crate::ast::*;

impl Writer {
    pub fn layer_ref(&mut self, layer: &LayerRef) {
        match layer {
            LayerRef::Named(name) => self.name(name),
            other => self.word(other.name()),
        }
    }

    fn points(&mut self, points: &[Point]) {
        for (i, point) in points.iter().enumerate() {
            if points.len() > POINTS_PER_LINE && i % POINTS_PER_LINE == 0 {
                self.line_break();
            }
            self.number(point.x);
            self.number(point.y);
        }
    }

    pub fn shape(&mut self, shape: &Shape) {
        match shape {
            Shape::Rect {
                layer,
                x1,
                y1,
                x2,
                y2,
            } => {
                self.open("rect");
                self.layer_ref(layer);
                for value in [x1, y1, x2, y2] {
                    self.number(*value);
                }
            }
            Shape::Circle {
                layer,
                diameter,
                center,
            } => {
                self.open("circle");
                self.layer_ref(layer);
                self.number(*diameter);
                self.number(center.x);
                self.number(center.y);
            }
            Shape::Polygon {
                layer,
                aperture_width,
                points,
            } => {
                self.open("polygon");
                self.layer_ref(layer);
                self.number(*aperture_width);
                self.points(points);
            }
            Shape::Path {
                layer,
                width,
                points,
            } => {
                self.open("path");
                self.layer_ref(layer);
                self.number(*width);
                self.points(points);
            }
            Shape::PolylinePath {
                layer,
                width,
                points,
            } => {
                self.open("polyline_path");
                self.layer_ref(layer);
                self.number(*width);
                self.points(points);
            }
        }
        self.close();
    }

    pub fn window(&mut self, shape: &Shape) {
        self.open("window");
        self.shape(shape);
        self.close();
    }

    pub fn area(&mut self, area: &Area) {
        if let Some(name) = &area.name {
            self.name(name);
        }
        self.shape(&area.shape);
        for window in &area.windows {
            self.window(window);
        }
        if let Some(class) = &area.clearance_class {
            self.name_scope("clearance_class", class);
        }
    }

    pub fn keepout(&mut self, keepout: &Keepout) {
        self.open(keepout.kind.keyword());
        self.area(&keepout.area);
        self.close();
    }

    fn length(&mut self, length: &LengthRule) {
        self.open("length");
        self.number(length.max);
        if let Some(min) = length.min {
            self.number(min);
        }
        self.close();
    }

    pub fn rule_items(&mut self, rules: &[Rule]) {
        for rule in rules {
            match rule {
                Rule::Width(width) => {
                    self.open("width");
                    self.number(*width);
                    self.close();
                }
                Rule::Clearance { value, types } => {
                    self.open("clearance");
                    self.number(*value);
                    if !types.is_empty() {
                        self.open_inline("type");
                        for name in types {
                            self.word(&quote(name, Some('-')));
                        }
                        self.close();
                    }
                    self.close();
                }
                Rule::Length(length) => self.length(length),
            }
        }
    }

    /// `(rule ...)`, written only if there are rules.
    pub fn rules(&mut self, rules: &[Rule]) {
        if !rules.is_empty() {
            self.open("rule");
            self.rule_items(rules);
            self.close();
        }
    }

    pub fn layer_rule(&mut self, layer_rule: &LayerRule) {
        self.open("layer_rule");
        self.names(&layer_rule.layers);
        self.rules(&layer_rule.rules);
        self.close();
    }

    pub fn circuit(&mut self, circuit: &Circuit) {
        self.open("circuit");
        if let Some(length) = &circuit.length {
            self.length(length);
        }
        if !circuit.use_via.is_empty() {
            self.open("use_via");
            self.names(&circuit.use_via);
            self.close();
        }
        if !circuit.use_layer.is_empty() {
            self.open("use_layer");
            self.names(&circuit.use_layer);
            self.close();
        }
        self.close();
    }

    pub fn resolution(&mut self, resolution: &Resolution) {
        self.open("resolution");
        self.word(resolution.unit.name());
        self.integer(resolution.value);
        self.close();
    }

    pub fn snap_angle(&mut self, angle: SnapAngle) {
        self.open("snap_angle");
        self.word(angle.keyword());
        self.close();
    }

    pub fn flip_style(&mut self, rotate_first: bool) {
        self.open("flip_style");
        self.word(if rotate_first {
            "rotate_first"
        } else {
            "mirror_first"
        });
        self.close();
    }

    pub fn parser_info(&mut self, info: &ParserInfo) {
        self.open("parser");
        if let Some(quote_char) = &info.string_quote {
            self.open("string_quote");
            self.word(quote_char);
            self.close();
        }
        if let Some(space) = info.space_in_quoted_tokens {
            self.on_off("space_in_quoted_tokens", space);
        }
        if let Some(host_cad) = &info.host_cad {
            self.name_scope("host_cad", host_cad);
        }
        if let Some(host_version) = &info.host_version {
            self.name_scope("host_version", host_version);
        }
        for (name, value) in &info.constants {
            self.open("constant");
            self.name(name);
            self.name(value);
            self.close();
        }
        if let Some(resolution) = &info.write_resolution {
            self.open("write_resolution");
            self.name(&resolution.name);
            self.integer(resolution.value);
            self.close();
        }
        if info.generated_by_freerouting {
            self.open("generated_by_freeroute");
            self.close();
        }
        self.close();
    }

    pub fn padstack(&mut self, padstack: &Padstack) {
        self.open("padstack");
        self.name(&padstack.name);
        for shape in &padstack.shapes {
            self.open("shape");
            self.shape(shape);
            self.close();
        }
        if let Some(attach) = padstack.attach {
            self.on_off("attach", attach);
        }
        if let Some(absolute) = padstack.absolute {
            self.on_off("absolute", absolute);
        }
        self.close();
    }

    pub fn placement(&mut self, placement: &Placement) {
        self.open("placement");
        if let Some(resolution) = &placement.resolution {
            self.resolution(resolution);
        }
        if let Some(rotate_first) = placement.flip_style_rotate_first {
            self.open("place_control");
            self.flip_style(rotate_first);
            self.close();
        }
        for component in &placement.components {
            self.open("component");
            self.name(&component.image);
            for place in &component.places {
                self.place(place);
            }
            self.close();
        }
        self.close();
    }

    fn place(&mut self, place: &Place) {
        self.open("place");
        self.name(&place.name);
        if let Some(location) = &place.location {
            self.number(location.at.x);
            self.number(location.at.y);
            self.word(match location.side {
                Side::Front => "front",
                Side::Back => "back",
            });
            self.number(location.rotation);
            if place.position_locked {
                self.open_inline("lock_type");
                self.word("position");
                self.close();
            }
            for (keyword, items) in [
                ("pin", &place.pins),
                ("keepout", &place.keepouts),
                ("via_keepout", &place.via_keepouts),
                ("place_keepout", &place.place_keepouts),
            ] {
                for item in items {
                    self.open(keyword);
                    self.name(&item.name);
                    self.open_inline("clearance_class");
                    self.name(&item.clearance_class);
                    self.close();
                    self.close();
                }
            }
            if let Some(part_number) = &place.part_number {
                self.open_inline("PN");
                self.name(part_number);
                self.close();
            }
        }
        self.close();
    }

    pub fn pin_ref(&mut self, pin: &PinRef) {
        let component = quote(&pin.component, Some('-'));
        let pin = quote(&pin.pin, None);
        self.word(&format!("{component}-{pin}"));
    }

    pub fn pin_list(&mut self, keyword: &str, pins: &[PinRef]) {
        self.open(keyword);
        for (i, pin) in pins.iter().enumerate() {
            if i > 0 && i % super::NAMES_PER_LINE == 0 {
                self.line_break();
            }
            self.pin_ref(pin);
        }
        self.close();
    }

    pub fn network_via(&mut self, via: &NetworkVia) {
        self.open("via");
        self.name(&via.name);
        self.name(&via.padstack);
        self.name(&via.clearance_class);
        if via.attach {
            self.word("attach");
        }
        self.close();
    }

    pub fn via_rule(&mut self, rule: &ViaRule) {
        self.open("via_rule");
        self.name(&rule.name);
        self.names(&rule.vias);
        self.close();
    }

    pub fn class(&mut self, class: &NetClass) {
        self.open("class");
        self.name(&class.name);
        if !class.nets.is_empty() {
            self.line_break();
            self.names(&class.nets);
        }
        if let Some(clearance_class) = &class.clearance_class {
            self.name_scope("clearance_class", clearance_class);
        }
        if let Some(via_rule) = &class.via_rule {
            self.name_scope("via_rule", via_rule);
        }
        self.rules(&class.rules);
        for layer_rule in &class.layer_rules {
            self.layer_rule(layer_rule);
        }
        if let Some(circuit) = &class.circuit {
            self.circuit(circuit);
        }
        if let Some(shove_fixed) = class.shove_fixed {
            self.on_off("shove_fixed", shove_fixed);
        }
        if let Some(pull_tight) = class.pull_tight {
            self.on_off("pull_tight", pull_tight);
        }
        self.close();
    }

    fn net_id(&mut self, net: &NetId) {
        self.open("net");
        self.name(&net.name);
        if let Some(subnet) = net.subnet {
            self.integer(subnet);
        }
        self.close();
    }

    fn wire_type(&mut self, wire_type: &WireType) {
        self.open("type");
        match wire_type {
            WireType::Normal => self.word("normal"),
            WireType::Fix => self.word("fix"),
            WireType::ShoveFixed => self.word("shove_fixed"),
            WireType::Protect => self.word("protect"),
            WireType::Other(other) => self.quoted(other),
        }
        self.close();
    }

    pub fn wiring_item(&mut self, item: &WiringItem) {
        match item {
            WiringItem::Wire(wire) => {
                self.open("wire");
                for shape in &wire.shapes {
                    self.shape(shape);
                }
                for window in &wire.windows {
                    self.window(window);
                }
                if let Some(net) = &wire.net {
                    self.net_id(net);
                }
                if let Some(class) = &wire.clearance_class {
                    self.name_scope("clearance_class", class);
                }
                if let Some(wire_type) = &wire.wire_type {
                    self.wire_type(wire_type);
                }
                self.close();
            }
            WiringItem::Via(via) => {
                self.open("via");
                self.name(&via.padstack);
                self.number(via.at.x);
                self.number(via.at.y);
                if let Some(net) = &via.net {
                    self.net_id(net);
                }
                if let Some(class) = &via.clearance_class {
                    self.name_scope("clearance_class", class);
                }
                if let Some(via_type) = &via.via_type {
                    self.wire_type(via_type);
                }
                self.close();
            }
        }
    }

    pub fn autoroute_settings(&mut self, settings: &AutorouteSettings) {
        self.open("autoroute_settings");
        for (keyword, value) in [
            ("fanout", settings.fanout),
            ("autoroute", settings.autoroute),
            ("postroute", settings.postroute),
            ("vias", settings.vias),
        ] {
            if let Some(value) = value {
                self.on_off(keyword, value);
            }
        }
        for (keyword, value) in [
            ("via_costs", settings.via_costs),
            ("plane_via_costs", settings.plane_via_costs),
            ("start_ripup_costs", settings.start_ripup_costs),
            ("start_pass_no", settings.start_pass_no),
        ] {
            if let Some(value) = value {
                self.open(keyword);
                self.integer(value);
                self.close();
            }
        }
        for rule in &settings.layer_rules {
            self.open("layer_rule");
            self.name(&rule.layer);
            if let Some(active) = rule.active {
                self.on_off("active", active);
            }
            if let Some(direction) = rule.preferred_direction {
                self.open("preferred_direction");
                self.word(match direction {
                    Direction::Horizontal => "horizontal",
                    Direction::Vertical => "vertical",
                });
                self.close();
            }
            for (keyword, value) in [
                (
                    "preferred_direction_trace_costs",
                    rule.preferred_direction_trace_costs,
                ),
                (
                    "against_preferred_direction_trace_costs",
                    rule.against_preferred_direction_trace_costs,
                ),
            ] {
                if let Some(value) = value {
                    self.open(keyword);
                    self.number(value);
                    self.close();
                }
            }
            self.close();
        }
        self.close();
    }
}

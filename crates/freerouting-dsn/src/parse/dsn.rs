//! The design file: `(pcb name ...)`.

use super::common::is_shape_keyword;
use super::{Item, Parser};
use crate::ast::*;
use crate::error::{Error, Pos, Result};
use crate::lexer::TokenKind;

pub(crate) fn parse_pcb(parser: &mut Parser<'_>) -> Result<Pcb> {
    let (keyword, pos) = parser.header()?;
    if keyword != "pcb" {
        return Err(Error::new(
            pos,
            format!("not a Specctra design file: '(pcb' expected, found '({keyword}'"),
        ));
    }
    let mut pcb = Pcb {
        name: parser.optional_name()?.unwrap_or_default(),
        ..Pcb::default()
    };
    loop {
        let Some(item) = parser.item_or_eof("pcb")? else {
            let pos = parser.lexer.current_pos();
            parser.warn(pos, "end of file before the closing bracket of pcb");
            break;
        };
        match item {
            Item::End => break,
            Item::Scope(keyword, pos) => match keyword.as_str() {
                "parser" => {
                    parser.parser_scope(pcb.parser.get_or_insert_with(ParserInfo::default))?
                }
                "resolution" => pcb.resolution = Some(parser.resolution_scope()?),
                "unit" => {
                    if let Some(unit) = parser.unit_scope()? {
                        pcb.unit = Some(unit);
                    }
                }
                "structure" => {
                    parser.structure_scope(pcb.structure.get_or_insert_with(Structure::default))?
                }
                "placement" => {
                    parser.placement_scope(pcb.placement.get_or_insert_with(Placement::default))?
                }
                // Java's generic scope reader also accepts these directly in
                // the pcb scope.
                "component" => {
                    let component = parser.component_scope()?;
                    pcb.placement
                        .get_or_insert_with(Placement::default)
                        .components
                        .push(component);
                }
                "place_control" => {
                    if let Some(value) = parser.place_control_scope()? {
                        pcb.placement
                            .get_or_insert_with(Placement::default)
                            .flip_style_rotate_first = Some(value);
                    }
                }
                "library" => {
                    parser.library_scope(pcb.library.get_or_insert_with(Library::default))?
                }
                "part_library" => parser.part_library_scope(
                    pcb.part_library.get_or_insert_with(PartLibrary::default),
                )?,
                "network" => {
                    parser.network_scope(pcb.network.get_or_insert_with(Network::default))?
                }
                "wiring" => parser.wiring_scope(pcb.wiring.get_or_insert_with(Wiring::default))?,
                _ => parser.skip_unknown(&keyword, pos, "pcb")?,
            },
        }
    }
    Ok(pcb)
}

impl Parser<'_> {
    /// `(unit u)` after its keyword.
    fn unit_scope(&mut self) -> Result<Option<Unit>> {
        let token = self.peek()?;
        let unit = match token.kind {
            TokenKind::Atom(atom) => {
                self.next()?;
                Unit::from_name(atom.text)
            }
            _ => None,
        };
        if unit.is_none() {
            self.warn(token.pos, "unit mil, inch, mm or um expected; ignored");
        }
        self.finish_scope("unit")?;
        Ok(unit)
    }

    fn structure_scope(&mut self, structure: &mut Structure) -> Result<()> {
        loop {
            match self.item("structure")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "boundary" => structure.boundaries.push(self.boundary_scope()?),
                    "layer" => structure.layers.push(self.layer_scope()?),
                    "via" => structure.via = Some(self.via_padstacks("via")?),
                    "rule" => structure.rules.extend(self.rule_scope()?),
                    "keepout" | "wire_keepout" | "via_keepout" | "place_keepout" => {
                        if let Some(keepout) = self.keepout_scope(&keyword, pos)? {
                            structure.keepouts.push(keepout);
                        }
                    }
                    "plane" => {
                        let net = self.name("plane net name")?;
                        if let Some(area) = self.area_scope("plane", pos)? {
                            structure.planes.push(Plane { net, area });
                        }
                    }
                    "autoroute_settings" => {
                        structure.autoroute_settings = Some(self.autoroute_settings_scope()?)
                    }
                    "control" => {
                        self.control_scope(structure.control.get_or_insert_with(Control::default))?
                    }
                    "flip_style" => {
                        structure.flip_style_rotate_first = Some(self.flip_style_scope()?)
                    }
                    "snap_angle" => {
                        if let Some(angle) = self.snap_angle_scope()? {
                            structure.snap_angle = Some(angle);
                        }
                    }
                    _ => self.skip_unknown(&keyword, pos, "structure")?,
                },
            }
        }
    }

    fn boundary_scope(&mut self) -> Result<Boundary> {
        let mut boundary = Boundary {
            shapes: Vec::new(),
            clearance_class: None,
        };
        loop {
            match self.item("boundary")? {
                Item::End => return Ok(boundary),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    k if is_shape_keyword(k) => boundary.shapes.extend(self.shape_body(k, pos)?),
                    "clearance_class" => {
                        boundary.clearance_class = Some(self.string_scope("clearance_class")?)
                    }
                    _ => self.skip_unknown(&keyword, pos, "boundary")?,
                },
            }
        }
    }

    fn layer_scope(&mut self) -> Result<Layer> {
        let mut layer = Layer {
            name: self.raw(None)?,
            layer_type: None,
            rules: Vec::new(),
            use_net: Vec::new(),
        };
        loop {
            match self.item("layer")? {
                Item::End => return Ok(layer),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "type" => {
                        let token = self.peek()?;
                        match token.kind {
                            TokenKind::Atom(atom) => {
                                self.next()?;
                                let bare = !atom.quoted;
                                layer.layer_type = Some(if atom.text == "jumper" {
                                    LayerType::Jumper
                                } else if bare && atom.text.eq_ignore_ascii_case("signal") {
                                    LayerType::Signal
                                } else if bare && atom.text.eq_ignore_ascii_case("power") {
                                    LayerType::Power
                                } else {
                                    LayerType::Other(atom.text.to_string())
                                });
                            }
                            _ => self.warn(token.pos, "layer type expected"),
                        }
                        self.finish_scope("type")?;
                    }
                    "rule" => layer.rules.extend(self.rule_scope()?),
                    "use_net" => layer.use_net.extend(self.names_to_end("use_net")?),
                    _ => self.skip_unknown(&keyword, pos, "layer")?,
                },
            }
        }
    }

    fn keepout_scope(&mut self, keyword: &str, pos: Pos) -> Result<Option<Keepout>> {
        let kind = match keyword {
            "keepout" => KeepoutKind::Keepout,
            "wire_keepout" => KeepoutKind::WireKeepout,
            "via_keepout" => KeepoutKind::ViaKeepout,
            _ => KeepoutKind::PlaceKeepout,
        };
        Ok(self
            .area_scope(keyword, pos)?
            .map(|area| Keepout { kind, area }))
    }

    fn control_scope(&mut self, control: &mut Control) -> Result<()> {
        loop {
            match self.item("control")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "via_at_smd" => control.via_at_smd = Some(self.on_off_scope("via_at_smd")?),
                    _ => self.skip_unknown(&keyword, pos, "control")?,
                },
            }
        }
    }

    fn library_scope(&mut self, library: &mut Library) -> Result<()> {
        loop {
            match self.item("library")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "image" => library.images.push(self.image_scope()?),
                    "padstack" => library.padstacks.push(self.padstack_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "library")?,
                },
            }
        }
    }

    fn image_scope(&mut self) -> Result<Image> {
        let mut image = Image {
            name: self.name("image name")?,
            side: None,
            pins: Vec::new(),
            outlines: Vec::new(),
            keepouts: Vec::new(),
        };
        loop {
            match self.item("image")? {
                Item::End => return Ok(image),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "pin" => image.pins.push(self.image_pin_scope()?),
                    "side" => {
                        let back = matches!(self.optional_keyword()?, Some((k, _)) if k == "back");
                        image.side = Some(if back { Side::Back } else { Side::Front });
                        self.finish_scope("side")?;
                    }
                    "outline" => {
                        let shape = self.shape_scope("outline")?;
                        image.outlines.extend(shape);
                        self.finish_scope("outline")?;
                    }
                    "keepout" | "wire_keepout" | "via_keepout" | "place_keepout" => {
                        if let Some(keepout) = self.keepout_scope(&keyword, pos)? {
                            image.keepouts.push(keepout);
                        }
                    }
                    _ => self.skip_unknown(&keyword, pos, "image")?,
                },
            }
        }
    }

    /// `(pin padstack [(rotate r)] name x y [(rotate r)])` after its keyword.
    fn image_pin_scope(&mut self) -> Result<ImagePin> {
        let padstack = self.name("pin padstack")?;
        let mut rotation = None;
        if self.peek()?.kind == TokenKind::Open {
            self.next()?;
            match self.optional_keyword()? {
                Some((keyword, _)) if keyword == "rotate" => {
                    rotation = Some(self.number("rotate")?);
                    self.finish_scope("rotate")?;
                }
                Some((keyword, pos)) => self.skip_unknown(&keyword, pos, "pin")?,
                None => {
                    let pos = self.lexer.current_pos();
                    self.skip_unknown("", pos, "pin")?
                }
            }
        }
        let name = self.name("pin name")?;
        let x = self.number("pin x")?;
        let y = self.number("pin y")?;
        loop {
            match self.item("pin")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "rotate" => {
                        rotation = Some(self.number("rotate")?);
                        self.finish_scope("rotate")?;
                    }
                    _ => self.skip_unknown(&keyword, pos, "pin")?,
                },
            }
        }
        Ok(ImagePin {
            padstack,
            name,
            at: Point::new(x, y),
            rotation,
        })
    }

    fn part_library_scope(&mut self, library: &mut PartLibrary) -> Result<()> {
        loop {
            match self.item("part_library")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "logical_part_mapping" => {
                        let name = self.name("logical_part_mapping name")?;
                        let mut components = Vec::new();
                        loop {
                            match self.item("logical_part_mapping")? {
                                Item::End => break,
                                Item::Scope(k, kpos) => match k.as_str() {
                                    "component" => {
                                        components.extend(self.names_to_end("component")?)
                                    }
                                    _ => self.skip_unknown(&k, kpos, "logical_part_mapping")?,
                                },
                            }
                        }
                        library
                            .mappings
                            .push(LogicalPartMapping { name, components });
                    }
                    "logical_part" => {
                        let name = self.name("logical_part name")?;
                        let mut pins = Vec::new();
                        loop {
                            match self.item("logical_part")? {
                                Item::End => break,
                                Item::Scope(k, kpos) => match k.as_str() {
                                    "pin" => pins.push(self.part_pin_scope()?),
                                    _ => self.skip_unknown(&k, kpos, "logical_part")?,
                                },
                            }
                        }
                        library.parts.push(LogicalPart { name, pins });
                    }
                    _ => self.skip_unknown(&keyword, pos, "part_library")?,
                },
            }
        }
    }

    fn part_pin_scope(&mut self) -> Result<PartPin> {
        let name = self.name("pin name")?;
        let code = self.integer("pin")?;
        let gate = self.name("gate name")?;
        let gate_swap_code = self.integer("gate swap code")?;
        let gate_pin = self.name("gate pin name")?;
        let gate_pin_swap_code = self.integer("gate pin swap code")?;
        // Java skips the subgates that may follow.
        let mut depth = 1usize;
        while depth > 0 {
            let token = self.next_name_token()?;
            match token.kind {
                TokenKind::Open => depth += 1,
                TokenKind::Close => depth -= 1,
                TokenKind::Atom(_) => {}
                TokenKind::Eof => return Err(Self::eof_error(token.pos, "pin")),
            }
        }
        Ok(PartPin {
            name,
            code,
            gate,
            gate_swap_code,
            gate_pin,
            gate_pin_swap_code,
        })
    }

    fn network_scope(&mut self, network: &mut Network) -> Result<()> {
        loop {
            match self.item("network")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "net" => network.nets.push(self.net_scope()?),
                    "via" => network.vias.push(self.network_via_scope()?),
                    "via_rule" => network.via_rules.extend(self.via_rule_scope(pos)?),
                    "class" => network.classes.push(self.class_scope()?),
                    "class_class" => network.class_classes.push(self.class_class_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "network")?,
                },
            }
        }
    }

    fn net_scope(&mut self) -> Result<Net> {
        let name = self.raw(None)?;
        let token = self.peek()?;
        let subnet = super::integer_of(&token);
        if subnet.is_some() {
            self.next()?;
        }
        let mut net = Net {
            name,
            subnet,
            pins: Vec::new(),
            ordered: false,
            fromtos: Vec::new(),
            rules: Vec::new(),
            circuit: None,
        };
        loop {
            match self.item("net")? {
                Item::End => return Ok(net),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "pins" => net.pins.extend(self.pin_list("pins")?),
                    "order" => {
                        net.ordered = true;
                        net.pins.extend(self.pin_list("order")?);
                    }
                    "fromto" => net.fromtos.push(self.pin_list("fromto")?),
                    "rule" => net.rules.extend(self.rule_scope()?),
                    "circuit" => net.circuit = Some(self.circuit_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "net")?,
                },
            }
        }
    }

    fn class_class_scope(&mut self) -> Result<ClassClass> {
        let mut class_class = ClassClass::default();
        loop {
            match self.item("class_class")? {
                Item::End => return Ok(class_class),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "classes" => {
                        class_class.classes.extend(self.raw_list(None)?);
                        self.finish_scope("classes")?;
                    }
                    "rule" => class_class.rules.extend(self.rule_scope()?),
                    "layer_rule" => class_class.layer_rules.push(self.layer_rule_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "class_class")?,
                },
            }
        }
    }

    fn wiring_scope(&mut self, wiring: &mut Wiring) -> Result<()> {
        loop {
            match self.item("wiring")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "wire" => wiring.items.push(WiringItem::Wire(self.wire_scope()?)),
                    "via" => wiring.items.push(WiringItem::Via(self.wiring_via_scope()?)),
                    _ => self.skip_unknown(&keyword, pos, "wiring")?,
                },
            }
        }
    }
}

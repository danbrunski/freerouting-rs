//! Scopes shared by design, session and rules files.

use super::{Item, Parser, describe, number_of};
use crate::ast::*;
use crate::error::{Error, Pos, Result};
use crate::lexer::TokenKind;

/// Whether `keyword` (canonical) starts a shape scope.
pub(super) fn is_shape_keyword(keyword: &str) -> bool {
    matches!(
        keyword,
        "rect" | "circle" | "polygon" | "path" | "polyline_path"
    )
}

impl Parser<'_> {
    // -----------------------------------------------------------------------
    // Shapes and areas
    // -----------------------------------------------------------------------

    /// Reads a shape scope after its keyword. Returns `None`, with a warning,
    /// for shapes the Java reader rejects.
    pub(super) fn shape_body(&mut self, keyword: &str, pos: Pos) -> Result<Option<Shape>> {
        if !is_shape_keyword(keyword) {
            self.warn(pos, format!("unsupported shape '{keyword}' skipped"));
            self.skip_scope()?;
            return Ok(None);
        }
        let layer = LayerRef::from_name(&self.raw(None)?);
        let Some(numbers) = self.shape_numbers(keyword, pos)? else {
            return Ok(None);
        };
        let shape = match keyword {
            "rect" => {
                if numbers.len() < 4 {
                    self.warn(pos, "rect needs 4 coordinates; skipped");
                    return Ok(None);
                }
                if numbers.len() > 4 {
                    self.warn(pos, "extra numbers in rect ignored");
                }
                Shape::Rect {
                    layer,
                    x1: numbers[0],
                    y1: numbers[1],
                    x2: numbers[2],
                    y2: numbers[3],
                }
            }
            "circle" => {
                if numbers.len() > 3 {
                    self.warn(pos, "extra numbers in circle ignored");
                }
                let get = |i: usize| numbers.get(i).copied().unwrap_or(0.0);
                Shape::Circle {
                    layer,
                    diameter: get(0),
                    center: Point::new(get(1), get(2)),
                }
            }
            _ => {
                let Some((&first, rest)) = numbers.split_first() else {
                    self.warn(pos, format!("{keyword} without width skipped"));
                    return Ok(None);
                };
                let points = self.points(rest, keyword, pos);
                match keyword {
                    "polygon" => Shape::Polygon {
                        layer,
                        aperture_width: first,
                        points,
                    },
                    "path" => Shape::Path {
                        layer,
                        width: first,
                        points,
                    },
                    _ => Shape::PolylinePath {
                        layer,
                        width: first,
                        points,
                    },
                }
            }
        };
        Ok(Some(shape))
    }

    /// Reads the numbers of a shape up to its closing bracket. Nested scopes
    /// are skipped. Returns `None`, with a warning, if a token is not a
    /// number.
    fn shape_numbers(&mut self, keyword: &str, pos: Pos) -> Result<Option<Vec<f64>>> {
        let mut numbers = Vec::new();
        let mut bad = None;
        loop {
            let token = self.next()?;
            match token.kind {
                TokenKind::Close => break,
                TokenKind::Eof => return Err(Self::eof_error(token.pos, keyword)),
                TokenKind::Open => self.skip_scope()?,
                TokenKind::Atom(_) => match number_of(&token) {
                    Some(value) => numbers.push(value),
                    None => {
                        if bad.is_none() {
                            bad = Some((token.pos, describe(&token)));
                        }
                    }
                },
            }
        }
        if let Some((bad_pos, found)) = bad {
            self.warn(
                bad_pos,
                format!("number expected in {keyword} at {pos}, found {found}; shape skipped"),
            );
            return Ok(None);
        }
        Ok(Some(numbers))
    }

    fn points(&mut self, coordinates: &[f64], keyword: &str, pos: Pos) -> Vec<Point> {
        if coordinates.len() % 2 != 0 {
            self.warn(
                pos,
                format!("odd number of coordinates in {keyword}; last ignored"),
            );
        }
        coordinates
            .chunks_exact(2)
            .map(|pair| Point::new(pair[0], pair[1]))
            .collect()
    }

    /// Reads `(shape ...)` where a shape scope is expected, Java's
    /// `Shape.readScope`. Returns `None` if there is no valid shape; a closing
    /// bracket is left unread.
    pub(super) fn shape_scope(&mut self, context: &str) -> Result<Option<Shape>> {
        let token = self.peek()?;
        match token.kind {
            TokenKind::Open => {
                self.next()?;
                match self.optional_keyword()? {
                    Some((keyword, pos)) => self.shape_body(&keyword, pos),
                    None => {
                        self.warn(token.pos, format!("shape expected in {context}"));
                        self.skip_scope()?;
                        Ok(None)
                    }
                }
            }
            TokenKind::Eof => Err(Self::eof_error(token.pos, context)),
            _ => {
                self.warn(token.pos, format!("shape expected in {context}"));
                Ok(None)
            }
        }
    }

    /// Reads `(window shape)` after its keyword.
    pub(super) fn window_scope(&mut self) -> Result<Option<Shape>> {
        let shape = self.shape_scope("window")?;
        self.finish_scope("window")?;
        Ok(shape)
    }

    /// Reads an area after its keyword: `[name] shape (window shape)...
    /// (clearance_class c)`, Java's `Shape.readAreaScope`.
    pub(super) fn area_scope(&mut self, context: &str, pos: Pos) -> Result<Option<Area>> {
        let name = self.optional_name()?.filter(|name| !name.is_empty());
        let shape = self.shape_scope(context)?;
        let mut windows = Vec::new();
        let mut clearance_class = None;
        loop {
            match self.item(context)? {
                Item::End => break,
                Item::Scope(keyword, kpos) => match keyword.as_str() {
                    "window" => windows.extend(self.window_scope()?),
                    "clearance_class" => {
                        clearance_class = Some(self.string_scope("clearance_class")?)
                    }
                    _ => self.skip_unknown(&keyword, kpos, context)?,
                },
            }
        }
        match shape {
            Some(shape) => Ok(Some(Area {
                name,
                shape,
                windows,
                clearance_class,
            })),
            None => {
                self.warn(pos, format!("{context} without a valid shape skipped"));
                Ok(None)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Rules
    // -----------------------------------------------------------------------

    /// Reads `(rule ...)` after its keyword.
    pub(super) fn rule_scope(&mut self) -> Result<Vec<Rule>> {
        let mut rules = Vec::new();
        loop {
            match self.item("rule")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "width" => {
                        let value = self.number("width")?;
                        self.finish_scope("width")?;
                        rules.push(Rule::Width(value));
                    }
                    "clearance" => rules.push(self.clearance_rule()?),
                    "length" => rules.push(Rule::Length(self.length_scope()?)),
                    _ => self.skip_unknown(&keyword, pos, "rule")?,
                },
            }
        }
        Ok(rules)
    }

    /// `(clearance value (type a b ...))` after its keyword.
    fn clearance_rule(&mut self) -> Result<Rule> {
        let value = self.number("clearance")?;
        let mut types = Vec::new();
        loop {
            match self.item("clearance")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "type" => {
                        types.extend(self.raw_list(Some(b'-'))?);
                        self.finish_scope("type")?;
                    }
                    _ => self.skip_unknown(&keyword, pos, "clearance")?,
                },
            }
        }
        Ok(Rule::Clearance { value, types })
    }

    /// `(length max [min] ...)` after its keyword, Java's
    /// `Circuit.readLengthScope`. Nested scopes such as `(type ratio)` are
    /// skipped.
    fn length_scope(&mut self) -> Result<LengthRule> {
        let max = self.number("length")?;
        let mut min = None;
        loop {
            let token = self.next()?;
            match token.kind {
                TokenKind::Close => break,
                TokenKind::Eof => return Err(Self::eof_error(token.pos, "length")),
                TokenKind::Open => self.skip_scope()?,
                TokenKind::Atom(atom) => match number_of(&token) {
                    Some(value) if min.is_none() => min = Some(value),
                    _ => self.warn(
                        token.pos,
                        format!("unexpected '{}' in length ignored", atom.text),
                    ),
                },
            }
        }
        Ok(LengthRule { max, min })
    }

    /// `(layer_rule layer... (rule ...)...)` after its keyword.
    pub(super) fn layer_rule_scope(&mut self) -> Result<LayerRule> {
        let mut layer_rule = LayerRule::default();
        loop {
            let token = self.next_name_token()?;
            match token.kind {
                TokenKind::Atom(atom) => layer_rule.layers.push(atom.text.to_string()),
                TokenKind::Close => return Ok(layer_rule),
                TokenKind::Eof => return Err(Self::eof_error(token.pos, "layer_rule")),
                TokenKind::Open => {
                    match self.optional_keyword()? {
                        Some((keyword, _)) if keyword == "rule" => {
                            layer_rule.rules.extend(self.rule_scope()?);
                        }
                        Some((keyword, pos)) => self.skip_unknown(&keyword, pos, "layer_rule")?,
                        None => self.skip_unknown("", token.pos, "layer_rule")?,
                    }
                    break;
                }
            }
        }
        loop {
            match self.item("layer_rule")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "rule" => layer_rule.rules.extend(self.rule_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "layer_rule")?,
                },
            }
        }
        Ok(layer_rule)
    }

    /// `(circuit ...)` after its keyword.
    pub(super) fn circuit_scope(&mut self) -> Result<Circuit> {
        let mut circuit = Circuit::default();
        loop {
            match self.item("circuit")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "length" => circuit.length = Some(self.length_scope()?),
                    "use_via" => {
                        let vias = self.via_padstacks("use_via")?;
                        circuit.use_via.extend(vias.padstacks);
                        circuit.use_via.extend(vias.spare);
                    }
                    "use_layer" => {
                        circuit.use_layer.extend(self.raw_list(None)?);
                        self.finish_scope("use_layer")?;
                    }
                    _ => self.skip_unknown(&keyword, pos, "circuit")?,
                },
            }
        }
        Ok(circuit)
    }

    /// `(via padstack... (spare padstack...))` after its keyword, Java's
    /// `Structure.readViaPadstacks`.
    pub(super) fn via_padstacks(&mut self, context: &str) -> Result<ViaPadstacks> {
        let mut vias = ViaPadstacks::default();
        loop {
            let token = self.next_name_token()?;
            match token.kind {
                TokenKind::Atom(atom) => vias.padstacks.push(atom.text.to_string()),
                TokenKind::Close => return Ok(vias),
                TokenKind::Eof => return Err(Self::eof_error(token.pos, context)),
                TokenKind::Open => match self.optional_keyword()? {
                    Some((keyword, _)) if keyword == "spare" => {
                        let spare = self.via_padstacks("spare")?;
                        vias.spare.extend(spare.padstacks);
                        vias.spare.extend(spare.spare);
                    }
                    Some((keyword, pos)) => self.skip_unknown(&keyword, pos, context)?,
                    None => self.skip_unknown("", token.pos, context)?,
                },
            }
        }
    }

    // -----------------------------------------------------------------------
    // Small scopes
    // -----------------------------------------------------------------------

    /// `(resolution unit value)` after its keyword.
    pub(super) fn resolution_scope(&mut self) -> Result<Resolution> {
        let token = self.next()?;
        let unit = match token.kind {
            TokenKind::Atom(atom) => Unit::from_name(atom.text),
            _ => None,
        };
        let Some(unit) = unit else {
            return Err(Error::new(
                token.pos,
                format!(
                    "resolution unit mil, inch, mm or um expected, found {}",
                    describe(&token)
                ),
            ));
        };
        let value = self.integer("resolution")?;
        self.finish_scope("resolution")?;
        Ok(Resolution { unit, value })
    }

    /// `(snap_angle none|ninety_degree|fortyfive_degree)` after its keyword.
    pub(super) fn snap_angle_scope(&mut self) -> Result<Option<SnapAngle>> {
        let token = self.peek()?;
        let angle = match self.optional_keyword()? {
            Some((keyword, _)) => match keyword.as_str() {
                "none" => Some(SnapAngle::None),
                "ninety_degree" => Some(SnapAngle::NinetyDegree),
                "fortyfive_degree" => Some(SnapAngle::FortyfiveDegree),
                _ => None,
            },
            None => None,
        };
        if angle.is_none() {
            self.warn(token.pos, "snap angle expected; ignored");
        }
        self.finish_scope("snap_angle")?;
        Ok(angle)
    }

    /// `(flip_style rotate_first|mirror_first)` after its keyword. Returns
    /// whether it is `rotate_first`.
    pub(super) fn flip_style_scope(&mut self) -> Result<bool> {
        let rotate_first = matches!(
            self.optional_keyword()?,
            Some((keyword, _)) if keyword == "rotate_first"
        );
        self.finish_scope("flip_style")?;
        Ok(rotate_first)
    }

    /// `(parser ...)` after its keyword, merged into `info`.
    pub(super) fn parser_scope(&mut self, info: &mut ParserInfo) -> Result<()> {
        loop {
            match self.item("parser")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "string_quote" => {
                        match self.lexer.atom_ignoring_quotes()? {
                            Some(token) => {
                                if let TokenKind::Atom(atom) = token.kind {
                                    info.string_quote = Some(atom.text.to_string());
                                }
                            }
                            None => self.warn(pos, "quote character expected in string_quote"),
                        }
                        self.finish_scope("string_quote")?;
                    }
                    "space_in_quoted_tokens" => {
                        info.space_in_quoted_tokens =
                            Some(self.on_off_scope("space_in_quoted_tokens")?);
                    }
                    "host_cad" => info.host_cad = Some(self.string_scope("host_cad")?),
                    "host_version" => info.host_version = Some(self.string_scope("host_version")?),
                    "constant" => {
                        let name = self.name("constant name")?;
                        let value = self.name("constant value")?;
                        self.finish_scope("constant")?;
                        info.constants.push((name, value));
                    }
                    "write_resolution" => {
                        let name = self.name("write_resolution name")?;
                        let value = self.integer("write_resolution")?;
                        self.finish_scope("write_resolution")?;
                        info.write_resolution = Some(WriteResolution { name, value });
                    }
                    "generated_by_freeroute" => {
                        info.generated_by_freerouting = true;
                        self.skip_scope()?;
                    }
                    _ => self.skip_unknown(&keyword, pos, "parser")?,
                },
            }
        }
    }

    // -----------------------------------------------------------------------
    // Library items
    // -----------------------------------------------------------------------

    /// `(padstack name (shape ...) (attach on|off) (absolute on|off))` after
    /// its keyword.
    pub(super) fn padstack_scope(&mut self) -> Result<Padstack> {
        let name = self.name("padstack name")?;
        let mut padstack = Padstack {
            name,
            shapes: Vec::new(),
            attach: None,
            absolute: None,
        };
        loop {
            match self.item("padstack")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "shape" => {
                        let shape = self.shape_scope("shape")?;
                        padstack.shapes.extend(shape);
                        self.finish_scope("shape")?;
                    }
                    "attach" => padstack.attach = Some(self.on_off_scope("attach")?),
                    "absolute" => padstack.absolute = Some(self.on_off_scope("absolute")?),
                    _ => self.skip_unknown(&keyword, pos, "padstack")?,
                },
            }
        }
        Ok(padstack)
    }

    // -----------------------------------------------------------------------
    // Placement
    // -----------------------------------------------------------------------

    /// `(placement ...)` after its keyword, merged into `placement`.
    pub(super) fn placement_scope(&mut self, placement: &mut Placement) -> Result<()> {
        loop {
            match self.item("placement")? {
                Item::End => return Ok(()),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "component" => placement.components.push(self.component_scope()?),
                    "place_control" => {
                        if let Some(value) = self.place_control_scope()? {
                            placement.flip_style_rotate_first = Some(value);
                        }
                    }
                    "resolution" => placement.resolution = Some(self.resolution_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "placement")?,
                },
            }
        }
    }

    /// `(place_control (flip_style ...))` after its keyword.
    pub(super) fn place_control_scope(&mut self) -> Result<Option<bool>> {
        let mut flip = None;
        loop {
            match self.item("place_control")? {
                Item::End => return Ok(flip),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "flip_style" => flip = Some(self.flip_style_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "place_control")?,
                },
            }
        }
    }

    /// `(component image (place ...)...)` after its keyword.
    pub(super) fn component_scope(&mut self) -> Result<ComponentPlacement> {
        // Java's SES writer emits `(component` with no image name for components
        // whose package has no name (e.g. mounting holes); read that as "".
        let pos = self.peek_name_token()?.pos;
        let image = match self.optional_name()? {
            Some(image) => image,
            None => {
                self.warn(pos, "component without an image name");
                String::new()
            }
        };
        let mut places = Vec::new();
        loop {
            match self.item("component")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "place" => places.push(self.place_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "component")?,
                },
            }
        }
        Ok(ComponentPlacement { image, places })
    }

    /// `(place name [x y side rotation] ...)` after its keyword.
    fn place_scope(&mut self) -> Result<Place> {
        let name = self.raw(None)?;
        let mut place = Place {
            name,
            location: None,
            position_locked: false,
            pins: Vec::new(),
            keepouts: Vec::new(),
            via_keepouts: Vec::new(),
            place_keepouts: Vec::new(),
            part_number: None,
        };
        let token = self.peek()?;
        if token.kind == TokenKind::Close {
            self.next()?;
            return Ok(place);
        }
        let x = self.number("place x")?;
        let y = self.number("place y")?;
        let token = self.peek()?;
        let side = match token.kind {
            TokenKind::Atom(atom) => {
                self.next()?;
                if !atom.quoted && atom.text.eq_ignore_ascii_case("back") {
                    Side::Back
                } else {
                    if atom.quoted || !atom.text.eq_ignore_ascii_case("front") {
                        self.warn(token.pos, "front or back expected; front assumed");
                    }
                    Side::Front
                }
            }
            _ => {
                self.warn(token.pos, "front or back expected; front assumed");
                Side::Front
            }
        };
        let rotation = match self.optional_number()? {
            Some(rotation) => rotation,
            None => {
                let token = self.peek()?;
                self.warn(token.pos, "rotation expected; 0 assumed");
                0.0
            }
        };
        place.location = Some(PlaceLocation {
            at: Point::new(x, y),
            side,
            rotation,
        });
        loop {
            match self.item("place")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "lock_type" => loop {
                        let token = self.next()?;
                        match token.kind {
                            TokenKind::Close => break,
                            TokenKind::Eof => {
                                return Err(Self::eof_error(token.pos, "lock_type"));
                            }
                            TokenKind::Open => self.skip_scope()?,
                            TokenKind::Atom(atom) => {
                                if !atom.quoted && atom.text.eq_ignore_ascii_case("position") {
                                    place.position_locked = true;
                                }
                            }
                        }
                    },
                    "pin" => place.pins.extend(self.item_clearance(pos, "pin")?),
                    // A place holds names only, so a `wire_keepout` here is read as a
                    // `keepout` (Java's lexer made the same alias).
                    "keepout" | "wire_keepout" => {
                        place.keepouts.extend(self.item_clearance(pos, "keepout")?)
                    }
                    "via_keepout" => place
                        .via_keepouts
                        .extend(self.item_clearance(pos, "via_keepout")?),
                    "place_keepout" => place
                        .place_keepouts
                        .extend(self.item_clearance(pos, "place_keepout")?),
                    "pn" => place.part_number = Some(self.string_scope("PN")?),
                    _ => self.skip_unknown(&keyword, pos, "place")?,
                },
            }
        }
        Ok(place)
    }

    /// `(pin name (clearance_class c))` inside a place, after its keyword.
    fn item_clearance(&mut self, pos: Pos, context: &str) -> Result<Option<ItemClearance>> {
        let name = self.name("name")?;
        let mut clearance_class = None;
        loop {
            match self.item(context)? {
                Item::End => break,
                Item::Scope(keyword, kpos) => match keyword.as_str() {
                    "clearance_class" | "clearanceclass" => {
                        clearance_class = Some(self.string_scope("clearance_class")?);
                    }
                    _ => self.skip_unknown(&keyword, kpos, context)?,
                },
            }
        }
        match clearance_class {
            Some(clearance_class) => Ok(Some(ItemClearance {
                name,
                clearance_class,
            })),
            None => {
                self.warn(pos, format!("{context} without clearance_class ignored"));
                Ok(None)
            }
        }
    }

    // -----------------------------------------------------------------------
    // Network items
    // -----------------------------------------------------------------------

    /// Reads pin references `comp-pin` up to the closing bracket, Java's
    /// `Network.readNetPins`.
    pub(super) fn pin_list(&mut self, context: &str) -> Result<Vec<PinRef>> {
        let mut pins = Vec::new();
        while let Some(token) = self.lexer.raw(Some(b'-'))? {
            let TokenKind::Atom(atom) = token.kind else {
                break;
            };
            let component = atom.text.to_string();
            let pin = if self.lexer.eat_byte(b'-') {
                self.raw(None)?
            } else {
                self.warn(
                    token.pos,
                    format!("pin reference '{component}' without '-'"),
                );
                String::new()
            };
            pins.push(PinRef { component, pin });
        }
        self.finish_scope(context)?;
        Ok(pins)
    }

    /// `(via name padstack clearance_class [attach])` in a network or rules
    /// file, after its keyword.
    pub(super) fn network_via_scope(&mut self) -> Result<NetworkVia> {
        let name = self.name("via name")?;
        let padstack = self.name("via padstack")?;
        let clearance_class = self.name("via clearance class")?;
        let mut attach = false;
        if let Some((keyword, pos)) = self.optional_keyword()? {
            if keyword == "attach" {
                attach = true;
            } else {
                self.warn(pos, format!("unexpected '{keyword}' in via ignored"));
            }
        }
        self.finish_scope("via")?;
        Ok(NetworkVia {
            name,
            padstack,
            clearance_class,
            attach,
        })
    }

    /// `(via_rule name via...)` after its keyword.
    pub(super) fn via_rule_scope(&mut self, pos: Pos) -> Result<Option<ViaRule>> {
        let mut names = self.names_to_end("via_rule")?;
        if names.is_empty() {
            self.warn(pos, "empty via_rule ignored");
            return Ok(None);
        }
        let name = names.remove(0);
        Ok(Some(ViaRule { name, vias: names }))
    }

    /// `(class name net... (rule ...) ...)` after its keyword.
    pub(super) fn class_scope(&mut self) -> Result<NetClass> {
        let mut class = NetClass::new(self.raw(None)?);
        class.nets = self.raw_list(None)?;
        loop {
            match self.item("class")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "rule" => class.rules.extend(self.rule_scope()?),
                    "layer_rule" => class.layer_rules.push(self.layer_rule_scope()?),
                    "via_rule" => class.via_rule = Some(self.string_scope("via_rule")?),
                    "circuit" => {
                        let circuit = self.circuit_scope()?;
                        let merged = class.circuit.get_or_insert_with(Circuit::default);
                        merged.length = circuit.length;
                        merged.use_via.extend(circuit.use_via);
                        merged.use_layer.extend(circuit.use_layer);
                    }
                    "clearance_class" => {
                        class.clearance_class = Some(self.string_scope("clearance_class")?)
                    }
                    "shove_fixed" => class.shove_fixed = Some(self.on_off_scope("shove_fixed")?),
                    "pull_tight" => class.pull_tight = Some(self.on_off_scope("pull_tight")?),
                    _ => self.skip_unknown(&keyword, pos, "class")?,
                },
            }
        }
        Ok(class)
    }

    // -----------------------------------------------------------------------
    // Wiring
    // -----------------------------------------------------------------------

    /// `(wire shape (window ...) (net ...) (clearance_class c) (type t) (attr a))`
    /// after its keyword.
    pub(super) fn wire_scope(&mut self) -> Result<Wire> {
        let mut wire = Wire {
            shapes: Vec::new(),
            windows: Vec::new(),
            net: None,
            clearance_class: None,
            wire_type: None,
            attr: None,
        };
        loop {
            match self.item("wire")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    k if is_shape_keyword(k) => wire.shapes.extend(self.shape_body(k, pos)?),
                    "window" => wire.windows.extend(self.window_scope()?),
                    "net" => wire.net = Some(self.net_id_scope()?),
                    "clearance_class" => {
                        wire.clearance_class = Some(self.string_scope("clearance_class")?)
                    }
                    "type" => wire.wire_type = self.wire_type_scope()?,
                    "attr" => wire.attr = Some(self.string_scope("attr")?),
                    _ => self.skip_unknown(&keyword, pos, "wire")?,
                },
            }
        }
        Ok(wire)
    }

    /// `(via padstack x y (net ...) (clearance_class c) (type t))` in a wiring
    /// or session, after its keyword.
    pub(super) fn wiring_via_scope(&mut self) -> Result<Via> {
        let padstack = self.name("via padstack")?;
        let x = self.number("via x")?;
        let y = self.number("via y")?;
        let mut via = Via {
            padstack,
            at: Point::new(x, y),
            net: None,
            clearance_class: None,
            via_type: None,
        };
        loop {
            match self.item("via")? {
                Item::End => break,
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "net" => via.net = Some(self.net_id_scope()?),
                    "clearance_class" => {
                        via.clearance_class = Some(self.string_scope("clearance_class")?)
                    }
                    "type" => via.via_type = self.wire_type_scope()?,
                    _ => self.skip_unknown(&keyword, pos, "via")?,
                },
            }
        }
        Ok(via)
    }

    /// `(net name [subnet])` inside a wire or via, after its keyword.
    fn net_id_scope(&mut self) -> Result<NetId> {
        let name = self.raw(None)?;
        let token = self.peek()?;
        let subnet = super::integer_of(&token);
        if subnet.is_some() {
            self.next()?;
        }
        self.finish_scope("net")?;
        Ok(NetId { name, subnet })
    }

    /// `(type t)` of a wire or via, after its keyword.
    fn wire_type_scope(&mut self) -> Result<Option<WireType>> {
        let token = self.peek()?;
        let wire_type = match token.kind {
            TokenKind::Atom(atom) => {
                self.next()?;
                let keyword = if atom.quoted {
                    String::new()
                } else {
                    atom.text.to_ascii_lowercase()
                };
                Some(match keyword.as_str() {
                    "normal" => WireType::Normal,
                    "fix" => WireType::Fix,
                    "shove_fixed" => WireType::ShoveFixed,
                    "protect" => WireType::Protect,
                    _ => WireType::Other(atom.text.to_string()),
                })
            }
            _ => {
                self.warn(token.pos, "wire type expected");
                None
            }
        };
        self.finish_scope("type")?;
        Ok(wire_type)
    }

    // -----------------------------------------------------------------------
    // Autoroute settings
    // -----------------------------------------------------------------------

    /// `(autoroute_settings ...)` after its keyword.
    pub(super) fn autoroute_settings_scope(&mut self) -> Result<AutorouteSettings> {
        let mut settings = AutorouteSettings::default();
        loop {
            match self.item("autoroute_settings")? {
                Item::End => return Ok(settings),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "fanout" => settings.fanout = Some(self.on_off_scope("fanout")?),
                    "autoroute" => settings.autoroute = Some(self.on_off_scope("autoroute")?),
                    "postroute" => settings.postroute = Some(self.on_off_scope("postroute")?),
                    "vias" => settings.vias = Some(self.on_off_scope("vias")?),
                    "via_costs" => settings.via_costs = self.integer_scope("via_costs")?,
                    "plane_via_costs" => {
                        settings.plane_via_costs = self.integer_scope("plane_via_costs")?
                    }
                    "start_ripup_costs" => {
                        settings.start_ripup_costs = self.integer_scope("start_ripup_costs")?
                    }
                    "start_pass_no" => {
                        settings.start_pass_no = self.integer_scope("start_pass_no")?
                    }
                    "layer_rule" => settings
                        .layer_rules
                        .push(self.autoroute_layer_rule_scope()?),
                    _ => self.skip_unknown(&keyword, pos, "autoroute_settings")?,
                },
            }
        }
    }

    fn autoroute_layer_rule_scope(&mut self) -> Result<AutorouteLayerRule> {
        let layer = self.name("layer name")?;
        let mut rule = AutorouteLayerRule {
            layer,
            active: None,
            preferred_direction: None,
            preferred_direction_trace_costs: None,
            against_preferred_direction_trace_costs: None,
        };
        loop {
            match self.item("layer_rule")? {
                Item::End => return Ok(rule),
                Item::Scope(keyword, pos) => match keyword.as_str() {
                    "active" => rule.active = Some(self.on_off_scope("active")?),
                    "preferred_direction" => {
                        let token = self.peek()?;
                        rule.preferred_direction = match self.optional_keyword()? {
                            Some((k, _)) if k == "horizontal" => Some(Direction::Horizontal),
                            Some((k, _)) if k == "vertical" => Some(Direction::Vertical),
                            _ => {
                                self.warn(token.pos, "horizontal or vertical expected");
                                None
                            }
                        };
                        self.finish_scope("preferred_direction")?;
                    }
                    "preferred_direction_trace_costs" => {
                        rule.preferred_direction_trace_costs =
                            self.float_scope("preferred_direction_trace_costs")?;
                    }
                    "against_preferred_direction_trace_costs" => {
                        rule.against_preferred_direction_trace_costs =
                            self.float_scope("against_preferred_direction_trace_costs")?;
                    }
                    _ => self.skip_unknown(&keyword, pos, "layer_rule")?,
                },
            }
        }
    }
}

//! Typed syntax tree for Specctra design (`.dsn`), session (`.ses`) and rules
//! (`.rules`) files.
//!
//! The tree records what the file says, in file units, without interpreting
//! it: no layer lookup, no unit conversion, no board model. Where the Java
//! reader combines or reorders data while building the board, the tree keeps
//! the file's order and the field documentation says what Java does with it.
//!
//! Optional scopes are `Option`s, so a file that leaves a setting out is
//! distinguishable from one that writes the default.

/// A point in file coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Point {
    pub x: f64,
    pub y: f64,
}

impl Point {
    pub fn new(x: f64, y: f64) -> Self {
        Point { x, y }
    }
}

/// The layer a shape is on.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LayerRef {
    /// `pcb`: the board outline layer, all layers.
    Pcb,
    /// `signal`: all signal layers.
    Signal,
    /// A layer from the structure's layer list, by name.
    Named(String),
}

impl LayerRef {
    /// Classifies a layer name read from a file. `pcb` and `signal` are
    /// matched without regard to case.
    pub fn from_name(name: &str) -> Self {
        if name.eq_ignore_ascii_case("pcb") {
            LayerRef::Pcb
        } else if name.eq_ignore_ascii_case("signal") {
            LayerRef::Signal
        } else {
            LayerRef::Named(name.to_string())
        }
    }

    pub fn name(&self) -> &str {
        match self {
            LayerRef::Pcb => "pcb",
            LayerRef::Signal => "signal",
            LayerRef::Named(name) => name,
        }
    }
}

/// A geometric shape scope.
#[derive(Clone, Debug, PartialEq)]
pub enum Shape {
    /// `(rect layer x1 y1 x2 y2)`. Corners are as written, not normalized.
    Rect {
        layer: LayerRef,
        x1: f64,
        y1: f64,
        x2: f64,
        y2: f64,
    },
    /// `(circle layer diameter [x y])`. A missing center is `(0, 0)`.
    Circle {
        layer: LayerRef,
        diameter: f64,
        center: Point,
    },
    /// `(polygon layer aperture_width x y ...)`: a closed polygon.
    Polygon {
        layer: LayerRef,
        aperture_width: f64,
        points: Vec<Point>,
    },
    /// `(path layer width x y ...)`: a trace or open outline. Java drops
    /// paths with fewer than two points; the tree keeps them.
    Path {
        layer: LayerRef,
        width: f64,
        points: Vec<Point>,
    },
    /// `(polyline_path layer width x1 y1 x2 y2 ...)`: separate line
    /// segments, two points each.
    PolylinePath {
        layer: LayerRef,
        width: f64,
        points: Vec<Point>,
    },
}

impl Shape {
    pub fn layer(&self) -> &LayerRef {
        match self {
            Shape::Rect { layer, .. }
            | Shape::Circle { layer, .. }
            | Shape::Polygon { layer, .. }
            | Shape::Path { layer, .. }
            | Shape::PolylinePath { layer, .. } => layer,
        }
    }
}

/// A design rule, from a `(rule ...)` scope.
#[derive(Clone, Debug, PartialEq)]
pub enum Rule {
    /// `(width w)`.
    Width(f64),
    /// `(clearance value [(type t1 t2 ...)])`. The type list is split at
    /// blanks and `-`, as Java does, so a pair `"a"-"b"` gives `["a", "b"]`.
    Clearance { value: f64, types: Vec<String> },
    /// `(length max [min])`.
    Length(LengthRule),
}

/// `(length max [min])`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LengthRule {
    pub max: f64,
    /// Java uses 0 when missing.
    pub min: Option<f64>,
}

/// `(layer_rule layer... (rule ...))`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LayerRule {
    pub layers: Vec<String>,
    pub rules: Vec<Rule>,
}

/// `(circuit (length ...) (use_via ...) (use_layer ...))`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Circuit {
    pub length: Option<LengthRule>,
    pub use_via: Vec<String>,
    pub use_layer: Vec<String>,
}

/// An area: a shape with holes. Used by keepouts and planes.
#[derive(Clone, Debug, PartialEq)]
pub struct Area {
    /// The optional identifier before the shape. An empty name is `None`.
    pub name: Option<String>,
    pub shape: Shape,
    /// `(window shape)` holes.
    pub windows: Vec<Shape>,
    pub clearance_class: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeepoutKind {
    /// `keepout`.
    Keepout,
    /// `wire_keepout`, which KiCad writes for a rule area that bans tracks
    /// only. The board treats it as [`KeepoutKind::Keepout`] (it also blocks
    /// vias), as Java does; the tree keeps the difference so that a later
    /// change is one `match` arm.
    WireKeepout,
    ViaKeepout,
    PlaceKeepout,
}

impl KeepoutKind {
    pub fn keyword(self) -> &'static str {
        match self {
            KeepoutKind::Keepout => "keepout",
            KeepoutKind::WireKeepout => "wire_keepout",
            KeepoutKind::ViaKeepout => "via_keepout",
            KeepoutKind::PlaceKeepout => "place_keepout",
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Keepout {
    pub kind: KeepoutKind,
    pub area: Area,
}

/// A pin reference `component-pin`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct PinRef {
    pub component: String,
    pub pin: String,
}

impl PinRef {
    pub fn new(component: impl Into<String>, pin: impl Into<String>) -> Self {
        PinRef {
            component: component.into(),
            pin: pin.into(),
        }
    }
}

/// `(net name [subnet])` inside a wire or via.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct NetId {
    pub name: String,
    /// Java uses 0 for wires and vias when missing.
    pub subnet: Option<i32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Side {
    Front,
    Back,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Unit {
    Mil,
    Inch,
    Mm,
    Um,
}

impl Unit {
    pub fn from_name(name: &str) -> Option<Unit> {
        match name.to_ascii_lowercase().as_str() {
            "mil" => Some(Unit::Mil),
            "inch" => Some(Unit::Inch),
            "mm" => Some(Unit::Mm),
            "um" => Some(Unit::Um),
            _ => None,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            Unit::Mil => "mil",
            Unit::Inch => "inch",
            Unit::Mm => "mm",
            Unit::Um => "um",
        }
    }
}

/// `(resolution unit value)`: `value` file units per `unit`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Resolution {
    pub unit: Unit,
    pub value: i32,
}

/// `(write_resolution name value)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WriteResolution {
    pub name: String,
    pub value: i32,
}

/// The `(parser ...)` scope.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ParserInfo {
    /// `(string_quote ")`.
    pub string_quote: Option<String>,
    /// `(space_in_quoted_tokens on)`. Java ignores it.
    pub space_in_quoted_tokens: Option<bool>,
    pub host_cad: Option<String>,
    pub host_version: Option<String>,
    /// `(constant name value)` entries.
    pub constants: Vec<(String, String)>,
    pub write_resolution: Option<WriteResolution>,
    /// `(generated_by_freeroute)`: the file was written by Freerouting.
    pub generated_by_freerouting: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SnapAngle {
    None,
    NinetyDegree,
    FortyfiveDegree,
}

impl SnapAngle {
    pub fn keyword(self) -> &'static str {
        match self {
            SnapAngle::None => "none",
            SnapAngle::NinetyDegree => "ninety_degree",
            SnapAngle::FortyfiveDegree => "fortyfive_degree",
        }
    }
}

/// A padstack from the library or a rules file.
#[derive(Clone, Debug, PartialEq)]
pub struct Padstack {
    pub name: String,
    /// One shape per `(shape ...)` scope, in file order.
    pub shapes: Vec<Shape>,
    /// `(attach on|off)`: whether vias may be placed on the pad. Java
    /// defaults to `on`.
    pub attach: Option<bool>,
    /// `(absolute on|off)`. Java defaults to `off`.
    pub absolute: Option<bool>,
}

/// The `(type ...)` of a wire or via.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum WireType {
    Normal,
    /// `fix`: Java's `SYSTEM_FIXED`.
    Fix,
    ShoveFixed,
    /// `protect`: Java's `USER_FIXED`.
    Protect,
    /// Any other type, such as `route`. Java treats it as `USER_FIXED`.
    Other(String),
}

/// How Java fixes an item with a given [`WireType`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FixedState {
    Unfixed,
    ShoveFixed,
    UserFixed,
    SystemFixed,
}

impl WireType {
    /// The fixed state Java gives an item of this type. Items without a type
    /// are unfixed.
    pub fn fixed_state(&self) -> FixedState {
        match self {
            WireType::Normal => FixedState::Unfixed,
            WireType::ShoveFixed => FixedState::ShoveFixed,
            WireType::Fix => FixedState::SystemFixed,
            WireType::Protect | WireType::Other(_) => FixedState::UserFixed,
        }
    }
}

/// A `(wire ...)` scope: a trace, or a conduction area when the shape is an
/// area shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Wire {
    /// The shapes in file order; normally exactly one. Java uses the last
    /// rect/polygon/circle if there is one (a conduction area), otherwise the
    /// last path/polyline_path (a trace).
    pub shapes: Vec<Shape>,
    /// `(window shape)` holes of a conduction area.
    pub windows: Vec<Shape>,
    pub net: Option<NetId>,
    pub clearance_class: Option<String>,
    pub wire_type: Option<WireType>,
    /// `(attr a)`. Specctra's values are `test`, `fanout`, `bus` and
    /// `jumper`, which Java skips; [`WIRE_ATTR_OBSTACLE`] is this port's own
    /// (Q22.4).
    pub attr: Option<String>,
}

/// `(attr obstacle)` on a conduction-area wire: the area is an obstacle to
/// the traces and vias of other nets whatever `router.plane_as_obstacle`
/// says. Not Specctra and not Java: the KiCad plugin and the `.kicad_pcb`
/// reader write it for a copper shape KiCad gives a net (Q22.4), which is
/// fixed copper, not a pour. Java skips the scope.
pub const WIRE_ATTR_OBSTACLE: &str = "obstacle";

/// A `(via padstack x y ...)` scope in the wiring or a session.
#[derive(Clone, Debug, PartialEq)]
pub struct Via {
    pub padstack: String,
    pub at: Point,
    pub net: Option<NetId>,
    pub clearance_class: Option<String>,
    pub via_type: Option<WireType>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum WiringItem {
    Wire(Wire),
    Via(Via),
}

// ---------------------------------------------------------------------------
// Design file
// ---------------------------------------------------------------------------

/// A Specctra design file: `(pcb name ...)`.
///
/// Repeated top-level scopes are merged: lists are appended, single settings
/// take the last value.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pcb {
    pub name: String,
    pub parser: Option<ParserInfo>,
    pub resolution: Option<Resolution>,
    /// `(unit u)`. Java ignores it.
    pub unit: Option<Unit>,
    pub structure: Option<Structure>,
    pub placement: Option<Placement>,
    pub library: Option<Library>,
    pub part_library: Option<PartLibrary>,
    pub network: Option<Network>,
    pub wiring: Option<Wiring>,
}

/// `(structure ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Structure {
    pub layers: Vec<Layer>,
    /// `(boundary ...)` scopes in file order.
    pub boundaries: Vec<Boundary>,
    /// `(via ...)`: the default via padstacks. The last scope wins.
    pub via: Option<ViaPadstacks>,
    /// Default rules from all `(rule ...)` scopes.
    pub rules: Vec<Rule>,
    /// Keepouts of all three kinds, in file order.
    pub keepouts: Vec<Keepout>,
    pub planes: Vec<Plane>,
    /// Java reads only the first `autoroute_settings` and only when it comes
    /// before any layer-dependent scope; the tree keeps the last one.
    pub autoroute_settings: Option<AutorouteSettings>,
    pub control: Option<Control>,
    /// `(flip_style rotate_first)` written in the structure (Electra does).
    pub flip_style_rotate_first: Option<bool>,
    pub snap_angle: Option<SnapAngle>,
}

/// `(layer name (type ...) (rule ...) (use_net ...))`.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    pub name: String,
    pub layer_type: Option<LayerType>,
    pub rules: Vec<Rule>,
    pub use_net: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum LayerType {
    Signal,
    /// A plane layer.
    Power,
    /// Treated as a signal layer.
    Jumper,
    /// Any other type. Java drops layers of unknown type.
    Other(String),
}

impl LayerType {
    /// Whether Java treats a layer of this type as a signal layer. Layers
    /// with no type are signal layers.
    pub fn is_signal(&self) -> bool {
        !matches!(self, LayerType::Power)
    }
}

/// `(boundary shape... (clearance_class c))`.
#[derive(Clone, Debug, PartialEq)]
pub struct Boundary {
    /// Shapes in file order. Java uses the first `pcb` rect/polygon/circle
    /// as the board outline and everything else as extra outline shapes.
    pub shapes: Vec<Shape>,
    pub clearance_class: Option<String>,
}

/// `(via padstack... (spare padstack...))`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ViaPadstacks {
    pub padstacks: Vec<String>,
    /// Java appends the spare vias after the normal ones.
    pub spare: Vec<String>,
}

/// `(plane net shape (window ...))`: a conduction area. Java ignores the
/// windows when the host CAD is Allegro, which cuts out every pin.
#[derive(Clone, Debug, PartialEq)]
pub struct Plane {
    pub net: String,
    pub area: Area,
}

/// `(control ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Control {
    pub via_at_smd: Option<bool>,
}

/// `(autoroute_settings ...)`, written by Freerouting.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AutorouteSettings {
    pub fanout: Option<bool>,
    pub autoroute: Option<bool>,
    pub postroute: Option<bool>,
    pub vias: Option<bool>,
    pub via_costs: Option<i32>,
    pub plane_via_costs: Option<i32>,
    pub start_ripup_costs: Option<i32>,
    /// Java writes it but ignores it when reading.
    pub start_pass_no: Option<i32>,
    pub layer_rules: Vec<AutorouteLayerRule>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Horizontal,
    Vertical,
}

/// `(layer_rule name ...)` inside `autoroute_settings`.
#[derive(Clone, Debug, PartialEq)]
pub struct AutorouteLayerRule {
    pub layer: String,
    pub active: Option<bool>,
    pub preferred_direction: Option<Direction>,
    pub preferred_direction_trace_costs: Option<f64>,
    pub against_preferred_direction_trace_costs: Option<f64>,
}

/// `(placement ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Placement {
    /// `(resolution ...)`: written by sessions; Java's DSN reader applies it
    /// to the whole design.
    pub resolution: Option<Resolution>,
    /// `(place_control (flip_style rotate_first))`.
    pub flip_style_rotate_first: Option<bool>,
    pub components: Vec<ComponentPlacement>,
}

/// `(component image (place ...)...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ComponentPlacement {
    /// The library image the components use.
    pub image: String,
    pub places: Vec<Place>,
}

/// `(place name [x y side rotation] ...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Place {
    pub name: String,
    /// `None` for an unplaced component (`(place name)`).
    pub location: Option<PlaceLocation>,
    /// `(lock_type position)`.
    pub position_locked: bool,
    /// `(pin name (clearance_class c))`.
    pub pins: Vec<ItemClearance>,
    pub keepouts: Vec<ItemClearance>,
    pub via_keepouts: Vec<ItemClearance>,
    pub place_keepouts: Vec<ItemClearance>,
    /// `(PN part_number)`.
    pub part_number: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlaceLocation {
    pub at: Point,
    pub side: Side,
    /// Degrees, counterclockwise.
    pub rotation: f64,
}

/// A clearance class override for a named pin or keepout of a placed
/// component.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ItemClearance {
    pub name: String,
    pub clearance_class: String,
}

/// `(library ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Library {
    pub images: Vec<Image>,
    pub padstacks: Vec<Padstack>,
}

/// `(image name ...)`: a footprint.
#[derive(Clone, Debug, PartialEq)]
pub struct Image {
    pub name: String,
    /// `(side front|back)`. Java defaults to front.
    pub side: Option<Side>,
    pub pins: Vec<ImagePin>,
    pub outlines: Vec<Shape>,
    pub keepouts: Vec<Keepout>,
}

/// `(pin padstack [(rotate r)] name x y)`.
#[derive(Clone, Debug, PartialEq)]
pub struct ImagePin {
    pub padstack: String,
    pub name: String,
    pub at: Point,
    /// Degrees. Java uses 0 when missing.
    pub rotation: Option<f64>,
}

/// `(part_library ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PartLibrary {
    pub mappings: Vec<LogicalPartMapping>,
    pub parts: Vec<LogicalPart>,
}

/// `(logical_part_mapping name (comp c...))`.
#[derive(Clone, Debug, PartialEq)]
pub struct LogicalPartMapping {
    pub name: String,
    pub components: Vec<String>,
}

/// `(logical_part name (pin ...)...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct LogicalPart {
    pub name: String,
    pub pins: Vec<PartPin>,
}

/// `(pin name code gate gate_swap_code gate_pin gate_pin_swap_code)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PartPin {
    pub name: String,
    /// The integer after the pin name. Java ignores it.
    pub code: i32,
    pub gate: String,
    pub gate_swap_code: i32,
    pub gate_pin: String,
    pub gate_pin_swap_code: i32,
}

/// `(network ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Network {
    pub nets: Vec<Net>,
    pub vias: Vec<NetworkVia>,
    pub via_rules: Vec<ViaRule>,
    pub classes: Vec<NetClass>,
    pub class_classes: Vec<ClassClass>,
}

/// `(net name [subnet] (pins ...) ...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct Net {
    pub name: String,
    /// Java uses 1 when missing.
    pub subnet: Option<i32>,
    /// Pins from all `(pins ...)` and `(order ...)` scopes.
    pub pins: Vec<PinRef>,
    /// The pins were given in an `(order ...)` scope: Java connects them in
    /// sequence, one subnet per consecutive pair.
    pub ordered: bool,
    /// `(fromto ...)` pin lists. When present Java makes one subnet per list
    /// and ignores `pins`.
    pub fromtos: Vec<Vec<PinRef>>,
    pub rules: Vec<Rule>,
    /// The last `(circuit ...)` scope.
    pub circuit: Option<Circuit>,
}

/// `(via name padstack clearance_class [attach])` in the network or rules.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NetworkVia {
    pub name: String,
    pub padstack: String,
    pub clearance_class: String,
    pub attach: bool,
}

/// `(via_rule name via...)`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ViaRule {
    pub name: String,
    pub vias: Vec<String>,
}

/// `(class name net... ...)`.
#[derive(Clone, Debug, PartialEq)]
pub struct NetClass {
    pub name: String,
    pub nets: Vec<String>,
    pub clearance_class: Option<String>,
    pub via_rule: Option<String>,
    /// Rules from all `(rule ...)` scopes.
    pub rules: Vec<Rule>,
    pub layer_rules: Vec<LayerRule>,
    /// All `(circuit ...)` scopes merged the way Java merges them: the length
    /// of the last one, the `use_via` and `use_layer` lists of all of them.
    pub circuit: Option<Circuit>,
    /// Java defaults to off.
    pub shove_fixed: Option<bool>,
    /// Java defaults to on.
    pub pull_tight: Option<bool>,
}

impl NetClass {
    pub fn new(name: impl Into<String>) -> Self {
        NetClass {
            name: name.into(),
            nets: Vec::new(),
            clearance_class: None,
            via_rule: None,
            rules: Vec::new(),
            layer_rules: Vec::new(),
            circuit: None,
            shove_fixed: None,
            pull_tight: None,
        }
    }
}

/// `(class_class (classes a b) (rule ...))`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ClassClass {
    pub classes: Vec<String>,
    pub rules: Vec<Rule>,
    pub layer_rules: Vec<LayerRule>,
}

/// `(wiring ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Wiring {
    /// Wires and vias in file order.
    pub items: Vec<WiringItem>,
}

// ---------------------------------------------------------------------------
// Session file
// ---------------------------------------------------------------------------

/// A Specctra session file: `(session name ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Session {
    pub name: String,
    pub base_design: Option<String>,
    pub placement: Option<Placement>,
    /// `(was_is (pins a-b c-d))`: pin swaps, as pairs of pins.
    pub was_is: Option<Vec<PinSwap>>,
    pub routes: Option<Routes>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinSwap {
    pub was: PinRef,
    pub is: PinRef,
}

/// `(routes ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Routes {
    pub resolution: Option<Resolution>,
    pub parser: Option<ParserInfo>,
    /// `(library_out (padstack ...))`: via padstacks used by the routes.
    pub library_out: Vec<Padstack>,
    /// `(network_out (net ...))`.
    pub network_out: Vec<SessionNet>,
}

/// `(net name (wire ...) (via ...))` inside `network_out`.
#[derive(Clone, Debug, PartialEq)]
pub struct SessionNet {
    pub name: String,
    pub items: Vec<WiringItem>,
}

// ---------------------------------------------------------------------------
// Rules file
// ---------------------------------------------------------------------------

/// A Freerouting rules file: `(rules PCB name ...)`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct RulesFile {
    pub design: String,
    /// The scopes in file order. Java applies them to the board in order.
    pub items: Vec<RulesItem>,
}

#[derive(Clone, Debug, PartialEq)]
pub enum RulesItem {
    SnapAngle(SnapAngle),
    AutorouteSettings(AutorouteSettings),
    /// `(rule ...)`: default rules.
    Rule(Vec<Rule>),
    /// `(layer name (rule ...))`: layer rules.
    Layer {
        name: String,
        rules: Vec<Rule>,
    },
    Padstack(Padstack),
    Via(NetworkVia),
    ViaRule(ViaRule),
    Class(NetClass),
}

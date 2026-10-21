//! K14: the KiCad plugin's DSN fix-ups (K09, `kicad-plugin/freerouting_rs/dsn_fixups.py`),
//! done on the reader's tree for a `.kicad_pcb` input.
//!
//! The plugin rewrites the text of KiCad's DSN export; a `.kicad_pcb` input has no such
//! text, so the engine applies the same changes to the tree [`crate::read`] builds, from
//! the router settings `router.input_fixups.*` (`job::prepare` in the engine, after the
//! settings are merged and before the board is built):
//!
//! - [`InputFixups::class_widths`] (`class_widths`): a class's track width, the first
//!   `Rule::Width` of the class, as the plugin writes the class's `(rule (width W))`;
//! - [`InputFixups::route_on_plane_layers`] (`route_on_plane_layers`): a power layer
//!   becomes a signal layer, as the export's `(type power)` becomes `(type signal)`;
//!   its planes stay;
//! - [`InputFixups::drop_pour_nets`] (`drop_pour_nets`): the planes of the listed nets on
//!   the outer copper layers (the first and last layer of the structure) go, as the
//!   plugin drops their `(plane ...)`; inner-layer planes of those nets stay;
//! - [`InputFixups::fixed_pour_classes`] (`fixed_pour_classes` in `wires` mode, K14.4):
//!   each filled outer-layer pour whose net is in a listed class, and not in
//!   `drop_pour_nets` (dropping wins), becomes fixed wires of its net covering its fill,
//!   and the planes of that net on that layer go, as `dsn_fixups.apply` does to the
//!   export. The loop is the plugin's: the board's own zones in file order (rule areas
//!   and footprint zones left out, as `board.Zones()` leaves them), each on `F.Cu` then
//!   `B.Cu` (`GetLayerSet().Seq()`), filled as `ZONE::IsFilled` says
//!   ([`crate::zones::is_filled`]); a net's class from the network's class lists, the
//!   last one listing it winning. The fill comes from the file's `(filled_polygon ...)`
//!   ([`pour_fills`], read again for this setting only; the tree keeps no fills), the
//!   rectangles and wires from [`crate::fill_wires`] (bit-identical to the plugin's
//!   `fill_rects` and `_wire`), and the wires go first in the wiring, `(type fix)`, in
//!   the plugin's order. Fixed wires are `SYSTEM_FIXED` on the board: the session
//!   writer leaves them out, so `ImportSpecctraSES` never sees them, as on the DSN
//!   path, and the engine's `.kicad_pcb` writer leaves them out too.
//!
//!   One difference can remain: the plugin walks pcbnew's zones in memory, the reader
//!   the file's. They are in the same order once the board was loaded from a file
//!   KiCad saved; a zone drawn in the session before the snapshot may sit elsewhere in
//!   the saved file, which changes only the order of the wires.
//!
//! The values are names only. The plugin resolves its `auto` lists from the board
//! (`dsn_fixups.resolve_on_board`) and passes explicit names. A class with no width, a
//! fixed pour class the network does not have, and a layer that does not exist or is
//! not a power layer are errors that name it
//! ([`FixupError`]), checked before anything changes, as the DSN fix-ups raise them. A net
//! the board does not have is a warning line and is left out (the DSN fix-up passes it
//! silently). A class whose width an applied `.kicad_dru` `track_width` rule set keeps the
//! rule's width ([`apply_with_rules`]), as on the DSN path, where `--kicad-dru` runs after
//! the plugin's text fix-up. The tree is the
//! export's except where `docs/DIVERGENCES.md` says otherwise, so on the same board the
//! fixed-up tree equals the plugin's fixed-up export, parsed (`tests/k14_fixups.rs`).

use std::collections::BTreeMap;
use std::fmt;

use freerouting_dsn::ast::{
    LayerRef, LayerType, NetId, Pcb, Point, Rule, Shape, Wire, WireType, WiringItem,
};

use crate::fill_wires::{FixedWire, MmPoint, fill_rects, wire};

/// The settings' section, for messages.
pub const SECTION: &str = "router.input_fixups";

/// The fix-ups to apply, from the `router.input_fixups.*` settings.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InputFixups {
    /// `(class, track width in µm)`, in the order given.
    pub class_widths: Vec<(String, f64)>,
    /// Power layers to open to routing, by DSN layer name.
    pub route_on_plane_layers: Vec<String>,
    /// Nets whose outer-layer planes are dropped.
    pub drop_pour_nets: Vec<String>,
    /// Net classes whose filled outer-layer pours become fixed wires covering
    /// their fill (K14.4, `fixed_pour_classes` in `wires` mode).
    pub fixed_pour_classes: Vec<String>,
}

/// A fix-up that does not fit the board, or a value that does not parse.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FixupError {
    /// The setting, such as `router.input_fixups.class_widths`.
    pub setting: String,
    pub message: String,
}

impl fmt::Display for FixupError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.setting, self.message)
    }
}

impl std::error::Error for FixupError {}

fn error(name: &str, message: String) -> FixupError {
    FixupError {
        setting: format!("{SECTION}.{name}"),
        message,
    }
}

impl InputFixups {
    /// True when no fix-up is asked for.
    pub fn is_empty(&self) -> bool {
        self.class_widths.is_empty()
            && self.route_on_plane_layers.is_empty()
            && self.drop_pour_nets.is_empty()
            && self.fixed_pour_classes.is_empty()
    }

    /// [`InputFixups::fixed_pour_classes`] from the setting's list; empty entries are
    /// left out.
    pub fn with_fixed_pour_classes(mut self, classes: &[String]) -> InputFixups {
        self.fixed_pour_classes = classes
            .iter()
            .map(|n| n.trim())
            .filter(|n| !n.is_empty())
            .map(str::to_owned)
            .collect();
        self
    }

    /// The fix-ups of the settings' lists. `class_widths` entries are
    /// `<class>=<width µm>`, split at the last `=`; the width must be a positive
    /// number. Empty entries are left out.
    pub fn from_lists(
        class_widths: &[String],
        route_on_plane_layers: &[String],
        drop_pour_nets: &[String],
    ) -> Result<InputFixups, FixupError> {
        let mut widths = Vec::new();
        for entry in class_widths
            .iter()
            .map(|e| e.trim())
            .filter(|e| !e.is_empty())
        {
            let bad = || {
                error(
                    "class_widths",
                    format!("`{entry}` is not `<class>=<width in um>`"),
                )
            };
            let (name, value) = entry.rsplit_once('=').ok_or_else(bad)?;
            let name = name.trim();
            let um: f64 = value.trim().parse().map_err(|_| bad())?;
            if name.is_empty() || !um.is_finite() || um <= 0.0 {
                return Err(bad());
            }
            widths.push((name.to_owned(), um));
        }
        let names = |list: &[String]| -> Vec<String> {
            list.iter()
                .map(|n| n.trim())
                .filter(|n| !n.is_empty())
                .map(str::to_owned)
                .collect()
        };
        Ok(InputFixups {
            class_widths: widths,
            route_on_plane_layers: names(route_on_plane_layers),
            drop_pour_nets: names(drop_pour_nets),
            fixed_pour_classes: Vec::new(),
        })
    }
}

/// Applies `fixups` to `pcb`. Returns one log line per change, worded like the
/// plugin's log; on an error nothing has changed. [`InputFixups::fixed_pour_classes`]
/// needs the zone fills ([`apply_with_fills`]); here it is an error.
pub fn apply(pcb: &mut Pcb, fixups: &InputFixups) -> Result<Vec<String>, FixupError> {
    apply_with_rules(pcb, fixups, &[])
}

/// [`apply`] on a tree the board's `.kicad_dru` rules were applied to. `rule_widths`
/// ([`crate::dru::DruReport::track_widths`]) are the classes an applied `track_width`
/// rule set, with the rule: `class_widths` leaves them at the rule's width, with one
/// line naming the class and the rule (the last rule that set it). On the DSN path the
/// plugin's text fix-up comes first and `--kicad-dru` after it, so the rule wins there
/// too, and the board's own width rule is never undercut.
pub fn apply_with_rules(
    pcb: &mut Pcb,
    fixups: &InputFixups,
    rule_widths: &[(String, String)],
) -> Result<Vec<String>, FixupError> {
    apply_fixups(pcb, fixups, rule_widths, None)
}

/// [`apply_with_rules`] with the board's zones and their fills ([`pour_fills`] of
/// the `.kicad_pcb` the tree was read from), which
/// [`InputFixups::fixed_pour_classes`] covers with fixed wires.
pub fn apply_with_fills(
    pcb: &mut Pcb,
    fixups: &InputFixups,
    rule_widths: &[(String, String)],
    fills: &[PourFill],
) -> Result<Vec<String>, FixupError> {
    apply_fixups(pcb, fixups, rule_widths, Some(fills))
}

fn apply_fixups(
    pcb: &mut Pcb,
    fixups: &InputFixups,
    rule_widths: &[(String, String)],
    fills: Option<&[PourFill]>,
) -> Result<Vec<String>, FixupError> {
    check(pcb, fixups)?;
    let fixed = match (fixups.fixed_pour_classes.is_empty(), fills) {
        (true, _) => None,
        (false, Some(fills)) => Some(fixed_pours(pcb, fixups, fills)),
        (false, None) => {
            return Err(error(
                "fixed_pour_classes",
                "needs the zone fills of the .kicad_pcb the board was read from".to_owned(),
            ));
        }
    };
    let mut log = Vec::new();
    if let Some(network) = pcb.network.as_mut() {
        let mut widths: Vec<&(String, f64)> = fixups.class_widths.iter().collect();
        widths.sort_by(|a, b| a.0.cmp(&b.0));
        for (name, um) in widths {
            let rule = rule_widths.iter().rev().find(|(c, _)| c == name);
            let class = network.classes.iter_mut().find(|c| &c.name == name);
            if let Some(Rule::Width(w)) = class.and_then(|c| first_width(&mut c.rules)) {
                match rule {
                    Some((_, rule)) => log.push(format!(
                        "class {name}: width {} um kept, set by the .kicad_dru rule '{rule}' \
                         (class_widths skipped)",
                        fmt_um(*w)
                    )),
                    None => {
                        log.push(format!("class {name}: width {} -> {um} um", fmt_um(*w)));
                        *w = *um;
                    }
                }
            }
        }
    }
    // an unknown net warns and is left out: the DSN fix-up passes it silently, and a run
    // never fails just because of the input choice (decided 2026-10-21)
    let known = known_nets(pcb);
    let mut nets: Vec<String> = Vec::new();
    for name in &fixups.drop_pour_nets {
        if known.contains(&name.as_str()) {
            nets.push(name.clone());
        } else {
            log.push(format!(
                "WARNING: {SECTION}.drop_pour_nets: the board has no net {name}; left out"
            ));
        }
    }
    if let Some(structure) = pcb.structure.as_mut() {
        if !nets.is_empty() {
            let outer = outer_layers(&structure.layers);
            let before = structure.planes.len();
            structure.planes.retain(|p| {
                !(nets.contains(&p.net)
                    && matches!(p.area.shape.layer(), LayerRef::Named(l) if outer.contains(&l.as_str())))
            });
            nets.sort();
            nets.dedup();
            log.push(format!(
                "pours: {} outer plane(s) removed (dropped nets: {})",
                before - structure.planes.len(),
                nets.join(", ")
            ));
        }
        if let Some(fixed) = &fixed {
            let before = structure.planes.len();
            structure.planes.retain(|p| {
                !matches!(p.area.shape.layer(), LayerRef::Named(l)
                    if fixed.pours.iter().any(|(n, pl)| n == &p.net && pl == l))
            });
            let mut classes = fixups.fixed_pour_classes.clone();
            classes.sort();
            classes.dedup();
            log.push(format!(
                "pours: {} fixed wires for {} pour(s) of {}; {} outer plane(s) removed",
                fixed.wires.len(),
                fixed.pours.len(),
                classes.join(", "),
                before - structure.planes.len()
            ));
        }
        for name in &fixups.route_on_plane_layers {
            if let Some(layer) = structure.layers.iter_mut().find(|l| &l.name == name)
                && layer.layer_type == Some(LayerType::Power)
            {
                layer.layer_type = Some(LayerType::Signal);
                log.push(format!("layer {name}: power -> signal"));
            }
        }
    }
    if let Some(fixed) = fixed
        && !fixed.wires.is_empty()
    {
        // `dsn_fixups.apply` puts them first in the wiring, before KiCad's own wires
        let wiring = pcb.wiring.get_or_insert_with(Default::default);
        wiring
            .items
            .splice(0..0, fixed.wires.into_iter().map(WiringItem::Wire));
    }
    Ok(log)
}

/// One copper zone of the board on one outer copper layer, as the plugin's board
/// facts give it (`kicad_board._zones`), for [`InputFixups::fixed_pour_classes`].
#[derive(Clone, Debug, PartialEq)]
pub struct PourFill {
    /// The zone's net as KiCad names it (`ZONE::GetNetname`); empty without a net.
    pub net: String,
    /// The layer's name in the tree (the DSN's layer name).
    pub layer: String,
    /// `ZONE::IsFilled` ([`crate::zones::is_filled`]).
    pub filled: bool,
    /// The fill's outlines on this layer, in mm, board frame (y down), as
    /// `kicad_board._chains` gives them: `pcbnew.ToMM` of the nanometres.
    pub chains: Vec<Vec<MmPoint>>,
}

/// The board's own copper zones (not rule areas, not the zones footprints own: the
/// plugin's `board.Zones()`) in file order, each on its outer copper layers (the
/// first and last copper layer, `F.Cu` and `B.Cu`) in KiCad's layer-id order
/// (`GetLayerSet().Seq()`), with their fill. Reads the `.kicad_pcb` text again (the
/// tree does not keep fills); call it only for [`InputFixups::fixed_pour_classes`].
pub fn pour_fills(text: &str) -> Result<Vec<PourFill>, crate::KicadError> {
    use crate::zones::{ZoneLayerMap, is_filled, layer_id_order, parse_zone, read_fills};
    let mut tree = crate::sexpr::Tree::parse(text)?;
    let hdr = crate::header::read_header(&tree)?;
    crate::legacy::normalise(&mut tree, hdr.version);
    let board = crate::header::board(&tree)?;
    let nets = crate::nets::NetTable::build(&tree, hdr.version);
    let map = ZoneLayerMap::from_board(board);
    let count = map.layers.count();
    let mut out = Vec::new();
    for node in board.children("zone") {
        let Some(zone) = parse_zone(node, &map, &nets, false, &mut Vec::new()) else {
            continue;
        };
        if zone.rule_area {
            continue;
        }
        let order = layer_id_order(zone.layers, count);
        let filled = is_filled(node);
        let fills = if filled {
            read_fills(node, &map, order.first().copied())
        } else {
            Vec::new()
        };
        let net = nets
            .name(zone.net)
            .filter(|_| zone.net > 0)
            .unwrap_or("")
            .to_owned();
        for p in order.into_iter().filter(|&p| p == 0 || p + 1 == count) {
            let chains = fills
                .iter()
                .filter(|(l, _)| *l == p)
                .map(|(_, pts)| {
                    pts.iter()
                        .map(|&(x, y)| (nm_to_mm(x), nm_to_mm(y)))
                        .collect()
                })
                .collect();
            out.push(PourFill {
                net: net.clone(),
                layer: map.layers.name(p).to_owned(),
                filled,
                chains,
            });
        }
    }
    Ok(out)
}

/// `pcbnew.ToMM`: nanometres over `IU_PER_MM`.
fn nm_to_mm(nm: i32) -> f64 {
    f64::from(nm) / 1e6
}

/// What [`InputFixups::fixed_pour_classes`] makes of the fills.
struct FixedPours {
    /// The fixed wires, in the plugin's order.
    wires: Vec<Wire>,
    /// `(net, layer)` of each pour that became wires; their planes go.
    pours: Vec<(String, String)>,
}

/// `dsn_fixups.apply`'s pour loop: each filled outer pour whose net is in a listed
/// class (by the network's class lists, the last class listing a net winning, as
/// the plugin's `Dsn.net_class`) and not in `drop_pour_nets` gives one fixed wire
/// per [`fill_rects`] rectangle, and its `(net, layer)` loses its planes.
fn fixed_pours(pcb: &Pcb, fixups: &InputFixups, fills: &[PourFill]) -> FixedPours {
    let mut net_class: BTreeMap<&str, &str> = BTreeMap::new();
    for class in pcb.network.iter().flat_map(|n| &n.classes) {
        for net in &class.nets {
            net_class.insert(net, &class.name);
        }
    }
    let mut out = FixedPours {
        wires: Vec::new(),
        pours: Vec::new(),
    };
    for z in fills.iter().filter(|z| z.filled) {
        let listed = net_class
            .get(z.net.as_str())
            .is_some_and(|c| fixups.fixed_pour_classes.iter().any(|f| f == c));
        if fixups.drop_pour_nets.contains(&z.net) || !listed {
            continue;
        }
        let pour = (z.net.clone(), z.layer.clone());
        if !out.pours.contains(&pour) {
            out.pours.push(pour);
        }
        for rect in fill_rects(&z.chains) {
            out.wires.push(fixed_wire(&z.layer, &z.net, &wire(rect)));
        }
    }
    out
}

/// The `(wire (path <layer> W x0 y0 x1 y1) (net <net>) (type fix))` `_wire` writes.
fn fixed_wire(layer: &str, net: &str, w: &FixedWire) -> Wire {
    let [width, x0, y0, x1, y1] = w.dsn_path_um().map(|v| v as f64);
    Wire {
        shapes: vec![Shape::Path {
            layer: LayerRef::Named(layer.to_owned()),
            width,
            points: vec![Point { x: x0, y: y0 }, Point { x: x1, y: y1 }],
        }],
        windows: Vec::new(),
        net: Some(NetId {
            name: net.to_owned(),
            subnet: None,
        }),
        clearance_class: None,
        wire_type: Some(WireType::Fix),
        attr: None,
    }
}

fn first_width(rules: &mut [Rule]) -> Option<&mut Rule> {
    rules.iter_mut().find(|r| matches!(r, Rule::Width(_)))
}

/// A width as the DSN prints it: no `.0` on whole micrometres.
fn fmt_um(v: f64) -> String {
    if v.fract() == 0.0 && v.abs() < 1e15 {
        format!("{}", v as i64)
    } else {
        format!("{v}")
    }
}

/// The outer copper layers: the first and the last of the structure's layers.
fn outer_layers(layers: &[freerouting_dsn::ast::Layer]) -> Vec<&str> {
    layers
        .first()
        .into_iter()
        .chain(layers.last())
        .map(|l| l.name.as_str())
        .collect()
}

/// Every class and layer the fix-ups name is on the board.
fn check(pcb: &Pcb, fixups: &InputFixups) -> Result<(), FixupError> {
    let classes = pcb.network.as_ref().map(|n| &n.classes[..]).unwrap_or(&[]);
    for (name, _) in &fixups.class_widths {
        let has_width = classes
            .iter()
            .find(|c| &c.name == name)
            .is_some_and(|c| c.rules.iter().any(|r| matches!(r, Rule::Width(_))));
        if !has_width {
            return Err(error(
                "class_widths",
                format!("the board has no net class {name} with a track width"),
            ));
        }
    }
    let layers = pcb.structure.as_ref().map(|s| &s.layers[..]).unwrap_or(&[]);
    for name in &fixups.route_on_plane_layers {
        match layers.iter().find(|l| &l.name == name) {
            None => {
                return Err(error(
                    "route_on_plane_layers",
                    format!("the board has no copper layer {name}"),
                ));
            }
            Some(l) if l.layer_type != Some(LayerType::Power) => {
                return Err(error(
                    "route_on_plane_layers",
                    format!("layer {name} is not a plane (power) layer"),
                ));
            }
            Some(_) => {}
        }
    }
    for name in &fixups.fixed_pour_classes {
        if !classes.iter().any(|c| &c.name == name) {
            return Err(error(
                "fixed_pour_classes",
                format!("the board has no net class {name}"),
            ));
        }
    }
    Ok(())
}

/// The nets of the network and of the planes.
fn known_nets(pcb: &Pcb) -> Vec<&str> {
    let nets = pcb
        .network
        .iter()
        .flat_map(|n| &n.nets)
        .map(|n| n.name.as_str());
    let planes = pcb
        .structure
        .iter()
        .flat_map(|s| &s.planes)
        .map(|p| p.net.as_str());
    nets.chain(planes).collect()
}

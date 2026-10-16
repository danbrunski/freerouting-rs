//! Zones: copper pours and rule areas as KiCad holds them (K06.2, K06.4).
//!
//! `parseZONE` (`pcb_io_kicad_sexpr_parser.cpp` at tag `10.0.0`,
//! GPL-2.0-or-later) reads a `(zone ...)` into a `ZONE`. A zone with a
//! `(keepout ...)` or a `(placement ...)` child is a rule area
//! (`SetIsRuleArea`). The first `(polygon (pts ...))` is the outline; each
//! further polygon is a hole (`ZONE::AddPolygon`). `(arc ...)` inside `pts` is
//! appended to the outline as the polyline `SHAPE_LINE_CHAIN::Append( SHAPE_ARC )`
//! makes at `SHAPE_ARC::DefaultAccuracyForPCB() / 5`, 1 µm. The outline is what
//! KiCad's DSN export writes (`zone->IterateWithHoles()`), never the
//! `filled_polygon`: fills can be stale or absent on a routed board.
//!
//! Defaults come from `ZONE::ZONE` and `ZONE_SETTINGS::ZONE_SETTINGS`
//! (`zone_settings.cpp`): tracks, vias and pads `not_allowed`, footprints and
//! copper pour `allowed`. A `(keepout ...)` child resets pads and footprints
//! to allowed before it reads its flags (older files have no `pads` or
//! `footprints` flag); a `(placement ...)` area without a `(keepout ...)`
//! child keeps the constructor's defaults. Files KiCad writes always carry
//! the `(keepout ...)` child of a rule area.

use std::collections::BTreeSet;

use crate::arc::ShapeArc;
use crate::error::Warning;
use crate::layers::{CuMask, Layers};
use crate::legacy::Pt;
use crate::nets::NetTable;
use crate::sexpr::NodeRef;
use crate::units::mm_to_nm;

/// `SHAPE_LINE_CHAIN::Append( SHAPE_ARC )` polygonises at
/// `SHAPE_ARC::DefaultAccuracyForPCB() / 5`: 1 µm.
pub const POLYGON_ARC_ERROR: i32 = 1_000;

/// The flags of a rule area: `true` means "not allowed".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RuleFlags {
    pub tracks: bool,
    pub vias: bool,
    pub pads: bool,
    pub footprints: bool,
    pub copper_pour: bool,
}

impl Default for RuleFlags {
    /// `ZONE_SETTINGS::ZONE_SETTINGS`.
    fn default() -> Self {
        RuleFlags {
            tracks: true,
            vias: true,
            pads: true,
            footprints: false,
            copper_pour: false,
        }
    }
}

impl RuleFlags {
    /// `tracks=not_allowed vias=allowed ...`, the words of the file.
    pub fn describe(&self) -> String {
        let w = |b: bool| if b { "not_allowed" } else { "allowed" };
        format!(
            "tracks {}, vias {}, pads {}, copper pour {}, footprints {}",
            w(self.tracks),
            w(self.vias),
            w(self.pads),
            w(self.copper_pour),
            w(self.footprints)
        )
    }
}

/// A zone with its outline in board coordinates (nanometres).
#[derive(Clone, Debug, PartialEq)]
pub struct Zone {
    /// Arena index of the zone's node in the tree.
    pub node: u32,
    pub name: Option<String>,
    pub rule_area: bool,
    pub flags: RuleFlags,
    /// The copper layers the zone lives on (limited to the board's).
    pub layers: CuMask,
    /// The outline, without a repeated first point.
    pub outline: Vec<Pt>,
    /// The holes: every polygon after the first.
    pub holes: Vec<Vec<Pt>>,
    /// The net code; 0 for rule areas, zones off copper and unconnected zones.
    pub net: u32,
    /// Arcs of the outline and holes that were turned into polylines.
    pub arcs: u32,
    /// Whether the zone lives in a footprint.
    pub in_footprint: bool,
}

impl Zone {
    /// `ZONE::Mirror` about the horizontal line `y = cy`, then the layer flip.
    pub fn flip_top_bottom(&mut self, cy: i32, copper_count: usize) {
        let m = |p: &mut Pt| p.1 = cy.wrapping_sub(p.1.wrapping_sub(cy));
        self.outline.iter_mut().for_each(m);
        for h in &mut self.holes {
            h.iter_mut().for_each(m);
        }
        self.layers = self.layers.flipped(copper_count);
    }
}

/// How layer names in zones resolve: the board's copper layers (standard and
/// user names) and every other layer name the board declares.
#[derive(Clone, Debug)]
pub struct ZoneLayerMap {
    pub layers: Layers,
    known: BTreeSet<String>,
}

impl ZoneLayerMap {
    /// Reads the `(layers ...)` list of the board.
    pub fn from_board(board: NodeRef<'_>) -> ZoneLayerMap {
        let layers = Layers::read(board, &mut Vec::new());
        let mut known = BTreeSet::new();
        if let Some(list) = board.child("layers") {
            for e in list.items().skip(1).filter(|e| e.is_list()) {
                for i in [1usize, 3] {
                    if let Some(n) = e.str_value(i) {
                        known.insert(n.into_owned());
                    }
                }
            }
        }
        ZoneLayerMap { layers, known }
    }

    /// The copper layers a layer token names. `None` when the board has no
    /// such layer (KiCad's reader would refuse the file; the reader warns and
    /// goes on without it, never silently).
    pub fn resolve(&self, token: &str) -> Option<CuMask> {
        let count = self.layers.count();
        match token {
            "*.Cu" => return Some(CuMask::all(count)),
            "F&B.Cu" => return Some(CuMask::at(0, count).or(CuMask::at(count - 1, count))),
            "*In.Cu" | "*.In.Cu" => {
                let mut m = CuMask::NONE;
                for p in 1..count.saturating_sub(1) {
                    m = m.or(CuMask::at(p, count));
                }
                return Some(m);
            }
            _ => {}
        }
        if let Some(p) = self
            .layers
            .copper
            .iter()
            .position(|l| l.std_name == token || l.name == token)
        {
            return Some(CuMask::at(p, count));
        }
        if self.known.contains(token) || token.starts_with("*.") || token.starts_with("F&B.") {
            return Some(CuMask::NONE);
        }
        None
    }
}

fn pt_of(node: NodeRef<'_>) -> Option<Pt> {
    Some((
        mm_to_nm(node.atom(1)?.parse().ok()?),
        mm_to_nm(node.atom(2)?.parse().ok()?),
    ))
}

/// `SHAPE_LINE_CHAIN::Append( x, y )`: a point equal to the last one is dropped.
fn append_point(chain: &mut Vec<Pt>, p: Pt) {
    if chain.last() != Some(&p) {
        chain.push(p);
    }
}

/// `parseOutlinePoints` for one `(polygon (pts ...))`. Returns the chain and
/// the number of arcs in it.
fn read_chain(pts: NodeRef<'_>) -> (Vec<Pt>, u32) {
    let mut chain: Vec<Pt> = Vec::new();
    let mut arcs = 0;
    for item in pts.items().skip(1).filter(|i| i.is_list()) {
        match item.head() {
            Some("xy") => {
                if let Some(p) = pt_of(item) {
                    append_point(&mut chain, p);
                }
            }
            Some("arc") => {
                let get = |n: &str| item.child(n).and_then(pt_of);
                if let (Some(s), Some(m), Some(e)) = (get("start"), get("mid"), get("end")) {
                    let poly = ShapeArc::from_three_points(s, m, e).to_polyline(POLYGON_ARC_ERROR);
                    // SHAPE_LINE_CHAIN::Append( chain ): the first point is
                    // skipped when it is the last one already
                    for (i, p) in poly.into_iter().enumerate() {
                        if i == 0 && chain.last() == Some(&p) {
                            continue;
                        }
                        chain.push(p);
                    }
                    arcs += 1;
                }
            }
            _ => {}
        }
    }
    (chain, arcs)
}

/// Reads one `(zone ...)`. `None` when the zone has no outline of at least
/// three corners (a warning says so).
pub fn parse_zone(
    node: NodeRef<'_>,
    map: &ZoneLayerMap,
    nets: &NetTable,
    in_footprint: bool,
    warnings: &mut Vec<Warning>,
) -> Option<Zone> {
    let count = map.layers.count();
    let mut zone = Zone {
        node: node.index(),
        name: None,
        rule_area: false,
        flags: RuleFlags::default(),
        layers: CuMask::NONE,
        outline: Vec::new(),
        holes: Vec::new(),
        net: 0,
        arcs: 0,
        in_footprint,
    };
    let mut unresolved: Vec<String> = Vec::new();
    let mut legacy_net_name: Option<String> = None;
    let mut first_polygon = true;
    let resolve = |token: &str, unresolved: &mut Vec<String>| -> CuMask {
        match map.resolve(token) {
            Some(m) => m,
            None => {
                unresolved.push(token.to_owned());
                CuMask::NONE
            }
        }
    };
    for child in node.items().filter(|c| c.is_list()) {
        match child.head().unwrap_or("") {
            "layer" => {
                if let Some(t) = child.str_value(1) {
                    zone.layers = resolve(&t, &mut unresolved);
                }
            }
            "layers" => {
                let mut m = CuMask::NONE;
                for i in 1..child.len() {
                    if let Some(tok) = child.str_value(i) {
                        m = m.or(resolve(&tok, &mut unresolved));
                    }
                }
                zone.layers = m;
            }
            "name" => zone.name = child.str_value(1).map(|n| n.into_owned()),
            "net_name" => legacy_net_name = child.str_value(1).map(|n| n.into_owned()),
            "keepout" => {
                zone.rule_area = true;
                // pads and footprints are absent in older files
                zone.flags.pads = false;
                zone.flags.footprints = false;
                for flag in child.items().skip(1).filter(|f| f.is_list()) {
                    let banned = flag.atom(1) == Some("not_allowed");
                    match flag.head() {
                        Some("tracks") => zone.flags.tracks = banned,
                        Some("vias") => zone.flags.vias = banned,
                        Some("copperpour") => zone.flags.copper_pour = banned,
                        Some("pads") => zone.flags.pads = banned,
                        Some("footprints") => zone.flags.footprints = banned,
                        _ => {}
                    }
                }
            }
            "placement" => zone.rule_area = true,
            "polygon" => {
                if let Some(pts) = child.child("pts") {
                    let (chain, arcs) = read_chain(pts);
                    zone.arcs += arcs;
                    // the first polygon is the outline, the others are holes
                    if first_polygon {
                        zone.outline = chain;
                        first_polygon = false;
                    } else {
                        zone.holes.push(chain);
                    }
                }
            }
            _ => {}
        }
    }
    zone.layers = zone.layers.and(CuMask::all(count));
    for t in unresolved {
        warnings.push(Warning {
            line: node.line(),
            message: format!(
                "zone{}: layer '{t}' is not a layer of the board; ignored",
                zone.name
                    .as_deref()
                    .map(|n| format!(" '{n}'"))
                    .unwrap_or_default()
            ),
        });
    }
    if zone.outline.len() < 3 {
        warnings.push(Warning {
            line: node.line(),
            message: "zone has fewer than three corners; ignored".to_owned(),
        });
        return None;
    }
    if !zone.rule_area && zone.layers.any() {
        let by_ref = nets.net_of(node).unwrap_or(0);
        zone.net = match legacy_net_name {
            // legacy files: a netcode that does not match the name is fixed
            // from the name
            Some(n) if !n.is_empty() && nets.name(by_ref) != Some(n.as_str()) => {
                nets.codes_by_name.get(&n).copied().unwrap_or(by_ref)
            }
            _ => by_ref,
        };
    }
    zone.holes.retain(|h| !h.is_empty());
    Some(zone)
}

/// The layers of a mask in `LSET` order, which is KiCad's layer-id order:
/// F.Cu, B.Cu, In1.Cu, In2.Cu ... Returns physical positions (0 top).
pub fn layer_id_order(mask: CuMask, count: usize) -> Vec<usize> {
    let mut order: Vec<usize> = vec![0];
    if count > 1 {
        order.push(count - 1);
    }
    order.extend(1..count.saturating_sub(1));
    order
        .into_iter()
        .filter(|&p| mask.and(CuMask::at(p, count)).any())
        .collect()
}

/// `LSET::CuStack()`: physical order, top to bottom.
pub fn cu_stack_order(mask: CuMask, count: usize) -> Vec<usize> {
    (0..count)
        .filter(|&p| mask.and(CuMask::at(p, count)).any())
        .collect()
}

/// The plane a copper zone makes on one copper layer: the outline and its
/// holes (K06.4). `frame` takes nanometres to the tree's frame.
pub fn make_plane(
    zone: &Zone,
    layer: &str,
    net: String,
    frame: &dyn Fn(Pt) -> freerouting_dsn::ast::Point,
) -> freerouting_dsn::ast::Plane {
    use freerouting_dsn::ast::{Area, LayerRef, Plane, Shape};
    let polygon = |pts: &[Pt]| {
        let mut points: Vec<_> = pts.iter().map(|&p| frame(p)).collect();
        if let Some(f) = points.first().copied() {
            points.push(f);
        }
        Shape::Polygon {
            layer: LayerRef::Named(layer.to_owned()),
            aperture_width: 0.0,
            points,
        }
    };
    Plane {
        net,
        area: Area {
            name: None,
            shape: polygon(&zone.outline),
            windows: zone.holes.iter().map(|h| polygon(h)).collect(),
            clearance_class: None,
        },
    }
}

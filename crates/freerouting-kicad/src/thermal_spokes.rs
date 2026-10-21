//! Q22.20: room for a zone's thermal spokes, as keepouts that carry the pad's
//! net.
//!
//! KiCad's zone fill joins a pad to a zone of its net with thermal relief
//! through spokes, and its DRC (`starved_thermal`) wants at least Board
//! Setup's `min_resolved_spokes` (default 2) to reach fill that connects to
//! something other than the pad. A track of another net across a spoke's way
//! out takes that fill away: on canary-pwr `ab786fcc` tracks along both sides
//! of J1's 2.54 mm header rows starved its GND pads on F.Cu and B.Cu.
//!
//! [`thermal_pads`] takes each pad's facts from the footprints as filed;
//! [`thermal_keepouts`] writes, for every pad that a copper zone of its net
//! connects by thermal relief on a signal layer, one rectangle per spoke
//! KiCad builds, as a keepout named [`THERMAL_SPOKE_KEEPOUT_NAME`] with
//! `(clearance_class null)`. Each runs from the pad's centre along the
//! spoke's direction to the spoke's end (`ZONE_FILLER::buildThermalSpokes`,
//! KiCad 10: the pad's box grown by the thermal gap and half the zone's
//! minimum width) and on by the spoke's width and the zone's clearance, so a
//! patch of fill as wide as the spoke stays beyond it; it is the spoke's width
//! plus the zone's clearance on each side wide. The router fits it to the pin
//! under it (`freerouting::io::specctra::parser::thermal_keepouts`): it
//! carries the pad's net, so only other nets keep out.
//!
//! - The connection is the pad's `zone_connect`, else its footprint's, else
//!   the zone's `connect_pads` (`thru_hole_only` is thermal for plated
//!   through-hole pads only); the gap and spoke width are the pad's
//!   `thermal_gap` and `thermal_bridge_width`, else the zone's; the spoke
//!   angle is the pad's `thermal_bridge_angle`, else 45° for a circle and 90°
//!   for any other shape. Custom DRC rules are not read.
//! - Spokes are as KiCad builds them: a circle's at 0°, 90°, 180°, 270°
//!   turned by the spoke angle; another shape's at the spoke angle plus
//!   multiples of 90° to its unrotated box; then turned by the pad's
//!   orientation. A spoke narrower than the zone's minimum width is none, and
//!   a custom pad gets none (KiCad's DRC counts its own spoke primitives).
//! - A keepout is written only where the zone's outline holds the pad's
//!   centre and the keepout's far end, and that end is in no copper zone of
//!   another net with a higher priority and in no rule area that keeps zone
//!   fill out. One zone per pad and layer: the first in file order.
//! - Nothing when `min_resolved_spokes` is 0 (KiCad does not check).
//!
//! The KiCad plugin writes the same keepouts into KiCad's export
//! (`kicad-plugin/freerouting_rs/thermal_spokes.py`). `docs/DIVERGENCES.md`
//! lists them.

use std::collections::BTreeSet;

use freerouting_dsn::ast::Keepout;

use crate::edge_keepouts::{F, mask_keepout};
use crate::footprints::{Footprint, PadAttr, PadShape};
use crate::layers::{CuMask, Layers};
use crate::legacy::Pt;
use crate::sexpr::{NodeRef, Tree};
use crate::zones::{Zone, layer_id_order};

/// The name of the keepouts: the router's `THERMAL_SPOKE_KEEPOUT_NAME`.
pub const THERMAL_SPOKE_KEEPOUT_NAME: &str = "thermal_spoke";

/// KiCad's `ZONE_CONNECTION`.
const CONNECT_NONE: i32 = 0;
const CONNECT_THERMAL: i32 = 1;
const CONNECT_FULL: i32 = 2;
const CONNECT_THT_THERMAL: i32 = 3;

/// KiCad's zone defaults (`ZONE_SETTINGS`), nm.
const DEFAULT_GAP_NM: f64 = 500_000.0;
const DEFAULT_SPOKE_NM: f64 = 500_000.0;
const DEFAULT_CLEARANCE_NM: f64 = 500_000.0;
const DEFAULT_MIN_THICKNESS_NM: f64 = 250_000.0;

/// What the keepouts read of one pad (KiCad's frame, nm, as filed).
#[derive(Clone, Debug, PartialEq)]
pub struct ThermalPad {
    pub net: u32,
    /// The centre of the pad's shape (its position plus its turned offset).
    pub centre: F,
    pub size: F,
    /// Absolute orientation, degrees.
    pub orient: f64,
    pub circle: bool,
    pub custom: bool,
    pub pth: bool,
    pub layers: CuMask,
    /// The pad's `zone_connect`, else its footprint's.
    pub zone_connect: Option<i32>,
    pub gap: Option<f64>,
    pub spoke: Option<f64>,
    /// Spoke angle, degrees.
    pub angle: f64,
}

/// What the keepouts read of one copper zone.
#[derive(Clone, Debug, PartialEq)]
pub struct ThermalZone {
    pub net: u32,
    pub layers: CuMask,
    pub priority: i32,
    pub connect: i32,
    pub gap: f64,
    pub spoke: f64,
    pub clearance: f64,
    pub min_thickness: f64,
    pub outline: Vec<Pt>,
    pub holes: Vec<Vec<Pt>>,
    /// A rule area that keeps zone fill out (`copperpour not_allowed`).
    pub no_fill: bool,
    pub rule_area: bool,
}

/// KiCad's `RotatePoint`: `v` turned by `deg` (counter-clockwise on screen,
/// Y down).
fn rotate(v: F, deg: f64) -> F {
    let r = deg.to_radians();
    let (s, c) = (r.sin(), r.cos());
    (v.0 * c + v.1 * s, -v.0 * s + v.1 * c)
}

fn mm_nm(node: Option<NodeRef<'_>>) -> Option<f64> {
    node.and_then(|n| n.f64(1)).map(|mm| mm * 1e6)
}

fn int_of(node: Option<NodeRef<'_>>) -> Option<i32> {
    node.and_then(|n| n.atom(1))
        .and_then(|a| a.parse::<i32>().ok())
}

/// The pads of `footprints` (as filed, before any flip), footprint then pad
/// order, with their thermal facts from the tree.
pub fn thermal_pads(tree: &Tree, footprints: &[Footprint]) -> Vec<ThermalPad> {
    let mut out = Vec::new();
    for fp in footprints {
        let node = tree.node(fp.line);
        let fp_connect = int_of(node.child("zone_connect")).filter(|c| *c >= 0);
        for (pad, pn) in fp.pads.iter().zip(node.children("pad")) {
            let off = rotate(
                (f64::from(pad.offset.0), f64::from(pad.offset.1)),
                pad.orient,
            );
            let circle = pad.shape == PadShape::Circle
                || (pad.shape == PadShape::Oval && pad.size.0 == pad.size.1);
            let custom_circle = pad.shape == PadShape::Custom && !pad.anchor_rect;
            out.push(ThermalPad {
                net: pad.net,
                centre: (f64::from(pad.pos.0) + off.0, f64::from(pad.pos.1) + off.1),
                size: (f64::from(pad.size.0), f64::from(pad.size.1)),
                orient: pad.orient,
                circle,
                custom: pad.shape == PadShape::Custom,
                pth: pad.attr == PadAttr::Pth,
                layers: pad.layers,
                zone_connect: int_of(pn.child("zone_connect"))
                    .filter(|c| *c >= 0)
                    .or(fp_connect),
                gap: mm_nm(pn.child("thermal_gap")),
                spoke: mm_nm(
                    pn.child("thermal_bridge_width")
                        .or_else(|| pn.child("thermal_width")),
                ),
                angle: pn
                    .child("thermal_bridge_angle")
                    .and_then(|a| a.f64(1))
                    .unwrap_or(if circle || custom_circle { 45.0 } else { 90.0 }),
            });
        }
    }
    out
}

/// The thermal facts of `zones` (board zones, then the footprints' as
/// filed), from the tree.
pub fn thermal_zones<'z>(tree: &Tree, zones: impl Iterator<Item = &'z Zone>) -> Vec<ThermalZone> {
    zones
        .map(|z| {
            let node = tree.node(z.node);
            let cp = node.child("connect_pads");
            let connect = match cp.and_then(|c| c.atom(1)) {
                Some("yes") => CONNECT_FULL,
                Some("no") => CONNECT_NONE,
                Some("thru_hole_only") => CONNECT_THT_THERMAL,
                _ => CONNECT_THERMAL,
            };
            let fill = node.child("fill");
            ThermalZone {
                net: z.net,
                layers: z.layers,
                priority: int_of(node.child("priority")).unwrap_or(0),
                connect,
                gap: mm_nm(fill.and_then(|f| f.child("thermal_gap"))).unwrap_or(DEFAULT_GAP_NM),
                spoke: mm_nm(fill.and_then(|f| f.child("thermal_bridge_width")))
                    .unwrap_or(DEFAULT_SPOKE_NM),
                clearance: mm_nm(cp.and_then(|c| c.child("clearance")))
                    .unwrap_or(DEFAULT_CLEARANCE_NM),
                min_thickness: mm_nm(node.child("min_thickness"))
                    .unwrap_or(DEFAULT_MIN_THICKNESS_NM),
                outline: z.outline.clone(),
                holes: z.holes.clone(),
                no_fill: z.rule_area && z.flags.copper_pour,
                rule_area: z.rule_area,
            }
        })
        .collect()
}

/// Even-odd crossing test on floats (the plugin's `_inside`).
fn inside_ring(ring: &[Pt], p: F) -> bool {
    let n = ring.len();
    let mut inside = false;
    for i in 0..n {
        let a = (f64::from(ring[i].0), f64::from(ring[i].1));
        let b = (
            f64::from(ring[(i + 1) % n].0),
            f64::from(ring[(i + 1) % n].1),
        );
        if (a.1 > p.1) != (b.1 > p.1) {
            let x = a.0 + (p.1 - a.1) * (b.0 - a.0) / (b.1 - a.1);
            if p.0 < x {
                inside = !inside;
            }
        }
    }
    inside
}

fn zone_holds(z: &ThermalZone, p: F) -> bool {
    inside_ring(&z.outline, p) && !z.holes.iter().any(|h| inside_ring(h, p))
}

/// The connection `zone` makes with `pad`: KiCad's `EvalZoneConnection`
/// without custom rules.
fn is_thermal(pad: &ThermalPad, zone: &ThermalZone) -> bool {
    match pad.zone_connect.unwrap_or(zone.connect) {
        CONNECT_THERMAL => true,
        CONNECT_THT_THERMAL => pad.pth,
        _ => false,
    }
}

/// The far ends of the spokes KiCad builds for `pad` (module docs), each
/// with its unit direction, from the pad's centre, in KiCad's frame.
fn spoke_ends(pad: &ThermalPad, grow: f64) -> [(F, F); 4] {
    let ends: [F; 4] = if pad.circle {
        let r = pad.size.0 / 2.0 + grow;
        [(r, 0.0), (0.0, r), (-r, 0.0), (0.0, -r)].map(|e| rotate(e, pad.angle))
    } else {
        let (hx, hy) = (pad.size.0 / 2.0 + grow, pad.size.1 / 2.0 + grow);
        [0.0, 90.0, 180.0, 270.0].map(|k: f64| {
            let r = (pad.angle + k).to_radians();
            let d = (r.cos(), r.sin());
            let tx = if d.0.abs() < 1e-12 {
                f64::INFINITY
            } else {
                hx / d.0.abs()
            };
            let ty = if d.1.abs() < 1e-12 {
                f64::INFINITY
            } else {
                hy / d.1.abs()
            };
            let t = tx.min(ty);
            (d.0 * t, d.1 * t)
        })
    };
    ends.map(|e| {
        let e = rotate(e, pad.orient);
        let len = e.0.hypot(e.1);
        (e, (e.0 / len, e.1 / len))
    })
}

/// One corridor: the rectangle from the pad's centre along `dir` to `len`,
/// `half` either side.
fn corridor(c: F, dir: F, len: f64, half: f64) -> Vec<F> {
    let n = (-dir.1 * half, dir.0 * half);
    let far = (c.0 + dir.0 * len, c.1 + dir.1 * len);
    vec![
        (c.0 + n.0, c.1 + n.1),
        (far.0 + n.0, far.1 + n.1),
        (far.0 - n.0, far.1 - n.1),
        (c.0 - n.0, c.1 - n.1),
    ]
}

/// The corridors (KiCad's frame, nm) of `pads` against `zones`, each with its
/// copper layer's stack position, zone by zone, its layers in KiCad's layer-id
/// order, then pad by pad and spoke by spoke (module docs). `signal[p]` says
/// whether stack position `p` is a signal layer.
pub fn corridors(
    pads: &[ThermalPad],
    zones: &[ThermalZone],
    signal: &[bool],
    min_resolved_spokes: i32,
) -> Vec<(usize, Vec<F>)> {
    let mut out = Vec::new();
    if min_resolved_spokes <= 0 {
        return out;
    }
    let count = signal.len();
    let mut done: BTreeSet<(usize, usize)> = BTreeSet::new();
    for (zi, zone) in zones.iter().enumerate() {
        if zone.rule_area || zone.net == 0 {
            continue;
        }
        for p in layer_id_order(zone.layers, count) {
            if !signal[p] {
                continue;
            }
            let on_layer = CuMask::at(p, count);
            for (pi, pad) in pads.iter().enumerate() {
                if pad.net != zone.net
                    || pad.custom
                    || !pad.layers.and(on_layer).any()
                    || !is_thermal(pad, zone)
                    || !zone_holds(zone, pad.centre)
                    || !done.insert((pi, p))
                {
                    continue;
                }
                let gap = pad.gap.unwrap_or(zone.gap);
                let spoke = pad
                    .spoke
                    .unwrap_or(zone.spoke)
                    .min(pad.size.0)
                    .min(pad.size.1);
                if spoke < zone.min_thickness {
                    continue;
                }
                let grow = gap + zone.min_thickness / 2.0;
                let half = spoke / 2.0 + zone.clearance;
                for (end, dir) in spoke_ends(pad, grow) {
                    let len = end.0.hypot(end.1) + spoke + zone.clearance;
                    let far = (pad.centre.0 + dir.0 * len, pad.centre.1 + dir.1 * len);
                    let blocked = zones.iter().enumerate().any(|(oi, o)| {
                        oi != zi
                            && o.layers.and(on_layer).any()
                            && ((o.rule_area && o.no_fill)
                                || (!o.rule_area
                                    && o.net != zone.net
                                    && o.priority > zone.priority))
                            && zone_holds(o, far)
                    });
                    if zone_holds(zone, far) && !blocked {
                        out.push((p, corridor(pad.centre, dir, len, half)));
                    }
                }
            }
        }
    }
    out
}

/// [`corridors`] as DSN keepouts on their layers.
pub fn thermal_keepouts(corridors: &[(usize, Vec<F>)], layers: &Layers) -> Vec<Keepout> {
    corridors
        .iter()
        .map(|(p, poly)| mask_keepout(THERMAL_SPOKE_KEEPOUT_NAME, layers.name(*p), poly))
        .collect()
}

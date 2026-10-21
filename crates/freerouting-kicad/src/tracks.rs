//! Tracks to wires (K06.5).
//!
//! `(segment ...)` and `(arc ...)` in file order, merged into wires the way
//! `SPECCTRA_DB::FromBOARD` merges them ("create the wires from tracks",
//! `specctra_export.cpp` at tag `10.0.0`): a new wire starts when the net, the
//! width or the layer changes, or when the item's start is not the last point
//! of the current path; otherwise the item's end is appended. Tracks of net 0
//! are skipped and do not touch the merge state. Points are compared in
//! nanometres.
//!
//! **Arcs follow the curve.** KiCad exports an arc as its chord (start to end),
//! which can cut a corner; the reader replaces it with a polyline through
//! points on the arc: the ends exact, interior points rounded to the nearest
//! nanometre, with no chord farther than 1 µm from the arc and no chord longer
//! than 0.1 rad of arc (so that a polyline of a small arc is within 0.05 % of
//! its length). The circle comes from the three points in `f64`, relative to the
//! start point, in a fixed operation order (no fused multiply-add), so two
//! runs give identical points.

use freerouting_dsn::ast::{NetId, Shape, Wire, WireType};

use crate::error::Warning;
use crate::layers::Layers;
use crate::legacy::Pt;
use crate::nets::NetTable;
use crate::sexpr::NodeRef;
use crate::units::{ki_round, map_pt, parse_board_units, um};

/// The longest arc one chord may span, radians.
pub const MAX_CHORD_ANGLE: f64 = 0.1;
/// The largest distance between a chord and its arc, nanometres.
pub const MAX_SAGITTA_NM: f64 = 1_000.0;

/// The points of an arc track from `start` through `mid` to `end`: both ends
/// exact. Collinear points (no circle) give the straight line.
pub fn arc_points(start: Pt, mid: Pt, end: Pt) -> Vec<Pt> {
    // coordinates relative to the start point keep the products small
    let (bx, by) = (
        f64::from(mid.0) - f64::from(start.0),
        f64::from(mid.1) - f64::from(start.1),
    );
    let (cx, cy) = (
        f64::from(end.0) - f64::from(start.0),
        f64::from(end.1) - f64::from(start.1),
    );
    let cross = bx * cy - by * cx;
    if cross == 0.0 || !cross.is_finite() {
        return vec![start, end];
    }
    let d = 2.0 * cross;
    let b2 = bx * bx + by * by;
    let c2 = cx * cx + cy * cy;
    let ux = (cy * b2 - by * c2) / d;
    let uy = (bx * c2 - cx * b2) / d;
    let r = ux.hypot(uy);
    if !(r.is_finite() && r > 0.0) {
        return vec![start, end];
    }
    let ts = (-uy).atan2(-ux);
    let te = (cy - uy).atan2(cx - ux);
    let tau = std::f64::consts::TAU;
    let mut sweep = te - ts;
    if cross > 0.0 {
        while sweep <= 0.0 {
            sweep += tau;
        }
    } else {
        while sweep >= 0.0 {
            sweep -= tau;
        }
    }
    let ratio = (MAX_SAGITTA_NM / r).min(1.0);
    let by_sagitta = 2.0 * (1.0 - ratio).acos();
    let step = by_sagitta.min(MAX_CHORD_ANGLE);
    let n = ((sweep.abs() / step).ceil() as usize).clamp(1, 100_000);
    let mut out = Vec::with_capacity(n + 1);
    out.push(start);
    let (ox, oy) = (f64::from(start.0) + ux, f64::from(start.1) + uy);
    for i in 1..n {
        let t = ts + sweep * (i as f64) / (n as f64);
        out.push((ki_round(ox + r * t.cos()), ki_round(oy + r * t.sin())));
    }
    out.push(end);
    out
}

/// A wire under construction, in nanometres.
struct Draft {
    net: u32,
    width: i32,
    layer: usize,
    locked: bool,
    points: Vec<Pt>,
}

/// Whether a board item is locked: the bare atom `locked` (older files) or
/// `(locked yes)`.
pub fn is_locked(node: NodeRef<'_>) -> bool {
    for (i, it) in node.items().enumerate().skip(1) {
        if it.is_list() {
            if it.head() == Some("locked") {
                return it.atom(1) != Some("no");
            }
        } else if i > 0 && it.text() == "locked" {
            return true;
        }
    }
    false
}

/// Position of a copper layer given by its standard or user name.
pub fn layer_position(layers: &Layers, name: &str) -> Option<usize> {
    layers
        .copper
        .iter()
        .position(|l| l.std_name == name || l.name == name)
}

fn point(node: NodeRef<'_>, name: &str) -> Option<Pt> {
    let c = node.child(name)?;
    Some((
        parse_board_units(c.atom(1)?)?,
        parse_board_units(c.atom(2)?)?,
    ))
}

/// What reading the tracks found.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TrackStats {
    /// Segments and arcs read with a net.
    pub tracks: usize,
    /// Arcs replaced by polylines.
    pub arcs_flattened: usize,
}

/// The wires of the board, in file order.
pub fn read_wires(
    board: NodeRef<'_>,
    layers: &Layers,
    nets: &NetTable,
    warnings: &mut Vec<Warning>,
) -> (Vec<Wire>, TrackStats) {
    let mut stats = TrackStats::default();
    let mut drafts: Vec<Draft> = Vec::new();
    let mut state: Option<(u32, i32, usize)> = None;
    for node in board.items().filter(|c| c.is_list()) {
        let is_arc = match node.head() {
            Some("segment") => false,
            Some("arc") => true,
            _ => continue,
        };
        let net = nets.net_of(node).unwrap_or(0);
        if net == 0 {
            continue;
        }
        let (Some(start), Some(end)) = (point(node, "start"), point(node, "end")) else {
            warnings.push(Warning {
                line: node.line(),
                message: "track without start or end; ignored".to_owned(),
            });
            continue;
        };
        let width = node
            .child("width")
            .and_then(|w| w.atom(1))
            .and_then(parse_board_units)
            .unwrap_or(0);
        let layer = node
            .child("layer")
            .and_then(|l| l.str_value(1))
            .and_then(|n| layer_position(layers, &n));
        let Some(layer) = layer else {
            warnings.push(Warning {
                line: node.line(),
                message: "track on a layer that is not a copper layer of the board; ignored"
                    .to_owned(),
            });
            continue;
        };
        stats.tracks += 1;
        let new_wire = state != Some((net, width, layer))
            || drafts.last().and_then(|d| d.points.last()) != Some(&start);
        if new_wire {
            state = Some((net, width, layer));
            drafts.push(Draft {
                net,
                width,
                layer,
                locked: is_locked(node),
                points: vec![start],
            });
        }
        let Some(draft) = drafts.last_mut() else {
            continue;
        };
        if is_arc {
            match point(node, "mid") {
                Some(mid) => {
                    let pts = arc_points(start, mid, end);
                    stats.arcs_flattened += 1;
                    draft.points.extend(pts.into_iter().skip(1));
                }
                None => {
                    warnings.push(Warning {
                        line: node.line(),
                        message: "arc track without a mid point is read as a chord".to_owned(),
                    });
                    draft.points.push(end);
                }
            }
        } else {
            draft.points.push(end);
        }
    }
    let wires = drafts
        .into_iter()
        .map(|d| Wire {
            shapes: vec![Shape::Path {
                layer: freerouting_dsn::ast::LayerRef::Named(layers.name(d.layer).to_owned()),
                width: um(i64::from(d.width)),
                points: d
                    .points
                    .iter()
                    .map(|&(x, y)| map_pt(i64::from(x), i64::from(y)))
                    .collect(),
            }],
            windows: Vec::new(),
            net: Some(NetId {
                name: nets.name(d.net).unwrap_or("").replace('"', "''"),
                subnet: None,
            }),
            clearance_class: None,
            wire_type: Some(if d.locked {
                WireType::Fix
            } else {
                WireType::Other("route".to_owned())
            }),
            attr: None,
        })
        .collect();
    (wires, stats)
}

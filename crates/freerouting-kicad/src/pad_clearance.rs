//! K13.1: pads' own clearances as copper keepouts.
//!
//! KiCad's DRC keeps other copper a pad's local clearance (`(clearance v)` in
//! the pad, `PAD::GetLocalClearance`) from it when that is larger than the
//! net class's; the DSN export writes no per-pad clearance. canary-pwr's
//! fiducials (0.6 mm against a 0.2 mm class) gave one routing-caused
//! `clearance` on the bare export in Q22.6. The plugin's DSN fix-up
//! `edge_and_pad_keepouts` (K09.5, on by default) writes a circle keepout
//! round each such pad (`dsn_fixups._pad_keepouts`); this module gives the
//! `.kicad_pcb` reader the same circles, named [`KEEPOUT_NAME`]:
//!
//! - a pad whose local clearance exceeds the default clearance (the
//!   structure's, the Default class's) gets, on each outer copper layer it
//!   is on, a circle of diameter `max(size x, size y) + 2 * (local - default)`
//!   round its centre;
//! - the router keeps its default clearance from a keepout, so the circle
//!   makes up the rest;
//! - footprint and pad order, front before back, as the plugin.
//!
//! The pad's own clearance only, as the plugin reads it; a footprint-level
//! `(clearance v)` is not taken. Coordinates keep the file's nanometres
//! (the plugin rounds to whole µm). A keepout holds every net out, the pad's
//! own net too, as on the DSN path.

use freerouting_dsn::ast::{Area, Keepout, KeepoutKind, LayerRef, Shape};

use crate::legacy::{Angle, Pt, rotate_origin};
use crate::sexpr::NodeRef;
use crate::units::{map_pt, parse_board_units, um};

/// The name of the keepouts this module makes.
pub const KEEPOUT_NAME: &str = "pad_clearance";

/// A pad with a local clearance of its own.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PadClearance {
    /// The pad's centre on the board, nm (file frame, Y down).
    pub at: Pt,
    /// `max(size x, size y)`, nm.
    pub size: i32,
    /// The pad's `(clearance v)`, nm.
    pub local: i32,
    pub front: bool,
    pub back: bool,
}

fn pt(node: NodeRef<'_>) -> Option<Pt> {
    Some((
        parse_board_units(node.atom(1)?)?,
        parse_board_units(node.atom(2)?)?,
    ))
}

/// Every pad of `board`'s footprints with a positive `(clearance v)` on an
/// outer copper layer, in footprint and pad order.
pub fn read_pad_clearances(board: NodeRef<'_>) -> Vec<PadClearance> {
    let mut out = Vec::new();
    let footprints = board
        .items()
        .filter(|c| c.is_list() && matches!(c.head(), Some("footprint") | Some("module")));
    for fp in footprints {
        let at = fp.child("at");
        let fp_pos = at.and_then(pt).unwrap_or((0, 0));
        let fp_orient = at.and_then(|a| a.f64(3)).unwrap_or(0.0);
        for pad in fp.children("pad") {
            let Some(local) = pad
                .child("clearance")
                .and_then(|c| c.atom(1))
                .and_then(parse_board_units)
                .filter(|v| *v > 0)
            else {
                continue;
            };
            let (mut front, mut back) = (false, false);
            if let Some(layers) = pad.child("layers") {
                for t in layers.items().skip(1) {
                    match t.text().trim_matches('"') {
                        "F.Cu" => front = true,
                        "B.Cu" => back = true,
                        "*.Cu" | "F&B.Cu" => (front, back) = (true, true),
                        _ => {}
                    }
                }
            }
            if !front && !back {
                continue;
            }
            let rel = pad.child("at").and_then(pt).unwrap_or((0, 0));
            let r = rotate_origin(rel.0, rel.1, Angle(fp_orient));
            let size = pad.child("size").and_then(pt).unwrap_or((0, 0));
            out.push(PadClearance {
                at: (r.0.wrapping_add(fp_pos.0), r.1.wrapping_add(fp_pos.1)),
                size: size.0.max(size.1),
                local,
                front,
                back,
            });
        }
    }
    out
}

/// The circles of `pads` whose clearance exceeds `default_nm`, on `front`
/// and `back` (the outer copper layers' names), in the DSN frame.
pub fn keepouts(pads: &[PadClearance], default_nm: i32, front: &str, back: &str) -> Vec<Keepout> {
    let mut out = Vec::new();
    for p in pads.iter().filter(|p| p.local > default_nm) {
        let diameter = um(i64::from(p.size) + 2 * (i64::from(p.local) - i64::from(default_nm)));
        let center = map_pt(i64::from(p.at.0), i64::from(p.at.1));
        let sides = [(p.front, front), (p.back, back)];
        for (_, layer) in sides.into_iter().filter(|(on, _)| *on) {
            out.push(Keepout {
                kind: KeepoutKind::Keepout,
                area: Area {
                    name: Some(KEEPOUT_NAME.to_owned()),
                    shape: Shape::Circle {
                        layer: LayerRef::Named(layer.to_owned()),
                        diameter,
                        center,
                    },
                    windows: Vec::new(),
                    clearance_class: None,
                },
            });
        }
    }
    out
}

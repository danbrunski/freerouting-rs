//! Q22.20: room for a zone's thermal spokes, as keepouts that carry the pad's
//! net. Not Java.
//!
//! KiCad's zone fill joins a pad to a zone of its net with thermal relief
//! through spokes, and its DRC (`starved_thermal`) wants at least Board
//! Setup's `min_resolved_spokes` of them (default 2) to reach fill that
//! connects to something other than the pad. Routed copper of another net
//! that crosses a spoke's way out leaves the spoke without fill: on
//! canary-pwr `ab786fcc`, tracks along both sides of J1's 2.54 mm header rows
//! starved its GND pads 6, 14, 20 and 25 on F.Cu and B.Cu.
//!
//! The KiCad plugin (`kicad-plugin/freerouting_rs/thermal_spokes.py`) and the
//! `.kicad_pcb` reader (`freerouting_kicad::thermal_spokes`) write one
//! rectangle per spoke direction KiCad uses, for each pad a zone of its net
//! connects by thermal relief, as a `keepout` named
//! [`THERMAL_SPOKE_KEEPOUT_NAME`] with `(clearance_class null)`: it is
//! already the spoke's width plus the zone's clearance on each side. It runs
//! from the pad's centre, the midpoint of one of its short sides, so its own
//! pin is under that point.
//! [`insert_thermal_keepouts`] inserts them after the components:
//!
//! - **pins of one net at its short sides' midpoints** (its start, and its far
//!   end): a keepout that carries that net, so
//!   the pad's own net routes through it, while traces and vias of every
//!   other net keep out (`ObstacleArea::is_pad_keepout`: no copper, never an
//!   obstacle for pins or other areas, so a neighbouring pad of another net
//!   that it overlaps on a fine-pitch header is no violation);
//! - **no pin, or pins of two or more nets there** (the far end in another
//!   net's pad: the spoke has no fill to reach there anyway): left out, with
//!   one log line for all of them.
//!
//! The DSN writer writes it back under its name, so it reads back the same.
//! A DSN without the name reads as Java reads it.
//!
//! [`THERMAL_SPOKE_KEEPOUT_NAME`]: crate::board::items::THERMAL_SPOKE_KEEPOUT_NAME

use std::collections::BTreeSet;

use freerouting_geometry::{IntBox, TileShape};

use super::ReadContext;
use super::pad_keepouts::pins_in;
use super::shape::{DsnArea, DsnShapeKind};
use super::structure::insert_keepout;
use crate::board::items::THERMAL_SPOKE_KEEPOUT_NAME;

/// True if `area` is a thermal-spoke keepout (module docs).
pub(crate) fn is_thermal_keepout(area: &DsnArea) -> bool {
    area.name.as_deref() == Some(THERMAL_SPOKE_KEEPOUT_NAME)
}

/// Inserts the keepouts [`super::structure::read_structure`] put aside
/// (module docs), in file order. `false` if one cannot be inserted.
pub(crate) fn insert_thermal_keepouts(ctx: &mut ReadContext) -> bool {
    let areas = std::mem::take(&mut ctx.thermal_keepouts);
    if ctx.board.is_none() || areas.is_empty() {
        return true;
    }
    let mut left_out = 0usize;
    for area in &areas {
        match net_under(ctx, area) {
            Some(net) => {
                if !insert_keepout(ctx, area, 0, false, Some(&[net])) {
                    return false;
                }
            }
            None => left_out += 1,
        }
    }
    if left_out > 0 {
        ctx.log(format!(
            "keepout '{THERMAL_SPOKE_KEEPOUT_NAME}': {left_out} of {} over no pin or over pins of \
             several nets, left out",
            areas.len()
        ));
    }
    true
}

/// The one net of the pins at the midpoints of `area`'s two short sides: one
/// is the pad's centre, where the writers start each keepout, the other its
/// far end (a rectangle written back by the DSN writer keeps its corners, not
/// their order).
fn net_under(ctx: &ReadContext, area: &DsnArea) -> Option<i32> {
    let layer = area.layer();
    if layer.no < 0 || layer.is_signal_pseudo_layer() {
        return None;
    }
    let shape = area
        .shapes
        .first()
        .filter(|s| s.kind == DsnShapeKind::Polygon)?;
    let c = &shape.coor;
    let mut corners: Vec<(f64, f64)> = c.chunks_exact(2).map(|p| (p[0], p[1])).collect();
    if corners.len() > 1 && corners.first() == corners.last() {
        corners.pop();
    }
    if corners.len() != 4 {
        return None;
    }
    let mut sides: Vec<(f64, usize)> = (0..4)
        .map(|i| {
            let (p, q) = (corners[i], corners[(i + 1) % 4]);
            ((q.0 - p.0).hypot(q.1 - p.1), i)
        })
        .collect();
    sides.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
    let at: Vec<TileShape> = sides[..2]
        .iter()
        .map(|&(_, i)| {
            let (p, q) = (corners[i], corners[(i + 1) % 4]);
            let m = ctx
                .coordinate_transform
                .dsn_to_board_point((p.0 + q.0) / 2.0, (p.1 + q.1) / 2.0)
                .round();
            TileShape::IntBox(IntBox::new(m.x, m.y, m.x, m.y))
        })
        .collect();
    let board = ctx.board.as_ref()?;
    let nets: BTreeSet<i32> = pins_in(board, &at, layer.no)
        .iter()
        .filter_map(|id| board.item(*id))
        .flat_map(|item| item.net_numbers().iter().copied())
        .filter(|n| *n > 0)
        .collect();
    match nets.len() {
        1 => nets.first().copied(),
        _ => None,
    }
}

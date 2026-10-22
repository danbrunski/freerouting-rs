//! K15.19: the pins' real drilled holes on a `.kicad_pcb` board.
//!
//! Not Java. KiCad's DSN export writes no pad drill, so Java (and the port
//! on a DSN) estimates a pin's drill radius from its padstack
//! ([`Padstack::get_drill_radius`](crate::library::Padstack::get_drill_radius):
//! 0.45 of the smallest pad radius when the padstack's name gives none). The
//! hole clearance (`router.hole_clearance_um`) is then kept from that
//! estimate. On wisweep's driver board J10's NPTH is an oval 0.95 x 0.65 mm
//! slot with no copper, exported as a pin whose pad is the slot: the
//! estimate is a 0.146 mm radius inside a 0.325 mm half width, so the router
//! kept only the 0.2 mm copper clearance from the hole's wall, under the
//! board's 0.25 mm `min_hole_clearance` (K15.14's run: 0.2148 and 0.2247 mm).
//!
//! A `.kicad_pcb` job ([`crate::job::prepare::prepare_job`]) gives each pin
//! on more than one layer the hole its pad has in the file
//! ([`BasicBoard::set_pad_holes`], [`DrillData::hole`](super::items::DrillData::hole)),
//! matched by its centre. The search trees' hole inflation
//! (`drillHoleClearanceDelta`, the hole obstacle on a layer without pad) and
//! Q20's `hole_clearance` check ([`crate::drc::board_rules`]) then measure the
//! real hole; a DSN board is unchanged. The hole-to-hole distance between
//! drills (Q22.11, [`crate::board::hole_spacing`]) still takes the padstack's
//! radius.

use std::collections::BTreeMap;

use freerouting_geometry::FloatPoint;

use super::BasicBoard;
use super::items::{DrillHole, ItemId};

/// A pad's hole on the board: the segment `start..end` (one point for a
/// round hole) widened by `radius`, in board units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PadHoleOnBoard {
    pub start: FloatPoint,
    pub end: FloatPoint,
    pub radius: f64,
}

impl BasicBoard {
    /// Gives every pin on more than one layer whose centre (rounded to board
    /// units) is a hole's centre that hole, the first hole of `holes` at the
    /// centre where several are. Pins already in the search trees are
    /// reinserted when the board checks a hole clearance (their tree shapes
    /// depend on the hole only then). Returns the number of pins that got a
    /// hole.
    pub fn set_pad_holes(&mut self, holes: &[PadHoleOnBoard]) -> usize {
        let mut at: BTreeMap<(i32, i32), DrillHole> = BTreeMap::new();
        for hole in holes {
            let centre = FloatPoint::new(
                (hole.start.x + hole.end.x) / 2.0,
                (hole.start.y + hole.end.y) / 2.0,
            );
            let key = (centre.x.round() as i32, centre.y.round() as i32);
            at.entry(key).or_insert(DrillHole {
                radius: hole.radius,
                half: (
                    (hole.end.x - hole.start.x) / 2.0,
                    (hole.end.y - hole.start.y) / 2.0,
                ),
            });
        }
        let pins: Vec<(ItemId, DrillHole)> = self
            .items()
            .filter(|item| item.is_pin())
            .filter_map(|item| {
                let drill = item.drill()?;
                if drill.first_layer == drill.last_layer {
                    return None;
                }
                let hole = at.get(&(drill.center.x, drill.center.y))?;
                (drill.hole != Some(*hole)).then_some((item.id(), *hole))
            })
            .collect();
        let reinsert = self.rules.hole_clearance() > 0;
        for (id, hole) in &pins {
            if reinsert {
                self.remove_from_trees(*id);
            }
            if let Some(super::items::ItemKind::Pin(pin)) =
                self.items.get_mut(id).map(|item| &mut item.kind)
            {
                pin.drill.hole = Some(*hole);
            }
            if reinsert {
                self.insert_into_trees(*id);
            }
        }
        pins.len()
    }
}

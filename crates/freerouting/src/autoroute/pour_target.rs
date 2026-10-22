//! K15.25: `router.pour_fill_target`, where the maze search reaches a
//! signal-layer pour of a `.kicad_pcb`.
//!
//! Java's search reaches a conduction area anywhere inside its outline: the
//! area's tree shapes are its outline tiles, a complete room that meets one
//! gets a target door to it (`CompleteFreeSpaceExpansionRoom.calculateTargetDoors`),
//! the door's connection point is the nearest point of the tile to the
//! entry (`MazeSearchEngine.expandToTargetDoors`), and the located path
//! ends at the centre of gravity of the tile within the room
//! (`FoundConnectionLocator.calculateStartingPoint`), or, at a start area,
//! at the nearest point of the tile within the room shrunk by the trace's
//! half width (the constructor's final target shape). A pour is no obstacle,
//! so other nets cross it, and KiCad's refill keeps its fill clear of their
//! copper and drops every part narrower than the zone's `min_thickness`. A
//! trace that ends inside the outline where the fill no longer reaches is
//! KiCad's `track_dangling`, and the connection is open (K15.23's fixture
//! `tests/data/kicad/pcu-k1523/a-stub-beside-a-crossing`: `/CH6_NO` from J2
//! ends at (124.5279, 109.2002) mm, in a 0.347 mm strip of EFUSE_NO_6 right
//! of a crossing track).
//!
//! Under `router.pour_fill_target = true` (opt-in: unset in both profiles,
//! and unset or `false` is Java's rule; see "Not in `enhanced`" below) the
//! session takes, for each
//! conduction area of its start and destination sets that
//! [`PourFills::pieces`] judges by its opened fill (a signal-layer pour that
//! is a zone of the `.kicad_pcb`; K15.23), the core of that fill on the
//! board as the connection's search begins ([`fill_tiles`],
//! [`opened_fill_core`]): the fill model eroded by the opening's radius `r`
//! (half the zone's `min_thickness` less 1 µm), as trapezoid tiles. Every
//! point of the core lies at least `r` inside the opened fill, so an end
//! there is in KiCad's fill even where K15.23's growth back is more generous
//! than KiCad's round inflate, and the cleanup counts it a contact (the core
//! lies in its pieces). Then:
//!
//! - a target door to such an area is expanded only where the tile within
//!   the room meets the core in two dimensions ([`fill_parts`]), and its
//!   connection point is the nearest point of those parts to the entry
//!   ([`nearest_point`]);
//! - the located path ends in one of those parts ([`end_point`]): at Java's
//!   point if a part contains it, else at the centre of gravity of the part
//!   nearest to it; at a start area the final target shape is the part
//!   nearest the previous corner ([`nearest_part`]), shrunk by the half
//!   width as Java's;
//! - a start door whose room meets no part of the core is not queued, and
//!   one that does is queued at that end point.
//!
//! **The cycle rule.** Java removes a trace both of whose ends lie in one
//! conduction area of its net as a cycle through it
//! (`PolylineTrace.split`, `Trace.isOverlap`). The insertion of a found
//! path splits it at its corners and normalizes each piece, so a path that
//! crosses the outline outside the fill and ends in the core lost every
//! piece after its first corner inside the outline, and its end moved back
//! there (the pcu board, K15.25's first run: `/CH2_NO`'s fanout reached
//! EFUSE_NO_2's core at (138.049, 102.1244) mm, the piece from (139.644,
//! 102.1244) on was removed, and KiCad reported the 0.1017 mm rest
//! `track_dangling`). Under the setting the board's cycle rule
//! ([`BasicBoard::area_joins_trace_ends`], set on the board by
//! `prepare_job`) keeps such a trace only as a bridge into the fill: its
//! core, computed at that moment, holds exactly one end corner, and the
//! other end is not at a pin or via that the opened fill reaches (the core
//! grown back by `r`, [`FillCore::reaches`]). Every other trace with both
//! ends in the outline is removed as Java removes it: both ends in the
//! core, neither (the trace does not reach the core, so it joins nothing
//! through the fill), or the far end at a pin or via the fill connects.
//! The bridge end is judged by the core, not the opened fill: early in the
//! run (the fanout) the fill still reached the corner 0.02 mm inside the
//! outline, and the routing after it cut it off (K15.25's second run, with
//! the fill's pieces: the same stub). A first version kept every trace
//! whose ends the core did not both hold: on the pcu board it kept 25
//! traces Java removes, most of them redundant copper inside a pour (a
//! `Net-(U24-SW2)` trace from (52.8133, 122.7795) to (52.8133, 122.3244)
//! mm, neither end in the fill, made `Net-(U24-BST1)` route around it and
//! `/V_PD_IN`'s C71-2 connection fail to insert at (53.4463, 121.6225) mm);
//! the bridge rule keeps 8 (K15.25's note).
//!
//! Everything else is Java's: the router's own contacts stay the outline
//! (`BasicBoard::conduction_area_normal_contacts`), so a trace ending in the
//! fill is a contact as before, and the rooms, doors, costs and the queue
//! order are unchanged. K15.31's `router.pour_fill_contacts`
//! ([`crate::board::pour_contacts`]) takes the connectivity's contacts from
//! the fill's islands; with both settings the search reaches an area in the
//! core of the island the area stands for ([`fill_tiles`]), and the cycle
//! rule joins two trace ends only in one island. Power layers, a DSN input and an area no zone of the
//! file matches keep the outline (`PourFills` gives them no fill). The fill
//! is computed per search, so it is the fill of the board the connection is
//! routed on (about 0.3 ms per pour on the pcu board); later connections
//! that cross the pour can still cut a routed end off, which the stub
//! cleanup and the output check handle (K15.23, Q20).
//!
//! **Not in `enhanced`** (decided 2026-10-22 by K15.25's rule: it joins
//! `enhanced` as a correctness rule only if neither the pcu board's KiCad
//! unconnected nor a second board's goes up). With the bridge rule, the
//! pcu board's `.kicad_pcb` route (K15.18's off-arm command) has KiCad
//! unconnected 47 -> 49 and router incomplete 11 -> 14 with the setting,
//! canary-pwr's (K15.14's `pcb-off` command) 40 -> 31, both with 0
//! routing-caused items. The pcu board's extra items are connections the
//! router counts done through a pour's outline where KiCad's fill does not
//! join them (`/CH2_NO`'s tracks from two U4 pads, `/CH5_NO` at R11, a
//! `SPARE_V` island), which the router's contacts (still the outline) do
//! not see, and two nets without a pour that fail to route (`SCL`,
//! `/Charger Circuit/DRV_SUP`). So the setting is opt-in, unset in both
//! profiles.
//!
//! K15.30 measured it as a default (preregistered, decided 2026-10-22): over
//! 16 `.kicad_pcb` corpus rows KiCad unconnected went 370 -> 363 against a
//! set noise of 67.09, with 0 manufacturing breaches and 0 below-minimum
//! items in all 34 graded arms, so it is not kept. The KiCad plugin offers
//! it as a per-board option (`RouteOptions.pour_fill_target`, off in both
//! profiles).
//!
//! A deliberate divergence (`docs/DIVERGENCES.md`, "freerouting (board
//! model)"); pinned by
//! `tests/k15_pour_fill_stub.rs::the_route_from_j2_leaves_no_stub_with_the_fill_target`
//! and `the_cycle_rule_keeps_only_a_bridge_into_the_fill`.
//!
//! [`PourFills::pieces`]: crate::quality::pour_fill::PourFills::pieces

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{FloatPoint, Point, PolylineShapeOps, TileShape, TileShapeOps};

use crate::board::BasicBoard;
use crate::board::items::ItemId;
use crate::quality::pour_fill::{FillCore, opened_fill_core};

impl BasicBoard {
    /// `router.pour_fill_target` (K15.25) on the board's cycle rule.
    pub fn pour_fill_target(&self) -> bool {
        self.pour_fill_target
    }

    /// Sets `router.pour_fill_target` on the board (`prepare_job`).
    pub fn set_pour_fill_target(&mut self, on: bool) {
        self.pour_fill_target = on;
    }

    /// True if contact `contact`, common to both ends of trace `trace`,
    /// joins them into a cycle (Java: always). Under
    /// `router.pour_fill_contacts` (K15.31) a judged pour joins them only
    /// where one island of its fill holds both ends. Under
    /// `router.pour_fill_target` a conduction area that `PourFills` judges
    /// by its opened fill keeps the trace only as a bridge into its fill
    /// (module docs, "The cycle rule"): one end in the core, computed now
    /// ([`opened_fill_core`]), and the other end out of the core and not
    /// at a pin or via of the opened fill.
    pub(crate) fn area_joins_trace_ends(&self, contact: ItemId, trace: ItemId) -> bool {
        // K15.31: under `router.pour_fill_contacts` the pour joins the two
        // ends only if one island of its fill holds both
        // (`board::pour_contacts`); then the rest of the rule.
        if let (Some(islands), Some(t)) = (
            self.pour_islands(contact),
            self.item(trace).and_then(|i| i.as_trace()),
        ) {
            match (
                islands.island_at(&t.first_corner()),
                islands.island_at(&t.last_corner()),
            ) {
                (Some(a), Some(b)) if a == b => {}
                _ => return false,
            }
        }
        if !self.pour_fill_target || !read_from_kicad_pcb(self) {
            return true;
        }
        let (Some(area), Some(t)) = (
            self.item(contact).filter(|i| i.is_conduction_area()),
            self.item(trace).and_then(|i| i.as_trace()),
        ) else {
            return true;
        };
        let Some(core) = opened_fill_core(self, area) else {
            return true;
        };
        let (first, last) = (t.first_corner(), t.last_corner());
        match (core.holds(&first), core.holds(&last)) {
            (true, false) => self.end_joins_fill(&core, &last, &self.trace_end_contacts(trace)),
            (false, true) => self.end_joins_fill(&core, &first, &self.trace_start_contacts(trace)),
            // Both ends in the core, or neither: the trace is no bridge
            // into the fill, so Java's rule.
            _ => true,
        }
    }

    /// True if trace end `corner` outside `core` still joins the fill: a
    /// pin or a via of `contacts` is there and the opened fill reaches it,
    /// so the fill connects it as KiCad's refill does.
    fn end_joins_fill(&self, core: &FillCore, corner: &Point, contacts: &BTreeSet<ItemId>) -> bool {
        core.reaches(corner)
            && contacts
                .iter()
                .filter_map(|id| self.item(*id))
                .any(|item| item.is_pin() || item.is_via())
    }
}

/// True if `board` was read from a `.kicad_pcb`: the setting acts only
/// there, so a DSN input routes as Java's under both profiles
/// ([`KicadReportRules::kicad_pcb`](crate::drc::KicadReportRules::kicad_pcb)).
fn read_from_kicad_pcb(board: &BasicBoard) -> bool {
    board.kicad_report_rules().kicad_pcb
}

/// Per conduction area of `items` that `PourFills` judges by its opened
/// fill, the core of that fill on `board` now ([`opened_fill_core`]); empty
/// for a board not read from a `.kicad_pcb`. Under
/// `router.pour_fill_contacts` (K15.31) the core of the area's main island
/// only ([`BasicBoard::pour_islands`]).
pub fn fill_tiles<'a>(
    board: &BasicBoard,
    items: impl IntoIterator<Item = &'a ItemId>,
) -> BTreeMap<ItemId, Vec<TileShape>> {
    if !read_from_kicad_pcb(board) {
        return BTreeMap::new();
    }
    let areas: BTreeSet<ItemId> = items
        .into_iter()
        .copied()
        .filter(|id| board.item(*id).is_some_and(|i| i.is_conduction_area()))
        .collect();
    areas
        .into_iter()
        .filter_map(|id| {
            // K15.31: under `router.pour_fill_contacts` the area stands for
            // its main island, so the search reaches that island's core.
            if let Some(islands) = board.pour_islands(id) {
                return Some((id, islands.main_core()));
            }
            Some((id, opened_fill_core(board, board.item(id)?)?.tiles))
        })
        .collect()
}

/// The parts of `shape` (an area's tile within a room) that `tiles` (its
/// opened fill) cover in two dimensions, in the order of `tiles`.
pub fn fill_parts(tiles: &[TileShape], shape: &TileShape) -> Vec<TileShape> {
    if shape.is_empty() {
        return Vec::new();
    }
    let bounds = shape.bounding_box();
    tiles
        .iter()
        .filter(|tile| tile.bounding_box().intersects_box(&bounds))
        .map(|tile| shape.intersection(tile))
        .filter(|part| part.dimension() >= 2)
        .collect()
}

/// The part of `parts` the path takes at `point`: the first that contains
/// it, or else the first nearest to it. `None` without parts.
pub fn nearest_part(parts: &[TileShape], point: FloatPoint) -> Option<&TileShape> {
    if let Some(part) = parts.iter().find(|p| p.contains_float(point)) {
        return Some(part);
    }
    let mut best: Option<(&TileShape, f64)> = None;
    for part in parts {
        let d = part.distance(point);
        if best.is_none_or(|(_, b)| d < b) {
            best = Some((part, d));
        }
    }
    best.map(|(p, _)| p)
}

/// The point of `parts` nearest to `point` (the first part wins a tie);
/// `None` without parts.
pub fn nearest_point(parts: &[TileShape], point: FloatPoint) -> Option<FloatPoint> {
    let mut best: Option<(FloatPoint, f64)> = None;
    for part in parts {
        let Some(p) = part.nearest_point_approx(point) else {
            continue;
        };
        let d = p.distance(point);
        if best.is_none_or(|(_, b)| d < b) {
            best = Some((p, d));
        }
    }
    best.map(|(p, _)| p)
}

/// Where the located path ends in `parts` near Java's end `point`: `point`
/// itself if a part contains it, else the centre of gravity of the nearest
/// part. `None` without parts.
pub fn end_point(parts: &[TileShape], point: FloatPoint) -> Option<FloatPoint> {
    let part = nearest_part(parts, point)?;
    if part.contains_float(point) {
        Some(point)
    } else {
        Some(part.centre_of_gravity())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::IntBox;

    fn tile(x0: i32, y0: i32, x1: i32, y1: i32) -> TileShape {
        TileShape::IntBox(IntBox::new(x0, y0, x1, y1))
    }

    /// Parts are the two-dimensional intersections, in tile order; an edge
    /// contact is no part.
    #[test]
    fn the_parts_are_the_fill_tiles_within_the_shape() {
        let shape = tile(0, 0, 100, 100);
        let tiles = [
            tile(200, 0, 300, 100),
            tile(50, 50, 150, 150),
            tile(100, 0, 120, 10),
            tile(-10, -10, 10, 10),
        ];
        let parts = fill_parts(&tiles, &shape);
        let boxes: Vec<IntBox> = parts.iter().map(|p| p.bounding_box()).collect();
        assert_eq!(
            boxes,
            vec![IntBox::new(50, 50, 100, 100), IntBox::new(0, 0, 10, 10)]
        );
        assert!(fill_parts(&tiles, &tile(400, 400, 500, 500)).is_empty());
    }

    /// The end stays where Java puts it inside a part, and moves to the
    /// centre of the nearest part outside them.
    #[test]
    fn the_end_is_javas_inside_the_fill_and_the_nearest_parts_centre_outside() {
        let parts = [tile(0, 0, 10, 10), tile(50, 0, 70, 10)];
        let inside = FloatPoint::new(55.0, 5.0);
        assert_eq!(end_point(&parts, inside), Some(inside));
        let between = FloatPoint::new(40.0, 5.0);
        assert_eq!(end_point(&parts, between), Some(FloatPoint::new(60.0, 5.0)));
        assert_eq!(
            nearest_point(&parts, between),
            Some(FloatPoint::new(50.0, 5.0))
        );
        assert_eq!(end_point(&[], between), None);
        assert_eq!(nearest_point(&[], between), None);
    }
}

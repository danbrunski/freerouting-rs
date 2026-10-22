//! Planes cut where a higher-priority zone of another net fills (K15.15).
//!
//! KiCad's DSN export writes each copper zone's outline as its plane
//! (`specctra_export.cpp`, `zone->IterateWithHoles()`), whatever other zones
//! lie on the same layer. Its zone filler does not fill that outline: it
//! knocks out every zone of another net that is higher in
//! `ZONE::HigherPriority` (`zone.cpp` at tag `10.0.0`: a teardrop above a zone
//! that is none, then the larger `(priority N)`, then the larger UUID), by
//! that zone's fill plus the clearance between the two
//! (`ZONE_FILLER::buildDifferentNetZoneClearances`, `zone_filler.cpp`). The
//! router took the outline for copper, so a via of the lower zone's net
//! dropped where the higher zone fills counted as connected and KiCad found
//! it dangling (the power-control unit's In2.Cu, V_BUS_24V priority 41 over
//! V_BUS_12V priority 1).
//!
//! The reader gives the lower zone's plane a window for every part of a
//! higher zone of another net on that layer, grown by the clearance between
//! the two, that lies in the lower zone's outline ([`knockout_windows`]): the
//! convex pieces of both outlines (`PolygonShape::split_to_convex`), each piece
//! of the higher zone enlarged by the clearance (`TileShape::enlarge`, its
//! border lines moved out and cut by the enlarged bounding octagon), then
//! intersected pair by pair, each intersection of dimension 2 one window. The
//! clearance is the larger of the two zones' own (`(connect_pads (clearance
//! C))`, `ZONE::GetLocalClearance`), which KiCad's clearance constraint takes
//! before any net class. The windows are convex, inside the
//! outline, and may touch each other; the router cuts them out of the plane
//! as it cuts out the zone's own holes (`PolylineArea::split_to_convex`). The
//! cut is computed on a 100 nm grid (a board up to 3.3 m stays within the
//! geometry's `CRIT_INT`), so a window corner may lie up to 50 nm off.
//!
//! The cut uses the higher zone's outline, not its fill: where the higher
//! zone's fill does not reach its outline (its own holes, an even higher
//! zone, foreign copper) the lower plane is cut anyway, so the router may
//! count a connection open that KiCad's fill would make. The enlargement is
//! mitred where the octagon does not cut it, so the cut may reach up to about
//! 8 % of the clearance further at a corner. A fill is not used because a
//! routed board's fills are stale and a pad's thermal relief is no fill at the
//! pad's centre (the router's contact test).

use freerouting_geometry::{
    FloatPoint, IntPoint, Point, PolygonShape, PolylineShapeOps, TileShape,
};

use crate::layers::CuMask;
use crate::legacy::Pt;
use crate::zones::Zone;

/// The grid of the cut, nanometres.
const GRID_NM: f64 = 100.0;

/// KiCad `ZONE::HigherPriority`: `a` is filled before `b` where they overlap. Two
/// zones of equal priority without both UUIDs are neither higher (KiCad would
/// compare the random UUIDs it makes for them).
pub fn higher_priority(a: &Zone, b: &Zone) -> bool {
    if a.teardrop != b.teardrop {
        return a.teardrop;
    }
    if a.priority != b.priority {
        return a.priority > b.priority;
    }
    match (a.uuid, b.uuid) {
        (Some(x), Some(y)) => x > y,
        _ => false,
    }
}

/// True if `other` cuts `zone`'s plane on copper `layer`: a copper zone of
/// another net on that layer, higher in [`higher_priority`].
pub fn cuts(zone: &Zone, other: &Zone, layer: usize, copper_count: usize) -> bool {
    !other.rule_area
        && other.net != zone.net
        && other.layers.and(CuMask::at(layer, copper_count)).any()
        && higher_priority(other, zone)
}

fn bounds(points: &[Pt]) -> (i32, i32, i32, i32) {
    points.iter().fold(
        (i32::MAX, i32::MAX, i32::MIN, i32::MIN),
        |(x0, y0, x1, y1), &(x, y)| (x0.min(x), y0.min(y), x1.max(x), y1.max(y)),
    )
}

fn overlap(a: (i32, i32, i32, i32), b: (i32, i32, i32, i32)) -> bool {
    a.0 < b.2 && b.0 < a.2 && a.1 < b.3 && b.1 < a.3
}

/// The convex pieces of an outline on the cut's grid; `None` if the split fails.
fn convex_pieces(outline: &[Pt]) -> Option<Vec<TileShape>> {
    let on_grid = |v: i32| (f64::from(v) / GRID_NM).round() as i32;
    let corners: Vec<Point> = outline
        .iter()
        .map(|&(x, y)| Point::Int(IntPoint::new(on_grid(x), on_grid(y))))
        .collect();
    PolygonShape::new(&corners).split_to_convex()
}

fn to_nm(p: FloatPoint) -> Pt {
    (
        (p.x * GRID_NM).round() as i32,
        (p.y * GRID_NM).round() as i32,
    )
}

/// The windows the zones `higher` cut into the plane of `lower`'s outline,
/// each with the index in `higher` of the zone that cuts it: one per pair of
/// convex pieces of the outline and of a higher zone's outline, grown by the
/// larger of the two zones' clearances, that overlap with positive area, in the
/// order of `higher`, then of the pieces.
/// `None` if an outline cannot be split into convex pieces (it is not a simple
/// polygon).
pub fn knockout_windows(lower: &Zone, higher: &[&Zone]) -> Option<Vec<(usize, Vec<Pt>)>> {
    let outline = &lower.outline;
    let own_box = bounds(outline);
    let mut own: Option<Vec<TileShape>> = None;
    let mut windows = Vec::new();
    for (i, zone) in higher.iter().enumerate() {
        let gap = lower.clearance.max(zone.clearance).max(0);
        let (x0, y0, x1, y1) = bounds(&zone.outline);
        let grown = (
            x0.saturating_sub(gap),
            y0.saturating_sub(gap),
            x1.saturating_add(gap),
            y1.saturating_add(gap),
        );
        if !overlap(own_box, grown) {
            continue;
        }
        let offset = f64::from(gap) / GRID_NM;
        let pieces = match &own {
            Some(p) => p,
            None => own.insert(convex_pieces(outline)?),
        };
        let cut = convex_pieces(&zone.outline)?;
        for p in pieces {
            for q in &cut {
                let r = p.intersection(&q.enlarge(offset));
                if r.is_empty() || r.dimension() < 2 {
                    continue;
                }
                let mut ring: Vec<Pt> = r.corner_approx_arr().into_iter().map(to_nm).collect();
                ring.dedup();
                if ring.len() > 1 && ring.first() == ring.last() {
                    ring.pop();
                }
                if ring.len() >= 3 {
                    windows.push((i, ring));
                }
            }
        }
    }
    Some(windows)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zone(priority: u32, uuid: u8, teardrop: bool) -> Zone {
        Zone {
            node: 0,
            name: None,
            rule_area: false,
            flags: Default::default(),
            layers: CuMask::all(4),
            outline: vec![
                (0, 0),
                (1_000_000, 0),
                (1_000_000, 1_000_000),
                (0, 1_000_000),
            ],
            holes: Vec::new(),
            net: 1,
            arcs: 0,
            in_footprint: false,
            priority,
            uuid: Some([uuid; 16]),
            teardrop,
            clearance: 0,
        }
    }

    #[test]
    fn higher_priority_is_kicads_order() {
        assert!(higher_priority(&zone(2, 0, false), &zone(1, 9, false)));
        assert!(!higher_priority(&zone(1, 9, false), &zone(2, 0, false)));
        // equal priorities: the larger UUID
        assert!(higher_priority(&zone(1, 9, false), &zone(1, 0, false)));
        assert!(!higher_priority(&zone(1, 0, false), &zone(1, 9, false)));
        // a teardrop above any zone that is none
        assert!(higher_priority(&zone(0, 0, true), &zone(9, 9, false)));
        let mut a = zone(1, 0, false);
        a.uuid = None;
        assert!(!higher_priority(&a, &zone(1, 9, false)));
        assert!(!higher_priority(&zone(1, 9, false), &a));
    }

    #[test]
    fn a_square_over_a_corner_cuts_one_window_inside_the_outline() {
        let lower = zone(0, 0, false);
        let mut upper = zone(1, 0, false);
        upper.net = 2;
        upper.outline = vec![
            (500_000, 500_000),
            (2_000_000, 500_000),
            (2_000_000, 2_000_000),
            (500_000, 2_000_000),
        ];
        let w = knockout_windows(&lower, &[&upper]).unwrap();
        assert_eq!(w.len(), 1);
        let mut ring = w[0].1.clone();
        ring.sort();
        assert_eq!(
            ring,
            [
                (500_000, 500_000),
                (500_000, 1_000_000),
                (1_000_000, 500_000),
                (1_000_000, 1_000_000)
            ]
        );
        // apart: nothing
        upper.outline.iter_mut().for_each(|p| p.0 += 5_000_000);
        assert!(knockout_windows(&lower, &[&upper]).unwrap().is_empty());
    }

    #[test]
    fn the_clearance_grows_the_cut() {
        // the higher zone 0.1 mm right of the lower one, clearances 0.3 and 0.2 mm: a
        // 0.2 mm strip of the lower plane is cut
        let mut lower = zone(0, 0, false);
        lower.clearance = 200_000;
        let mut upper = zone(1, 0, false);
        upper.net = 2;
        upper.clearance = 300_000;
        upper.outline = vec![
            (1_100_000, 0),
            (2_000_000, 0),
            (2_000_000, 1_000_000),
            (1_100_000, 1_000_000),
        ];
        let w = knockout_windows(&lower, &[&upper]).unwrap();
        assert_eq!(w.len(), 1);
        let mut ring = w[0].1.clone();
        ring.sort();
        assert_eq!(
            ring,
            [
                (800_000, 0),
                (800_000, 1_000_000),
                (1_000_000, 0),
                (1_000_000, 1_000_000)
            ]
        );
        // with no clearance the two do not meet
        upper.clearance = 0;
        lower.clearance = 0;
        assert!(knockout_windows(&lower, &[&upper]).unwrap().is_empty());
    }

    #[test]
    fn the_cut_holds_at_the_board_limits() {
        // a 2 m board edge: 2e9 nm, 2e7 on the 100 nm grid, below CRIT_INT (2^25)
        let far = 2_000_000_000;
        let mut lower = zone(0, 0, false);
        lower.outline = vec![(-far, -far), (far, -far), (far, far), (-far, far)];
        let mut upper = zone(1, 0, false);
        upper.net = 2;
        upper.outline = vec![
            (far - 1_000_000, -far),
            (far + 1_000, -far),
            (far + 1_000, far),
            (far - 1_000_000, far),
        ];
        let w = knockout_windows(&lower, &[&upper]).unwrap();
        assert_eq!(w.len(), 1);
        let (x0, x1) = w[0]
            .1
            .iter()
            .fold((i32::MAX, i32::MIN), |(a, b), p| (a.min(p.0), b.max(p.0)));
        assert_eq!((x0, x1), (far - 1_000_000, far));
    }
}

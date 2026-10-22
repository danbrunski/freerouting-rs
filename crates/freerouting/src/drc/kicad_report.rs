//! K15.5, K15.11: the DRC report counted as KiCad counts.
//!
//! Not Java. [`DesignRulesChecker::generate_report`] writes Java's violation
//! types, and under [`ReportRules::Kicad`] (the default) it counts as KiCad
//! 10's DRC does:
//!
//! - **`clearance`** is between copper of different nets only. Java's
//!   `Item.isObstacle` makes a via an obstacle for a pin (or a via) of its own
//!   net when the via's padstack does not allow attaching, and the KiCad
//!   reader writes `(attach off)` on every padstack, so every via of a pad's
//!   net inside or beside the pad is a clearance violation for Java. A pair
//!   whose items share a net is left out ([`reports_clearance`]) unless one of
//!   its items is copper the router placed (a trace or via that is not user
//!   or system fixed): a via the run put in a pad stays in the report, as the
//!   router must never place one there (D8). The search's obstacle test
//!   (`BasicBoard::is_obstacle`) is unchanged.
//!
//!   K15.11: a pair kept is measured again on the items' own copper
//!   ([`kicad_copper`](super::kicad_copper): round trace ends, circles for
//!   vias and round pads; Java measures the search-tree octagons), against the
//!   clearance of the board's unconditional `.kicad_dru` rule where there is
//!   one ([`KicadReportRules::clearance_um`]; KiCad takes the rule over the
//!   net-class clearance, the router the larger of the two), and stays a
//!   violation only if it is closer by more than `clearance_tolerance_um`.
//!   A pair is reported once per layer, as Java and KiCad 10.0.7 do (two
//!   through vias 0.1673 mm apart under a 0.2 mm clearance are four
//!   `clearance` findings on a 4-layer board), with KiCad's type
//!   `clearance` (Java's `holeClearance` is copper clearance with a pin or
//!   via in the pair, not a drill). A pair with an area or the outline keeps
//!   Java's measure.
//! - **`hole_clearance`** (K15.11): copper (a trace, via or pad) closer to a
//!   drilled hole of another net (a via's, a pad's, an NPTH) than KiCad's hole
//!   clearance ([`kicad_hole_clearances`], [`KicadReportRules`]), once per
//!   pair and layer, unless the pair is already a `clearance` finding on that
//!   layer (KiCad: "no need for multiple violations"). Only a board read from a `.kicad_pcb` carries
//!   the pads' holes and the hole clearance; a DSN board has none.
//! - **`via_dangling`**: a via is dangling when its copper meets copper of its
//!   net on fewer than two layers ([`via_dangling`], Q16's
//!   [`via_contact_layers`](crate::quality::stub_contacts::via_contact_layers)),
//!   not when Java's contacts (items of its net whose corner or centre is the
//!   via's centre, and areas that contain it) are fewer than two or all on one
//!   layer (`Item.isTail`). So a via that joins an SMD pad, off the pad's
//!   centre, to its net's plane on an inner layer is connected. K15.11: a pour
//!   the reader left out of the board (`BasicBoard::dropped_pours`, a pour of
//!   a net with a power-layer plane) counts as copper of its net on its layer
//!   where it contains the via's centre: KiCad fills it.
//! - **`track_dangling`** (K15.11: KiCad's `TestTrackEndpointDangling`, per
//!   segment of the design). A board trace is the design's segments: its
//!   polyline split at its corners and at the wire corners the DSN reader
//!   dropped as collinear (`BasicBoard::wire_joints`). A segment is connected
//!   to another item of its net on its layer when an end of either lies on
//!   the other's copper (KiCad's anchors: a segment's ends, a pad's or via's
//!   centre). An end is met when the disc of half the segment's width around
//!   it touches such an item. A zone (a conduction area, a dropped pour, at
//!   a point inside it) or a pad or via that meets both ends connects the
//!   segment; another segment that meets both ends meets only the end nearer
//!   to its own ends, so a short segment lying on one track at both ends is
//!   dangling. A trace is reported once if any of its segments is dangling.
//!
//! [`ReportRules::Java`] keeps Java's report: `tests/reference_drc.rs`
//! compares it with Java's DRC command line. The board-level findings
//! ([`DesignRulesChecker::check`], [`DesignRulesChecker::all_unconnected_items`])
//! that a viewer draws stay Java's, and so do the split nets and the report's
//! scores (they are the router's: see
//! `router.java_quirks.same_net_via_pad_violation` for its count of the
//! same-net pairs).
//!
//! [`DesignRulesChecker::generate_report`]: super::DesignRulesChecker::generate_report
//! [`DesignRulesChecker::check`]: super::DesignRulesChecker::check
//! [`DesignRulesChecker::all_unconnected_items`]: super::DesignRulesChecker::all_unconnected_items
//! [`KicadReportRules::clearance_um`]: super::KicadReportRules::clearance_um
//! [`KicadReportRules`]: super::KicadReportRules

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{FloatPoint, IntBox, Point, TileShape};

use super::clearance::ClearanceViolation;
use super::kicad_copper::{Copper, exact_clearance, point_segment_distance};
use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::board::structure::FixedState;
use crate::quality::stub_contacts;

/// Which DRC's counting a report follows (see the module docs).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ReportRules {
    /// KiCad's: clearance between different nets only, on the items' own
    /// copper, once per pair; hole clearance of pads; dangling vias and tracks
    /// by copper overlap.
    #[default]
    Kicad,
    /// Java's `DesignRulesChecker.generateReport`.
    Java,
}

/// True if `item` is copper the router placed or may move: a trace or via
/// that is neither user nor system fixed. Input copper read from a
/// `.kicad_pcb` is user fixed.
pub(crate) fn is_routed_copper(item: &Item) -> bool {
    (item.is_trace() || item.is_via()) && item.fixed_state() < FixedState::UserFixed
}

/// True if violation `v` is between items of a common net and neither of them
/// is copper the router placed ([`is_routed_copper`]): a via of the input in
/// or beside a pad of its own net.
pub(crate) fn is_same_net_input_pair(board: &BasicBoard, v: &ClearanceViolation) -> bool {
    let (Some(first), Some(second)) = (board.item(v.first_item), board.item(v.second_item)) else {
        return false;
    };
    first.shares_net(second) && !is_routed_copper(first) && !is_routed_copper(second)
}

/// True if a KiCad-counted report keeps clearance violation `v` (see the
/// module docs).
pub(crate) fn reports_clearance(board: &BasicBoard, v: &ClearanceViolation) -> bool {
    !is_same_net_input_pair(board, v)
}

/// The report's shortfall tolerance in board units
/// (`router.clearance_tolerance_um`).
fn tolerance_units(board: &BasicBoard) -> f64 {
    board.rules.clearance_tolerance_um.max(0.0) / board.board_unit_to_um_factor()
}

/// True if `item` is copper KiCad measures on its own shape: a trace, via or
/// pin.
fn is_copper(item: &Item) -> bool {
    item.is_trace() || item.is_via() || item.is_pin()
}

/// The clearance violations of `java` a KiCad-counted report keeps (module
/// docs), in Java's order: once per pair of items and layer, as Java and
/// KiCad report them.
pub(crate) fn kicad_clearances(
    board: &BasicBoard,
    java: &[ClearanceViolation],
) -> Vec<ClearanceViolation> {
    let unit_to_um = board.board_unit_to_um_factor();
    let rule = board
        .kicad_report_rules()
        .clearance_um
        .map(|um| um / unit_to_um);
    let tolerance = tolerance_units(board);
    java.iter()
        .filter(|v| reports_clearance(board, v))
        .filter_map(|violation| {
            let first = board.item(violation.first_item)?;
            let second = board.item(violation.second_item)?;
            let mut violation = violation.clone();
            if let Some(actual) = exact_clearance(first, second, violation.layer) {
                if let Some(rule) = rule
                    && is_copper(first)
                    && is_copper(second)
                {
                    violation.expected_clearance = rule;
                }
                if violation.expected_clearance - actual <= tolerance {
                    return None;
                }
                violation.actual_clearance = actual.max(0.0);
            }
            Some(violation)
        })
        .collect()
}

/// The hole of a [`HoleClearance`] finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum HoleOwner {
    /// A via's or a plated pad's hole.
    Item(ItemId),
    /// The NPTH hole at this index of [`KicadReportRules::holes`](super::KicadReportRules::holes).
    Npth(usize),
}

/// A KiCad `hole_clearance` finding: copper too close to a hole.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct HoleClearance {
    pub copper: ItemId,
    pub hole: HoleOwner,
    /// The hole clearance, in board units.
    pub required: f64,
    /// The gap from the copper to the hole's wall, in board units.
    pub actual: f64,
    /// The hole's centre.
    pub location: FloatPoint,
}

/// A drilled hole to check: its owner, its shape on the board and its layers.
struct Hole {
    owner: HoleOwner,
    shape: Copper,
    centre: FloatPoint,
    reach: f64,
    layers: std::ops::RangeInclusive<i32>,
}

/// The board's drilled holes: the vias' (from their padstacks), and the
/// pads' read from the `.kicad_pcb` (a plated one belongs to the pin whose
/// centre is the hole's; one with no such pin is left out).
fn holes(board: &BasicBoard) -> Vec<Hole> {
    let rules = board.kicad_report_rules();
    let all_layers = 0..=(board.layer_count() as i32 - 1);
    let mut pins_at: BTreeMap<(i32, i32), Vec<ItemId>> = BTreeMap::new();
    for item in board.items().filter(|i| i.is_pin()) {
        if let Some(drill) = item.drill() {
            let c = drill.center;
            pins_at.entry((c.x, c.y)).or_default().push(item.id());
        }
    }
    let mut out = Vec::new();
    for item in board.items().filter(|i| i.is_via()) {
        let (Some(radius), Some(drill)) = (board.hole_radius(item), item.drill()) else {
            continue;
        };
        let centre = drill.center.to_float();
        out.push(Hole {
            owner: HoleOwner::Item(item.id()),
            shape: Copper::disc(centre, radius),
            centre,
            reach: radius,
            layers: drill.first_layer..=drill.last_layer,
        });
    }
    for (index, hole) in rules.holes.iter().enumerate() {
        let centre = hole.centre();
        let owner = if hole.plated {
            let key = (centre.x.round() as i32, centre.y.round() as i32);
            match pins_at.get(&key).and_then(|ids| ids.first()) {
                Some(id) => HoleOwner::Item(*id),
                None => continue,
            }
        } else {
            HoleOwner::Npth(index)
        };
        let half_length = (hole.end.x - hole.start.x).hypot(hole.end.y - hole.start.y) / 2.0;
        out.push(Hole {
            owner,
            shape: Copper::Rounded(vec![(hole.start, hole.end)], hole.radius),
            centre,
            reach: hole.radius + half_length,
            layers: all_layers.clone(),
        });
    }
    out
}

/// KiCad's `hole_clearance` findings of the board (module docs), once per
/// copper item, hole and layer, sorted by copper item, hole and layer; `skip`
/// holds the item pairs (lower id first) and layers that are `clearance`
/// findings already.
pub(crate) fn kicad_hole_clearances(
    board: &BasicBoard,
    skip: &BTreeSet<(i32, i32, i32)>,
) -> Vec<HoleClearance> {
    let Some(required_um) = board.kicad_report_rules().hole_clearance_um else {
        return Vec::new();
    };
    let required = required_um / board.board_unit_to_um_factor();
    if required <= 0.0 {
        return Vec::new();
    }
    let tolerance = tolerance_units(board);
    let mut found: BTreeMap<(ItemId, HoleOwner, i32), HoleClearance> = BTreeMap::new();
    for hole in holes(board) {
        let owner = match hole.owner {
            HoleOwner::Item(id) => board.item(id),
            HoleOwner::Npth(_) => None,
        };
        let r = hole.reach + required + 1.0;
        let region = TileShape::IntBox(IntBox::new(
            (hole.centre.x - r).floor() as i32,
            (hole.centre.y - r).floor() as i32,
            (hole.centre.x + r).ceil() as i32,
            (hole.centre.y + r).ceil() as i32,
        ));
        for layer in hole.layers.clone() {
            for id in board.overlapping_objects(&region, layer) {
                let item = board.item_ref(id);
                if !is_copper(item)
                    || owner.is_some_and(|o| o.id() == id || o.shares_net(item))
                    || owner.is_some_and(|o| {
                        let (a, b) = (o.id().0, id.0);
                        skip.contains(&(a.min(b), a.max(b), layer))
                    })
                {
                    continue;
                }
                let Some(copper) = Copper::of(item, layer) else {
                    continue;
                };
                let gap = copper.gap(&hole.shape);
                if gap >= required - tolerance {
                    continue;
                }
                let entry = found
                    .entry((id, hole.owner, layer))
                    .or_insert_with(|| HoleClearance {
                        copper: id,
                        hole: hole.owner,
                        required,
                        actual: f64::MAX,
                        location: hole.centre,
                    });
                entry.actual = entry.actual.min(gap.max(0.0));
            }
        }
    }
    found.into_values().collect()
}

/// True if via `id` meets copper of its net on fewer than two layers, a
/// dropped pour that contains its centre included.
pub(crate) fn via_dangling(board: &BasicBoard, id: ItemId) -> bool {
    let Some(via) = board.item(id) else {
        return false;
    };
    let mut layers = stub_contacts::via_contact_layers(board, via);
    if let Some(drill) = via.drill() {
        let centre = Point::Int(drill.center);
        for pour in board.dropped_pours() {
            if (drill.first_layer..=drill.last_layer).contains(&pour.layer)
                && via.contains_net(pour.net_number)
                && pour.area.contains(&centre)
            {
                layers.insert(pour.layer);
            }
        }
    }
    layers.len() <= 1
}

/// One of the design's segments of a board trace: its ends and half width.
#[derive(Clone, Copy, Debug)]
struct Segment {
    a: FloatPoint,
    b: FloatPoint,
    half_width: f64,
}

impl Segment {
    fn copper(&self) -> Copper {
        Copper::Rounded(vec![(self.a, self.b)], self.half_width)
    }

    /// KiCad's `getMinDist` of a track: the distance to its nearer end.
    fn min_dist(&self, p: FloatPoint) -> f64 {
        (self.a.x - p.x)
            .hypot(self.a.y - p.y)
            .min((self.b.x - p.x).hypot(self.b.y - p.y))
    }
}

/// The design's segments trace `item` is made of: its polyline split at its
/// corners and at the wire corners the reader dropped as collinear
/// (`BasicBoard::wire_joints`) that lie on it.
fn segments(board: &BasicBoard, item: &Item) -> Vec<Segment> {
    let Some(trace) = item.as_trace() else {
        return Vec::new();
    };
    let half_width = f64::from(trace.half_width);
    let corners = trace.polyline.corner_approx_arr();
    let bounds = item.bounding_box();
    let joints: Vec<FloatPoint> = board
        .wire_joints()
        .range((trace.layer, bounds.ll.x, i32::MIN)..=(trace.layer, bounds.ur.x, i32::MAX))
        .filter(|(_, _, y)| (bounds.ll.y..=bounds.ur.y).contains(y))
        .map(|(_, x, y)| FloatPoint::new(f64::from(*x), f64::from(*y)))
        .collect();
    let mut out = Vec::new();
    for w in corners.windows(2) {
        let (a, b) = (w[0], w[1]);
        let (dx, dy) = (b.x - a.x, b.y - a.y);
        let length_squared = dx * dx + dy * dy;
        let mut cuts: Vec<(f64, FloatPoint)> = joints
            .iter()
            .filter(|p| point_segment_distance(a, b, **p) <= 1.0)
            .map(|p| (((p.x - a.x) * dx + (p.y - a.y) * dy) / length_squared, *p))
            .filter(|(t, _)| *t > 0.0 && *t < 1.0)
            .collect();
        cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
        let mut from = a;
        for (_, p) in cuts {
            out.push(Segment {
                a: from,
                b: p,
                half_width,
            });
            from = p;
        }
        out.push(Segment {
            a: from,
            b,
            half_width,
        });
    }
    out
}

/// Which ends of a segment a connected item meets.
enum Meets {
    /// A zone, pad or via that meets both ends: the segment is connected.
    Both,
    /// Whether the item meets the start and the end.
    Ends(bool, bool),
}

/// What a zone whose copper `contains` a point does for segment `s`: KiCad
/// joins a zone at an anchor inside its fill.
fn zone_meets(contains: impl Fn(FloatPoint) -> bool, s: &Segment) -> Meets {
    match (contains(s.a), contains(s.b)) {
        (true, true) => Meets::Both,
        (start, end) => Meets::Ends(start, end),
    }
}

/// What a pad or via with copper `copper` and centre `centre` does for
/// segment `s` (module docs).
fn pad_meets(copper: &Copper, centre: FloatPoint, s: &Segment) -> Meets {
    let connected = copper.contains(s.a) || copper.contains(s.b) || s.copper().contains(centre);
    if !connected {
        return Meets::Ends(false, false);
    }
    match (
        copper.distance_to(s.a) <= s.half_width,
        copper.distance_to(s.b) <= s.half_width,
    ) {
        (true, true) => Meets::Both,
        (start, end) => Meets::Ends(start, end),
    }
}

/// What another segment `o` of the net does for segment `s` (module docs).
fn segment_meets(o: &Segment, s: &Segment) -> Meets {
    let (oc, sc) = (o.copper(), s.copper());
    let connected = oc.contains(s.a) || oc.contains(s.b) || sc.contains(o.a) || sc.contains(o.b);
    if !connected {
        return Meets::Ends(false, false);
    }
    let hit_start = oc.distance_to(s.a) <= s.half_width;
    let hit_end = oc.distance_to(s.b) <= s.half_width;
    if hit_start && hit_end {
        // Both ends on one track: it meets only the nearer end.
        return if o.min_dist(s.a) < o.min_dist(s.b) {
            Meets::Ends(true, false)
        } else {
            Meets::Ends(false, true)
        };
    }
    Meets::Ends(hit_start, hit_end)
}

/// True if no item meets both ends of segment `s` (KiCad's
/// `TestTrackEndpointDangling`).
fn segment_dangling(meets: impl IntoIterator<Item = Meets>) -> bool {
    let (mut start, mut end) = (false, false);
    for meet in meets {
        match meet {
            Meets::Both => return false,
            Meets::Ends(s, e) => {
                start |= s;
                end |= e;
            }
        }
        if start && end {
            return false;
        }
    }
    true
}

/// True if a segment of trace `id` is dangling as KiCad's
/// `TestTrackEndpointDangling` counts it (module docs).
pub(crate) fn track_dangling(board: &BasicBoard, id: ItemId) -> bool {
    let Some(item) = board.item(id) else {
        return false;
    };
    let Some(trace) = item.as_trace() else {
        return false;
    };
    let layer = trace.layer;
    let own = segments(board, item);
    let others: Vec<&Item> = board
        .overlapping_objects(&TileShape::IntBox(item.bounding_box()), layer)
        .into_iter()
        .filter(|other| *other != id)
        .map(|other| board.item_ref(other))
        .filter(|other| other.shares_net(item))
        .collect();
    let other_segments: Vec<Segment> = others
        .iter()
        .flat_map(|other| segments(board, other))
        .collect();
    let pours: Vec<_> = board
        .dropped_pours()
        .iter()
        .filter(|pour| pour.layer == layer && item.contains_net(pour.net_number))
        .collect();
    own.iter().enumerate().any(|(k, s)| {
        let siblings = own
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != k)
            .map(|(_, o)| segment_meets(o, s));
        let tracks = other_segments.iter().map(|o| segment_meets(o, s));
        let items = others.iter().filter(|o| !o.is_trace()).map(|other| {
            if let ItemKind::Area(area) = &other.kind {
                if !area.is_conduction_area() {
                    return Meets::Ends(false, false);
                }
                let pieces = stub_contacts::pieces(board, other);
                return zone_meets(
                    |p| {
                        pieces
                            .iter()
                            .any(|(l, shape)| *l == layer && shape.contains_float(p))
                    },
                    s,
                );
            }
            match (Copper::of(other, layer), other.drill()) {
                (Some(copper), Some(drill)) => pad_meets(&copper, drill.center.to_float(), s),
                _ => Meets::Ends(false, false),
            }
        });
        let poured = pours
            .iter()
            .map(|pour| zone_meets(|p| pour.area.contains_float(p), s));
        segment_dangling(siblings.chain(tracks).chain(items).chain(poured))
    })
}

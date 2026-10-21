//! Q20.1: the board's manufacturing rules, checked on the copper of a board.
//!
//! Not Java. [`check_board_rules`] reports every place where a trace or via
//! breaks a rule the router knows (decided 2026-10-20: routing never breaks
//! the board's manufacturing rules), each marked [`Origin::Routed`] or
//! [`Origin::Input`]:
//!
//! | [`BoardRule`] | what breaks it |
//! |---|---|
//! | `clearance` | a trace or via closer to copper of another net than the clearance matrix allows ([`BasicBoard::clearance_violations`]) |
//! | `copper_edge_clearance` | a trace or via closer to the board outline than its clearance class (`board_edge`, `router.copper_to_edge_clearance_um`) |
//! | `keepout` | a trace or via in a keepout (the plugin's edge and pad keepouts, a DSN keepout) |
//! | `track_width` | a trace narrower than the board's minimum track width ([`board_min_trace_half_width`](crate::autoroute::control::board_min_trace_half_width)) |
//! | `via_diameter`, `drill_out_of_range`, `annular_width` | a via below `router.board_minimums.via_*` (the measure of [`AutorouteControl::via_meets_minimums`](crate::autoroute::control::AutorouteControl::via_meets_minimums)) |
//! | `hole_clearance` | copper of another net closer to a drill hole (a via's, a through-hole pin's) than `router.hole_clearance_um` |
//! | `hole_to_hole` | a via's drill hole closer to another drill (a via's, a through-hole pin's), of any net, than `router.board_minimums.hole_to_hole_um` (Q22.11, [`crate::board::hole_spacing`]) |
//! | `solder_mask_bridge` | a trace or via of another net inside a pad's mask opening on an outer layer (a listed pad enlarged by its expansion, [`BoardRuleLimits::mask_openings`]) |
//! | `via_in_pad` | a via whose copper overlaps an SMD pad on a common layer (`freerouting-dev`'s gate metric) |
//! | `track_dangling`, `via_dangling` | a trace end that meets no copper of its net, a via that meets its net on at most one layer (Q16's [`stub_contacts`](crate::quality::stub_contacts)) |
//!
//! Every rule is a pair or a single item that includes a trace or via:
//! pin-to-pin and pin-to-outline breaches belong to the footprints, never
//! to routing, and are not reported. The rule names are KiCad's DRC types
//! where KiCad has one, so a KiCad report compares by name.
//!
//! **The router's own model.** The check measures what the router models:
//! its clearance matrix, outline class, keepouts, hole clearance and drill
//! radii (a pin's drill is its padstack's, estimated from the pad when the
//! name gives none, as the router's search tree does). An edge the DSN does
//! not carry (a Margin-layer line, an Edge.Cuts segment inside the outline)
//! is no keepout unless the export carries it (Q18.5: as a keepout, so it
//! is reported as `keepout`), and a pad's mask opening is known only for the
//! pads `router.solder_mask_expansions` lists (Q18.7), each with its own
//! front and back expansion ([`BoardRuleLimits::mask_openings`],
//! [`BoardRuleLimits::with_mask_expansions`]); none listed skips the rule.
//!
//! **Tolerance.** Clearance and hole clearance are measured with no
//! shortfall tolerance ([`CHECK_TOLERANCE_UM`]), not the DRC's 1 µm
//! `router.clearance_tolerance_um` (Q22: KiCad reports items 0.5-0.8 µm
//! short).
//!
//! **Origin.** A breach is [`Origin::Input`] when every trace and via in it
//! is input copper ([`InputCopper`]): an item on the board as loaded, or a
//! piece a routed connection split off one (same nets, layer and width, its
//! ends on the input trace). A breach is routed only if at least one of its
//! items is copper the router placed; only those items are ever removed.
//!
//! **Order.** The result is sorted by rule, then by item ids; the items are
//! checked on the analysis threads with an ordered merge, so the result does
//! not depend on the thread count.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{FloatPoint, Shape, TileShape};

use super::clearance::parallel_map;
use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::job::solder_mask::{PinExpansion, settings_pin_expansions};
use crate::quality::stub_contacts;
use crate::settings::RouterSettings;
use crate::settings::board::BoardRouterSettings;

/// The shortfall the check lets pass, in micrometres: none. The DRC's
/// `router.clearance_tolerance_um` (1 µm by default) would pass KiCad
/// clearance items 0.5-0.8 µm short (Q22.1), so the check measures clearance
/// and hole clearance without it.
pub const CHECK_TOLERANCE_UM: f64 = 0.0;

/// A rule of the board's that copper can break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BoardRule {
    /// Clearance to copper of another net.
    Clearance,
    /// Clearance to the board outline.
    EdgeClearance,
    /// Copper inside a keepout.
    Keepout,
    /// A trace under the minimum track width.
    TrackWidth,
    /// A via under the minimum diameter.
    ViaDiameter,
    /// A via drill under the minimum drill.
    ViaDrill,
    /// A via annular ring under the minimum.
    AnnularRing,
    /// Copper of another net too close to a drill hole.
    HoleClearance,
    /// A via's drill hole too close to another drill hole, of any net.
    HoleToHole,
    /// Copper of another net inside a pad's mask opening.
    MaskBridge,
    /// A via on an SMD pad.
    ViaInPad,
    /// A trace end that meets nothing of its net.
    TrackDangling,
    /// A via that meets its net on at most one layer.
    ViaDangling,
}

impl BoardRule {
    /// Every rule, in report order.
    pub const ALL: [BoardRule; 13] = [
        BoardRule::Clearance,
        BoardRule::EdgeClearance,
        BoardRule::Keepout,
        BoardRule::TrackWidth,
        BoardRule::ViaDiameter,
        BoardRule::ViaDrill,
        BoardRule::AnnularRing,
        BoardRule::HoleClearance,
        BoardRule::HoleToHole,
        BoardRule::MaskBridge,
        BoardRule::ViaInPad,
        BoardRule::TrackDangling,
        BoardRule::ViaDangling,
    ];

    /// The rule's name: KiCad's DRC type where KiCad has one.
    pub fn name(self) -> &'static str {
        match self {
            BoardRule::Clearance => "clearance",
            BoardRule::EdgeClearance => "copper_edge_clearance",
            BoardRule::Keepout => "keepout",
            BoardRule::TrackWidth => "track_width",
            BoardRule::ViaDiameter => "via_diameter",
            BoardRule::ViaDrill => "drill_out_of_range",
            BoardRule::AnnularRing => "annular_width",
            BoardRule::HoleClearance => "hole_clearance",
            BoardRule::HoleToHole => "hole_to_hole",
            BoardRule::MaskBridge => "solder_mask_bridge",
            BoardRule::ViaInPad => "via_in_pad",
            BoardRule::TrackDangling => "track_dangling",
            BoardRule::ViaDangling => "via_dangling",
        }
    }
}

/// Whose copper breaks a rule.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Origin {
    /// Copper the router made.
    Routed,
    /// Copper of the board as loaded.
    Input,
}

/// One breach of a board rule.
#[derive(Clone, Debug, PartialEq)]
pub struct BoardRuleViolation {
    /// The rule.
    pub rule: BoardRule,
    /// The items, the trace or via checked first, in id order after it.
    pub items: Vec<ItemId>,
    /// The layer, or -1 for a rule of the whole item (a via's size).
    pub layer: i32,
    /// Where, in board coordinates.
    pub location: FloatPoint,
    /// The value the rule requires, in board units.
    pub required: f64,
    /// The value found, in board units.
    pub actual: f64,
    /// Whose copper it is.
    pub origin: Origin,
    /// The traces and vias of the breach that the router made: what a repair
    /// may remove. Empty for an input breach.
    pub routed_items: Vec<ItemId>,
}

/// The limits the check holds copper to, in board units. The clearances
/// come from the board's own rules (its clearance matrix and
/// [`BoardRules::hole_clearance`](crate::rules::BoardRules::hole_clearance)).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BoardRuleLimits {
    /// Half the minimum track width (0: no minimum).
    pub min_trace_half_width: i32,
    /// The minimum via diameter (0: none).
    pub min_via_diameter: f64,
    /// The minimum via drill (0: none).
    pub min_via_drill: f64,
    /// The minimum via annular ring (0: none).
    pub min_via_annular: f64,
    /// The pads with a solder-mask aperture and their expansions per outer
    /// side (Q18.7: `router.solder_mask_expansions`); none skips the mask
    /// rule. [`BoardRuleLimits::with_mask_expansions`] fills it.
    pub mask_openings: Vec<PinExpansion>,
}

impl BoardRuleLimits {
    /// The limits the router routes under: the board minimums of
    /// `settings` (Q18) as [`AutorouteControl`](crate::autoroute::control::AutorouteControl)
    /// floors them.
    pub fn of(board: &BasicBoard, settings: &BoardRouterSettings) -> BoardRuleLimits {
        let minimums = settings.board_minimums();
        let units = |um: f64| crate::autoroute::control::um_to_units(board, um);
        BoardRuleLimits {
            min_trace_half_width: crate::autoroute::control::board_min_trace_half_width(
                board, settings,
            ),
            min_via_diameter: units(minimums.via_diameter_um),
            min_via_drill: units(minimums.via_drill_um),
            min_via_annular: units(minimums.via_annular_um),
            mask_openings: Vec::new(),
        }
    }

    /// These limits with the mask openings of the pads `settings` list
    /// (`router.solder_mask_expansions`, the ones
    /// [`apply_solder_mask_expansions`](crate::job::solder_mask::apply_solder_mask_expansions)
    /// gave a clearance class of their own).
    pub fn with_mask_expansions(mut self, board: &BasicBoard, settings: &RouterSettings) -> Self {
        self.mask_openings = settings_pin_expansions(board, settings);
        self
    }
}

/// A trace of the board as loaded.
#[derive(Clone, Debug)]
struct InputTrace {
    nets: Vec<i32>,
    layer: i32,
    half_width: i32,
    polyline: freerouting_geometry::Polyline,
}

/// The traces and vias of a board as loaded: what [`check_board_rules`]
/// marks [`Origin::Input`] and what a repair never removes.
#[derive(Clone, Debug, Default)]
pub struct InputCopper {
    ids: BTreeSet<ItemId>,
    traces: Vec<InputTrace>,
    /// The vias: centre, padstack and nets.
    vias: BTreeSet<(i32, i32, usize, Vec<i32>)>,
}

impl InputCopper {
    /// The traces and vias of `board`, taken before routing starts.
    pub fn of(board: &BasicBoard) -> InputCopper {
        let mut copper = InputCopper::default();
        for item in board.items() {
            if !(item.is_trace() || item.is_via()) {
                continue;
            }
            copper.ids.insert(item.id());
            if let Some(key) = via_key(item) {
                copper.vias.insert(key);
            }
            if let Some(trace) = item.as_trace() {
                copper.traces.push(InputTrace {
                    nets: item.net_numbers().to_vec(),
                    layer: trace.layer,
                    half_width: trace.half_width,
                    polyline: trace.polyline.clone(),
                });
            }
        }
        copper
    }

    /// No input copper: everything on the board counts as routed.
    pub fn none() -> InputCopper {
        InputCopper::default()
    }

    /// The ids of the input traces and vias.
    pub fn ids(&self) -> &BTreeSet<ItemId> {
        &self.ids
    }

    /// True if `item` is input copper: an input item, a via with the centre,
    /// padstack and nets of an input via, or a trace with the nets, layer
    /// and width of an input trace whose two ends lie on it (a piece split
    /// off it, or the input's wiring read back from a session).
    pub fn contains(&self, item: &Item) -> bool {
        if self.ids.contains(&item.id()) {
            return true;
        }
        if let Some(key) = via_key(item) {
            return self.vias.contains(&key);
        }
        let Some(trace) = item.as_trace() else {
            return false;
        };
        let ends = [
            trace.first_corner().to_float(),
            trace.last_corner().to_float(),
        ];
        self.traces.iter().any(|input| {
            input.layer == trace.layer
                && input.half_width == trace.half_width
                && input.nets == item.net_numbers()
                && ends.iter().all(|end| input.polyline.distance(*end) <= 1.0)
        })
    }
}

/// What makes two vias the same: centre, padstack and nets.
fn via_key(item: &Item) -> Option<(i32, i32, usize, Vec<i32>)> {
    let ItemKind::Via(via) = &item.kind else {
        return None;
    };
    let centre = via.drill.center;
    Some((
        centre.x,
        centre.y,
        via.drill.padstack.0,
        item.net_numbers().to_vec(),
    ))
}

/// The breaches of the board's rules by the traces and vias of `board`
/// (see the module docs), sorted by rule, then items.
pub fn check_board_rules(
    board: &BasicBoard,
    limits: &BoardRuleLimits,
    input: &InputCopper,
) -> Vec<BoardRuleViolation> {
    let copper: Vec<ItemId> = board
        .items()
        .filter(|item| item.is_trace() || item.is_via())
        .map(Item::id)
        .collect();
    let reach = hole_reach(board);
    let per_item = parallel_map(&copper, |id| item_violations(board, *id, limits, reach));
    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for raw in per_item.into_iter().flatten() {
        let mut key_items = raw.items.clone();
        key_items.sort_unstable();
        if !seen.insert((raw.rule, key_items, raw.layer)) {
            continue;
        }
        result.push(finish(board, raw, input));
    }
    result.extend(mask_bridges(board, limits, input));
    result.sort_by(|a, b| {
        (a.rule, &a.items, a.layer)
            .cmp(&(b.rule, &b.items, b.layer))
            .then(a.actual.total_cmp(&b.actual))
    });
    result
}

/// A breach before its origin is known.
struct Raw {
    rule: BoardRule,
    items: Vec<ItemId>,
    layer: i32,
    location: FloatPoint,
    required: f64,
    actual: f64,
}

fn finish(board: &BasicBoard, raw: Raw, input: &InputCopper) -> BoardRuleViolation {
    let routed_items: Vec<ItemId> = raw
        .items
        .iter()
        .copied()
        .filter(|id| {
            board
                .item(*id)
                .is_some_and(|item| (item.is_trace() || item.is_via()) && !input.contains(item))
        })
        .collect();
    let origin = if routed_items.is_empty() {
        Origin::Input
    } else {
        Origin::Routed
    };
    BoardRuleViolation {
        rule: raw.rule,
        items: raw.items,
        layer: raw.layer,
        location: raw.location,
        required: raw.required,
        actual: raw.actual,
        origin,
        routed_items,
    }
}

/// The breaches of trace or via `id` (pairs found from either side are
/// merged by the caller).
fn item_violations(
    board: &BasicBoard,
    id: ItemId,
    limits: &BoardRuleLimits,
    reach: i32,
) -> Vec<Raw> {
    let Some(item) = board.item(id) else {
        return Vec::new();
    };
    let mut out = clearance(board, item);
    out.extend(hole_clearance(board, item, reach));
    match &item.kind {
        ItemKind::Trace(trace) => {
            if trace.half_width < limits.min_trace_half_width {
                out.push(Raw {
                    rule: BoardRule::TrackWidth,
                    items: vec![id],
                    layer: trace.layer,
                    location: trace.first_corner().to_float(),
                    required: 2.0 * f64::from(limits.min_trace_half_width),
                    actual: 2.0 * f64::from(trace.half_width),
                });
            }
            for end in [trace.first_corner(), trace.last_corner()] {
                if !end_on_copper(board, item, &end) {
                    out.push(Raw {
                        rule: BoardRule::TrackDangling,
                        items: vec![id],
                        layer: trace.layer,
                        location: end.to_float(),
                        required: 0.0,
                        actual: 0.0,
                    });
                    break;
                }
            }
        }
        ItemKind::Via(via) => {
            out.extend(via_size(board, item, limits));
            out.extend(hole_to_hole(board, item));
            out.extend(via_in_pad(board, item));
            if stub_contacts::via_contact_layers(board, item).len() <= 1 {
                out.push(Raw {
                    rule: BoardRule::ViaDangling,
                    items: vec![id],
                    layer: -1,
                    location: via.drill.center.to_float(),
                    required: 0.0,
                    actual: 0.0,
                });
            }
        }
        _ => {}
    }
    out
}

/// True if the end point `end` of trace `item` lies on copper of its net on
/// its layer (KiCad connects a track end that way: its anchor inside the
/// other item, not its round cap touching it).
fn end_on_copper(board: &BasicBoard, item: &Item, end: &freerouting_geometry::Point) -> bool {
    let Some(trace) = item.as_trace() else {
        return true;
    };
    let region = TileShape::get_instance_point(end);
    board
        .overlapping_objects(&TileShape::IntBox(region), trace.layer)
        .into_iter()
        .filter(|id| *id != item.id())
        .any(|id| {
            let other = board.item_ref(id);
            other.shares_net(item)
                && stub_contacts::pieces(board, other)
                    .iter()
                    .any(|(l, shape)| *l == trace.layer && shape.contains(end))
        })
}

/// The pair's items, `first` first and the rest in id order.
fn pair(first: ItemId, other: ItemId) -> Vec<ItemId> {
    vec![first, other]
}

fn clearance(board: &BasicBoard, item: &Item) -> Vec<Raw> {
    let holes = board.rules.hole_clearance() > 0;
    board
        .clearance_violations_within(item.id(), CHECK_TOLERANCE_UM)
        .into_iter()
        .filter_map(|v| {
            let other = board.item(v.second_item)?;
            // With a hole clearance the search tree inflates drill items
            // (Java `drillHoleClearanceDelta`), and the clearance check
            // measures the inflated shape; the copper itself is measured
            // here, the hole by `hole_clearance`.
            if holes
                && (item.is_drill_item() || other.is_drill_item())
                && !copper_closer_than(board, item, other, v.layer, v.expected_clearance)
            {
                return None;
            }
            let rule = match &other.kind {
                ItemKind::BoardOutline(_) => BoardRule::EdgeClearance,
                ItemKind::Area(area) if !area.is_conduction_area() => BoardRule::Keepout,
                _ => BoardRule::Clearance,
            };
            // A pair of two traces or vias is found from both sides; the
            // lower id names it first.
            let (first, second) = if (other.is_trace() || other.is_via()) && other.id() < item.id()
            {
                (other.id(), item.id())
            } else {
                (item.id(), other.id())
            };
            Some(Raw {
                rule,
                items: pair(first, second),
                layer: v.layer,
                location: v.location(),
                required: v.expected_clearance,
                actual: v.actual_clearance,
            })
        })
        .collect()
}

/// True if the copper of `a` and `b` on `layer` comes closer than
/// `clearance` (less one board unit): `a`'s pieces enlarged by it meet `b`'s.
fn copper_closer_than(board: &BasicBoard, a: &Item, b: &Item, layer: i32, clearance: f64) -> bool {
    let offset = (clearance - 1.0).max(0.0);
    let other: Vec<Shape> = stub_contacts::pieces(board, b)
        .into_iter()
        .filter(|(l, _)| *l == layer)
        .map(|(_, s)| s)
        .collect();
    stub_contacts::pieces(board, a)
        .into_iter()
        .filter(|(l, _)| *l == layer)
        .any(|(_, shape)| {
            let grown = if offset > 0.0 {
                shape.enlarge(offset)
            } else {
                Some(shape)
            };
            grown.is_some_and(|g| other.iter().any(|o| g.intersects(o)))
        })
}

/// The drill radius of a drill item, or `None` for an SMD pad or no
/// padstack.
fn drill_radius(board: &BasicBoard, item: &Item) -> Option<f64> {
    let drill = item.drill()?;
    if item.is_pin() && drill.first_layer == drill.last_layer {
        return None;
    }
    let radius = board
        .library
        .padstacks
        .get(drill.padstack)?
        .get_drill_radius();
    (radius > 0.0).then_some(radius)
}

/// Hole clearance of `item` (a trace or via): its copper against the drill
/// holes of other nets' vias and pins, and, for a via, its own hole against
/// other nets' traces and vias.
fn hole_clearance(board: &BasicBoard, item: &Item, reach: i32) -> Vec<Raw> {
    let hole_clearance = f64::from(board.rules.hole_clearance());
    if hole_clearance <= 0.0 {
        return Vec::new();
    }
    let tolerance = tolerance_units(board);
    let mut found: BTreeMap<ItemId, Raw> = BTreeMap::new();
    let pieces = stub_contacts::pieces(board, item);
    for (layer, shape) in &pieces {
        let region = TileShape::IntBox(shape.bounding_box().offset(f64::from(reach)));
        for other_id in board.overlapping_objects(&region, *layer) {
            if other_id == item.id() {
                continue;
            }
            let other = board.item_ref(other_id);
            if other.shares_net(item) || !(other.is_pin() || other.is_via() || other.is_trace()) {
                continue;
            }
            // The other item's hole against this copper.
            if let Some(radius) = drill_radius(board, other) {
                let centre = other.drill().expect("a drill item").center.to_float();
                let gap = shape.distance(centre) - radius;
                note_hole(
                    &mut found,
                    item,
                    other,
                    centre,
                    gap,
                    hole_clearance,
                    tolerance,
                );
            }
            // This via's hole against the other item's copper (pins
            // included: a pin is never the checked item).
            if let Some(radius) = drill_radius(board, item) {
                let centre = item.drill().expect("a drill item").center.to_float();
                for (other_layer, other_shape) in stub_contacts::pieces(board, other) {
                    if other_layer != *layer {
                        continue;
                    }
                    let gap = other_shape.distance(centre) - radius;
                    note_hole(
                        &mut found,
                        item,
                        other,
                        centre,
                        gap,
                        hole_clearance,
                        tolerance,
                    );
                }
            }
        }
    }
    found.into_values().collect()
}

fn note_hole(
    found: &mut BTreeMap<ItemId, Raw>,
    item: &Item,
    other: &Item,
    centre: FloatPoint,
    gap: f64,
    required: f64,
    tolerance: f64,
) {
    if gap >= required - tolerance {
        return;
    }
    let gap = gap.max(0.0);
    let (first, second) = if (other.is_trace() || other.is_via()) && other.id() < item.id() {
        (other.id(), item.id())
    } else {
        (item.id(), other.id())
    };
    let entry = found.entry(other.id()).or_insert(Raw {
        rule: BoardRule::HoleClearance,
        items: pair(first, second),
        layer: -1,
        location: centre,
        required,
        actual: gap,
    });
    if gap < entry.actual {
        entry.actual = gap;
        entry.location = centre;
    }
}

/// Q22.11: the drills, of any net, that via `via`'s hole comes closer to
/// than the board's hole-to-hole distance (none without one).
fn hole_to_hole(board: &BasicBoard, via: &Item) -> Vec<Raw> {
    let required = f64::from(board.rules.hole_to_hole());
    let (Some(radius), Some(drill)) = (board.hole_radius(via), via.drill()) else {
        return Vec::new();
    };
    if required <= 0.0 {
        return Vec::new();
    }
    let centre = drill.center.to_float();
    board
        .hole_to_hole_breaches(centre, radius, &[via.id()])
        .into_iter()
        .map(|(other_id, gap)| {
            let other = board.item_ref(other_id);
            let (first, second) = if other.is_via() && other_id < via.id() {
                (other_id, via.id())
            } else {
                (via.id(), other_id)
            };
            Raw {
                rule: BoardRule::HoleToHole,
                items: pair(first, second),
                layer: -1,
                location: centre,
                required,
                actual: gap.max(0.0),
            }
        })
        .collect()
}

/// How far a hole query reaches around copper: the hole clearance plus the
/// largest drill radius of the board's vias and pins.
fn hole_reach(board: &BasicBoard) -> i32 {
    let hole_clearance = f64::from(board.rules.hole_clearance());
    if hole_clearance <= 0.0 {
        return 0;
    }
    let radius = board
        .items()
        .filter(|item| item.is_via() || item.is_pin())
        .filter_map(|item| drill_radius(board, item))
        .fold(0.0, f64::max);
    crate::geometry::jmath::d2i((hole_clearance + radius).ceil())
}

/// The check's shortfall tolerance in board units: [`CHECK_TOLERANCE_UM`].
fn tolerance_units(board: &BasicBoard) -> f64 {
    CHECK_TOLERANCE_UM / board.board_unit_to_um_factor()
}

fn via_size(board: &BasicBoard, item: &Item, limits: &BoardRuleLimits) -> Vec<Raw> {
    let Some(drill) = item.drill() else {
        return Vec::new();
    };
    let Some(padstack) = board.library.padstacks.get(drill.padstack) else {
        return Vec::new();
    };
    let hole = 2.0 * padstack.get_drill_radius();
    let diameter = crate::job::via_minimums::padstack_diameter(board, drill.padstack);
    let centre = drill.center.to_float();
    let raw = |rule, required, actual| Raw {
        rule,
        items: vec![item.id()],
        layer: -1,
        location: centre,
        required,
        actual,
    };
    let mut out = Vec::new();
    // A rounding unit of slack, as `AutorouteControl::via_meets_minimums`.
    if hole + 0.5 < limits.min_via_drill {
        out.push(raw(BoardRule::ViaDrill, limits.min_via_drill, hole));
    }
    if let Some(diameter) = diameter {
        if diameter + 0.5 < limits.min_via_diameter {
            out.push(raw(
                BoardRule::ViaDiameter,
                limits.min_via_diameter,
                diameter,
            ));
        }
        let annular = (diameter - hole) / 2.0;
        if 2.0 * annular + 0.5 < 2.0 * limits.min_via_annular {
            out.push(raw(BoardRule::AnnularRing, limits.min_via_annular, annular));
        }
    }
    out
}

/// A via whose copper overlaps an SMD pad (a pin on one layer), of any net,
/// on a layer both reach.
fn via_in_pad(board: &BasicBoard, via: &Item) -> Vec<Raw> {
    let Some(drill) = via.drill() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for layer in drill.first_layer..=drill.last_layer {
        let Some(via_shape) = drill.shape_on_layer(layer) else {
            continue;
        };
        let region = TileShape::IntBox(via_shape.bounding_box());
        for other_id in board.overlapping_objects(&region, layer) {
            let other = board.item_ref(other_id);
            let Some(pad) = other
                .drill()
                .filter(|d| other.is_pin() && d.first_layer == d.last_layer)
            else {
                continue;
            };
            if pad
                .shape_on_layer(layer)
                .is_some_and(|shape| shape.intersects(via_shape))
                && !out.iter().any(|r: &Raw| r.items[1] == other_id)
            {
                out.push(Raw {
                    rule: BoardRule::ViaInPad,
                    items: pair(via.id(), other_id),
                    layer,
                    location: drill.center.to_float(),
                    required: 0.0,
                    actual: 0.0,
                });
            }
        }
    }
    out
}

/// Traces and vias of another net inside the mask opening of a listed pad on
/// an outer layer: the pad's shape enlarged by its expansion on that side
/// (the top layer's front expansion, the bottom layer's back one).
fn mask_bridges(
    board: &BasicBoard,
    limits: &BoardRuleLimits,
    input: &InputCopper,
) -> Vec<BoardRuleViolation> {
    if limits.mask_openings.is_empty() {
        return Vec::new();
    }
    let last = board.layer_count() as i32 - 1;
    let per_pin = parallel_map(&limits.mask_openings, |opening| {
        let pin = board.item_ref(opening.pin);
        let mut out: Vec<Raw> = Vec::new();
        let Some(drill) = pin.drill() else {
            return out;
        };
        let sides: &[(i32, i32)] = if last == 0 {
            &[(0, opening.front)]
        } else {
            &[(0, opening.front), (last, opening.back)]
        };
        for &(layer, expansion) in sides {
            // Copper that only touches the opening is at the pad's mask
            // clearance, where the router puts it (its clearance class is
            // raised to the expansion): the opening is one board unit
            // smaller, so touching is no overlap.
            if expansion <= 1 {
                continue;
            }
            let Some(pad) = drill.shape_on_layer(layer) else {
                continue;
            };
            let Some(opening_shape) = pad.enlarge(f64::from(expansion - 1)) else {
                continue;
            };
            let region = TileShape::IntBox(opening_shape.bounding_box());
            for other_id in board.overlapping_objects(&region, layer) {
                let other = board.item_ref(other_id);
                if !(other.is_trace() || other.is_via()) || other.shares_net(pin) {
                    continue;
                }
                let hits = stub_contacts::pieces(board, other)
                    .iter()
                    .any(|(l, shape)| *l == layer && shape.intersects(&opening_shape));
                if hits
                    && !out
                        .iter()
                        .any(|r| r.items[0] == other_id && r.layer == layer)
                {
                    out.push(Raw {
                        rule: BoardRule::MaskBridge,
                        items: pair(other_id, opening.pin),
                        layer,
                        location: drill.center.to_float(),
                        required: f64::from(expansion),
                        actual: 0.0,
                    });
                }
            }
        }
        out
    });
    per_pin
        .into_iter()
        .flatten()
        .map(|raw| finish(board, raw, input))
        .collect()
}

/// Counts of breaches per rule and origin.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BoardRuleCounts {
    /// Per rule, the routed breaches.
    pub routed: BTreeMap<BoardRule, usize>,
    /// Per rule, the input breaches.
    pub input: BTreeMap<BoardRule, usize>,
}

impl BoardRuleCounts {
    /// The counts of `violations`.
    pub fn of(violations: &[BoardRuleViolation]) -> BoardRuleCounts {
        let mut counts = BoardRuleCounts::default();
        for v in violations {
            let map = match v.origin {
                Origin::Routed => &mut counts.routed,
                Origin::Input => &mut counts.input,
            };
            *map.entry(v.rule).or_default() += 1;
        }
        counts
    }

    /// The routed breaches.
    pub fn routed_total(&self) -> usize {
        self.routed.values().sum()
    }

    /// The input breaches.
    pub fn input_total(&self) -> usize {
        self.input.values().sum()
    }

    /// `rule n, rule n` for the rules of `map`, in report order.
    pub fn text(map: &BTreeMap<BoardRule, usize>) -> String {
        map.iter()
            .map(|(rule, n)| format!("{} {n}", rule.name()))
            .collect::<Vec<_>>()
            .join(", ")
    }
}

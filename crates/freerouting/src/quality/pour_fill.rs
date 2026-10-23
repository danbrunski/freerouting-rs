//! K15.23, K15.26: a pour's copper as the stub cleanup judges it.
//!
//! The router's contacts are Java's: a conduction area joins every trace of
//! its net that ends inside its outline (`ConductionArea.getNormalContacts`),
//! and Q16.4's cleanup contacts ([`stub_contacts`]) take the outline too
//! (opt-in, K15.31's `router.pour_fill_contacts` takes the connectivity's
//! from this fill, island by island: [`crate::board::pour_contacts`]). A
//! pour is no obstacle (Java's `Structure.insertPlanes`; `enhanced` leaves
//! `router.plane_obstacle_all_pours` unset), so other nets route across it,
//! and KiCad's refill keeps the fill clear of their copper and then removes
//! every part of it narrower than the zone's `min_thickness` (the filler's
//! deflate-inflate opening). A trace end inside the outline where the
//! refilled fill no longer reaches is KiCad's `track_dangling`, and the
//! cleanup kept it (Q16's known limit; the pcu board's `/CH6_NO` stub in
//! `EFUSE_NO_6`, K15.18).
//!
//! So on a board read from a `.kicad_pcb` the cleanup takes the copper of a
//! signal-layer conduction area that is one of the file's zones
//! ([`KicadReportRules::zones`](crate::drc::KicadReportRules)) from the
//! router's fill model, opened as KiCad opens it ([`PourFills::pieces`]):
//!
//! - **the fill model** is the area's detailed fill (outline minus the
//!   obstacles of other nets grown by the router's clearances,
//!   [`fill_islands`](crate::drc::fill_islands)), the router copper on the
//!   board included;
//! - **the opening**: the fill eroded by `r`, half the zone's
//!   `min_thickness` less KiCad's 1 µm epsilon
//!   ([`eroded_fill_islands`](crate::drc::eroded_fill_islands):
//!   each foreign obstacle grown by its clearance plus `r`, strips of `r`
//!   along the outline and its holes, the net's own pads filled over), then
//!   each trapezoid of the result, rounded outwards to whole units, grown
//!   back by `r` (`TileShape::enlarge`, mitred corners capped by the bounding
//!   octagon). The growth back is a little more generous than KiCad's round
//!   inflate, and the pads are filled over (KiCad's thermal reliefs are not
//!   modelled), so the cleanup keeps a trace KiCad might still call
//!   dangling, never the reverse, as Q16.4 asks;
//! - **the file's fill** is not used: it is the fill of the board as read,
//!   and the cleanup has something to judge only after the router has added
//!   copper, which makes the file's fill stale
//!   ([`KicadReportRules::fills_current`](crate::drc::KicadReportRules::fills_current)
//!   is then false);
//! - **power layers** keep the outline, as does a conduction area of a
//!   `.kicad_pcb` that no zone of the file matches.
//!
//! **A DSN input** (K15.26) carries no zone: every conduction area of a net
//! on a signal layer is a pour, and its copper is the same fill model. The
//! DSN gives neither the zone's minimum thickness nor its clearance (KiCad's
//! export writes the net class clearance, and the zone's own, often larger,
//! is lost), so the opening comes from
//! [`DsnPourRules`](crate::drc::DsnPourRules)
//! (`router.cleanup.pour_min_thickness_um` and `pour_clearance_um`, which
//! the KiCad plugin can pass as the smallest of the board's signal-layer
//! zones):
//!
//! - **unset** (both 0): the fill model itself, with the router's
//!   clearances and no opening. KiCad's zone clearance is never below the
//!   net class clearance and its opening only removes copper, so this fill
//!   covers KiCad's wherever the router's clearances are the board's: a
//!   trace end it misses lies where KiCad's fill does not reach either;
//! - **set**: each foreign obstacle is kept the clearance away
//!   ([`opened_fill_islands`](crate::drc::opened_fill_islands); K15.33: not
//!   the larger of it and the router's, since a `.kicad_dru` rule replaces
//!   the net class clearance in KiCad's filler) and the fill is opened by
//!   half the thickness less 1 µm, as above. Values no larger than every
//!   zone's keep the cleanup's rule: it may keep a trace KiCad calls
//!   dangling, never remove one KiCad counts connected.
//!
//! **Blobs (K15.33).** After the deflate KiCad's filler deletes every
//! polygon whose bounding box is smaller than the zone's `min_thickness` on
//! both axes, so a corner another net's track cuts off, wide enough for the
//! opening but not twice as wide, has no fill (the pcu board's `/CH5_NO` via
//! in EFUSE_NO_5). The model drops such an island of the eroded fill, with a
//! slack of a quarter of the radius on each side for its octagonal erosion.
//! On a DSN the blob test takes the net's own thickness
//! (`router.cleanup.pour_min_thickness_nets`, else `pour_min_thickness_um`);
//! the opening keeps the board's smallest, and an island goes only when it
//! holds a piece of the fill eroded by the net's radius and every such piece
//! is a blob, so the model never loses copper the board's smallest
//! thickness keeps but where KiCad deletes all of it.
//!
//! **Keepouts (K15.35).** On a DSN input the model takes the board's
//! keepouts as KiCad's filler does, not with the clearance Java's fill and
//! the floor keep from them ([`opened_fill_islands`](crate::drc::opened_fill_islands)'s
//! `kicad_keepouts`):
//!
//! - a `keepout` (a rule area that bans tracks and vias, in the structure or
//!   a footprint's image; KiCad's default rule area bans the pour too) and
//!   an NPTH without copper (a keepout the export already grew by the
//!   board's hole clearance) cut the fill at their outline, with no
//!   clearance (`ZONE_FILLER::buildCopperItemClearances`: "Keepouts use
//!   outline with no clearance");
//! - a fitted mask-aperture or thermal-spoke keepout (no copper) and a
//!   `via_keepout`, `wire_keepout` or `place_keepout` (the DSN does not say
//!   they ban the pour) are left out;
//! - a copper keepout (`copper_graphic`, `copper_text`, `net_tie`) keeps the
//!   clearance like other copper.
//!
//! On the pcu board's DSN route the model lay 0.127 mm short of KiCad's fill
//! along five footprint rule areas and an NPTH (6 pieces of 1.1-1.9 mm2), and
//! the opening closes a corridor up to 0.127 mm wider than KiCad's does. The
//! DSN does not say whether a `keepout` bans the pour (the plugin removes
//! the rule areas that ban only the pour or pads): one that lets the pour in
//! is the one place the model is narrower than KiCad's fill. The edge strips
//! the plugin writes (the board edge's clearance less the router's default)
//! are cut at their outline too, but since K15.40 the board outline keeps the
//! edge clearance (below), so they cut nothing more.
//!
//! **Keepouts on a `.kicad_pcb` (K15.38).** The reader writes the same
//! keepouts (a rule area as unnamed keepouts of its outline, one per copper
//! layer, by its tracks, vias and footprints flags; a copper-less round hole
//! as a footprint's keepout circle grown by the hole clearance), and also
//! says which rule areas ban copper pour (`Extras::rule_areas`, carried to
//! [`BoardRuleArea`](crate::drc::BoardRuleArea)s; a keepout is an area's
//! when their boxes agree within 1 µm on its layer). So, unlike the DSN:
//!
//! - the keepouts of a rule area that bans the pour are cut at their
//!   outline whatever their kind, and those of one that allows it are left
//!   out (KiCad's filler passes through it);
//! - an NPTH's circle is cut at its outline, as on a DSN;
//! - every keepout the reader names keeps the clearance, as in Java's fill:
//!   the copper ones; `edge_clearance` and `pad_clearance`, which the reader
//!   draws so that the router's clearance from them is KiCad's (K15.40 takes
//!   a pad-clearance circle as its pad, below); `solder_mask_shared`, under which
//!   the pads are cut back; and the fitted `solder_mask_pads`, `solder_mask`
//!   and `thermal_spoke`, which the DSN path leaves out as KiCad's filler
//!   does (left out on a `.kicad_pcb` too, they moved canary-pwr's
//!   `pour_fill_target` + `pour_fill_contacts` route from 26 to 30 KiCad
//!   unconnected, so K15.38 keeps them); so does an unnamed keepout of no
//!   rule area (a cutout of the board outline, which KiCad keeps the edge
//!   clearance from).
//!
//! A rule area that bans only the pour (or the pads and the pour) gives no
//! keepout, so the model still passes through it where KiCad's fill does
//! not. This model feeds the router's pour target (K15.25,
//! `router.pour_fill_target`) and K15.31's contacts (`router.pour_fill_contacts`)
//! too.
//! For those two (K15.42, [`opened_fill_core`]) the model keeps the zone's own
//! clearance (`(connect_pads (clearance C))`, [`BoardZone::fill_clearance`](crate::drc::BoardZone::fill_clearance))
//! from other nets' copper where it is above the rule's, as KiCad's filler
//! does, unless the `.kicad_dru` has a clearance rule (then KiCad's rule
//! decides and the field is 0); the cleanup's model keeps the rule's, as
//! before. With the rule's 0.254 mm, interf_u's GND fill joined two vias of a
//! F.Cu chain to U9 pad L7, where KiCad's fill (0.5 mm) left their islands with
//! no pad and removed them (`via_dangling`, K15.37's `pcb-on` arm; pinned by
//! `tests/k15_pour_pair_drc.rs::the_fill_model_keeps_the_zones_clearance_and_the_via_chain_joins_no_pin`).
//!
//! **Pad clearances and the board edge (K15.40), on both inputs.**
//!
//! - A pad-clearance circle (the KiCad plugin's `edge_and_pad_keepouts` and
//!   the reader's `pad_clearance`: a structure-level circle of the pad's own
//!   clearance less the default, round a pad whose clearance exceeds it) is
//!   taken as KiCad's filler takes the pad: left out round a pin of the
//!   pour's net (KiCad joins it; the model fills over the pin), and kept
//!   exactly the default clearance it was drawn less round a pad or an NPTH
//!   of no net or another net (KiCad knocks it out by its own clearance:
//!   CM5_MINIMA_3's netless 3.0 mm NPTH with `(clearance 1.7)` keeps the fill
//!   3.2005 mm from its centre). The circles are on the outer layers only;
//!   K15.45 keeps the same clearance on the layers that have none (the
//!   pin's or NPTH's outer-layer circle, `drc::zone_islands`'
//!   `unwritten_pad_clearance`), as KiCad's `knockoutPadClearance` does on
//!   every layer.
//! - K15.45, `.kicad_pcb` only: a pad or footprint `(clearance v)` below the
//!   net class's replaces it round that pad (KiCad's local override; the
//!   reader's `Extras::pad_clearances`, `KicadReportRules::pad_override_at`),
//!   so on CM5_MINIMA_3's Module302 connector KiCad's fill comes 0.1275 mm
//!   from the pads and so does the model, not 0.2016 mm. A DSN input loses
//!   the override (narrower there by 0.0225 mm, the allowed side).
//! - A zone of another net is cut by its outline where KiCad cuts by its
//!   fill (`knockoutZoneClearance`), K15.15's limit: the model is narrower
//!   there (decided 2026-10-22: kept). K15.45: the zone-join model
//!   ([`PourFills::zone_island_model`], not yet the island repair's) also
//!   cuts a zone of the pour's net with a higher priority (not a teardrop)
//!   out at its outline, as KiCad's `subtractHigherPriorityZones` does; the
//!   cleanup's model and the repair's do not.
//! - The board outline keeps the edge clearance KiCad's filler keeps from
//!   the Edge.Cuts line (`EDGE_CLEARANCE_CONSTRAINT`), not the floor: the
//!   router's `copper_to_edge_clearance_um` without Java's safety margin,
//!   less the outline lines' half width and 5 µm for KiCad's arc chords, and
//!   no more than the least of Board Setup's value and the `.kicad_dru`'s
//!   `edge_clearance` rules (KiCad takes a rule below Board Setup's value;
//!   the router's setting only rises to it). Where a rule covers some layers
//!   only, the model is wider than KiCad's fill on the others.
//!
//! K15.29: the islands of either path that KiCad's filler removes (no pad
//! reaches their copper group, `island_removal_mode` 0) are left out
//! ([`pour_islands`](crate::quality::pour_islands)): on a `.kicad_pcb` by
//! the zone's own mode, on a DSN input when `router.cleanup.pour_island_removal`
//! is on and the net is not in `pour_islands_kept_nets`. Before K15.29
//! neither path removed them.
//!
//! The fills are computed when first asked for and kept until an item
//! removal could change them ([`PourFills::forget_near`]): removing copper
//! of another net only grows a fill, so a fill is never judged smaller than
//! the board gives it. Nothing reads a hash order; the same board gives the
//! same pieces.
//!
//! **The maze search (K15.25).** Under `router.pour_fill_target` the search
//! reaches such a pour only in the core of its opened fill
//! ([`opened_fill_core`]: the eroded fill before the growth back), which it
//! computes per connection on the board that connection is routed on
//! ([`pour_target`](crate::autoroute::pour_target)). The core lies inside
//! the pieces, so the cleanup counts a search's end there as a contact.
//!
//! [`stub_contacts`]: crate::quality::stub_contacts

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use freerouting_geometry::{
    IntBox, IntPoint, Point, PolylineShapeOps, Shape, TileShape, TileShapeOps,
};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::structure::Unit;
use crate::drc::{
    CellCache, FillOperands, Island, Trapezoid, eroded_fill_islands, fill_operands_islands,
    fill_operands_islands_cached, opened_fill_operands,
};
use crate::quality::pour_islands::IslandCache;

/// One piece of copper: its layer and its exact shape.
pub type Piece = (i32, Shape);

/// KiCad's epsilon of the minimum-thickness opening, in µm
/// (`ZONE_FILLER::fillCopperZone`: deflate and inflate by half the minimum
/// thickness less 1 µm).
const OPENING_EPSILON_UM: f64 = 1.0;

/// The opened fills of a board's pours, computed on demand (module docs).
#[derive(Debug)]
pub struct PourFills {
    /// Per conduction area: its pieces, or `None` when it keeps its outline.
    cache: RefCell<BTreeMap<ItemId, Option<Rc<Vec<Piece>>>>>,
    /// K15.29: the islands of the pieces KiCad's filler keeps.
    islands: IslandCache,
    /// K15.45: the islands of [`zone_island_model`](Self::zone_island_model),
    /// for [`zone_joined_untouched_kept_islands`](Self::zone_joined_untouched_kept_islands).
    zone_islands: IslandCache,
}

impl Default for PourFills {
    fn default() -> PourFills {
        PourFills {
            cache: RefCell::default(),
            islands: IslandCache::default(),
            zone_islands: IslandCache::with_zone_clearance(),
        }
    }
}

impl PourFills {
    pub fn new() -> PourFills {
        PourFills::default()
    }

    /// The copper of conduction area `area` as the opened fill model without
    /// the islands KiCad's filler removes (K15.29), or `None` when it is
    /// judged by its outline (module docs).
    pub fn pieces(&self, board: &BasicBoard, area: &Item) -> Option<Rc<Vec<Piece>>> {
        let all = self.all_pieces(board, area)?;
        Some(self.islands.kept(board, self, area, all))
    }

    /// K15.29: the fill model whose islands decide which islands KiCad's
    /// filler removes: [`all_pieces`](Self::all_pieces) opened alike but with
    /// the router's clearances (no `pour_clearance_um` floor), or `None` when
    /// `area` is judged by its outline (`quality::pour_islands`).
    pub(crate) fn island_model(&self, board: &BasicBoard, area: &Item) -> Option<Rc<Vec<Piece>>> {
        let o = opening(board, area)?;
        if o.clearance_floor <= 0.0 {
            return self.all_pieces(board, area);
        }
        let router = Opening {
            clearance_floor: 0.0,
            ..o
        };
        Some(Rc::new(opened_fill(board, area, router)))
    }

    /// K15.42: the bounding boxes of the islands of `area`'s island model
    /// that a zone that keeps its islands keeps and that no other item of
    /// its net touches (`quality::pour_islands`).
    pub(crate) fn untouched_kept_islands(&self, board: &BasicBoard, area: &Item) -> Vec<IntBox> {
        self.islands.untouched_kept(board, self, area)
    }

    /// K15.45: [`untouched_kept_islands`](Self::untouched_kept_islands) of
    /// `area`'s [`zone_island_model`](Self::zone_island_model), without the
    /// islands another zone of the net joins to a pin. The island repair's
    /// islands since K15.46 (`quality::island_repair`).
    pub(crate) fn zone_joined_untouched_kept_islands(
        &self,
        board: &BasicBoard,
        area: &Item,
    ) -> Vec<IntBox> {
        self.zone_islands.untouched_kept(board, self, area)
    }

    /// K15.45: [`island_model`](Self::island_model) with each `.kicad_pcb`
    /// zone's own clearance from other nets' copper where it is above the
    /// router's (`BoardZone::fill_clearance`, as the pour pair's
    /// [`opened_fill_core`] keeps it), or `None` when `area` is judged by its
    /// outline. KiCad's filler keeps the zone's clearance, so where a route
    /// of another net passes nearer than it to the zone's edge, KiCad's fill
    /// and its joins to the other zones of the net stop there; the router's
    /// clearances (the island model) keep a channel open that joins zones
    /// KiCad does not (moteus's V+ zone of priority 34, cut off from V+'s
    /// pad by a `+5V` track 0.444 mm from the lower zone's edge, against its
    /// 0.5 mm clearance). Its blob test has no slack: an island it keeps is
    /// one KiCad keeps, so a repair judging it would not re-route round a
    /// blob KiCad deletes (moteus's +3V priority-41 strip on F.Cu, 0.46 x
    /// 0.42 mm, 0.21 mm deflated against the zone's 0.25 mm minimum
    /// thickness). Not cached here: `zone_islands` holds the islands.
    pub(crate) fn zone_island_model(
        &self,
        board: &BasicBoard,
        area: &Item,
    ) -> Option<Rc<Vec<Piece>>> {
        let zone_clearance = zone_of(board, area).map_or(0.0, |zone| zone.fill_clearance);
        if zone_clearance <= 0.0 {
            return self.island_model(board, area);
        }
        let o = opening(board, area)?;
        let zone = Opening {
            clearance_floor: 0.0,
            clearance_min: zone_clearance,
            blob_slack: 0.0,
            same_net_zones: true,
            ..o
        };
        Some(Rc::new(opened_fill(board, area, zone)))
    }

    /// The opened fill model of conduction area `area` with all its islands,
    /// or `None` when it is judged by its outline.
    pub(crate) fn all_pieces(&self, board: &BasicBoard, area: &Item) -> Option<Rc<Vec<Piece>>> {
        if let Some(known) = self.cache.borrow().get(&area.id()) {
            return known.clone();
        }
        let pieces = opening(board, area).map(|o| Rc::new(opened_fill(board, area, o)));
        self.cache.borrow_mut().insert(area.id(), pieces.clone());
        pieces
    }

    /// Forgets the fills that removing copper on layers `first..=last`
    /// within `region` may change: those of the areas on these layers whose
    /// bounding box, grown by their largest clearance and opening, meets it.
    /// K15.29: every island decision is forgotten too.
    pub fn forget_near(&self, board: &BasicBoard, first: i32, last: i32, region: IntBox) {
        self.islands.forget();
        self.zone_islands.forget();
        self.cache.borrow_mut().retain(|id, pieces| {
            let Some(area) = board.item(*id) else {
                return false;
            };
            if pieces.is_none() {
                return true;
            }
            let layer = area.first_layer();
            if layer < first || layer > last {
                return true;
            }
            let reach = reach(board, area);
            let b = area.bounding_box();
            let grown = IntBox::new(
                b.ll.x.saturating_sub(reach),
                b.ll.y.saturating_sub(reach),
                b.ur.x.saturating_add(reach),
                b.ur.y.saturating_add(reach),
            );
            !grown.intersects_box(&region)
        });
    }
}

/// How far from its bounding box another item's removal can change `area`'s
/// opened fill: the largest clearance of its class on its layer plus its
/// minimum thickness, and a margin of 2000 resolution units as the fill
/// model's search box has.
pub(crate) fn reach(board: &BasicBoard, area: &Item) -> i32 {
    let clearance = board
        .rules
        .clearance_matrix
        .max_value(area.clearance_class(), area.first_layer());
    let thickness = match zone_of(board, area) {
        // K15.42: the router's model keeps the zone's own clearance, if larger
        Some(zone) => zone.min_thickness + (zone.fill_clearance - f64::from(clearance)).max(0.0),
        None if !board.kicad_report_rules().kicad_pcb => {
            let rules = &board.kicad_report_rules().dsn_pours;
            let net = area.net_numbers().first().copied().unwrap_or(0);
            rules.min_thickness_of(net) + rules.clearance
        }
        None => 0.0,
    };
    let margin = 2000.0
        * board
            .communication
            .get_resolution(crate::board::structure::Unit::Um);
    let total = f64::from(clearance) + thickness + margin;
    if total.is_finite() && total < f64::from(i32::MAX) {
        total.ceil() as i32
    } else {
        i32::MAX
    }
}

/// The net and layer of conduction area `area` if it is a pour of a net on a
/// signal layer.
fn signal_pour(board: &BasicBoard, area: &Item) -> Option<(i32, i32)> {
    let ca = area.as_conduction_area()?;
    let net = *area.net_numbers().first()?;
    let layer = ca.layer();
    let is_signal = usize::try_from(layer)
        .ok()
        .and_then(|l| board.layer_structure.layers.get(l))
        .is_some_and(|l| l.is_signal);
    (net > 0 && is_signal).then_some((net, layer))
}

/// The file's zone of conduction area `area`, if `area` is on a signal layer
/// of a board read from a `.kicad_pcb`.
pub(crate) fn zone_of<'b>(board: &'b BasicBoard, area: &Item) -> Option<&'b crate::drc::BoardZone> {
    let (net, layer) = signal_pour(board, area)?;
    let index = crate::drc::file_zone_of(board, area, net, layer)?;
    board.kicad_report_rules().zones.get(index)
}

/// How a pour's fill model is opened, in board units: the erosion radius,
/// the clearance to other nets' copper (0: the router's), and (K15.33) the
/// zone's minimum thickness, below which KiCad deletes a blob (0: none),
/// with the radius KiCad deflates that zone by, at least `radius`.
/// K15.35: `kicad_keepouts` takes the keepouts as KiCad's filler does (a DSN
/// input; module docs).
#[derive(Clone, Copy, Debug)]
struct Opening {
    radius: f64,
    clearance_floor: f64,
    /// K15.42: the least clearance to other nets' copper, above the
    /// router's (0: none): the router's model of a `.kicad_pcb` zone
    /// ([`opened_fill_core`]).
    clearance_min: f64,
    min_thickness: f64,
    blob_radius: f64,
    /// K15.45: the share of `blob_radius` by which the blob test errs to
    /// the side of keeping copper ([`BLOB_SLACK`]; 0 for the zone-join
    /// model, [`PourFills::zone_island_model`]).
    blob_slack: f64,
    kicad_keepouts: bool,
    /// K15.45: same-net zones of higher priority are cut out at their
    /// outline (`drc::zone_islands`' `higher_same_net_zone`); the zone-join
    /// model's only ([`PourFills::zone_island_model`]).
    same_net_zones: bool,
}

/// The opening of `area`'s fill model, if its copper is its opened fill
/// (module docs): a zone of a `.kicad_pcb` by its own minimum thickness, a
/// signal-layer pour of a DSN input by [`DsnPourRules`](crate::drc::DsnPourRules)
/// (K15.33: its net's thickness).
fn opening(board: &BasicBoard, area: &Item) -> Option<Opening> {
    if board.kicad_report_rules().kicad_pcb {
        return kicad_pcb_opening(board, area);
    }
    let (net, _) = signal_pour(board, area)?;
    let rules = &board.kicad_report_rules().dsn_pours;
    let epsilon = Unit::scale(
        OPENING_EPSILON_UM * f64::from(board.communication.resolution.max(1)),
        Unit::Um,
        board.communication.unit,
    );
    let half = |thickness: f64| {
        if thickness > 0.0 {
            (thickness / 2.0 - epsilon).max(0.0)
        } else {
            0.0
        }
    };
    let radius = half(rules.min_thickness);
    let min_thickness = rules.min_thickness_of(net).max(0.0);
    Some(Opening {
        radius,
        clearance_floor: rules.clearance.max(0.0),
        clearance_min: 0.0,
        min_thickness,
        blob_radius: half(min_thickness).max(radius),
        blob_slack: BLOB_SLACK,
        kicad_keepouts: true,
        same_net_zones: false,
    })
}

/// The opening of a `.kicad_pcb` zone `area` with the router's clearances,
/// if its copper is its opened fill (module docs); radius 0 for a zone
/// without a minimum thickness.
fn kicad_pcb_opening(board: &BasicBoard, area: &Item) -> Option<Opening> {
    let zone = zone_of(board, area)?;
    let epsilon = board
        .communication
        .coordinate_transform
        .dsn_to_board(OPENING_EPSILON_UM);
    let min_thickness = zone.min_thickness.max(0.0);
    let radius = (min_thickness / 2.0 - epsilon).max(0.0);
    Some(Opening {
        radius,
        clearance_floor: 0.0,
        clearance_min: 0.0,
        min_thickness,
        blob_radius: radius,
        blob_slack: BLOB_SLACK,
        kicad_keepouts: true,
        same_net_zones: false,
    })
}

/// K15.33: the share of the radius by which the blob test errs to the side
/// of keeping copper ([`kicad_keeps_blob`]).
const BLOB_SLACK: f64 = 0.25;

/// K15.33: true unless KiCad's filler deletes `island`, a piece of the
/// fill eroded by `opening`'s blob radius. KiCad deletes every deflated
/// polygon whose bounding box is smaller than the zone's minimum thickness
/// on both axes (`ZONE_FILLER::fillCopperZone`: "a blob min-thickness by
/// min-thickness is not useful"), before the inflate. The model erodes with
/// octagons, up to 8.2 % of the clearance plus the radius more than a round
/// offset, so an island counts as `blob_slack * radius` larger on each side
/// ([`BLOB_SLACK`]): the model may keep a blob KiCad deletes, never the
/// reverse. K15.45: the zone-join model takes no slack (it may then miss a
/// blob KiCad keeps, never keep one KiCad deletes).
fn kicad_keeps_blob(island: &Island, opening: Opening) -> bool {
    let r = opening.blob_radius;
    if r <= 0.0 || opening.min_thickness <= 0.0 {
        return true;
    }
    let [x0, y0, x1, y1] = island.bounds;
    let size = (x1 - x0).max(y1 - y0);
    size + 2.0 * opening.blob_slack * r >= opening.min_thickness
}

/// The islands of `area`'s fill model eroded by `radius` with `opening`'s
/// clearance and (K15.35) keepouts.
fn eroded_islands(board: &BasicBoard, area: &Item, opening: Opening, radius: f64) -> Vec<Island> {
    if opening.clearance_floor > 0.0 || opening.kicad_keepouts {
        crate::drc::opened_fill_islands_with(
            board,
            area,
            radius,
            [opening.clearance_floor, opening.clearance_min],
            opening.kicad_keepouts,
            opening.same_net_zones,
        )
    } else if radius > 0.0 {
        eroded_fill_islands(board, area, radius)
    } else {
        crate::drc::fill_islands(board, area)
    }
}

/// K15.33: `islands` (the fill model eroded by `opening`'s radius) without
/// the ones KiCad's filler deletes as blobs ([`kicad_keeps_blob`]). When
/// the zone's own thickness is larger than the opening's (a DSN pour whose
/// net has its own `pour_min_thickness_nets` value), the fill is eroded
/// again by the zone's radius, and an island goes only when it holds a piece
/// of that erosion and every such piece is a blob: the model is never
/// narrower than with the board's smallest thickness, but where KiCad
/// deletes the whole of it.
fn without_blobs(
    board: &BasicBoard,
    area: &Item,
    opening: Opening,
    islands: Vec<Island>,
) -> Vec<Island> {
    if opening.blob_radius <= opening.radius {
        return islands
            .into_iter()
            .filter(|island| kicad_keeps_blob(island, opening))
            .collect();
    }
    let inner = eroded_islands(board, area, opening, opening.blob_radius);
    let mut holds = vec![false; islands.len()];
    let mut keeps = vec![false; islands.len()];
    for piece in &inner {
        let Some(t) = piece.trapezoids.first() else {
            continue;
        };
        let x = (t.x_left + t.x_right) / 2.0;
        let y = (t.left[0] + t.left[1] + t.right[0] + t.right[1]) / 4.0;
        let point = freerouting_geometry::FloatPoint::new(x, y);
        if let Some(i) = islands.iter().position(|island| island.contains(point)) {
            holds[i] = true;
            keeps[i] |= kicad_keeps_blob(piece, opening);
        }
    }
    islands
        .into_iter()
        .enumerate()
        .filter(|(i, _)| !holds[*i] || keeps[*i])
        .map(|(_, island)| island)
        .collect()
}

/// The pieces of `area`'s fill model opened by `opening` (module docs).
fn opened_fill(board: &BasicBoard, area: &Item, opening: Opening) -> Vec<Piece> {
    let layer = area.first_layer();
    let r = opening.radius;
    eroded_tiles(board, area, opening)
        .map(|tile| {
            let grown = if r > 0.0 { tile.enlarge(r) } else { tile };
            (layer, Shape::from(grown))
        })
        .collect()
}

/// The tiles of `area`'s fill model eroded by `opening`, before the growth
/// back.
fn eroded_tiles(
    board: &BasicBoard,
    area: &Item,
    opening: Opening,
) -> impl Iterator<Item = TileShape> {
    let islands = eroded_islands(board, area, opening, opening.radius);
    without_blobs(board, area, opening, islands)
        .into_iter()
        .flat_map(|island| island.trapezoids)
        .filter_map(|t| trapezoid_tile(&t))
}

/// K15.25: the core of a pour's opened fill ([`opened_fill_core`]).
#[derive(Clone, Debug)]
pub struct FillCore {
    /// The fill model eroded by the opening's radius, as tiles.
    pub tiles: Vec<TileShape>,
    /// The opening's radius in board units: the core grown by it is the
    /// opened fill ([`PourFills::pieces`]).
    pub radius: f64,
}

impl FillCore {
    /// True if a tile of the core holds `point`.
    pub fn holds(&self, point: &Point) -> bool {
        self.tiles.iter().any(|tile| tile.contains(point))
    }

    /// True if the opened fill (the core grown back by the radius, as
    /// [`PourFills::pieces`] grows it) holds `point`.
    pub fn reaches(&self, point: &Point) -> bool {
        let r = self.radius;
        self.tiles.iter().any(|tile| {
            if r > 0.0 {
                tile.enlarge(r).contains(point)
            } else {
                tile.contains(point)
            }
        })
    }
}

/// K15.25: the core of `area`'s opened fill on the board now, the fill
/// model eroded by the opening's radius `r` before the growth back (module
/// docs), or `None` when [`PourFills::pieces`] judges `area` by its
/// outline. Every point of the core lies at least `r` inside the opened
/// fill, so a trace end there is in KiCad's refilled fill even where the
/// growth back is more generous than KiCad's round inflate. Not cached:
/// the maze search asks for it once per connection, on the board that
/// connection is routed on.
pub fn opened_fill_core(board: &BasicBoard, area: &Item) -> Option<FillCore> {
    Some(opened_fill_core_inputs(board, area)?.core())
}

/// K15.41: what [`opened_fill_core`] reads from the board for conduction
/// area `area`: its opening and its fill model's operands. Equal inputs
/// give an equal core ([`CoreInputs::core`]), so the router's pour contacts
/// reuse an area's fill while an item change leaves them as they were
/// ([`crate::board::pour_contacts`]).
#[derive(Debug)]
pub(crate) struct CoreInputs {
    opening: Opening,
    operands: Option<FillOperands>,
}

impl PartialEq for CoreInputs {
    fn eq(&self, other: &CoreInputs) -> bool {
        let (a, b) = (self.opening, other.opening);
        a.radius.to_bits() == b.radius.to_bits()
            && a.clearance_floor.to_bits() == b.clearance_floor.to_bits()
            && a.clearance_min.to_bits() == b.clearance_min.to_bits()
            && a.min_thickness.to_bits() == b.min_thickness.to_bits()
            && a.blob_radius.to_bits() == b.blob_radius.to_bits()
            && a.blob_slack.to_bits() == b.blob_slack.to_bits()
            && a.kicad_keepouts == b.kicad_keepouts
            && a.same_net_zones == b.same_net_zones
            && self.operands == other.operands
    }
}

/// K15.41: the inputs of [`opened_fill_core`] of `area` on `board` now, or
/// `None` when it has none.
pub(crate) fn opened_fill_core_inputs(board: &BasicBoard, area: &Item) -> Option<CoreInputs> {
    // `kicad_pcb_opening` takes a zone of the file: a DSN input's pours have
    // none, so the search's target stays their outline (K15.26's fills are
    // the cleanup's only).
    let mut opening = kicad_pcb_opening(board, area)?;
    opening.clearance_min = zone_of(board, area).map_or(0.0, |zone| zone.fill_clearance);
    // A `.kicad_pcb` zone's opening keeps the keepouts as KiCad's filler
    // does and deletes its blobs at the opening's own radius, so its tiles
    // are the islands of one operand set ([`eroded_tiles`]).
    debug_assert!(opening.kicad_keepouts && opening.blob_radius <= opening.radius);
    Some(CoreInputs {
        opening,
        operands: opened_fill_operands(
            board,
            area,
            opening.radius,
            opening.clearance_floor,
            opening.clearance_min,
            opening.kicad_keepouts,
            opening.same_net_zones,
        ),
    })
}

impl CoreInputs {
    /// The core these inputs give: [`eroded_tiles`] of the opening, from
    /// the operands' islands.
    pub(crate) fn core(&self) -> FillCore {
        let islands = self
            .operands
            .as_ref()
            .map(fill_operands_islands)
            .unwrap_or_default();
        self.core_of(islands)
    }

    /// The trapezoids of the core's tiles in their order (each tile is
    /// [`trapezoid_tile`] of one, where it has one), reusing the cells of
    /// `previous` (an earlier call's, [`CellCache`]) where their input is
    /// the same; with this call's cells.
    pub(crate) fn trapezoids_cached(&self, previous: &CellCache) -> (Vec<Trapezoid>, CellCache) {
        let (islands, cells) = match &self.operands {
            Some(operands) => fill_operands_islands_cached(operands, previous),
            None => (Vec::new(), CellCache::default()),
        };
        (self.kept_trapezoids(islands), cells)
    }

    /// The opening's radius: the core grown by it is the opened fill.
    pub(crate) fn radius(&self) -> f64 {
        self.opening.radius
    }

    fn core_of(&self, islands: Vec<Island>) -> FillCore {
        FillCore {
            tiles: self
                .kept_trapezoids(islands)
                .iter()
                .filter_map(trapezoid_tile)
                .collect(),
            radius: self.opening.radius,
        }
    }

    /// The trapezoids of the islands KiCad keeps ([`kicad_keeps_blob`]).
    fn kept_trapezoids(&self, islands: Vec<Island>) -> Vec<Trapezoid> {
        let opening = self.opening;
        islands
            .into_iter()
            .filter(|island| kicad_keeps_blob(island, opening))
            .flat_map(|island| island.trapezoids)
            .collect()
    }
}

/// A trapezoid of the fill model as an integer convex tile, its corners
/// rounded outwards; `None` for one with no area at the grid.
pub fn trapezoid_tile(t: &Trapezoid) -> Option<TileShape> {
    let fits = |v: f64| v.is_finite() && v.abs() < f64::from(i32::MAX);
    let values = [
        t.x_left, t.x_right, t.left[0], t.left[1], t.right[0], t.right[1],
    ];
    if !values.iter().all(|v| fits(*v)) {
        return None;
    }
    let (xl, xr) = (t.x_left.floor() as i32, t.x_right.ceil() as i32);
    if xr <= xl {
        return None;
    }
    let corners = [
        IntPoint::new(xl, t.left[0].floor() as i32),
        IntPoint::new(xr, t.right[0].floor() as i32),
        IntPoint::new(xr, t.right[1].ceil() as i32),
        IntPoint::new(xl, t.left[1].ceil() as i32),
    ];
    let mut points: Vec<Point> = Vec::with_capacity(4);
    for c in corners {
        if points.last() != Some(&Point::Int(c)) {
            points.push(Point::Int(c));
        }
    }
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    if points.len() < 3 {
        return None;
    }
    let tile = TileShape::get_instance_from_points(&points);
    (!tile.is_empty()).then_some(tile)
}

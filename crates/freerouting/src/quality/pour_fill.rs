//! K15.23, K15.26: a pour's copper as the stub cleanup judges it.
//!
//! The router's contacts are Java's: a conduction area joins every trace of
//! its net that ends inside its outline (`ConductionArea.getNormalContacts`),
//! and Q16.4's cleanup contacts ([`stub_contacts`]) take the outline too. A
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
//! - **set**: each foreign obstacle is kept at least the clearance away
//!   ([`opened_fill_islands`](crate::drc::opened_fill_islands)) and the
//!   fill is opened by half the thickness less 1 µm, as above. Values no
//!   larger than every zone's keep the cleanup's rule: it may keep a trace
//!   KiCad calls dangling, never remove one KiCad counts connected.
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
use crate::drc::{Trapezoid, eroded_fill_islands, opened_fill_islands};
use crate::quality::pour_islands::IslandCache;

/// One piece of copper: its layer and its exact shape.
pub type Piece = (i32, Shape);

/// KiCad's epsilon of the minimum-thickness opening, in µm
/// (`ZONE_FILLER::fillCopperZone`: deflate and inflate by half the minimum
/// thickness less 1 µm).
const OPENING_EPSILON_UM: f64 = 1.0;

/// The opened fills of a board's pours, computed on demand (module docs).
#[derive(Debug, Default)]
pub struct PourFills {
    /// Per conduction area: its pieces, or `None` when it keeps its outline.
    cache: RefCell<BTreeMap<ItemId, Option<Rc<Vec<Piece>>>>>,
    /// K15.29: the islands of the pieces KiCad's filler keeps.
    islands: IslandCache,
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
fn reach(board: &BasicBoard, area: &Item) -> i32 {
    let clearance = board
        .rules
        .clearance_matrix
        .max_value(area.clearance_class(), area.first_layer());
    let thickness = match zone_of(board, area) {
        Some(zone) => zone.min_thickness,
        None if !board.kicad_report_rules().kicad_pcb => {
            let rules = &board.kicad_report_rules().dsn_pours;
            rules.min_thickness + rules.clearance
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

/// How a pour's fill model is opened, in board units: the erosion radius and
/// the least clearance to other nets' copper (0: the router's).
#[derive(Clone, Copy, Debug)]
struct Opening {
    radius: f64,
    clearance_floor: f64,
}

/// The opening of `area`'s fill model, if its copper is its opened fill
/// (module docs): a zone of a `.kicad_pcb` by its own minimum thickness, a
/// signal-layer pour of a DSN input by [`DsnPourRules`](crate::drc::DsnPourRules).
fn opening(board: &BasicBoard, area: &Item) -> Option<Opening> {
    if board.kicad_report_rules().kicad_pcb {
        let radius = opening_radius(board, area)?;
        return Some(Opening {
            radius,
            clearance_floor: 0.0,
        });
    }
    signal_pour(board, area)?;
    let rules = &board.kicad_report_rules().dsn_pours;
    let radius = if rules.min_thickness > 0.0 {
        let epsilon = Unit::scale(
            OPENING_EPSILON_UM * f64::from(board.communication.resolution.max(1)),
            Unit::Um,
            board.communication.unit,
        );
        (rules.min_thickness / 2.0 - epsilon).max(0.0)
    } else {
        0.0
    };
    Some(Opening {
        radius,
        clearance_floor: rules.clearance.max(0.0),
    })
}

/// The radius of `area`'s opening in board units, if its copper is its
/// opened fill (module docs); 0 for a zone without a minimum thickness.
fn opening_radius(board: &BasicBoard, area: &Item) -> Option<f64> {
    let zone = zone_of(board, area)?;
    let epsilon = board
        .communication
        .coordinate_transform
        .dsn_to_board(OPENING_EPSILON_UM);
    Some((zone.min_thickness / 2.0 - epsilon).max(0.0))
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
    let r = opening.radius;
    let islands = if opening.clearance_floor > 0.0 {
        opened_fill_islands(board, area, r, opening.clearance_floor)
    } else if r > 0.0 {
        eroded_fill_islands(board, area, r)
    } else {
        crate::drc::fill_islands(board, area)
    };
    islands
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
    // `opening_radius` takes a zone of the file: a DSN input's pours have
    // none, so the search's target stays their outline (K15.26's fills are
    // the cleanup's only).
    let radius = opening_radius(board, area)?;
    let opening = Opening {
        radius,
        clearance_floor: 0.0,
    };
    Some(FillCore {
        tiles: eroded_tiles(board, area, opening).collect(),
        radius,
    })
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

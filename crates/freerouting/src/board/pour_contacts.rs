//! K15.31: `router.pour_fill_contacts`, which contacts of a signal-layer
//! pour of a `.kicad_pcb` the router's connectivity counts.
//!
//! Java joins a conduction area to every pin and via of its net centred in
//! its outline and every trace of its net ending there
//! (`ConductionArea.getNormalContacts`,
//! [`BasicBoard::normal_contacts`]). A pour is no obstacle, so other nets
//! route across it, and KiCad's refill keeps its fill clear of their copper,
//! drops every part narrower than the zone's `min_thickness`, and joins
//! only what the fill that is left touches. On the pcu board's `.kicad_pcb`
//! route (K15.25, `router.pour_fill_target`) `VCC_3V3`'s tracks cut zone
//! EFUSE_NO_2 (`/CH2_NO`) in three; the router counted U4's pads 8 and 10,
//! whose tracks end in different parts, as joined through the outline,
//! and KiCad reported them unconnected (`tests/k15_pour_fill_contacts.rs`).
//!
//! Under `router.pour_fill_contacts = true` (opt-in: unset in both
//! profiles, and unset or `false` is Java's rule) the board's connectivity
//! ([`BasicBoard::connected_set`] and everything built on it: the routing
//! ledger's incomplete count, the DRC's incompletes, the connections the
//! batch router routes) takes such a pour's contacts from its opened fill,
//! island by island:
//!
//! - **the fill** is K15.23's opened fill model of the area on the board as
//!   it is now ([`opened_fill_core`]: the fill eroded by `r`, half the
//!   zone's `min_thickness` less 1 µm, and each core tile grown back by
//!   `r`, as [`PourFills::all_pieces`] grows it). Only an area that
//!   `PourFills` judges by its fill has one: a signal-layer conduction area
//!   that is a zone of the `.kicad_pcb`. Power layers, a DSN input and an
//!   area no zone matches keep Java's outline, and so does an area whose
//!   fill has no piece at all (then nothing tells where KiCad's copper is);
//! - **the islands** are the connected components of the pieces, two
//!   pieces joined where they meet ([`island_numbers`], K15.29's rule). All
//!   islands count, also those K15.29's island removal drops
//!   ([`PourFills::pieces`]): an island KiCad removes is one whose copper
//!   group reaches no pad, so counting it joins no pad to another, and its
//!   test is a search over the net's copper that would run at every board
//!   change;
//! - **a contact** of Java's (a pin or via centre, a trace end, inside the
//!   outline) belongs to the island whose piece holds that point, and to
//!   none where no piece holds it;
//! - **the area item** stands for one island, its *main* island: the first
//!   of the islands with a pin contact, else with any contact, else all,
//!   that has the largest area (the sum of its pieces'). The area's
//!   contacts are the items with a point in that island. The items of every
//!   other island are in contact with each other directly, so each island
//!   is its own connected group, and an item with a point in no island is
//!   joined to nothing through the pour. A trace with its ends in two
//!   islands joins them, as its copper does.
//!
//! [`BasicBoard::normal_contacts`] stays Java's, so the trace
//! normalization, the tails and the shove see the outline as before. A
//! connected set that stops at planes (`stop_at_plane`) passes no pour, and
//! so no island either. **The cycle rule** follows the islands, because the
//! insertion removes a trace both of whose ends a pour joins: a judged pour
//! joins a trace's two ends only if one island holds both
//! (`area_joins_trace_ends`), and `Trace.isCycle` asks whether the trace's
//! start reaches its end without it over the island-aware contacts
//! ([`BasicBoard::pour_cycle`]), where it can differ from Java's search (a
//! cycle of Java's, or a trace with an end in a judged pour's outline). On
//! `tests/k15_pour_fill_contacts.rs`' board Java's rule removed every trace
//! the search found between two islands, and the vias and the inner-layer
//! trace between them with it.
//!
//! **Keeping the fill current.** The islands of an area are computed when
//! the connectivity first asks for them and cached on the board (behind a
//! [`Mutex`], as the routing ledger, so the board stays `Sync`). Every
//! insertion, removal or change of an item that the ledger hears
//! ([`BasicBoard`]'s `note_*` bookkeeping), and every change of the search
//! trees (which the fill model reads), forgets what it may change for each
//! judged area on one of the item's layers whose outline tiles' boxes,
//! grown by its largest clearance, its minimum thickness and a 2000-unit
//! margin (`pour_fill::reach`, as `PourFills::forget_near` grows them), meet
//! the item's, and marks that area's net for the ledger's recount. Another
//! net's item forgets the fill and the contacts; the net's own item only
//! the contacts, because the opening fills the net's own drills over and
//! takes none of its traces (with a positive opening radius).
//! `PourFills`' own cache is the cleanup's and is not shared: it forgets
//! only at removals, which is right for a cleanup that only removes and
//! wrong for a router that adds. A fill model costs about 0.3 ms per pour
//! on the pcu board (K15.25) and about 25 ms on canary-pwr's larger pours,
//! about 2000 of them in its route (K15.31's note).
//!
//! **What a forgotten fill keeps (K15.41).** The routing ledger's recount
//! after each routed connection asks again for the fill that connection
//! forgot, so a route computes a big pour's fill about once per connection
//! (303 fills on complex_hierarchy). Each area keeps its last fill with
//! its inputs (`FillMemo`, forgotten only where everything is: a
//! conduction area's or the contact rule's change), and a fill is made
//! from what did not change:
//!
//! - equal inputs ([`CoreInputs`]: the opening and the fill model's
//!   operands, compared bit for bit) give the kept fill;
//! - the fill model's grid cells whose clipped edges, roles and slab
//!   bounds equal the kept cell's take its sweep ([`CellCache`]);
//! - a trapezoid equal to a kept one takes its tile, grown tile and shape,
//!   and two kept pieces their kept shape test (`PieceCache`);
//! - the islands of the pieces are found with a box tree over the pieces
//!   and one shape test per pair that is not in one island yet
//!   ([`island_numbers_with`]), where the sweep before tested every pair
//!   whose boxes meet (289 calls, 41.3 s of complex_hierarchy's 45.8 s
//!   route).
//!
//! Each is a pure function of what it compares, and a union-find's
//! components do not depend on the order of its unions, so the islands,
//! the connected sets and the routes are the ones computed from scratch
//! (`tests/k15_pour_fill_contacts.rs`, K15.41's test).
//! [`BasicBoard::pour_fill_work`] counts the work done and reused.
//!
//! **The search.** Under `router.pour_fill_target` as well, the search
//! reaches such an area only in the core of its main island
//! ([`crate::autoroute::pour_target::fill_tiles`]), the copper the area
//! stands for.
//!
//! **Not in `enhanced`** (K15.37, preregistered, decided 2026-10-22, "not
//! kept, guard"). With `router.pour_fill_target` as well, over 14 judged
//! `.kicad_pcb` corpus rows KiCad unconnected went 338 -> 272 against a set
//! noise of 59.34 (the pcu board 47 -> 20, canary-pwr 40 -> 26), but
//! routing-caused DRC went 12 -> 27 and route time 1.95 x (limit 1.25 x), with
//! 0 manufacturing breaches in all 34 graded arms. So the setting is opt-in,
//! unset in both profiles; the KiCad plugin offers it as a per-board option
//! (`RouteOptions.pour_fill_contacts`), and K15.41-K15.43 fix the costs and
//! measure again.
//!
//! K15.43 measured the pair again (preregistered, decided 2026-10-22, "not
//! kept, guard"): over 16 judged `.kicad_pcb` corpus rows KiCad unconnected
//! went 370 -> 269 against a set noise of 67.09 (the pcu board 47 -> 26,
//! canary-pwr 40 -> 26) and routing-caused DRC 15 -> 11, with 0 manufacturing
//! breaches and 0 below-minimum items in all 34 graded arms, but route time
//! went 625.8 s -> 798.6 s, 1.28 x (limit 1.25 x; one board 120.7 s ->
//! 215.0 s). So the setting stays opt-in, unset in both profiles; K15.47
//! takes the route time and K15.48 measures again.
//!
//! A deliberate divergence (`docs/DIVERGENCES.md`, "freerouting (board
//! model)"); pinned by `tests/k15_pour_fill_contacts.rs`.
//!
//! [`opened_fill_core`]: crate::quality::pour_fill::opened_fill_core
//! [`PourFills::all_pieces`]: crate::quality::pour_fill::PourFills
//! [`PourFills::pieces`]: crate::quality::pour_fill::PourFills::pieces
//! [`island_numbers`]: crate::quality::pour_islands

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use freerouting_geometry::{IntBox, Point, PolylineShapeOps, Shape, TileShape, TileShapeOps};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::drc::CellCache;
use crate::drc::Trapezoid;
use crate::quality::pour_fill::{
    CoreInputs, FillCore, opened_fill_core_inputs, reach, trapezoid_tile, zone_of,
};
use crate::quality::pour_islands::island_numbers_with;

/// The switch and the cached islands of a board's judged pours.
#[derive(Debug, Default)]
pub struct PourContacts {
    on: bool,
    state: Mutex<State>,
}

#[derive(Clone, Debug, Default)]
struct State {
    /// The judged areas, computed on demand and forgotten when a
    /// conduction area changes.
    candidates: Option<Arc<Vec<Candidate>>>,
    /// Per area: its fill's pieces and islands, or `None` when it keeps
    /// Java's outline.
    geometry: BTreeMap<ItemId, Option<Arc<FillGeometry>>>,
    /// Per area: its islands with their contacts, or `None` when it keeps
    /// Java's outline.
    islands: BTreeMap<ItemId, Option<Arc<AreaIslands>>>,
    /// K15.41: per area the inputs of its last computed fill with that
    /// fill, kept when the fill is forgotten: a fill computed again from
    /// equal inputs is that fill.
    memo: BTreeMap<ItemId, Arc<FillMemo>>,
    /// K15.41: the work of the fills so far.
    work: PourFillWork,
}

/// K15.41: the fill of an area computed from `inputs` on `layer`, with the
/// cells of its fill model for the next computation.
#[derive(Debug)]
struct FillMemo {
    layer: i32,
    inputs: CoreInputs,
    fill: Option<Arc<FillGeometry>>,
    cells: CellCache,
    pieces: PieceCache,
}

/// A conduction area whose contacts the fill decides, with the region in
/// which another item's change can change them.
#[derive(Debug)]
struct Candidate {
    id: ItemId,
    layer: i32,
    region: IntBox,
    /// The area's tile shapes' boxes, each grown as `region` is.
    tiles: Vec<IntBox>,
    nets: Vec<i32>,
}

/// One area's opened fill: its pieces and their islands. Only other nets'
/// copper changes it: the opening fills the net's own drills over, and its
/// own traces and pours are no operand (`drc::fill_operands_eroded`), as
/// long as the opening's radius is positive.
#[derive(Debug)]
struct FillGeometry {
    /// The opened fill's pieces.
    pieces: Vec<Arc<FillPiece>>,
    /// Per piece its island.
    island: Vec<usize>,
    /// Per island the sum of its pieces' areas.
    size: Vec<f64>,
    /// The opening's radius.
    radius: f64,
}

/// K15.41: one piece of an opened fill, grown from the core tile of one
/// trapezoid of the fill model. `id` names it among the pieces of its
/// area's fills, for [`PieceCache`]'s shape tests.
#[derive(Debug)]
struct FillPiece {
    id: u64,
    /// The core tile.
    core: TileShape,
    /// The core tile grown by the opening's radius, with its box.
    grown: TileShape,
    bounds: IntBox,
    /// `grown` as a shape, with its box and the area of `grown`.
    shape: Shape,
    shape_bounds: IntBox,
    area: f64,
}

/// K15.41: per area the pieces of its last fill by the bits of their
/// trapezoid, and the shape tests between them (by the pieces' ids, in the
/// order asked), for the next fill: a piece is a function of its trapezoid,
/// the radius and the layer, and a test of its two pieces.
#[derive(Debug, Default)]
struct PieceCache {
    radius: u64,
    layer: i32,
    pieces: crate::util::HashMap<[u64; 6], Option<Arc<FillPiece>>>,
    next_id: u64,
    tests: crate::util::HashMap<(u64, u64), bool>,
    /// The work of the fill that built this cache.
    work: PourFillWork,
}

/// K15.41: the work of the pour contacts' fills on a board, summed over
/// its fills ([`BasicBoard::pour_fill_work`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PourFillWork {
    /// Fills computed, and fills taken whole from the last one of their
    /// area because its inputs were the same.
    pub fills_computed: usize,
    pub fills_reused: usize,
    /// Grid cells of the fill model swept, and taken from the last fill.
    pub cells_swept: usize,
    pub cells_reused: usize,
    /// Pieces grown from a trapezoid, and taken from the last fill.
    pub pieces_built: usize,
    pub pieces_reused: usize,
    /// Shape tests between two pieces run, and taken from the last fill.
    pub tests_run: usize,
    pub tests_reused: usize,
}

impl PourFillWork {
    fn add(&mut self, other: &PourFillWork) {
        self.fills_computed += other.fills_computed;
        self.fills_reused += other.fills_reused;
        self.cells_swept += other.cells_swept;
        self.cells_reused += other.cells_reused;
        self.pieces_built += other.pieces_built;
        self.pieces_reused += other.pieces_reused;
        self.tests_run += other.tests_run;
        self.tests_reused += other.tests_reused;
    }
}

/// The islands of one area's opened fill and the contacts in each.
#[derive(Debug)]
pub struct AreaIslands {
    fill: Arc<FillGeometry>,
    /// The island the area item stands for.
    main: usize,
    /// Per island the items of Java's contacts with a point in it, in id
    /// order.
    members: Vec<Vec<ItemId>>,
}

impl AreaIslands {
    /// The island holding `point`, if a piece does.
    pub(crate) fn island_at(&self, point: &Point) -> Option<usize> {
        let float = point.to_float();
        self.fill
            .pieces
            .iter()
            .position(|piece| {
                let (b, tile) = (&piece.bounds, &piece.grown);
                f64::from(b.ll.x) <= float.x
                    && float.x <= f64::from(b.ur.x)
                    && f64::from(b.ll.y) <= float.y
                    && float.y <= f64::from(b.ur.y)
                    && tile.contains(point)
            })
            .map(|i| self.fill.island[i])
    }

    /// The core tiles of the main island, in piece order.
    pub fn main_core(&self) -> Vec<TileShape> {
        self.fill
            .pieces
            .iter()
            .zip(&self.fill.island)
            .filter(|(_, k)| **k == self.main)
            .map(|(p, _)| p.core.clone())
            .collect()
    }

    /// The number of islands.
    pub fn island_count(&self) -> usize {
        self.members.len()
    }

    /// K15.42: true if island `k` is not the main one and holds no contact
    /// but `item`.
    pub(crate) fn holds_only(&self, k: usize, item: ItemId) -> bool {
        k != self.main && self.members.get(k).is_some_and(|m| m.as_slice() == [item])
    }
}

/// K15.41: the core of an area's opened fill as `router.pour_fill_target`'s
/// cycle rule reads it ([`BasicBoard::pour_fill_core`]): the fill the pour
/// contacts hold, or one computed now.
pub(crate) struct FillCoreView(CoreSource);

enum CoreSource {
    Held(Option<Arc<FillGeometry>>),
    Now(FillCore),
}

impl FillCoreView {
    /// [`FillCore::holds`]: a core tile holds `point`.
    pub(crate) fn holds(&self, point: &Point) -> bool {
        match &self.0 {
            CoreSource::Held(fill) => fill
                .as_ref()
                .is_some_and(|f| f.pieces.iter().any(|p| p.core.contains(point))),
            CoreSource::Now(core) => core.holds(point),
        }
    }

    /// [`FillCore::reaches`]: a core tile grown by the radius holds
    /// `point` (a grown piece holds only points in its box).
    pub(crate) fn reaches(&self, point: &Point) -> bool {
        match &self.0 {
            CoreSource::Held(fill) => fill.as_ref().is_some_and(|f| {
                let at = point.to_float();
                f.pieces.iter().any(|p| {
                    let b = &p.bounds;
                    f64::from(b.ll.x) <= at.x
                        && at.x <= f64::from(b.ur.x)
                        && f64::from(b.ll.y) <= at.y
                        && at.y <= f64::from(b.ur.y)
                        && p.grown.contains(point)
                })
            }),
            CoreSource::Now(core) => core.reaches(point),
        }
    }
}

impl Clone for PourContacts {
    /// A copy with the same switch and cache (the islands are shared).
    fn clone(&self) -> Self {
        PourContacts {
            on: self.on,
            state: Mutex::new(self.lock().clone()),
        }
    }
}

impl PourContacts {
    pub(crate) fn new(on: bool) -> PourContacts {
        PourContacts {
            on,
            state: Mutex::default(),
        }
    }

    /// True if `router.pour_fill_contacts` is on.
    pub(crate) fn is_on(&self) -> bool {
        self.on
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Forgets every island and the candidate list.
    pub(crate) fn forget_all(&mut self) {
        *self.state.get_mut().unwrap_or_else(|e| e.into_inner()) = State::default();
    }
}

impl BasicBoard {
    /// `router.pour_fill_contacts` (K15.31) on the board's connectivity.
    pub fn pour_fill_contacts(&self) -> bool {
        self.pour_contacts.is_on()
    }

    /// Sets `router.pour_fill_contacts` (`prepare_job`); the routing ledger
    /// is rebuilt on its next read, because the counts depend on it.
    pub fn set_pour_fill_contacts(&mut self, on: bool) {
        if self.pour_contacts.is_on() != on {
            self.pour_contacts = PourContacts::new(on);
            self.ledger.invalidate();
        }
    }

    /// K15.41: the work of the pour contacts' fills so far (test support:
    /// `tests/k15_pour_fill_contacts.rs` pins what a fill reuses).
    #[doc(hidden)]
    pub fn pour_fill_work(&self) -> PourFillWork {
        self.pour_contacts.lock().work
    }

    /// The islands of conduction area `area` under the setting, or `None`
    /// when its contacts are Java's (module docs).
    pub fn pour_islands(&self, area: ItemId) -> Option<Arc<AreaIslands>> {
        if !self.pour_contacts.is_on() || !self.kicad_report_rules().kicad_pcb {
            return None;
        }
        if let Some(known) = self.pour_contacts.lock().islands.get(&area) {
            return known.clone();
        }
        let fill = self.pour_contacts.lock().geometry.get(&area).cloned();
        let fill = match fill {
            Some(known) => known,
            None => {
                let previous = self.pour_contacts.lock().memo.get(&area).cloned();
                let (fill, memo, reused) = match self.item(area) {
                    Some(item) => fill_geometry(self, item, previous.as_ref()),
                    None => (None, None, false),
                };
                let mut state = self.pour_contacts.lock();
                state.geometry.insert(area, fill.clone());
                match memo {
                    Some(memo) => {
                        state.work.add(&memo.pieces.work);
                        state.memo.insert(area, memo);
                    }
                    None if reused => state.work.fills_reused += 1,
                    None => {}
                }
                fill
            }
        };
        let islands = fill.and_then(|fill| Some(area_islands(self, self.item(area)?, fill)));
        self.pour_contacts
            .lock()
            .islands
            .insert(area, islands.clone());
        islands
    }

    /// K15.41: the core of conduction area `area`'s opened fill now
    /// ([`opened_fill_core`](crate::quality::pour_fill::opened_fill_core)),
    /// taken from the fill the pour contacts hold when that fill's inputs
    /// are the inputs now, else computed.
    pub(crate) fn pour_fill_core(&self, area: &Item) -> Option<FillCoreView> {
        let inputs = opened_fill_core_inputs(self, area)?;
        if self.pour_contacts.is_on() {
            let memo = self.pour_contacts.lock().memo.get(&area.id()).cloned();
            if let Some(memo) = memo
                && memo.layer == area.first_layer()
                && memo.inputs == inputs
            {
                return Some(FillCoreView(CoreSource::Held(memo.fill.clone())));
            }
        }
        Some(FillCoreView(CoreSource::Now(inputs.core())))
    }

    /// The contacts of `id` the connectivity follows: `java`, its normal
    /// contacts, with each judged pour's contacts taken from its islands
    /// (module docs).
    pub(crate) fn pour_aware_contacts(&self, id: ItemId, java: Vec<ItemId>) -> Vec<ItemId> {
        self.pour_aware_contacts_at(id, java, None)
    }

    /// [`pour_aware_contacts`](Self::pour_aware_contacts) of item `id` at
    /// `only` (a trace end), or at all its contact points.
    fn pour_aware_contacts_at(
        &self,
        id: ItemId,
        java: Vec<ItemId>,
        only: Option<&Point>,
    ) -> Vec<ItemId> {
        if !self.pour_contacts.is_on() {
            return java;
        }
        let Some(item) = self.item(id) else {
            return java;
        };
        if item.is_conduction_area() {
            let Some(islands) = self.pour_islands(id) else {
                return java;
            };
            let main = &islands.members[islands.main];
            return java
                .into_iter()
                .filter(|c| {
                    self.item(*c).is_some_and(Item::is_conduction_area)
                        || main.binary_search(c).is_ok()
                })
                .collect();
        }
        let mut result = Vec::with_capacity(java.len());
        let mut through: BTreeSet<ItemId> = BTreeSet::new();
        for contact in java {
            let area = self.item(contact).filter(|c| c.is_conduction_area());
            let islands = area.and_then(|_| self.pour_islands(contact));
            let (Some(area), Some(islands)) = (area, islands) else {
                result.push(contact);
                continue;
            };
            let mut joined = false;
            let points = contact_points(item, area);
            for point in points.iter().filter(|p| only.is_none_or(|o| o == *p)) {
                match islands.island_at(point) {
                    Some(k) if k == islands.main => joined = true,
                    Some(k) => through.extend(islands.members[k].iter().filter(|m| **m != id)),
                    None => {}
                }
            }
            if joined {
                result.push(contact);
            }
        }
        for other in through {
            if !result.contains(&other) {
                result.push(other);
            }
        }
        result
    }

    /// K15.31: Java's `Trace.isCycle` on the island-aware contacts: true if
    /// the items at the trace's start reach the items at its end without
    /// it (Java's depth-first search finds the same, through the outline).
    /// Every trace takes it under the setting, also one with no end in a
    /// pour: a path from it may pass one (two vias joined by a trace on
    /// another layer, each in another island of a pour, are no cycle).
    /// `None` (Java's rule) when the setting is off or the trace's net
    /// class ignores cycles with areas (then no pour, and so no island, is
    /// passed).
    pub(crate) fn pour_cycle(&self, id: ItemId) -> Option<bool> {
        if !self.pour_contacts.is_on() {
            return None;
        }
        let item = self.item(id)?;
        let trace = item.as_trace()?;
        if self.ignore_cycles_with_areas(item) {
            return None;
        }
        let (first, last) = (trace.first_corner(), trace.last_corner());
        let start = self.trace_start_contacts(id);
        let end = self.trace_end_contacts(id);
        let side = |point: &Point, java: BTreeSet<ItemId>| -> BTreeSet<ItemId> {
            self.pour_aware_contacts_at(id, java.into_iter().collect(), Some(point))
                .into_iter()
                .filter(|c| *c != id)
                .collect()
        };
        let start = side(&first, start);
        let end = side(&last, end);
        let mut seen: BTreeSet<ItemId> = start.clone();
        seen.insert(id);
        let mut stack: Vec<ItemId> = start.into_iter().collect();
        while let Some(at) = stack.pop() {
            if end.contains(&at) {
                return Some(true);
            }
            let java: Vec<ItemId> = self.normal_contacts(at).into_iter().collect();
            for next in self.pour_aware_contacts(at, java) {
                if seen.insert(next) {
                    stack.push(next);
                }
            }
        }
        Some(false)
    }

    /// True if [`pour_cycle`](Self::pour_cycle) can differ from `java`,
    /// Java's `isCycle` of trace `id`: the island-aware contacts join no
    /// more than Java's, so a trace Java finds no cycle for has none either,
    /// unless an end of it lies in a judged pour's outline (Java's search
    /// then starts at the pour and skips the trace's other end in it).
    pub(crate) fn pour_cycle_asked(&self, id: ItemId, java: bool) -> bool {
        if !self.pour_contacts.is_on() {
            return false;
        }
        java || self
            .trace_start_contacts(id)
            .into_iter()
            .chain(self.trace_end_contacts(id))
            .any(|c| {
                self.item(c).is_some_and(Item::is_conduction_area) && self.pour_islands(c).is_some()
            })
    }

    /// K15.31: an item of `nets` on layers `first..=last` within `bounds`
    /// changed; forgets the islands it may change (their contacts, and
    /// their fill unless the item is of the area's own nets) and marks their
    /// nets for the ledger's recount. `area` is true if the item is a
    /// conduction area, which forgets everything.
    pub(crate) fn note_pour_change(
        &mut self,
        first: i32,
        last: i32,
        bounds: IntBox,
        nets: &[i32],
        area: bool,
    ) {
        if !self.pour_contacts.is_on() || !self.kicad_report_rules().kicad_pcb {
            return;
        }
        if area {
            self.pour_contacts.forget_all();
            self.ledger.invalidate();
            return;
        }
        let candidates = self.pour_candidates();
        let mut dirty: Vec<i32> = Vec::new();
        {
            let state = self
                .pour_contacts
                .state
                .get_mut()
                .unwrap_or_else(|e| e.into_inner());
            for c in candidates.iter() {
                if c.layer < first
                    || c.layer > last
                    || !c.region.intersects_box(&bounds)
                    || !c.tiles.iter().any(|t| t.intersects_box(&bounds))
                {
                    continue;
                }
                state.islands.remove(&c.id);
                let own = !nets.is_empty() && nets.iter().all(|n| c.nets.contains(n));
                let keeps_fill = own
                    && state
                        .geometry
                        .get(&c.id)
                        .is_some_and(|g| g.as_ref().is_some_and(|g| g.radius > 0.0));
                if !keeps_fill {
                    state.geometry.remove(&c.id);
                }
                dirty.extend(&c.nets);
            }
        }
        if !dirty.is_empty() {
            self.ledger.mark_dirty(&dirty);
        }
    }

    /// The judged conduction areas of the board, in id order.
    fn pour_candidates(&self) -> Arc<Vec<Candidate>> {
        if let Some(known) = &self.pour_contacts.lock().candidates {
            return known.clone();
        }
        let list: Vec<Candidate> = self
            .items()
            .filter(|i| i.is_conduction_area() && zone_of(self, i).is_some())
            .map(|i| {
                let r = reach(self, i);
                let grow = |b: IntBox| {
                    IntBox::new(
                        b.ll.x.saturating_sub(r),
                        b.ll.y.saturating_sub(r),
                        b.ur.x.saturating_add(r),
                        b.ur.y.saturating_add(r),
                    )
                };
                let tiles = (0..self.tile_shape_count(i))
                    .filter_map(|k| self.tile_shape(i, k))
                    .map(|t| grow(t.bounding_box()))
                    .collect();
                Candidate {
                    id: i.id(),
                    layer: i.first_layer(),
                    region: grow(i.bounding_box()),
                    tiles,
                    nets: i.net_numbers().to_vec(),
                }
            })
            .collect();
        let mut list = list;
        list.sort_by_key(|c| c.id);
        let list = Arc::new(list);
        self.pour_contacts.lock().candidates = Some(list.clone());
        list
    }
}

/// The points at which Java joins `item` to `area`: a drill item's centre,
/// a trace's end corners, each inside the area's outline.
fn contact_points(item: &Item, area: &Item) -> Vec<Point> {
    let Some(outline) = area.as_conduction_area().map(|a| a.area()) else {
        return Vec::new();
    };
    let points = match &item.kind {
        ItemKind::Pin(_) | ItemKind::Via(_) => item
            .drill()
            .map(|d| vec![Point::Int(d.center)])
            .unwrap_or_default(),
        ItemKind::Trace(t) => vec![t.first_corner(), t.last_corner()],
        _ => Vec::new(),
    };
    points.into_iter().filter(|p| outline.contains(p)).collect()
}

/// The opened fill of conduction area `area` on `board` now, or `None` when
/// its contacts are Java's (module docs), with the memo to keep when it was
/// computed. K15.41: `previous`'s fill when its inputs are the same (the
/// fill is a function of them, [`CoreInputs`]), and then true.
fn fill_geometry(
    board: &BasicBoard,
    area: &Item,
    previous: Option<&Arc<FillMemo>>,
) -> (Option<Arc<FillGeometry>>, Option<Arc<FillMemo>>, bool) {
    let Some(inputs) = opened_fill_core_inputs(board, area) else {
        return (None, None, false);
    };
    let layer = area.first_layer();
    if let Some(previous) = previous
        && previous.layer == layer
        && previous.inputs == inputs
    {
        return (previous.fill.clone(), None, true);
    }
    let (no_cells, no_pieces) = (CellCache::default(), PieceCache::default());
    let (old_cells, old_pieces) =
        previous.map_or((&no_cells, &no_pieces), |p| (&p.cells, &p.pieces));
    let (trapezoids, cells) = inputs.trapezoids_cached(old_cells);
    let (fill, mut pieces) = fill_of_trapezoids(&trapezoids, inputs.radius(), layer, old_pieces);
    pieces.work.fills_computed = 1;
    pieces.work.cells_swept = cells.swept;
    pieces.work.cells_reused = cells.reused;
    let memo = FillMemo {
        layer,
        inputs,
        fill: fill.clone(),
        cells,
        pieces,
    };
    (fill, Some(Arc::new(memo)), false)
}

/// The opened fill grown from the core tiles of `trapezoids` by `radius` on
/// `layer` (module docs), `None` for a core with no tile, with the pieces
/// and shape tests for the next fill. K15.41: a piece and a test that
/// `previous` holds are taken from it.
fn fill_of_trapezoids(
    trapezoids: &[Trapezoid],
    radius: f64,
    layer: i32,
    previous: &PieceCache,
) -> (Option<Arc<FillGeometry>>, PieceCache) {
    let reuse = previous.radius == radius.to_bits() && previous.layer == layer;
    let mut next = PieceCache {
        radius: radius.to_bits(),
        layer,
        pieces: crate::util::HashMap::with_capacity_and_hasher(
            trapezoids.len(),
            Default::default(),
        ),
        next_id: if reuse { previous.next_id } else { 0 },
        tests: crate::util::HashMap::with_capacity_and_hasher(
            previous.tests.len(),
            Default::default(),
        ),
        work: PourFillWork::default(),
    };
    let mut pieces: Vec<Arc<FillPiece>> = Vec::with_capacity(trapezoids.len());
    for t in trapezoids {
        let key = [
            t.x_left, t.x_right, t.left[0], t.left[1], t.right[0], t.right[1],
        ]
        .map(f64::to_bits);
        let known = reuse.then(|| previous.pieces.get(&key)).flatten();
        let piece = match known {
            Some(piece) => {
                next.work.pieces_reused += usize::from(piece.is_some());
                piece.clone()
            }
            None => trapezoid_tile(t).map(|core| {
                let grown = if radius > 0.0 {
                    core.enlarge(radius)
                } else {
                    core.clone()
                };
                let shape = Shape::from(grown.clone());
                next.next_id += 1;
                next.work.pieces_built += 1;
                Arc::new(FillPiece {
                    id: next.next_id,
                    bounds: grown.bounding_box(),
                    shape_bounds: shape.bounding_box(),
                    area: grown.area(),
                    core,
                    grown,
                    shape,
                })
            }),
        };
        if let Some(piece) = &piece {
            pieces.push(piece.clone());
        }
        next.pieces.insert(key, piece);
    }
    if pieces.is_empty() {
        return (None, next);
    }
    let boxes: Vec<IntBox> = pieces.iter().map(|p| p.shape_bounds).collect();
    let layers = vec![layer; pieces.len()];
    let (island, _) = island_numbers_with(&boxes, &layers, |i, j| {
        let key = (pieces[i].id, pieces[j].id);
        let meets = match reuse.then(|| previous.tests.get(&key)).flatten() {
            Some(known) => {
                next.work.tests_reused += 1;
                *known
            }
            None => {
                next.work.tests_run += 1;
                pieces[i].shape.intersects(&pieces[j].shape)
            }
        };
        next.tests.insert(key, meets);
        meets
    });
    let count = island.iter().copied().max().map_or(0, |m| m + 1);
    let mut size = vec![0.0f64; count];
    for (piece, k) in pieces.iter().zip(&island) {
        size[*k] += piece.area;
    }
    let fill = FillGeometry {
        pieces,
        island,
        size,
        radius,
    };
    (Some(Arc::new(fill)), next)
}

/// The islands of conduction area `area` with fill `fill`: the contacts in
/// each and the main island (module docs).
fn area_islands(board: &BasicBoard, area: &Item, fill: Arc<FillGeometry>) -> Arc<AreaIslands> {
    let count = fill.size.len();
    let mut islands = AreaIslands {
        fill,
        main: 0,
        members: vec![Vec::new(); count],
    };
    let mut has_pin = vec![false; count];
    for id in board.normal_contacts(area.id()) {
        let Some(item) = board.item(id).filter(|i| !i.is_conduction_area()) else {
            continue;
        };
        for point in contact_points(item, area) {
            if let Some(k) = islands.island_at(&point) {
                if islands.members[k].last() != Some(&id) {
                    islands.members[k].push(id);
                }
                has_pin[k] |= item.is_pin();
            }
        }
    }
    let size = &islands.fill.size;
    let rank = |k: usize| (has_pin[k], !islands.members[k].is_empty());
    let mut main = 0;
    for k in 1..count {
        if (rank(k), size[k]) > (rank(main), size[main]) {
            main = k;
        }
    }
    islands.main = main;
    for m in &mut islands.members {
        m.sort();
        m.dedup();
    }
    Arc::new(islands)
}

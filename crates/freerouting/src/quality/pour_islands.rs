//! K15.29: the pour islands KiCad's filler removes, left out of the stub
//! cleanup's fill model.
//!
//! KiCad's zone filler removes every island of a zone's fill whose copper
//! group (the island and everything of its net joined to it through copper,
//! other islands included) holds no pad, as the zone's `island_removal_mode`
//! 0 (always, KiCad's default) says; KiCad 10.0.7 keeps them all where every
//! island of the zone on that layer is isolated (K15.16,
//! `drc/kicad_connectivity.rs`). [`PourFills`] models a pour's copper by its
//! opened fill, and before K15.29 it kept every island of it. On the pcu
//! board's DSN route (K15.27, `Net-(U24-OUT)`) a routed via sat in an island
//! of zone USB_OUT (F.Cu) that `BUS_GND`'s track and via cut off from U24's
//! pad, and in an island of USB_OUT_POUR (B.Cu, no pad at all). KiCad removed
//! the F.Cu island and kept the B.Cu one, so the via was connected on one
//! layer (`via_dangling`); the cleanup counted two layers and kept it.
//!
//! So the cleanup leaves out the pieces of an area's opened fill that lie in
//! a **dropped** island of its *island model*: the same opened fill with the
//! router's clearances, without the `router.cleanup.pour_clearance_um` floor
//! ([`PourFills::island_model`]; on a `.kicad_pcb`, which has no floor, the
//! fill itself). A floor larger than the clearance KiCad's filler applies (a
//! custom rule of the board's `.kicad_dru` overrides the zone's clearance)
//! would cut islands KiCad's fill does not have, so the islands are judged
//! where the fill is widest. An island of the model is dropped when all of
//! these hold:
//!
//! - **the zone removes islands**: on a `.kicad_pcb` the file's zone has
//!   `island_removal_mode` 0; on a DSN input `router.cleanup.pour_island_removal`
//!   is on and the area's net is not in `pour_islands_kept_nets` (the KiCad
//!   plugin passes the nets of the zones that keep them). A zone that removes
//!   only the islands below an area (mode 2) keeps them all here;
//! - **no pad reaches it**: a search from the island over its net's copper
//!   (pins, traces and vias by [`stub_contacts::pieces`], other pours by the
//!   islands of their island model, a pour judged by its outline by the
//!   outline), joined where shapes meet on a layer, finds no pin;
//! - **another island of the area meets a pin's pad itself**, so KiCad's zone
//!   has an island that is not isolated and removes the others;
//! - **the net has no pour the reader dropped** (`BasicBoard::dropped_pours`:
//!   KiCad copper the board does not hold, which may join a pad).
//!
//! **The islands** of a fill are the connected components of its pieces, two
//! pieces joined where their shapes meet. A piece is a trapezoid of the
//! eroded fill grown back by the opening's radius, so two parts the erosion
//! separates are one island where their growths overlap, as they are in
//! KiCad's inflated fill. A piece of the fill is left out when it meets no
//! kept island of the island model.
//!
//! Every approximation keeps more than KiCad: the island model covers
//! KiCad's fill wherever the router's clearances are no larger than KiCad's
//! and the opening no larger than the zone's (K15.26), and copper meets where
//! shapes intersect, borders included, so a group here reaches every pad
//! KiCad's group reaches. An island dropped here is one KiCad removes, and
//! the cleanup keeps its rule (Q16.4): it may keep a trace or via KiCad calls
//! dangling, never remove one KiCad counts connected. The one judgement taken
//! from the model alone is the third point: an island of the model that meets
//! a pad stands for KiCad's.
//!
//! The islands and decisions are cached per area and forgotten at every
//! removal ([`IslandCache::forget`]): a removal may grow another net's fill,
//! which may join a group to a pad. The search runs in item-id order; nothing
//! reads a hash order.
//!
//! [`stub_contacts::pieces`]: crate::quality::stub_contacts::pieces

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::rc::Rc;

use freerouting_geometry::{IntBox, TileShape};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::drc::IslandRemoval;
use crate::quality::pour_fill::{Piece, PourFills, zone_of};
use crate::quality::stub_contacts;

/// The islands of one fill, each its pieces, in the order of their first
/// piece.
type Islands = Rc<Vec<Vec<Piece>>>;

/// The cached island models and kept pieces of a board's opened fills.
#[derive(Debug, Default)]
pub(crate) struct IslandCache {
    models: RefCell<BTreeMap<ItemId, Option<Islands>>>,
    kept: RefCell<BTreeMap<ItemId, Rc<Vec<Piece>>>>,
    /// K15.41: per island of a model the boxes of its pieces and their
    /// union, for [`meet_boxed`].
    boxes: RefCell<BTreeMap<ItemId, Rc<Vec<IslandBoxes>>>>,
}

/// K15.41: the bounding boxes of an island's pieces, and their union
/// (`None` for no piece).
#[derive(Debug)]
struct IslandBoxes {
    pieces: Vec<IntBox>,
    all: Option<IntBox>,
}

impl IslandBoxes {
    fn new(pieces: &[Piece]) -> IslandBoxes {
        let pieces: Vec<IntBox> = pieces.iter().map(|(_, s)| s.bounding_box()).collect();
        let all = pieces.iter().copied().reduce(|a, b| a.union_box(&b));
        IslandBoxes { pieces, all }
    }
}

impl IslandCache {
    /// The pieces of `all`, `area`'s opened fill, that KiCad's filler keeps
    /// (module docs).
    pub(crate) fn kept(
        &self,
        board: &BasicBoard,
        fills: &PourFills,
        area: &Item,
        all: Rc<Vec<Piece>>,
    ) -> Rc<Vec<Piece>> {
        if !removes_islands(board, area) {
            return all;
        }
        if let Some(known) = self.kept.borrow().get(&area.id()) {
            return known.clone();
        }
        let model = fills.island_model(board, area);
        let kept = match self.model(board, fills, area) {
            Some(islands) if islands.len() > 1 => {
                let keep = self.keep_flags(board, fills, area, &islands);
                if keep.iter().all(|k| *k) {
                    all
                } else if model.as_ref().is_some_and(|m| Rc::ptr_eq(m, &all)) {
                    Rc::new(kept_by_island(&all, &keep))
                } else {
                    let kept_islands: Vec<&Vec<Piece>> = islands
                        .iter()
                        .zip(&keep)
                        .filter(|(_, k)| **k)
                        .map(|(island, _)| island)
                        .collect();
                    Rc::new(
                        all.iter()
                            .filter(|piece| {
                                kept_islands
                                    .iter()
                                    .any(|island| meet(std::slice::from_ref(piece), island))
                            })
                            .cloned()
                            .collect(),
                    )
                }
            }
            _ => all,
        };
        self.kept.borrow_mut().insert(area.id(), kept.clone());
        kept
    }

    /// K15.42: the bounding boxes of the islands of `area`'s island model
    /// that no other item of its net touches, in island order, when `area`'s
    /// zone never removes islands (`island_removal_mode` 1); none for another
    /// mode, a pour judged by its outline and an area that is no zone of the
    /// file. KiCad keeps such an island, and unless another zone of the net
    /// overlaps it, its copper group holds no pad: KiCad's `isolated_copper`
    /// (`CN_CLUSTER::IsOrphaned`). The test is local (the island's own
    /// pieces against the copper of its net's pins, traces and vias, as
    /// [`stub_contacts::pieces`] gives it), so it needs no other pour's fill.
    pub(crate) fn untouched_kept(
        &self,
        board: &BasicBoard,
        fills: &PourFills,
        area: &Item,
    ) -> Vec<IntBox> {
        if zone_of(board, area).is_none_or(|zone| zone.removal != IslandRemoval::Never) {
            return Vec::new();
        }
        let Some(islands) = self.model(board, fills, area) else {
            return Vec::new();
        };
        islands
            .iter()
            .filter(|pieces| !touched(board, area, pieces))
            .filter_map(|pieces| {
                pieces
                    .iter()
                    .map(|(_, s)| s.bounding_box())
                    .reduce(|a, b| a.union_box(&b))
            })
            .collect()
    }

    /// Forgets every island model and keep decision (after a removal).
    pub(crate) fn forget(&self) {
        self.models.borrow_mut().clear();
        self.kept.borrow_mut().clear();
        self.boxes.borrow_mut().clear();
    }

    /// The boxes of the islands of `area`'s model `islands` (K15.41).
    fn model_boxes(&self, area: ItemId, islands: &[Vec<Piece>]) -> Rc<Vec<IslandBoxes>> {
        if let Some(known) = self.boxes.borrow().get(&area) {
            return known.clone();
        }
        let boxes = Rc::new(
            islands
                .iter()
                .map(|i| IslandBoxes::new(i))
                .collect::<Vec<_>>(),
        );
        self.boxes.borrow_mut().insert(area, boxes.clone());
        boxes
    }

    /// The islands of `area`'s island model, or `None` when it is judged by
    /// its outline.
    fn model(&self, board: &BasicBoard, fills: &PourFills, area: &Item) -> Option<Islands> {
        if let Some(known) = self.models.borrow().get(&area.id()) {
            return known.clone();
        }
        let islands = fills
            .island_model(board, area)
            .map(|pieces| Rc::new(islands_of(&pieces)));
        self.models.borrow_mut().insert(area.id(), islands.clone());
        islands
    }

    /// For each island of `area`'s island model, true if KiCad's filler
    /// keeps it (module docs).
    fn keep_flags(
        &self,
        board: &BasicBoard,
        fills: &PourFills,
        area: &Item,
        islands: &[Vec<Piece>],
    ) -> Vec<bool> {
        let on_pad: Vec<bool> = islands
            .iter()
            .map(|pieces| meets_pin(board, area, pieces))
            .collect();
        (0..islands.len())
            .map(|k| {
                on_pad[k]
                    || !on_pad.iter().enumerate().any(|(j, p)| j != k && *p)
                    || self.reaches_pin(board, fills, area, k, &islands[k])
            })
            .collect()
    }

    /// True if island `index` of `area`'s island model (its pieces `start`)
    /// reaches a pin of its net through copper (module docs).
    fn reaches_pin(
        &self,
        board: &BasicBoard,
        fills: &PourFills,
        area: &Item,
        index: usize,
        start: &[Piece],
    ) -> bool {
        let mut seen: BTreeSet<Node> = BTreeSet::from([Node::Island(area.id(), index)]);
        let mut stack: Vec<Vec<Piece>> = vec![start.to_vec()];
        while let Some(pieces) = stack.pop() {
            for piece in &pieces {
                let own = std::slice::from_ref(piece);
                for id in near(board, area, piece, None) {
                    let other = board.item_ref(id);
                    if other.is_conduction_area()
                        && let Some(islands) = self.model(board, fills, other)
                    {
                        let boxes = self.model_boxes(id, &islands);
                        for (k, island) in islands.iter().enumerate() {
                            let node = Node::Island(id, k);
                            if !seen.contains(&node) && meet_boxed(piece, island, &boxes[k]) {
                                seen.insert(node);
                                stack.push(island.clone());
                            }
                        }
                        continue;
                    }
                    let node = Node::Item(id);
                    if seen.contains(&node) {
                        continue;
                    }
                    let copper = stub_contacts::pieces(board, other);
                    if !meet(own, &copper) {
                        continue;
                    }
                    if other.is_pin() {
                        return true;
                    }
                    seen.insert(node);
                    stack.push(copper);
                }
            }
        }
        false
    }
}

/// True if KiCad's filler removes `area`'s islands no pad reaches (module
/// docs, the first and last points).
fn removes_islands(board: &BasicBoard, area: &Item) -> bool {
    let Some(&net) = area.net_numbers().first() else {
        return false;
    };
    if board.dropped_pours().iter().any(|p| p.net_number == net) {
        return false;
    }
    let rules = board.kicad_report_rules();
    if rules.kicad_pcb {
        zone_of(board, area).is_some_and(|zone| zone.removal == IslandRemoval::Always)
    } else {
        rules.dsn_pours.island_removal && !rules.dsn_pours.islands_kept_nets.contains(&net)
    }
}

/// The connected components of `pieces`, two pieces joined where their
/// shapes meet on a layer (a sweep over the bounding boxes in x order), in
/// the order of their first piece.
fn islands_of(pieces: &[Piece]) -> Vec<Vec<Piece>> {
    let numbers = island_numbers(pieces);
    let mut islands: Vec<Vec<Piece>> = Vec::new();
    for (piece, n) in pieces.iter().zip(numbers) {
        if n == islands.len() {
            islands.push(Vec::new());
        }
        islands[n].push(piece.clone());
    }
    islands
}

/// Per piece of `pieces` the number of its island ([`islands_of`]): the
/// islands numbered 0, 1, ... in the order of their first piece. K15.31's
/// router contacts take the islands from it too.
pub(crate) fn island_numbers(pieces: &[Piece]) -> Vec<usize> {
    island_numbers_counted(pieces).0
}

/// How many pieces before it a piece is first tried against
/// ([`island_numbers_counted`]).
const HINT_WINDOW: usize = 16;

/// [`island_numbers`] with the number of shape tests (`Shape::intersects`)
/// it made.
///
/// Two pieces join where they are on one layer, their bounding boxes meet
/// and their shapes intersect; the islands are the components of those
/// joins, whichever order finds them, and two pieces already in one island
/// need no test. K15.41 finds them in two steps:
///
/// - each piece is tried against the [`HINT_WINDOW`] pieces before it until
///   one joins it: a fill's pieces come in the order of its trapezoids,
///   slab by slab, so a piece mostly meets one shortly before it;
/// - a grid over the boxes lists each piece in every cell its box meets. A
///   pair whose boxes meet is in the cell that holds the lower left corner
///   of their overlap, and is tested there; a cell whose pieces are all in
///   one island already is skipped, because none of its pairs can join two
///   islands.
///
/// The sweep over the boxes in x order that this replaces tested every pair
/// whose boxes meet: on complex_hierarchy's route with both pour settings
/// 972 M pair steps and 151.6 M shape tests over 289 fills (41.3 s of the
/// route's 45.8 s), on pcie-active-backplane's 13.1 G steps and 511 M tests
/// (213 s of 277 s).
fn island_numbers_counted(pieces: &[Piece]) -> (Vec<usize>, usize) {
    let boxes: Vec<IntBox> = pieces.iter().map(|(_, s)| s.bounding_box()).collect();
    let layers: Vec<i32> = pieces.iter().map(|(l, _)| *l).collect();
    island_numbers_with(&boxes, &layers, |i, j| pieces[i].1.intersects(&pieces[j].1))
}

/// [`island_numbers_counted`] of pieces given by their bounding boxes,
/// their layers and `meets`, the test whether the shapes of two pieces
/// intersect (asked only for two pieces on one layer whose boxes meet).
pub(crate) fn island_numbers_with(
    boxes: &[IntBox],
    layers: &[i32],
    mut meets: impl FnMut(usize, usize) -> bool,
) -> (Vec<usize>, usize) {
    let count = boxes.len();
    let mut joins = Joins {
        parent: (0..count).collect(),
        tests: 0,
    };
    // True if pieces `i` and `j` are in one island after the call.
    let mut join = |i: usize, j: usize, joins: &mut Joins| -> bool {
        if layers[i] != layers[j] || !boxes[i].intersects_box(&boxes[j]) {
            return false;
        }
        let (a, b) = (joins.root(i), joins.root(j));
        if a == b {
            return true;
        }
        joins.tests += 1;
        // The two in the order of the sweep this replaces (by the left of
        // their boxes, then by index), which tested the earlier one's shape
        // against the later one's.
        let (p, q) = if (boxes[i].ll.x, i) <= (boxes[j].ll.x, j) {
            (i, j)
        } else {
            (j, i)
        };
        if meets(p, q) {
            joins.parent[a.max(b)] = a.min(b);
            return true;
        }
        false
    };
    // A box with no extent in x or y (none in a fill) is paired with every
    // piece, as the box test pairs it; the tree holds the others.
    let proper = |b: &IntBox| b.ll.x <= b.ur.x && b.ll.y <= b.ur.y;
    let (regular, odd): (Vec<usize>, Vec<usize>) = (0..count).partition(|i| proper(&boxes[*i]));
    for &i in &odd {
        for j in 0..count {
            if j != i {
                join(i, j, &mut joins);
            }
        }
    }
    for (k, &i) in regular.iter().enumerate() {
        for &j in regular[k.saturating_sub(HINT_WINDOW)..k].iter().rev() {
            if join(j, i, &mut joins) {
                break;
            }
        }
    }
    let mut tree = BoxTree::new(boxes, &regular);
    tree.note_islands(&mut joins);
    // Every pair of pieces in two islands has one outside the largest
    // island the hints left, so only those look for the pieces they meet.
    let largest = largest_island(&regular, &mut joins);
    for &i in &regular {
        if largest.is_some_and(|l| joins.root(l) == joins.root(i)) {
            continue;
        }
        tree.for_each_meeting(i, boxes, &mut joins, |j, joins| {
            if j != i {
                join(i, j, joins);
            }
        });
    }
    let mut number: BTreeMap<usize, usize> = BTreeMap::new();
    let numbers = (0..count)
        .map(|i| {
            let r = joins.root(i);
            let next = number.len();
            *number.entry(r).or_insert(next)
        })
        .collect();
    (numbers, joins.tests)
}

/// A piece of the island with the most of `pieces` (the first such island
/// by its root), `None` for no piece.
fn largest_island(pieces: &[usize], joins: &mut Joins) -> Option<usize> {
    let mut count: BTreeMap<usize, usize> = BTreeMap::new();
    for &i in pieces {
        *count.entry(joins.root(i)).or_insert(0) += 1;
    }
    let most = count.values().copied().max()?;
    count
        .into_iter()
        .find(|(_, c)| *c == most)
        .map(|(root, _)| root)
}

/// K15.41: the pieces of `all` that meet an island `keep` keeps, where the
/// islands are `all`'s own ([`islands_of`] of `all`, in that order): a piece
/// meets a piece of another island only if they are in one island (the
/// islands are the components of that test, which is symmetric), so it
/// meets a kept island exactly when its own island is kept and has another
/// piece, or the piece meets itself. The test of each piece against every
/// piece of the kept islands that this replaces was quadratic: 29 % of the
/// samples of pcie-active-backplane's route with both pour settings.
fn kept_by_island(all: &[Piece], keep: &[bool]) -> Vec<Piece> {
    let numbers = island_numbers(all);
    let mut size = vec![0usize; keep.len()];
    for k in &numbers {
        size[*k] += 1;
    }
    all.iter()
        .zip(&numbers)
        .filter(|(piece, k)| {
            keep[**k]
                && (size[**k] > 1
                    || meet(std::slice::from_ref(*piece), std::slice::from_ref(*piece)))
        })
        .map(|(piece, _)| piece.clone())
        .collect()
}

/// The union-find of [`island_numbers_counted`]: each component's root is
/// its smallest piece index.
struct Joins {
    parent: Vec<usize>,
    tests: usize,
}

impl Joins {
    fn root(&mut self, mut i: usize) -> usize {
        while self.parent[i] != i {
            self.parent[i] = self.parent[self.parent[i]];
            i = self.parent[i];
        }
        i
    }
}

/// The children of a node of [`BoxTree`].
const TREE_FANOUT: usize = 8;

/// A node of [`BoxTree`]: the box of its pieces, its children (entries for
/// a leaf, nodes of the level below otherwise) and the island all its pieces
/// were in when [`BoxTree::note_islands`] ran, if one.
struct TreeNode {
    bounds: IntBox,
    start: usize,
    len: usize,
    island: Option<usize>,
}

/// A static tree over the boxes of pieces with extent (sort-tile-recursive
/// packing): `levels[0]` are the leaves, the last level holds the top node.
struct BoxTree {
    levels: Vec<Vec<TreeNode>>,
    entries: Vec<usize>,
}

impl BoxTree {
    fn new(boxes: &[IntBox], pieces: &[usize]) -> BoxTree {
        let centre = |b: &IntBox| {
            (
                i64::from(b.ll.x) + i64::from(b.ur.x),
                i64::from(b.ll.y) + i64::from(b.ur.y),
            )
        };
        let mut entries = pieces.to_vec();
        str_order(&mut entries, |i| centre(&boxes[*i]));
        let leaves: Vec<TreeNode> = (0..entries.len())
            .step_by(TREE_FANOUT)
            .map(|start| {
                let len = TREE_FANOUT.min(entries.len() - start);
                let bounds = entries[start..start + len]
                    .iter()
                    .map(|i| boxes[*i])
                    .reduce(|a, b| a.union_box(&b))
                    .expect("a leaf holds a piece");
                TreeNode {
                    bounds,
                    start,
                    len,
                    island: None,
                }
            })
            .collect();
        let mut levels = vec![leaves];
        while levels.last().is_some_and(|l| l.len() > 1) {
            let below = levels.last_mut().expect("a level");
            str_order(below, |n| centre(&n.bounds));
            let above: Vec<TreeNode> = (0..below.len())
                .step_by(TREE_FANOUT)
                .map(|start| {
                    let len = TREE_FANOUT.min(below.len() - start);
                    let bounds = below[start..start + len]
                        .iter()
                        .map(|n| n.bounds)
                        .reduce(|a, b| a.union_box(&b))
                        .expect("a node holds a child");
                    TreeNode {
                        bounds,
                        start,
                        len,
                        island: None,
                    }
                })
                .collect();
            levels.push(above);
        }
        BoxTree { levels, entries }
    }

    /// Notes per node the island of all its pieces, if they are in one.
    fn note_islands(&mut self, joins: &mut Joins) {
        for level in 0..self.levels.len() {
            for k in 0..self.levels[level].len() {
                let (start, len) = (self.levels[level][k].start, self.levels[level][k].len);
                let island = if level == 0 {
                    let mut roots = self.entries[start..start + len]
                        .iter()
                        .map(|i| joins.root(*i));
                    let first = roots.next();
                    first.filter(|r| roots.all(|o| o == *r))
                } else {
                    let below = &self.levels[level - 1][start..start + len];
                    let first = below.first().and_then(|n| n.island);
                    first.filter(|r| below.iter().all(|n| n.island == Some(*r)))
                };
                self.levels[level][k].island = island;
            }
        }
    }

    /// Calls `visit` with each piece whose box meets piece `i`'s, but none
    /// in a node whose pieces were all in `i`'s island.
    fn for_each_meeting(
        &self,
        i: usize,
        boxes: &[IntBox],
        joins: &mut Joins,
        mut visit: impl FnMut(usize, &mut Joins),
    ) {
        let Some(top) = self.levels.len().checked_sub(1) else {
            return;
        };
        let query = boxes[i];
        let mut stack: Vec<(usize, usize)> =
            (0..self.levels[top].len()).map(|k| (top, k)).collect();
        while let Some((level, k)) = stack.pop() {
            let node = &self.levels[level][k];
            if !node.bounds.intersects_box(&query) {
                continue;
            }
            if let Some(island) = node.island
                && joins.root(island) == joins.root(i)
            {
                continue;
            }
            let children = node.start..node.start + node.len;
            if level == 0 {
                for &j in &self.entries[children] {
                    if boxes[j].intersects_box(&query) {
                        visit(j, joins);
                    }
                }
            } else {
                stack.extend(children.map(|c| (level - 1, c)));
            }
        }
    }
}

/// Sorts `items` into sort-tile-recursive order by `centre`: vertical
/// slices by x, each sorted by y, so consecutive runs of [`TREE_FANOUT`]
/// lie close together. Ties keep the given order (a stable sort).
fn str_order<T>(items: &mut [T], centre: impl Fn(&T) -> (i64, i64)) {
    let groups = items.len().div_ceil(TREE_FANOUT);
    let slices = (groups as f64).sqrt().ceil().max(1.0) as usize;
    let slice = groups.div_ceil(slices) * TREE_FANOUT;
    items.sort_by_key(|t| centre(t).0);
    for chunk in items.chunks_mut(slice.max(1)) {
        chunk.sort_by_key(|t| centre(t).1);
    }
}

/// The items of `area`'s net, other than `skip`, whose bounding box meets
/// `piece`'s on its layer, in id order.
fn near(board: &BasicBoard, area: &Item, piece: &Piece, skip: Option<ItemId>) -> Vec<ItemId> {
    let (layer, shape) = piece;
    board
        .overlapping_objects(&TileShape::IntBox(shape.bounding_box()), *layer)
        .into_iter()
        .filter(|id| Some(*id) != skip && board.item_ref(*id).shares_net(area))
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect()
}

/// True if a piece of `a` meets a piece of `b` on a common layer.
fn meet(a: &[Piece], b: &[Piece]) -> bool {
    a.iter().any(|(la, sa)| {
        let ba = sa.bounding_box();
        b.iter()
            .any(|(lb, sb)| la == lb && ba.intersects_box(&sb.bounding_box()) && sa.intersects(sb))
    })
}

/// [`meet`] of `piece` and `island`, whose boxes are `boxes` (K15.41: the
/// boxes once per island, not once per test).
fn meet_boxed(piece: &Piece, island: &[Piece], boxes: &IslandBoxes) -> bool {
    let (la, sa) = piece;
    let ba = sa.bounding_box();
    if !boxes.all.is_some_and(|all| ba.intersects_box(&all)) {
        return false;
    }
    island
        .iter()
        .zip(&boxes.pieces)
        .any(|((lb, sb), bb)| la == lb && ba.intersects_box(bb) && sa.intersects(sb))
}

/// K15.42: true if one of `pieces`, an island of `area`, meets the copper
/// of a pin, trace or via of its net.
fn touched(board: &BasicBoard, area: &Item, pieces: &[Piece]) -> bool {
    pieces.iter().any(|piece| {
        near(board, area, piece, Some(area.id()))
            .into_iter()
            .any(|id| {
                let other = board.item_ref(id);
                !other.is_conduction_area()
                    && meet(
                        std::slice::from_ref(piece),
                        &stub_contacts::pieces(board, other),
                    )
            })
    })
}

/// True if one of `pieces`, an island of `area`, meets a pin of its net.
fn meets_pin(board: &BasicBoard, area: &Item, pieces: &[Piece]) -> bool {
    pieces.iter().any(|piece| {
        near(board, area, piece, Some(area.id()))
            .into_iter()
            .any(|id| {
                let other = board.item_ref(id);
                other.is_pin()
                    && meet(
                        std::slice::from_ref(piece),
                        &stub_contacts::pieces(board, other),
                    )
            })
    })
}

/// A node of the search: an item, or an island of a pour's island model.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Node {
    Item(ItemId),
    Island(ItemId, usize),
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::Shape;

    fn square(layer: i32, x: i32, y: i32, side: i32) -> Piece {
        (
            layer,
            Shape::from(TileShape::IntBox(IntBox::new(x, y, x + side, y + side))),
        )
    }

    /// Pieces that touch, even at a corner, are one island; pieces apart or
    /// on another layer are not; the islands come in the order of their
    /// first piece.
    #[test]
    fn islands_are_the_pieces_that_meet() {
        let pieces = vec![
            square(0, 100, 0, 10),
            square(0, 0, 0, 10),
            square(0, 10, 10, 10),
            square(1, 0, 0, 10),
            square(0, 110, 5, 10),
        ];
        let boxes = |island: &Vec<Piece>| -> Vec<(i32, IntBox)> {
            island.iter().map(|(l, s)| (*l, s.bounding_box())).collect()
        };
        let islands: Vec<Vec<(i32, IntBox)>> = islands_of(&pieces).iter().map(boxes).collect();
        let expected: Vec<Vec<(i32, IntBox)>> = [vec![0, 4], vec![1, 2], vec![3]]
            .iter()
            .map(|members| boxes(&members.iter().map(|k| pieces[*k].clone()).collect()))
            .collect();
        assert_eq!(islands, expected);
    }

    /// The islands by the definition: every pair of pieces tested, the
    /// components found by a search over the pairs that meet, numbered in
    /// the order of their first piece.
    fn islands_by_every_pair(pieces: &[Piece]) -> Vec<usize> {
        let n = pieces.len();
        let meets =
            |i: usize, j: usize| pieces[i].0 == pieces[j].0 && pieces[i].1.intersects(&pieces[j].1);
        let mut number = vec![usize::MAX; n];
        let mut next = 0;
        for start in 0..n {
            if number[start] != usize::MAX {
                continue;
            }
            let mut stack = vec![start];
            number[start] = next;
            while let Some(i) = stack.pop() {
                let found: Vec<usize> = (0..n)
                    .filter(|&j| number[j] == usize::MAX && meets(i, j))
                    .collect();
                for j in found {
                    number[j] = next;
                    stack.push(j);
                }
            }
            next += 1;
        }
        number
    }

    /// K15.41: pieces already in one island are not tested against each
    /// other again. On complex_hierarchy's route with both pour settings the
    /// shape tests were 151.6 M over 289 fills (41.3 s of the route's
    /// 45.8 s); `n` pieces that all overlap need at most `n - 1` tests (one
    /// per join), not `n (n - 1) / 2`.
    #[test]
    fn pieces_of_one_island_are_tested_once_per_join() {
        let n = 200;
        let pieces: Vec<Piece> = (0..n).map(|k| square(0, k, k, 1000)).collect();
        let (numbers, tests) = island_numbers_counted(&pieces);
        assert_eq!(numbers, vec![0; n as usize]);
        assert!(tests < n as usize, "{tests} shape tests for {n} pieces");
    }

    /// K15.41: the islands are the components of the pairs that meet, on
    /// seeded random squares (overlapping, touching at an edge or a corner,
    /// apart, on two layers), whichever pairs the sweep skips.
    #[test]
    fn island_numbers_equal_the_components_of_every_pair() {
        let mut random = freerouting_geometry::jmath::JavaRandom::new(1541);
        for round in 0..40 {
            // A few rounds deep enough for three levels of the box tree.
            let count = if round < 4 {
                400 + random.next_int(200) as usize
            } else {
                1 + random.next_int(120) as usize
            };
            let pieces: Vec<Piece> = (0..count)
                .map(|_| {
                    let layer = random.next_int(2);
                    let x = random.next_int(60) * 10;
                    let y = random.next_int(60) * 10;
                    // Now and then a piece across many grid cells.
                    let steps = if random.next_int(10) == 0 { 30 } else { 4 };
                    let side = 10 * (1 + random.next_int(steps));
                    square(layer, x, y, side)
                })
                .collect();
            assert_eq!(
                island_numbers(&pieces),
                islands_by_every_pair(&pieces),
                "round {round}"
            );
        }
    }

    /// K15.41: the shape test the islands are the components of is
    /// symmetric on fill pieces (octagons grown from boxes, simplices grown
    /// from trapezoids, as `pour_fill` grows them): `kept_by_island` and the
    /// pour contacts' cached tests rely on it. Seeded random pieces, many
    /// of them touching or a unit apart.
    #[test]
    fn the_pieces_shape_test_is_symmetric() {
        use crate::drc::Trapezoid;
        use crate::quality::pour_fill::trapezoid_tile;
        let mut random = freerouting_geometry::jmath::JavaRandom::new(4115);
        let mut coordinate = |range: i32| f64::from(random.next_int(range)) + 0.5;
        let mut pieces: Vec<Shape> = Vec::new();
        while pieces.len() < 300 {
            let (x, y) = (coordinate(200), coordinate(200));
            let (w, h) = (1.0 + coordinate(40), 1.0 + coordinate(40));
            let (dl, dr) = (coordinate(20) - 10.0, coordinate(20) - 10.0);
            let t = Trapezoid {
                x_left: x,
                x_right: x + w,
                left: [y, y + h],
                right: [y + dl, y + h + dr],
            };
            let grow = coordinate(6) - 0.5;
            if let Some(tile) = trapezoid_tile(&t) {
                pieces.push(Shape::from(tile.enlarge(grow)));
            }
            let b = IntBox::new(x as i32, y as i32, (x + w) as i32, (y + h) as i32);
            pieces.push(Shape::from(TileShape::IntBox(b).enlarge(grow)));
        }
        let mut meeting = 0;
        for a in &pieces {
            for b in &pieces {
                let ab = a.intersects(b);
                assert_eq!(ab, b.intersects(a), "{a:?} {b:?}");
                meeting += usize::from(ab);
            }
        }
        assert!(meeting > pieces.len() * 4, "{meeting}");
    }
}

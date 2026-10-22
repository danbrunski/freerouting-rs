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
        let kept = match self.model(board, fills, area) {
            Some(islands) if islands.len() > 1 => {
                let keep = self.keep_flags(board, fills, area, &islands);
                if keep.iter().all(|k| *k) {
                    all
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

    /// Forgets every island model and keep decision (after a removal).
    pub(crate) fn forget(&self) {
        self.models.borrow_mut().clear();
        self.kept.borrow_mut().clear();
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
                        for (k, island) in islands.iter().enumerate() {
                            let node = Node::Island(id, k);
                            if !seen.contains(&node) && meet(own, island) {
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
    let boxes: Vec<IntBox> = pieces.iter().map(|(_, s)| s.bounding_box()).collect();
    let mut order: Vec<usize> = (0..pieces.len()).collect();
    order.sort_by_key(|i| (boxes[*i].ll.x, *i));
    let mut parent: Vec<usize> = (0..pieces.len()).collect();
    fn root(parent: &mut [usize], mut i: usize) -> usize {
        while parent[i] != i {
            parent[i] = parent[parent[i]];
            i = parent[i];
        }
        i
    }
    for (k, &i) in order.iter().enumerate() {
        for &j in &order[k + 1..] {
            if boxes[j].ll.x > boxes[i].ur.x {
                break;
            }
            let (li, si) = &pieces[i];
            let (lj, sj) = &pieces[j];
            if li == lj && boxes[i].intersects_box(&boxes[j]) && si.intersects(sj) {
                let (a, b) = (root(&mut parent, i), root(&mut parent, j));
                if a != b {
                    parent[a.max(b)] = a.min(b);
                }
            }
        }
    }
    let mut number: BTreeMap<usize, usize> = BTreeMap::new();
    let mut islands: Vec<Vec<Piece>> = Vec::new();
    for (i, piece) in pieces.iter().enumerate() {
        let r = root(&mut parent, i);
        let next = number.len();
        let n = *number.entry(r).or_insert(next);
        if n == islands.len() {
            islands.push(Vec::new());
        }
        islands[n].push(piece.clone());
    }
    islands
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
}

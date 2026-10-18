//! The candidate order of an optimizer pass (O06.2).
//!
//! Port of `BatchOptimizer.ReadSortedRouteItems.next` and
//! `prepareCandidateItems` (`BatchOptimizer.java:597-633`, `:1161-1258` at
//! `a773a70ec`).
//!
//! Java calls `next()` until it returns null; each call scans the item list
//! twice (vias, then traces) for the smallest key strictly greater than the
//! previous one. That is O(n²). The port sorts once, with the same result:
//!
//! - a via is a candidate when it is not user fixed and has no ignored net;
//!   its key is (centre x, centre y, first layer), the coordinates as
//!   `double`;
//! - a trace is a candidate when it is not shove fixed, has no ignored net
//!   and no unfixed via among its normal contacts; its key is (the
//!   lexicographically larger end corner, layer);
//! - the candidates are sorted by (key, vias before traces, item-list
//!   order, which is highest id first), and the first of each distinct key
//!   is kept: Java needs a strictly greater key for every next pick, so
//!   every other item with that key is skipped for the whole pass (kept on
//!   purpose: DIVERGENCES, "Java bugs kept on purpose");
//! - Java's scan starts above `(Integer.MIN_VALUE, Integer.MIN_VALUE, -1)`
//!   and only takes keys below `(Integer.MAX_VALUE, Integer.MAX_VALUE,
//!   Integer.MAX_VALUE)`; keys outside never come up, here as there.
//!
//! With [`ItemSelectionStrategy::Prioritized`] and the previous pass's
//! results, the items with a result come first in ascending
//! [`ItemRouteResult::compare`] order (Java's `PriorityQueue.poll`), then
//! the others in key order.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use freerouting_geometry::FloatPoint;

use crate::autoroute::item_route_result::ItemRouteResult;
use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::settings::ItemSelectionStrategy;

/// The sort key of a candidate: Java's `(minItemCoor, minItemLayer)`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CandidateKey {
    /// The x coordinate (`double`).
    pub x: f64,
    /// The y coordinate (`double`).
    pub y: f64,
    /// The via's first layer or the trace's layer.
    pub layer: i32,
}

impl CandidateKey {
    fn new(point: FloatPoint, layer: i32) -> CandidateKey {
        // `+ 0.0` turns -0.0 into 0.0: Java compares with `<` and `==`,
        // for which the two zeros are equal.
        CandidateKey {
            x: point.x + 0.0,
            y: point.y + 0.0,
            layer,
        }
    }

    /// Java's lexicographic comparison of `(x, y, layer)`. The coordinates
    /// are finite, so the order is total.
    fn cmp(&self, other: &CandidateKey) -> Ordering {
        self.x
            .partial_cmp(&other.x)
            .unwrap_or(Ordering::Equal)
            .then(self.y.partial_cmp(&other.y).unwrap_or(Ordering::Equal))
            .then(self.layer.cmp(&other.layer))
    }

    /// True if Java's scan can return this key: above the start key and
    /// below the "nothing found" key.
    fn in_java_range(&self) -> bool {
        let start = CandidateKey {
            x: f64::from(i32::MIN),
            y: f64::from(i32::MIN),
            layer: -1,
        };
        let end = CandidateKey {
            x: f64::from(i32::MAX),
            y: f64::from(i32::MAX),
            layer: i32::MAX,
        };
        self.cmp(&start) == Ordering::Greater && self.cmp(&end) == Ordering::Less
    }
}

/// One candidate of a pass.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Candidate {
    /// The item.
    pub id: ItemId,
    /// Its key.
    pub key: CandidateKey,
    /// True for a via.
    pub is_via: bool,
}

/// The key of `item` when it is a candidate on `board`, else `None`.
fn candidate_key(board: &BasicBoard, item: &Item) -> Option<CandidateKey> {
    if let Some(via) = item.as_via() {
        if item.is_user_fixed() || board.has_ignored_nets(item) {
            return None;
        }
        return Some(CandidateKey::new(
            via.drill.center.to_float(),
            via.drill.first_layer,
        ));
    }
    let trace = item.as_trace()?;
    if board.is_shove_fixed(item) || board.has_ignored_nets(item) {
        return None;
    }
    let first = trace.first_corner().to_float();
    let last = trace.last_corner().to_float();
    // PARITY: :1216-1221, the last corner when first < last, else the first.
    let compare = if first.x < last.x || first.x == last.x && first.y < last.y {
        last
    } else {
        first
    };
    // PARITY: :1232-1239, a trace touching an unfixed via never comes up
    // (it is checked only when the trace would win, but then it never
    // updates the minimum, so filtering it first gives the same picks).
    let touches_unfixed_via = board.normal_contacts(item.id()).into_iter().any(|id| {
        board
            .item(id)
            .is_some_and(|c| c.is_via() && !c.is_user_fixed())
    });
    if touches_unfixed_via {
        return None;
    }
    Some(CandidateKey::new(compare, trace.layer))
}

/// The candidates of a SEQUENTIAL pass, in Java's `next()` order.
pub fn sorted_candidates(board: &BasicBoard) -> Vec<Candidate> {
    // Item-list order (highest id first) is the position in this list.
    let mut all: Vec<(usize, Candidate)> = board
        .items()
        .filter_map(|item| {
            let key = candidate_key(board, item)?;
            key.in_java_range().then_some(Candidate {
                id: item.id(),
                key,
                is_via: item.is_via(),
            })
        })
        .enumerate()
        .collect();
    // Vias first at an equal key (Java scans the vias before the traces),
    // then list order (the first one a scan meets wins a tie).
    all.sort_by(|(pa, a), (pb, b)| {
        a.key
            .cmp(&b.key)
            .then(b.is_via.cmp(&a.is_via))
            .then(pa.cmp(pb))
    });
    // PARITY(quirk, java_quirks.opt_equal_key_skip): Java's strictly greater
    // cursor skips every later candidate of an equal key for the whole pass;
    // `false` keeps every sorted candidate.
    let skip_equal_keys = board.java_quirks().opt_equal_key_skip;
    let mut result: Vec<Candidate> = Vec::with_capacity(all.len());
    for (_, candidate) in all {
        let duplicate = skip_equal_keys
            && result
                .last()
                .is_some_and(|prev| prev.key.cmp(&candidate.key) == Ordering::Equal);
        if !duplicate {
            result.push(candidate);
        }
    }
    result
}

/// Java `prepareCandidateItems`: the candidate ids of a pass. With
/// [`ItemSelectionStrategy::Prioritized`] and a non-empty `results` map,
/// the items with a result come first, best first, then the rest in key
/// order. Clears `results`, as Java does.
pub fn prepare_candidates(
    board: &BasicBoard,
    strategy: ItemSelectionStrategy,
    results: &mut BTreeMap<ItemId, ItemRouteResult>,
) -> Vec<ItemId> {
    let sorted = sorted_candidates(board);
    let ids = if strategy == ItemSelectionStrategy::Prioritized && !results.is_empty() {
        let mut prioritized: Vec<ItemRouteResult> = Vec::new();
        let mut rest: Vec<ItemId> = Vec::new();
        for candidate in &sorted {
            match results.get(&candidate.id) {
                Some(result) => prioritized.push(*result),
                None => rest.push(candidate.id),
            }
        }
        // `compare` ends on the item id, so the order is total and equals
        // the `PriorityQueue`'s poll order.
        prioritized.sort_by(|a, b| a.compare(b));
        prioritized
            .iter()
            .map(ItemRouteResult::item_id)
            .chain(rest)
            .collect()
    } else {
        sorted.iter().map(|c| c.id).collect()
    };
    results.clear();
    ids
}

/// Java `getItemPosition`: a via's centre or a trace's first corner.
pub fn item_position(item: &Item) -> Option<FloatPoint> {
    if let Some(via) = item.as_via() {
        Some(via.drill.center.to_float())
    } else {
        item.as_trace().map(|trace| trace.first_corner().to_float())
    }
}

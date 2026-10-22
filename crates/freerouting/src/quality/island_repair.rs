//! K15.42: the cut that leaves a kept pour island isolated, routed again
//! around the pour.
//!
//! Not Java. A pour is no obstacle (Java's `Structure.insertPlanes`), so
//! other nets route across it, and KiCad's refill keeps its fill clear of
//! their copper. A zone that keeps its islands (`island_removal_mode` 1)
//! keeps one that the cuts leave with no pad in its copper group, and KiCad's
//! DRC reports it `isolated_copper`. On moteus (K15.37's `pcb-on` command on
//! main `0564a6d`, with `router.pour_fill_contacts` and
//! `router.pour_fill_target`, session `447b2a9c`) KiCad reported 9 such
//! islands against 3 without the pair: small mode-1 zones that the routes of
//! other nets cut, for example GNDPWR's zone of priority 33 on In1.Cu (x
//! 131.2-134.05, y 93.4-99.7 mm), which a `+3V` track at x = 133.4799 mm cut
//! in three, and GND's zone of priority 29 on In1.Cu (x 144.25-149.3, y
//! 101.9-109.15 mm), which `/VT Sense/VTEMP_MOT`, `/stm32g4/SWCLK` and `V+`
//! tracks cut in four, two of them with no GND item. The router's contacts
//! do not see such an island (under `router.pour_fill_contacts` an area
//! stands for its main island, and Java's outline joins everything inside
//! it), so nothing in the search avoids the cut.
//!
//! Under `router.pour_fill_target` on a `.kicad_pcb` input ([`enabled`]) the
//! pipeline runs this repair after the optimizer and the stub cleanup, before
//! the output check (Q20), unless the job was stopped:
//!
//! 1. **The islands** ([`isolated_islands`]): per conduction area that
//!    `PourFills` judges by its fill and whose zone keeps its islands (mode
//!    1), in item-id order, the islands of its island model (K15.29) that no
//!    pin, trace or via of its net touches (`PourFills::untouched_kept_islands`).
//!    The test is local, so it needs no other pour's fill (about 35 ms on
//!    moteus); it misses an island that another zone of the net joins (KiCad
//!    joins two zones where a corner of one lies in the other) and counts one
//!    that KiCad's fill does not have.
//! 2. **The cuts** of each island, in island order: per foreign net, in
//!    ascending net number, the routed traces and vias of that net on the
//!    area's layer that overlap the area's outline and whose boxes meet the
//!    island's box grown by the area's reach (its largest clearance, its
//!    minimum thickness and a 2000-unit margin, `pour_fill::reach`). Input
//!    copper and items whose deletion is forbidden are never cuts.
//! 3. **The trials** of an island: each net's cuts alone, in ascending net
//!    number, then all its nets' cuts together; each (area, nets) is tried
//!    once. A trial keeps a clone of the board, removes the cuts with the
//!    rest of their connections up to the next fork or terminal item (Java's
//!    `Item.getConnectionItems`, never input copper) and, if that leaves
//!    fewer such islands, routes those nets' incomplete connections again by
//!    the job's own stages (one pass, rip-up off, as Q20's retry) with the
//!    area an obstacle for their traces and vias (a temporary
//!    [`AreaObstaclePolicy`] beside the board's own). It is kept if the board
//!    then has fewer such islands and no more incomplete connections than
//!    before it; otherwise the clone is put back. After a kept trial the
//!    islands are found again and the scan starts over. Each trial runs at
//!    most once and a kept one lowers the count, so the repair ends.
//!
//! Nothing found, nothing changed: a board with no such island, or whose
//! islands have no cut that joins them, is not touched, so its session is the
//! one without the repair. A zone that removes its islands (mode 0) or the
//! islands below an area (mode 2) is not judged.
//!
//! On moteus (K15.42's run, numbers seen before K15.43's registration) the
//! repair keeps 4 of 11 routed trials in about 17 s, and KiCad's
//! `isolated_copper` goes 9 -> 5 (`pcb-off`: 3).
//!
//! A deliberate divergence (`docs/DIVERGENCES.md`, "freerouting (board
//! model)", the `pour_fill_target` entry); pinned by
//! `tests/k15_pour_pair_drc.rs::a_cut_that_isolates_a_kept_island_is_routed_around_it`.

use std::collections::BTreeSet;
use std::sync::Arc;
use std::time::Instant;

use freerouting_geometry::IntBox;

use crate::board::items::{Item, ItemId, StopConnectionOption};
use crate::board::{BasicBoard, RoutingBoard};
use crate::quality::pour_fill::{PourFills, reach};
use crate::rules::AreaObstaclePolicy;

/// A kept pour island that no pin, trace or via of its net touches.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IsolatedIsland {
    /// The conduction area the island is part of.
    pub area: ItemId,
    pub net: i32,
    pub layer: i32,
    /// The bounding box of the island's pieces.
    pub bounds: IntBox,
}

/// What one [`repair`] call did.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IslandRepair {
    /// Isolated kept islands before the repair.
    pub before: usize,
    /// Isolated kept islands after it.
    pub after: usize,
    /// Trials run (an area and a net whose cuts were removed and routed again).
    pub tried: usize,
    /// Trials kept.
    pub kept: usize,
    /// Wall-clock seconds the repair took (a log figure; nothing reads it).
    pub seconds: f64,
}

impl IslandRepair {
    /// True if a trial was kept, so the board changed.
    pub fn changed(&self) -> bool {
        self.kept > 0
    }

    /// The job log line.
    pub fn log_line(&self) -> String {
        format!(
            "Island repair: {} isolated pour island(s) before, {} after; {} cut(s) routed again, \
             {} kept, in {:.1} s",
            self.before, self.after, self.tried, self.kept, self.seconds
        )
    }
}

/// True if the repair runs on `board`: `router.pour_fill_target` is on and
/// the board was read from a `.kicad_pcb`.
pub fn enabled(board: &BasicBoard) -> bool {
    board.pour_fill_target() && board.kicad_report_rules().kicad_pcb
}

/// The kept pour islands of `board` that no item of their net touches
/// (module docs, step 1), in item-id order of their areas and island order
/// within an area.
pub fn isolated_islands(board: &BasicBoard) -> Vec<IsolatedIsland> {
    isolated_by(board, PourFills::untouched_kept_islands)
}

/// K15.45: [`isolated_islands`] judged with each zone's own clearance and
/// without the islands another zone of the net joins to a pin, as KiCad's
/// connectivity joins zones of one net (`PourFills::zone_island_model`). Not
/// the repair's yet: on moteus's `pcb-on` route it made the repair re-route
/// 19 cuts against 11 and leave 31 connections open against 29.
pub fn isolated_islands_with_zone_joins(board: &BasicBoard) -> Vec<IsolatedIsland> {
    isolated_by(board, PourFills::zone_joined_untouched_kept_islands)
}

fn isolated_by(
    board: &BasicBoard,
    islands_of: fn(&PourFills, &BasicBoard, &Item) -> Vec<IntBox>,
) -> Vec<IsolatedIsland> {
    let fills = PourFills::new();
    board
        .items()
        .filter(|item| item.is_conduction_area())
        .flat_map(|area| {
            let net = area.net_numbers().first().copied().unwrap_or(0);
            islands_of(&fills, board, area)
                .into_iter()
                .map(move |bounds| IsolatedIsland {
                    area: area.id(),
                    net,
                    layer: area.first_layer(),
                    bounds,
                })
        })
        .collect()
}

/// Routes the cuts of `board`'s isolated kept islands again around their
/// pours (module docs). `keep` is the input copper, never removed; `retry`
/// routes the incomplete connections of the nets it is given once.
pub fn repair(
    board: &mut RoutingBoard,
    keep: &BTreeSet<ItemId>,
    retry: &mut dyn FnMut(&mut RoutingBoard, &BTreeSet<i32>),
) -> IslandRepair {
    let start = Instant::now();
    let mut islands = isolated_islands(board);
    let mut outcome = IslandRepair {
        before: islands.len(),
        ..IslandRepair::default()
    };
    let mut tried: BTreeSet<(ItemId, Vec<i32>)> = BTreeSet::new();
    'scan: loop {
        for island in &islands {
            for (nets, cuts) in trials_of(&cuts_by_net(board, island, keep)) {
                if !tried.insert((island.area, nets.clone())) {
                    continue;
                }
                let found = islands.len();
                match trial(board, island.area, &nets, &cuts, keep, found, retry) {
                    Trial::Cut => {}
                    Trial::Rejected => outcome.tried += 1,
                    Trial::Kept(after) => {
                        outcome.tried += 1;
                        outcome.kept += 1;
                        islands = after;
                        continue 'scan;
                    }
                }
            }
        }
        break;
    }
    outcome.after = islands.len();
    outcome.seconds = start.elapsed().as_secs_f64();
    outcome
}

/// The trials of one island (module docs, step 3): each net's cuts alone, in
/// ascending net number, then, with more than one net, all of them together.
fn trials_of(by_net: &[(i32, Vec<ItemId>)]) -> Vec<(Vec<i32>, Vec<ItemId>)> {
    let mut trials: Vec<(Vec<i32>, Vec<ItemId>)> = by_net
        .iter()
        .map(|(net, cuts)| (vec![*net], cuts.clone()))
        .collect();
    if by_net.len() > 1 {
        trials.push((
            by_net.iter().map(|(net, _)| *net).collect(),
            by_net
                .iter()
                .flat_map(|(_, cuts)| cuts.iter().copied())
                .collect(),
        ));
    }
    trials
}

/// The cuts of `island` (module docs, step 2), grouped by net in ascending
/// net number, each group in ascending item id.
fn cuts_by_net(
    board: &BasicBoard,
    island: &IsolatedIsland,
    keep: &BTreeSet<ItemId>,
) -> Vec<(i32, Vec<ItemId>)> {
    let Some(area) = board.item(island.area) else {
        return Vec::new();
    };
    let grown = island.bounds.offset(f64::from(reach(board, area)));
    let mut found: BTreeSet<(i32, ItemId)> = BTreeSet::new();
    for i in 0..board.tile_shape_count(area) {
        let Some(tile) = board.tile_shape(area, i) else {
            continue;
        };
        for id in board.overlapping_objects(&tile, island.layer) {
            let item = board.item_ref(id);
            if is_cut(board, item, island, keep) && item.bounding_box().intersects_box(&grown) {
                if let Some(&net) = item.net_numbers().first() {
                    found.insert((net, id));
                }
            }
        }
    }
    let mut groups: Vec<(i32, Vec<ItemId>)> = Vec::new();
    for (net, id) in found {
        match groups.last_mut() {
            Some((last, ids)) if *last == net => ids.push(id),
            _ => groups.push((net, vec![id])),
        }
    }
    groups
}

/// True if `item` may be a cut of `island`: a routed trace or via of one
/// foreign net that the router may remove.
fn is_cut(
    board: &BasicBoard,
    item: &Item,
    island: &IsolatedIsland,
    keep: &BTreeSet<ItemId>,
) -> bool {
    removable(board, item, keep) && !item.contains_net(island.net)
}

/// True if `item` is a routed trace or via of one net that the router may
/// remove: not input copper (`keep`) and not deletion-forbidden.
fn removable(board: &BasicBoard, item: &Item, keep: &BTreeSet<ItemId>) -> bool {
    (item.is_trace() || item.is_via())
        && item.is_routable()
        && item.net_numbers().len() == 1
        && !keep.contains(&item.id())
        && !board.is_deletion_forbidden(item)
}

/// The net of an item of one net.
fn net_of(item: &Item) -> i32 {
    item.net_numbers().first().copied().unwrap_or(0)
}

/// How one trial ended.
enum Trial {
    /// Removing the cuts joined no island, so nothing was routed.
    Cut,
    /// Routed again and undone.
    Rejected,
    /// Kept, with the board's isolated islands after it.
    Kept(Vec<IsolatedIsland>),
}

/// One trial (module docs, step 3) of the cuts `cuts` of `nets` across
/// `area`; the board is as before it unless it is kept.
fn trial(
    board: &mut RoutingBoard,
    area: ItemId,
    nets: &[i32],
    cuts: &[ItemId],
    keep: &BTreeSet<ItemId>,
    isolated_before: usize,
    retry: &mut dyn FnMut(&mut RoutingBoard, &BTreeSet<i32>),
) -> Trial {
    let saved = board.clone();
    let incomplete_before = board.ledger_incomplete_count();
    let run: BTreeSet<ItemId> = cuts
        .iter()
        .flat_map(|id| board.connection_items(*id, StopConnectionOption::None))
        .filter(|id| {
            board
                .item(*id)
                .is_some_and(|item| removable(board, item, keep) && nets.contains(&net_of(item)))
        })
        .collect();
    for id in run {
        board.remove_item(id);
    }
    if isolated_islands(board).len() >= isolated_before {
        *board = saved;
        return Trial::Cut;
    }
    let own = board.rules.area_obstacle_policy.clone();
    board.rules.area_obstacle_policy = Some(Arc::new(Protect {
        inner: own.clone(),
        area,
        nets: nets.to_vec(),
    }));
    retry(board, &nets.iter().copied().collect());
    board.rules.area_obstacle_policy = own;
    let after = isolated_islands(board);
    if after.len() < isolated_before && board.ledger_incomplete_count() <= incomplete_before {
        return Trial::Kept(after);
    }
    *board = saved;
    Trial::Rejected
}

/// The board's own area policy, and area `area` an obstacle for `nets`.
struct Protect {
    inner: Option<Arc<dyn AreaObstaclePolicy>>,
    area: ItemId,
    nets: Vec<i32>,
}

impl AreaObstaclePolicy for Protect {
    fn blocks(&self, area: ItemId, net: i32) -> bool {
        (area == self.area && self.nets.contains(&net))
            || self.inner.as_ref().is_some_and(|p| p.blocks(area, net))
    }

    fn blocks_vias(&self, area: ItemId, net: i32) -> bool {
        (area == self.area && self.nets.contains(&net))
            || self
                .inner
                .as_ref()
                .is_some_and(|p| p.blocks_vias(area, net))
    }

    fn lets_vias_through(&self) -> bool {
        self.inner.as_ref().is_some_and(|p| p.lets_vias_through())
    }
}

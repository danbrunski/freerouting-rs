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
//! 1. **The islands** ([`isolated_islands`], K15.46): per conduction area
//!    that `PourFills` judges by its fill and whose zone keeps its islands
//!    (mode 1), in item-id order, the islands of its zone-join model
//!    (K15.45, `PourFills::zone_island_model`: the zone's own clearance, no
//!    blob slack, same-net higher-priority zones cut at their outline) that
//!    no pin, trace or via of its net touches and that no other zone of the
//!    net joins to a pin (`PourFills::zone_joined_untouched_kept_islands`).
//!    On moteus's `pcb-on` routes it finds KiCad's islands (`pcb-off`:
//!    KiCad's 3 exactly), where K15.42's model
//!    ([`isolated_islands_without_zone_joins`], the router's clearances)
//!    missed V+'s zone of priority 34, which KiCad joins to V+'s fill of
//!    priority 0 through a corner, and merged +3V's island of priority 44
//!    with a touched one.
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
//!    [`AreaObstaclePolicy`] beside the board's own). If a net then has more
//!    incomplete connections than before the trial, those nets are routed
//!    once more the same way without the obstacle (K15.46). The trial is
//!    kept if **no net** has more incomplete connections than before it
//!    (the routing ledger's count per net, which follows the opened fills
//!    under `router.pour_fill_contacts`) and either the board has fewer such
//!    islands, or as many and fewer incomplete connections; otherwise the
//!    clone is put back. After a kept trial the islands are found again and
//!    the scan starts over. Each trial runs at most once, so the repair ends.
//!
//! **Why per net** (K15.46): K15.42 kept a trial whose board-wide count did
//! not rise, so a trial that opened one connection of a cut net and closed
//! one of another was kept. On moteus both models' repairs kept such a trial:
//! removing `/Current Sense/FCUR3`'s route joined GND's F.Cu island of
//! priority 40 (x 153.66-154.43, y 109.2-111.2 mm) and closed a +3V
//! connection, and the re-route around the zone failed, leaving FCUR3 open
//! (the job log: "+3V 4->3, /Current Sense/FCUR3 0->1; kept"), which KiCad
//! 10.0.7 reported as U3 pad 18 to R15 pad 2 unconnected. Pinned by
//! `tests/k15_pour_pair_drc.rs::a_re_route_that_opens_a_connection_is_undone`.
//! KiCad counts such a trade as the router does (on that test's board KiCad
//! reports +3V unconnected with FCUR3's route and FCUR3 unconnected without
//! it), and on moteus no kept trial raised KiCad's unconnected count
//! (K15.46's runs with the repair stopped after each kept trial: 32, 31, 31,
//! 31).
//!
//! **The log** names every routed trial: the island (its net, area, layer
//! and box in mm), the cut nets, the islands and incomplete connections
//! before and after, the nets whose incomplete count changed and whether it
//! was kept; then each island left.
//!
//! Nothing found, nothing changed: a board with no such island, or whose
//! islands have no cut that joins them, is not touched, so its session is the
//! one without the repair. A zone that removes its islands (mode 0) or the
//! islands below an area (mode 2) is not judged.
//!
//! On moteus (K15.46's run, numbers seen after K15.43's registration, the
//! pair being opt-in) the repair keeps 4 of 8 routed trials in about 16 s;
//! KiCad's unconnected count is 30 and its `isolated_copper` 5 (K15.42's
//! repair: 29 and 5; no repair: 32 and 9; `pcb-off`: 40 and 3).
//!
//! A deliberate divergence (`docs/DIVERGENCES.md`, "freerouting (board
//! model)", the `pour_fill_target` entry); pinned by
//! `tests/k15_pour_pair_drc.rs::a_cut_that_isolates_a_kept_island_is_routed_around_it`
//! and `a_re_route_that_opens_a_connection_is_undone`.

use std::collections::{BTreeMap, BTreeSet};
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
#[derive(Clone, Debug, Default, PartialEq)]
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
    /// K15.46: one job log line per routed trial and per island left
    /// (module docs, "The log").
    pub lines: Vec<String>,
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

/// The kept pour islands of `board` that no item of their net touches and
/// no other zone of their net joins to a pin (module docs, step 1), in
/// item-id order of their areas and island order within an area: the
/// repair's islands.
pub fn isolated_islands(board: &BasicBoard) -> Vec<IsolatedIsland> {
    isolated_islands_with_zone_joins(board)
}

/// K15.45: the islands judged with each zone's own clearance and without
/// the islands another zone of the net joins to a pin, as KiCad's
/// connectivity joins zones of one net (`PourFills::zone_island_model`);
/// [`isolated_islands`] since K15.46.
pub fn isolated_islands_with_zone_joins(board: &BasicBoard) -> Vec<IsolatedIsland> {
    isolated_by(board, PourFills::zone_joined_untouched_kept_islands)
}

/// K15.42's islands: the island model with the router's clearances, no
/// zone-to-zone joins (`PourFills::untouched_kept_islands`). The repair's
/// until K15.46; kept to compare the two models.
pub fn isolated_islands_without_zone_joins(board: &BasicBoard) -> Vec<IsolatedIsland> {
    isolated_by(board, PourFills::untouched_kept_islands)
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
                    Trial::Rejected(record) => {
                        outcome.tried += 1;
                        outcome.lines.push(record.line(board, island, &nets, false));
                    }
                    Trial::Kept(after, record) => {
                        outcome.tried += 1;
                        outcome.kept += 1;
                        outcome.lines.push(record.line(board, island, &nets, true));
                        islands = after;
                        continue 'scan;
                    }
                }
            }
        }
        break;
    }
    outcome.lines.extend(
        islands
            .iter()
            .map(|island| format!("Island repair: left {}", describe(board, island))),
    );
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
    Rejected(TrialRecord),
    /// Kept, with the board's isolated islands after it.
    Kept(Vec<IsolatedIsland>, TrialRecord),
}

/// The counts one routed trial was judged by, for its log line.
struct TrialRecord {
    cuts: usize,
    islands: (usize, usize),
    incomplete: (usize, usize),
    /// The nets whose incomplete count changed, with it before and after.
    changed: Vec<(i32, usize, usize)>,
    /// The nets that opened were routed again without the obstacle.
    again: bool,
}

impl TrialRecord {
    fn line(
        &self,
        board: &BasicBoard,
        island: &IsolatedIsland,
        nets: &[i32],
        kept: bool,
    ) -> String {
        let names: Vec<String> = nets.iter().map(|n| net_name(board, *n)).collect();
        let changed: Vec<String> = self
            .changed
            .iter()
            .map(|(n, a, z)| format!("{} {a}->{z}", net_name(board, *n)))
            .collect();
        format!(
            "Island repair trial: {}, {} cut(s) of {}: islands {} -> {}, incomplete {} -> {}{}{}; {}",
            describe(board, island),
            self.cuts,
            names.join(", "),
            self.islands.0,
            self.islands.1,
            self.incomplete.0,
            self.incomplete.1,
            if changed.is_empty() {
                String::new()
            } else {
                format!(" ({})", changed.join(", "))
            },
            if self.again {
                ", opened nets routed again"
            } else {
                ""
            },
            if kept { "kept" } else { "undone" }
        )
    }
}

/// `<net> island of area #<id> on layer <n> (x .., y .. mm)`, KiCad's y.
fn describe(board: &BasicBoard, island: &IsolatedIsland) -> String {
    let b = &island.bounds;
    let mm = |v: i32| f64::from(v) / 10_000.0;
    format!(
        "{} island of area #{} on layer {} (x {:.2}-{:.2}, y {:.2}-{:.2} mm)",
        net_name(board, island.net),
        island.area.0,
        island.layer,
        mm(b.ll.x),
        mm(b.ur.x),
        mm(-b.ur.y),
        mm(-b.ll.y)
    )
}

fn net_name(board: &BasicBoard, net: i32) -> String {
    board
        .rules
        .nets
        .get(net)
        .map_or_else(|| format!("net {net}"), |n| n.name.clone())
}

/// The routing ledger's incomplete count of every net that has one.
fn incomplete_by_net(board: &BasicBoard) -> BTreeMap<i32, usize> {
    board
        .ledger_incomplete_net_numbers()
        .into_iter()
        .map(|n| (n, board.routing_ledger().net_incomplete_count(board, n)))
        .collect()
}

/// The nets whose count differs between `before` and `after`, ascending,
/// with both counts.
fn changed_nets(
    before: &BTreeMap<i32, usize>,
    after: &BTreeMap<i32, usize>,
) -> Vec<(i32, usize, usize)> {
    let nets: BTreeSet<i32> = before.keys().chain(after.keys()).copied().collect();
    nets.into_iter()
        .filter_map(|n| {
            let (a, z) = (
                before.get(&n).copied().unwrap_or(0),
                after.get(&n).copied().unwrap_or(0),
            );
            (a != z).then_some((n, a, z))
        })
        .collect()
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
    let by_net_before = incomplete_by_net(board);
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
    let opened = opened_nets(&by_net_before, &incomplete_by_net(board));
    let again = !opened.is_empty();
    if again {
        retry(board, &opened);
    }
    let after = isolated_islands(board);
    let by_net_after = incomplete_by_net(board);
    let record = TrialRecord {
        cuts: cuts.len(),
        islands: (isolated_before, after.len()),
        incomplete: (incomplete_before, board.ledger_incomplete_count()),
        changed: changed_nets(&by_net_before, &by_net_after),
        again,
    };
    if keeps(&record, &by_net_before, &by_net_after) {
        return Trial::Kept(after, record);
    }
    *board = saved;
    Trial::Rejected(record)
}

/// The nets with more incomplete connections in `after` than in `before`.
fn opened_nets(before: &BTreeMap<i32, usize>, after: &BTreeMap<i32, usize>) -> BTreeSet<i32> {
    changed_nets(before, after)
        .into_iter()
        .filter(|(_, a, z)| z > a)
        .map(|(n, _, _)| n)
        .collect()
}

/// The keep rule (module docs, step 3): no net opens a connection, and the
/// islands fall, or stay and the incomplete connections fall.
fn keeps(
    record: &TrialRecord,
    before: &BTreeMap<i32, usize>,
    after: &BTreeMap<i32, usize>,
) -> bool {
    let (islands_before, islands_after) = record.islands;
    let (incomplete_before, incomplete_after) = record.incomplete;
    opened_nets(before, after).is_empty()
        && (islands_after < islands_before
            || (islands_after == islands_before && incomplete_after < incomplete_before))
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

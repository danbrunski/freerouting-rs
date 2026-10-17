//! Blocker attribution from the maze search's failure record (Q06.1).
//!
//! A failed connection names the routed nets in its way. The record is the
//! A04 failure record of the Freerouting maze search ([`Blocked`]: the item
//! that stopped an expansion, its net, layer and reason), extended by
//! [`RecordingSink`] with the expansion step that met the block (the
//! "door": the popped element whose room's doors were being expanded) and
//! that step's location (the middle of the popped element's entry line).
//!
//! [`attribute`] counts expansions, not area. Per foreign net of a routed
//! item ([`NetBlocking`]):
//! - `blocked`: the record entries whose blocking item is on that net;
//! - `unique`: the entries where it is the only routed net blocking that
//!   expansion step (fixed copper, keepouts and the outline do not count
//!   as nets here: ripping them is not possible);
//! - `near_start` / `near_dest`: the entries within [`NEAR_RADIUS_MM`] of a
//!   start or destination pin.
//!
//! An item is **routed** if it is a trace or via whose fixed state is
//! [`FixedState::Unfixed`]; everything else (pins, keepouts, the outline,
//! conduction areas, fixed or shove-fixed wiring) is **fixed**.
//! [`Coverage`] says how many entries were of each kind.
//!
//! **Orders that decide results.** The record is sorted by `(net, item id,
//! layer, reason, step)` without duplicates ([`FailureEntries::finish`]);
//! the nets are counted in that order; the output is sorted by the rank key
//! descending, then by net number ascending, and cut to [`MAX_NETS`] after
//! sorting. The rank key is KiCadRoutingTools' `count` ranking: nets that
//! are the sole blocker of every entry they block first (by `unique`, then
//! `near_start + near_dest`), then the rest by the score `unique +
//! min(near, unique) + 0.5 · shared` (then `near`). The score is kept
//! doubled in integers, so no float comparison decides an order.
//!
//! Credit: the counting and the ranking follow the idea of
//! KiCadRoutingTools' `py_router/blocking_analysis.py`
//! (`analyze_frontier_blocking`, `rank_blockers` with `count`, at
//! `0486e799`); this module is written from the checklist's statement of
//! it, not from that code.
//!
//! [`analyze_connection`] produces a record for one connection by running
//! the Freerouting maze search on a copy of the board with ripup off, so
//! that routed copper is an obstacle and lands in the record (with ripup on,
//! Java's search enters routed traces by ripping them, and they never block
//! it). The copy has its own work meter: the analysis changes nothing on
//! the board it was given, and no later work limit sees its units.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{IntBox, IntPoint};

use crate::autoroute::connection::{Ripped, RouteLimits, route_connection};
use crate::autoroute::control::AutorouteControl;
use crate::autoroute::path::RipupCosts;
use crate::autoroute::session::Session;
use crate::autoroute::session::search::SearchSettings;
use crate::autoroute::session::sink::{BlockReason, Blocked, PopInfo, SearchSink, StepEnd};
use crate::autoroute::stages::ConnectionParams;
use crate::autoroute::{AttemptResult, AttemptState};
use crate::board::BasicBoard;
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::search::{UNITS_PER_NOMINAL_MS, WorkLimit};
use crate::board::structure::FixedState;
use crate::settings::board::BoardRouterSettings;

/// The radius around a start or destination pin within which an entry is
/// `near` it, in mm (KRT's 3 mm).
pub const NEAR_RADIUS_MM: f64 = 3.0;

/// The most nets an [`Attribution`] lists.
pub const MAX_NETS: usize = 10;

/// The most entries a [`FailureEntries`] keeps; the rest are counted in
/// [`Coverage::dropped`].
pub const RECORD_CAP: usize = 65_536;

/// The site name of the analysis search's work limit.
pub const ANALYSIS_SITE: &str = "q06_analysis";

/// The sort key of a [`BlockEntry`] (see [`BlockEntry::key`]).
pub type EntryKey = (i32, i32, i32, BlockReason, Option<u64>, Option<(i32, i32)>);

/// One entry of the failure record. Not `Ord`: [`ItemId`]'s order is
/// Java's `TreeSet<Item>` order (highest id first); [`BlockEntry::key`] is
/// the record's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct BlockEntry {
    /// The first net of the blocking item, 0 for none.
    pub net: i32,
    /// The blocking item.
    pub item: ItemId,
    pub layer: i32,
    pub reason: BlockReason,
    /// The expansion step (the popped element's step number) that met the
    /// block; `None` for a block met before the first pop.
    pub step: Option<u64>,
    /// The location of that step: the middle of the popped element's entry
    /// line, rounded; `None` with `step`.
    pub at: Option<IntPoint>,
}

impl BlockEntry {
    /// The record's order: `(net, item id, layer, reason, step, at)`, item
    /// ids ascending.
    pub fn key(&self) -> EntryKey {
        (
            self.net,
            self.item.0,
            self.layer,
            self.reason,
            self.step,
            self.at.map(|p| (p.x, p.y)),
        )
    }

    /// The entry of `blocked` met at `step` and `at`.
    pub fn new(blocked: &Blocked, step: Option<u64>, at: Option<IntPoint>) -> Self {
        BlockEntry {
            net: blocked.net,
            item: blocked.item,
            layer: blocked.layer,
            reason: blocked.reason,
            step,
            at,
        }
    }
}

/// The record of one search: [`BlockEntry`]s in the order they were met.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FailureEntries {
    entries: Vec<BlockEntry>,
}

impl FailureEntries {
    /// Adds an entry.
    pub fn push(&mut self, entry: BlockEntry) {
        self.entries.push(entry);
    }

    /// The entries in the order they were met.
    pub fn raw(&self) -> &[BlockEntry] {
        &self.entries
    }

    /// The entries sorted by `(net, item id, layer, reason, step)` without
    /// duplicates, cut to [`RECORD_CAP`], and the number cut off.
    pub fn finish(&self) -> (Vec<BlockEntry>, usize) {
        let mut entries = self.entries.clone();
        entries.sort_by_key(BlockEntry::key);
        entries.dedup();
        let dropped = entries.len().saturating_sub(RECORD_CAP);
        entries.truncate(RECORD_CAP);
        (entries, dropped)
    }
}

/// A [`SearchSink`] that records every block with the expansion step that
/// met it. It only observes: the search runs as with any other sink.
#[derive(Clone, Debug, Default)]
pub struct RecordingSink {
    /// The step being expanded and its location.
    current: Option<(u64, IntPoint)>,
    /// What was recorded.
    pub record: FailureEntries,
}

impl SearchSink for RecordingSink {
    fn on_pop(&mut self, _session: &Session<'_>, info: &PopInfo<'_>) {
        let entry = info.element.shape_entry;
        let middle = freerouting_geometry::FloatPoint {
            x: (entry.a.x + entry.b.x) / 2.0,
            y: (entry.a.y + entry.b.y) / 2.0,
        };
        self.current = Some((info.step, middle.round()));
    }

    fn on_blocked(&mut self, _session: &Session<'_>, blocked: &Blocked) {
        let (step, at) = match self.current {
            Some((step, at)) => (Some(step), Some(at)),
            None => (None, None),
        };
        self.record.push(BlockEntry::new(blocked, step, at));
    }

    fn on_step_end(&mut self, _session: &Session<'_>, _end: &StepEnd) {
        self.current = None;
    }
}

/// The pins a connection runs between, for the `near` counts.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ConnectionEnds {
    /// The bounding boxes of the start pins.
    pub start: Vec<IntBox>,
    /// The bounding boxes of the destination pins.
    pub dest: Vec<IntBox>,
    /// The `near` radius in board units.
    pub radius: i64,
}

impl ConnectionEnds {
    /// The pins of `start` and `dest` on `board`, with the radius
    /// [`NEAR_RADIUS_MM`].
    pub fn of(board: &BasicBoard, start: &BTreeSet<ItemId>, dest: &BTreeSet<ItemId>) -> Self {
        let boxes = |set: &BTreeSet<ItemId>| -> Vec<IntBox> {
            set.iter()
                .filter_map(|id| board.item(*id))
                .filter(|item| item.is_pin())
                .map(|item| item.bounding_box())
                .collect()
        };
        let upm = crate::quality::feasibility::units_per_mm(board);
        ConnectionEnds {
            start: boxes(start),
            dest: boxes(dest),
            radius: (NEAR_RADIUS_MM * upm).round() as i64,
        }
    }
}

/// True if `p` lies within `radius` of `b` (exact: the squares are `i128`).
fn within(p: IntPoint, b: &IntBox, radius: i64) -> bool {
    let gap = |v: i32, lo: i32, hi: i32| -> i128 {
        if v < lo {
            i128::from(lo) - i128::from(v)
        } else if v > hi {
            i128::from(v) - i128::from(hi)
        } else {
            0
        }
    };
    let dx = gap(p.x, b.ll.x, b.ur.x);
    let dy = gap(p.y, b.ll.y, b.ur.y);
    let r = i128::from(radius);
    dx * dx + dy * dy <= r * r
}

/// How one routed net blocked the search.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NetBlocking {
    pub net: i32,
    /// Entries whose blocking item is on this net.
    pub blocked: u32,
    /// Entries where this net is the only routed net at that step.
    pub unique: u32,
    /// Entries within the radius of a start pin.
    pub near_start: u32,
    /// Entries within the radius of a destination pin.
    pub near_dest: u32,
}

impl NetBlocking {
    /// The rank key, compared descending: `(tier, doubled score, near)`.
    /// Tier 2 is a net that is the sole blocker of every entry it blocks,
    /// scored by `unique`; tier 1 is scored by `unique + min(near, unique)
    /// + 0.5 · shared`.
    pub fn rank_key(&self) -> (u8, u64, u32) {
        let near = self.near_start + self.near_dest;
        if self.blocked > 0 && self.unique == self.blocked {
            return (2, 2 * u64::from(self.unique), near);
        }
        let shared = u64::from(self.blocked - self.unique);
        let unique = u64::from(self.unique);
        let near_unique = u64::from(near.min(self.unique));
        (1, 2 * (unique + near_unique) + shared, near)
    }
}

/// How many entries the record had, and of which kind.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Coverage {
    /// Entries in the record (after the duplicates are removed).
    pub entries: u32,
    /// Entries whose blocking item is routed.
    pub routed: u32,
    /// Entries whose blocking item is fixed.
    pub fixed: u32,
    /// Entries cut off by [`RECORD_CAP`].
    pub dropped: u32,
}

/// The routed nets in a connection's way.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Attribution {
    /// At most [`MAX_NETS`] nets, best-ranked first.
    pub nets: Vec<NetBlocking>,
    pub coverage: Coverage,
}

/// True if `id` is a routed item of `board`: a trace or via whose fixed
/// state is unfixed. A missing item is not routed.
pub fn is_routed(board: &BasicBoard, id: ItemId) -> bool {
    board.item(id).is_some_and(|item| {
        (item.is_trace() || item.is_via()) && item.fixed_state() == FixedState::Unfixed
    })
}

/// The attribution of `record` (sorted as [`FailureEntries::finish`]
/// leaves it) for a connection between `ends`, with items classified on
/// `board`.
pub fn attribute(board: &BasicBoard, record: &[BlockEntry], ends: &ConnectionEnds) -> Attribution {
    attribute_with(record, |entry| is_routed(board, entry.item), ends, 0)
}

/// [`attribute`] with the routed test `routed` and `dropped` entries cut
/// off before.
pub fn attribute_with(
    record: &[BlockEntry],
    routed: impl Fn(&BlockEntry) -> bool,
    ends: &ConnectionEnds,
    dropped: usize,
) -> Attribution {
    let mut coverage = Coverage {
        entries: to_u32(record.len()),
        dropped: to_u32(dropped),
        ..Coverage::default()
    };
    // The routed nets met at each step, for `unique`.
    let mut nets_at_step: BTreeMap<Option<u64>, BTreeSet<i32>> = BTreeMap::new();
    let routed_entries: Vec<&BlockEntry> = record
        .iter()
        .filter(|entry| {
            let is_routed = routed(entry);
            if is_routed {
                coverage.routed += 1;
            } else {
                coverage.fixed += 1;
            }
            is_routed && entry.net > 0
        })
        .collect();
    for entry in &routed_entries {
        nets_at_step
            .entry(entry.step)
            .or_default()
            .insert(entry.net);
    }
    let mut by_net: BTreeMap<i32, NetBlocking> = BTreeMap::new();
    for entry in routed_entries {
        let counts = by_net.entry(entry.net).or_insert(NetBlocking {
            net: entry.net,
            blocked: 0,
            unique: 0,
            near_start: 0,
            near_dest: 0,
        });
        counts.blocked += 1;
        // A block met before the first pop has no step to share.
        let sole =
            entry.step.is_none() || nets_at_step.get(&entry.step).is_some_and(|n| n.len() == 1);
        if sole {
            counts.unique += 1;
        }
        if let Some(at) = entry.at {
            if ends.start.iter().any(|b| within(at, b, ends.radius)) {
                counts.near_start += 1;
            }
            if ends.dest.iter().any(|b| within(at, b, ends.radius)) {
                counts.near_dest += 1;
            }
        }
    }
    let mut nets: Vec<NetBlocking> = by_net.into_values().collect();
    nets.sort_by(|a, b| b.rank_key().cmp(&a.rank_key()).then(a.net.cmp(&b.net)));
    nets.truncate(MAX_NETS);
    Attribution { nets, coverage }
}

fn to_u32(n: usize) -> u32 {
    u32::try_from(n).unwrap_or(u32::MAX)
}

/// How an analysis search ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum SearchEnd {
    /// The search ran out of rooms: the record is the whole frontier.
    Exhausted,
    /// The search reached the destination: the connection is routable on
    /// this board without ripping.
    Found,
    /// The analysis's own work limit stopped it.
    Cut,
    /// The search could not be set up (no control for the net, or nothing
    /// to connect).
    NotRun,
}

impl SearchEnd {
    /// The JSON name.
    pub fn name(self) -> &'static str {
        match self {
            SearchEnd::Exhausted => "exhausted",
            SearchEnd::Found => "found",
            SearchEnd::Cut => "cut",
            SearchEnd::NotRun => "not_run",
        }
    }
}

/// The result of [`analyze_connection`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Analysis {
    pub end: SearchEnd,
    /// The work units the search used.
    pub work: u64,
    pub attribution: Attribution,
    /// The sorted record ([`FailureEntries::finish`]).
    pub record: Vec<BlockEntry>,
}

/// Runs the Freerouting maze search from the connected set of `from` (net
/// `net`) to the rest of the net on a copy of `board`, with ripup off and
/// the via and trace costs of `params`, limited to `params.work_budget`
/// units, and attributes its record. `board` is not changed and its meter
/// does not count the search.
pub fn analyze_connection(
    board: &RoutingBoard,
    net: i32,
    from: ItemId,
    board_settings: &BoardRouterSettings,
    params: &ConnectionParams,
) -> Analysis {
    let not_run = Analysis {
        end: SearchEnd::NotRun,
        work: 0,
        attribution: Attribution::default(),
        record: Vec::new(),
    };
    // Every query runs on the copy: the board's own meter counts tree
    // queries too, and must not see the analysis.
    let mut copy = board.clone();
    let start = copy.connected_set(from, net, false);
    let dest = copy.unconnected_set(from, net);
    if start.is_empty() || dest.is_empty() {
        return not_run;
    }
    let Ok(mut control) = AutorouteControl::for_net(
        &copy,
        net,
        board_settings,
        params.via_cost,
        &params.trace_costs,
    ) else {
        return not_run;
    };
    control.ripup_allowed = false;
    control.ripup_costs = params.ripup_cost;
    control.remove_unconnected_vias = params.remove_unconnected_vias;
    let ends = ConnectionEnds::of(&copy, &start, &dest);

    let meter_start = copy.search_trees().work_meter().units();
    let limit = WorkLimit::with_site(
        copy.search_trees().work_meter(),
        (params.work_budget / UNITS_PER_NOMINAL_MS) as i64,
        ANALYSIS_SITE,
    );
    let limits = RouteLimits {
        settings: SearchSettings::from_router(board_settings),
        stop: None,
        work_limit: Some(limit.clone()),
    };
    let mut ripped = BTreeSet::new();
    let mut costs = RipupCosts::new();
    let ripped = Ripped {
        items: &mut ripped,
        costs: Some(&mut costs),
    };
    let (result, sink): (AttemptResult, RecordingSink) = route_connection(
        &mut copy,
        &start,
        &dest,
        &control,
        limits,
        ripped,
        RecordingSink::default(),
    );
    let meter = copy.search_trees().work_meter();
    let work = meter.units().wrapping_sub(meter_start);
    let end = if limit.exceeded(meter) {
        SearchEnd::Cut
    } else if result.state == AttemptState::Failed {
        SearchEnd::Exhausted
    } else {
        SearchEnd::Found
    };
    let (record, dropped) = sink.record.finish();
    // The items are classified on `board`: a found path was inserted on
    // the copy, which may have split its traces.
    let attribution = attribute_with(&record, |e| is_routed(board, e.item), &ends, dropped);
    Analysis {
        end,
        work,
        attribution,
        record,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(net: i32, item: i32, step: u64, at: (i32, i32)) -> BlockEntry {
        BlockEntry {
            net,
            item: ItemId(item),
            layer: 0,
            reason: BlockReason::NotRippable,
            step: Some(step),
            at: Some(IntPoint { x: at.0, y: at.1 }),
        }
    }

    fn far_ends() -> ConnectionEnds {
        ConnectionEnds {
            start: vec![IntBox {
                ll: IntPoint { x: 0, y: 0 },
                ur: IntPoint { x: 10, y: 10 },
            }],
            dest: vec![IntBox {
                ll: IntPoint { x: 1000, y: 0 },
                ur: IntPoint { x: 1010, y: 10 },
            }],
            radius: 30,
        }
    }

    #[test]
    fn a_single_blocking_net_is_unique_everywhere() {
        let record = [
            entry(4, 10, 1, (500, 0)),
            entry(4, 11, 2, (20, 0)),
            entry(4, 11, 3, (990, 5)),
        ];
        let a = attribute_with(&record, |_| true, &far_ends(), 0);
        assert_eq!(a.nets.len(), 1);
        let n = a.nets[0];
        assert_eq!((n.net, n.blocked, n.unique), (4, 3, 3));
        assert_eq!((n.near_start, n.near_dest), (1, 1));
        assert_eq!(
            a.coverage,
            Coverage {
                entries: 3,
                routed: 3,
                fixed: 0,
                dropped: 0
            }
        );
    }

    #[test]
    fn two_nets_sharing_every_door_have_no_unique_entries() {
        let mut record = Vec::new();
        for step in 0..4 {
            record.push(entry(2, 20, step, (500, 500)));
            record.push(entry(3, 30, step, (500, 500)));
        }
        record.sort_by_key(BlockEntry::key);
        let a = attribute_with(&record, |_| true, &far_ends(), 0);
        assert_eq!(a.nets.len(), 2);
        for n in &a.nets {
            assert_eq!((n.blocked, n.unique), (4, 0), "net {}", n.net);
        }
        // Equal keys: ties by net number.
        assert_eq!(a.nets[0].net, 2);
        assert_eq!(a.nets[1].net, 3);
    }

    #[test]
    fn fixed_items_count_in_coverage_but_name_no_net() {
        let record = [entry(5, 50, 0, (0, 0)), entry(6, 60, 0, (0, 0))];
        let a = attribute_with(&record, |e| e.net == 6, &far_ends(), 2);
        assert_eq!(
            a.coverage,
            Coverage {
                entries: 2,
                routed: 1,
                fixed: 1,
                dropped: 2
            }
        );
        // Net 5's pad does not make net 6 shared.
        assert_eq!(a.nets.len(), 1);
        assert_eq!((a.nets[0].net, a.nets[0].unique), (6, 1));
    }

    #[test]
    fn the_cap_keeps_the_ten_best_ranked_nets() {
        // Net n blocks n steps alone (unique == blocked, tier 2) for n in
        // 1..=12, and nets 20 and 21 share 50 steps (tier 1).
        let mut record = Vec::new();
        let mut step = 0;
        for net in 1..=12 {
            for _ in 0..net {
                record.push(entry(net, 100 + net, step, (500, 500)));
                step += 1;
            }
        }
        for _ in 0..50 {
            record.push(entry(20, 200, step, (500, 500)));
            record.push(entry(21, 210, step, (500, 500)));
            step += 1;
        }
        record.sort_by_key(BlockEntry::key);
        let a = attribute_with(&record, |_| true, &far_ends(), 0);
        let nets: Vec<i32> = a.nets.iter().map(|n| n.net).collect();
        // Sole blockers first, best first; the shared pair falls off.
        assert_eq!(nets, vec![12, 11, 10, 9, 8, 7, 6, 5, 4, 3]);
    }

    #[test]
    fn tier_one_score_counts_shared_entries_half() {
        let a = NetBlocking {
            net: 1,
            blocked: 6,
            unique: 2,
            near_start: 1,
            near_dest: 0,
        };
        // 2 + min(1, 2) + 0.5 * 4 = 5, doubled 10.
        assert_eq!(a.rank_key(), (1, 10, 1));
        let sole = NetBlocking {
            net: 2,
            blocked: 1,
            unique: 1,
            near_start: 0,
            near_dest: 0,
        };
        assert!(sole.rank_key() > a.rank_key());
    }

    #[test]
    fn near_uses_the_exact_distance_to_the_pin_box() {
        let b = IntBox {
            ll: IntPoint { x: 0, y: 0 },
            ur: IntPoint { x: 10, y: 10 },
        };
        assert!(within(IntPoint { x: 13, y: 14 }, &b, 5));
        assert!(!within(IntPoint { x: 14, y: 14 }, &b, 5));
        // At the coordinate limits the squares stay exact.
        let far = IntPoint {
            x: i32::MAX,
            y: i32::MIN,
        };
        assert!(!within(far, &b, i64::from(i32::MAX)));
        assert!(within(far, &b, i64::MAX / 2));
    }
}

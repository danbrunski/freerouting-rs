//! Unconnected items: nets split into several groups, dangling traces and
//! vias.
//!
//! Port of `drc.UnconnectedItems` and
//! `DesignRulesChecker.getAllUnconnectedItems`.

use std::collections::BTreeSet;

use crate::util::{HashMap, HashSet};

use super::kicad_report::{self, ReportRules};
use crate::board::BasicBoard;
use crate::board::items::ItemId;
use crate::util::java_hash_map_order;

/// The kind of an [`UnconnectedItems`] finding.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UnconnectedKind {
    /// A net whose items form two or more groups.
    UnconnectedItems,
    /// A trace with an end without contact.
    TrackDangling,
    /// A via with at most one contact or contacts on one layer only.
    ViaDangling,
}

impl UnconnectedKind {
    /// The Java type string of the finding.
    pub fn type_name(self) -> &'static str {
        match self {
            UnconnectedKind::UnconnectedItems => "unconnectedItems",
            UnconnectedKind::TrackDangling => "track_dangling",
            UnconnectedKind::ViaDangling => "via_dangling",
        }
    }
}

/// Items that should be connected but are not.
///
/// Port of `drc.UnconnectedItems`. For a split net `first_item` and
/// `second_item` represent the first two groups and `all_items` holds the
/// items of both; a dangling trace or via is `first_item` alone.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnconnectedItems {
    pub first_item: ItemId,
    pub second_item: Option<ItemId>,
    pub all_items: Vec<ItemId>,
    pub kind: UnconnectedKind,
}

/// Port of `DesignRulesChecker.getAllUnconnectedItems`.
///
/// The nets are visited in the iteration order of Java's
/// `HashMap<Integer, ...>`. Java collects each group in a `HashSet`, whose
/// order (and so the representative items and the item order) depends on
/// identity hash codes; here a group is ordered by item id, highest first,
/// and the representative is its first pin, else its first trace, else its
/// first item.
///
/// Under [`ReportRules::Kicad`] (K15.5, not Java) the dangling traces and vias
/// are KiCad's ([`kicad_report::track_dangling`], [`kicad_report::via_dangling`]);
/// the split nets are Java's under both.
pub(crate) fn all_unconnected_items(
    board: &BasicBoard,
    contacts: &mut HashMap<ItemId, Vec<ItemId>>,
    rules: ReportRules,
) -> Vec<UnconnectedItems> {
    let mut result: Vec<UnconnectedItems> = Vec::new();
    let mut items_by_net: HashMap<i32, Vec<ItemId>> = HashMap::default();
    let mut net_order: Vec<i32> = Vec::new();
    for item in board.items() {
        if item.is_connectable_kind() && item.net_count() > 0 {
            let net = item.net_number(0);
            items_by_net
                .entry(net)
                .or_insert_with(|| {
                    net_order.push(net);
                    Vec::new()
                })
                .push(item.id());
        }
    }
    for net in java_hash_map_order(&net_order) {
        let net_items = &items_by_net[&net];
        if net_items.len() <= 1 {
            continue;
        }
        let net_item_set: HashSet<ItemId> = net_items.iter().copied().collect();
        let mut connected_sets: Vec<BTreeSet<ItemId>> = Vec::new();
        let mut processed: HashSet<ItemId> = HashSet::default();
        for id in net_items {
            if processed.contains(id) {
                continue;
            }
            let set: BTreeSet<ItemId> = board
                .connected_set_cached(*id, net, false, contacts)
                .into_iter()
                .filter(|i| net_item_set.contains(i))
                .collect();
            if !set.is_empty() {
                processed.extend(set.iter().copied());
                connected_sets.push(set);
            }
            if connected_sets.len() >= 2 {
                // Only the first two groups are reported.
                break;
            }
        }
        if connected_sets.len() >= 2 {
            let (Some(first), Some(second)) = (
                representative_item(board, &connected_sets[0]),
                representative_item(board, &connected_sets[1]),
            ) else {
                continue;
            };
            let mut all_items: Vec<ItemId> = connected_sets[0].iter().copied().collect();
            all_items.extend(connected_sets[1].iter().copied());
            result.push(UnconnectedItems {
                first_item: first,
                second_item: Some(second),
                all_items,
                kind: UnconnectedKind::UnconnectedItems,
            });
        }
    }

    for item in board.items() {
        if !item.is_trace() {
            continue;
        }
        let id = item.id();
        let dangling = match rules {
            ReportRules::Java => {
                board.trace_start_contacts(id).is_empty() || board.trace_end_contacts(id).is_empty()
            }
            ReportRules::Kicad => kicad_report::track_dangling(board, id),
        };
        if dangling && !result.iter().any(|u| u.first_item == id) {
            result.push(UnconnectedItems {
                first_item: id,
                second_item: None,
                all_items: vec![id],
                kind: UnconnectedKind::TrackDangling,
            });
        }
    }
    for item in board.items() {
        let dangling = |id| match rules {
            ReportRules::Java => board.is_tail(id),
            ReportRules::Kicad => kicad_report::via_dangling(board, id),
        };
        if item.is_via() && dangling(item.id()) {
            result.push(UnconnectedItems {
                first_item: item.id(),
                second_item: None,
                all_items: vec![item.id()],
                kind: UnconnectedKind::ViaDangling,
            });
        }
    }
    result
}

/// Java `findRepresentativeItem`: a pin, else a trace, else any item.
fn representative_item(board: &BasicBoard, set: &BTreeSet<ItemId>) -> Option<ItemId> {
    let find = |pred: fn(&crate::board::items::Item) -> bool| {
        set.iter()
            .copied()
            .find(|id| board.item(*id).is_some_and(pred))
    };
    find(|i| i.is_pin())
        .or_else(|| find(|i| i.is_trace()))
        .or_else(|| set.first().copied())
}

//! Q16: the final stub cleanup (`router.cleanup.remove_stubs`).
//!
//! A connection the router started and never finished can leave copper on
//! the board that connects nothing: a via pair with a trace between them, a
//! trace ending in the open. KiCad's DRC reports such copper as
//! `via_dangling` and `track_dangling`. Java's rip-up removes the unconnected
//! vias of the connections it rips (`removeUnconnectedVias`), but nothing
//! removes the stubs still on the board when the run ends: a connection that
//! failed in the last pass, the sweep's partial inserts (Q10), the
//! optimizer's leftovers. With the setting on, [`run_pipeline`] calls
//! [`remove_stubs`] once, after the last routing or optimizer step and the
//! board stages of that phase.
//!
//! **A stub** ([`is_removable_stub`]) is a trace or via that
//! - the router inserted during this run: [`run_pipeline`] takes the ids of
//!   the traces and vias on the board as loaded ([`input_copper`]) before
//!   routing starts, and none of them is ever removed, locked or not, so a
//!   designer's unfinished hand-laid track stays. Item ids only count up
//!   (`ItemIdGenerator`), so an id of the input never names router copper;
//!   a piece the router split off an input trace has a new id and counts as
//!   the router's;
//! - is unfixed (`Item.isRoutable`, so `type fix` and `type protect` copper
//!   stays), of exactly one net, and its deletion is not forbidden;
//! - connects fewer than two of its net's items, counted as KiCad counts
//!   them ([`stub_contacts`], Q16.4): a trace with an end whose cap meets
//!   no other copper of its net (KiCad's `track_dangling`), a via whose pad
//!   meets copper of its net on at most one layer (KiCad's `via_dangling`).
//!   Copper meets copper wherever their shapes intersect on a layer, by the
//!   board's exact shape intersection: a via on a trace's body, a trace end
//!   on another trace's body, a via off a pad's centre all touch. Java's
//!   normal contacts (an end corner at a via's centre, a common end corner)
//!   are a subset; Q16.3 counted those alone and removed a via that sat on
//!   a fixed pour strip, which KiCad counted as a connection. A plane of
//!   the net is a contact, so a via that stitches a trace to its plane
//!   touches it on two layers and stays. **Known limit:** a plane is its
//!   DSN outline, not the zone fill KiCad pours inside it, so a trace that
//!   ends inside the outline where KiCad's fill does not reach stays, and
//!   KiCad reports it dangling (Q16.3, CM5_MINIMA_3);
//! - whose removal joins nothing apart: its contacts all touch one another
//!   (their copper meets). Without that guard a via whose two traces end on
//!   one layer at different points of its pad would be removed and split
//!   them. Such a via stays, dangling.
//!
//! So the cleanup removes only the router's copper, and only copper whose
//! removal cannot make a complete connection incomplete: the unconnected
//! count cannot rise.
//!
//! **Order.** The first round looks at every routed item in item-id order;
//! each later round looks at the routed contacts of the items the round
//! before removed, again in id order. Each item is judged on the board as it
//! is when its turn comes, so a chain of stubs goes in one call, one link
//! after the other. The rounds end when one removes nothing. Nothing reads a
//! hash order, so the same board gives the same result.
//!
//! **Stops.** The cleanup runs whenever the setting is on, also after a
//! timeout or a cancel: it only removes copper, and its work is bounded by a
//! few contact queries per item.
//!
//! [`run_pipeline`]: crate::autoroute::pipeline::run_pipeline

use std::collections::BTreeSet;

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::quality::stub_contacts;
use crate::settings::RouterSettings;

/// The settings key, without `router.`.
pub const SETTINGS_KEY: &str = "cleanup.remove_stubs";

/// True if `router.cleanup.remove_stubs` is on (unset is off).
pub fn enabled(settings: &RouterSettings) -> bool {
    settings.cleanup.remove_stubs == Some(true)
}

/// What one [`remove_stubs`] call removed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StubCleanup {
    /// Vias removed.
    pub vias: usize,
    /// Traces removed.
    pub traces: usize,
    /// Rounds run, the last of which removed nothing.
    pub rounds: usize,
}

impl StubCleanup {
    /// Items removed.
    pub fn removed(&self) -> usize {
        self.vias + self.traces
    }

    /// The job log line.
    pub fn log_line(&self) -> String {
        format!(
            "Stub cleanup: removed {} dangling via(s) and {} trace stub(s) in {} round(s)",
            self.vias, self.traces, self.rounds
        )
    }
}

/// The ids of the traces and vias on `board`: what the cleanup keeps when
/// taken before routing starts.
pub fn input_copper(board: &BasicBoard) -> BTreeSet<ItemId> {
    board
        .items()
        .filter(|item| item.is_trace() || item.is_via())
        .map(Item::id)
        .collect()
}

/// True if item `id` is a stub the cleanup may remove: not in `keep` (the
/// [`input_copper`] of the run) and a stub (see the module docs).
pub fn is_removable_stub(board: &BasicBoard, id: ItemId, keep: &BTreeSet<ItemId>) -> bool {
    if keep.contains(&id) {
        return false;
    }
    let Some(item) = board.item(id) else {
        return false;
    };
    if !item.is_routable() || item.net_count() != 1 || board.is_deletion_forbidden(item) {
        return false;
    }
    let dangling = match &item.kind {
        ItemKind::Trace(trace) => {
            !stub_contacts::end_has_contact(board, item, &trace.first_corner())
                || !stub_contacts::end_has_contact(board, item, &trace.last_corner())
        }
        ItemKind::Via(_) => stub_contacts::via_contact_layers(board, item).len() <= 1,
        _ => false,
    };
    dangling && contacts_touch_each_other(board, &stub_contacts::overlap_contacts(board, id))
}

/// True if the copper of every two of `contacts` meets, so that removing
/// the item they all touch keeps them joined.
fn contacts_touch_each_other(board: &BasicBoard, contacts: &BTreeSet<ItemId>) -> bool {
    let items: Vec<&Item> = contacts.iter().filter_map(|id| board.item(*id)).collect();
    items.iter().enumerate().all(|(i, a)| {
        items[i + 1..]
            .iter()
            .all(|b| stub_contacts::touches(board, a, b))
    })
}

/// Removes every stub of `board` that is not in `keep`, round after round
/// until a round removes nothing (see the module docs).
pub fn remove_stubs(board: &mut BasicBoard, keep: &BTreeSet<ItemId>) -> StubCleanup {
    let mut summary = StubCleanup::default();
    let mut pending: BTreeSet<ItemId> = board
        .items()
        .filter(|item| item.is_routable() && !keep.contains(&item.id()))
        .map(Item::id)
        .collect();
    while !pending.is_empty() {
        summary.rounds += 1;
        let round = std::mem::take(&mut pending);
        for id in round {
            if !is_removable_stub(board, id, keep) {
                continue;
            }
            let contacts = stub_contacts::overlap_contacts(board, id);
            let is_via = board.item(id).is_some_and(Item::is_via);
            if !board.remove_item(id) {
                continue;
            }
            if is_via {
                summary.vias += 1;
            } else {
                summary.traces += 1;
            }
            pending.extend(
                contacts
                    .into_iter()
                    .filter(|c| board.item(*c).is_some_and(Item::is_routable)),
            );
        }
    }
    summary
}

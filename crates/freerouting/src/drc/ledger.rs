//! The routing ledger: incremental incomplete-connection counts (B01.6).
//!
//! Port of `drc.NetRoutingLedger`. The first read builds the ledger from a
//! full scan; later changes mark the nets they touch dirty, and a read
//! recounts only the dirty nets with the same per-net count the
//! [`DesignRulesChecker`](super::DesignRulesChecker) uses
//! ([`NetIncompletes`]). The pass loop, the optimizer and
//! [`BoardStatistics`](crate::scoring::BoardStatistics) read it.
//!
//! The ledger counts exactly (B00 open question 1, decided 2026-10-15).
//! Java's ledger hears only `BoardItemRepository.insertItem/removeItem` and
//! so goes stale on undo and redo, fast-cutout pieces, in-place trace
//! changes and net changes. The board marks the ledger on every change that
//! can change a count: insert, remove, undo and redo (the cancelled and the
//! restored versions), trace changes and combines, net changes, fixed-state
//! and conduction changes, and `BasicBoard::note_changed` for callers that
//! change an item in place. `delete_all_traces_and_vias` and a snapshot
//! restore leave it unbuilt.
//!
//! The ledger lives in the board behind a [`Mutex`], because a read recounts
//! the dirty nets through a shared reference and the board must stay `Sync`.

use std::collections::BTreeSet;
use std::sync::Mutex;

use super::incompletes::{NetIncompletes, max_connections, net_item_lists};
use crate::board::BasicBoard;
use crate::board::items::ItemId;

#[derive(Clone, Debug, Default)]
struct LedgerState {
    built: bool,
    /// The connectable items per net; index 0 is net 1.
    items_by_net: Vec<BTreeSet<ItemId>>,
    incomplete_by_net: Vec<usize>,
    incomplete_total: usize,
    /// Per net its pins and conduction areas minus one; index 0 is net 1.
    max_by_net: Vec<usize>,
    maximum_connections: usize,
    dirty_nets: BTreeSet<i32>,
}

impl LedgerState {
    fn slot(&mut self, net: i32) -> Option<&mut BTreeSet<ItemId>> {
        if net < 1 {
            return None;
        }
        self.items_by_net.get_mut(net as usize - 1)
    }
}

/// Incremental incomplete-connection counts of a board.
#[derive(Debug, Default)]
pub struct RoutingLedger {
    state: Mutex<LedgerState>,
}

impl Clone for RoutingLedger {
    fn clone(&self) -> Self {
        RoutingLedger {
            state: Mutex::new(self.lock().clone()),
        }
    }
}

impl RoutingLedger {
    fn lock(&self) -> std::sync::MutexGuard<'_, LedgerState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn state_mut(&mut self) -> &mut LedgerState {
        self.state.get_mut().unwrap_or_else(|e| e.into_inner())
    }

    /// True once a read has built the ledger.
    pub fn is_built(&self) -> bool {
        self.lock().built
    }

    /// Drops the cache; the next read rebuilds it (Java `invalidate`).
    pub(crate) fn invalidate(&mut self) {
        *self.state_mut() = LedgerState::default();
    }

    /// Records that connectable item `id` was added to `nets`.
    pub(crate) fn note_inserted(&mut self, id: ItemId, nets: &[i32]) {
        let state = self.state_mut();
        if !state.built {
            return;
        }
        for &net in nets {
            if let Some(set) = state.slot(net) {
                set.insert(id);
                state.dirty_nets.insert(net);
            }
        }
    }

    /// Records that connectable item `id` left `nets`.
    pub(crate) fn note_removed(&mut self, id: ItemId, nets: &[i32]) {
        let state = self.state_mut();
        if !state.built {
            return;
        }
        for &net in nets {
            if let Some(set) = state.slot(net) {
                set.remove(&id);
                state.dirty_nets.insert(net);
            }
        }
    }

    /// Marks `nets` for a recount (an item of them changed in place).
    pub(crate) fn mark_dirty(&mut self, nets: &[i32]) {
        let state = self.state_mut();
        if !state.built {
            return;
        }
        for &net in nets {
            if net >= 1 && (net as usize) <= state.items_by_net.len() {
                state.dirty_nets.insert(net);
            }
        }
    }

    /// The total incomplete-connection count, recounting dirty nets first
    /// (Java `incompleteCount`).
    pub fn incomplete_count(&self, board: &BasicBoard) -> usize {
        let mut state = self.lock();
        Self::ensure_built(&mut state, board);
        Self::flush_dirty(&mut state, board);
        state.incomplete_total
    }

    /// Per net the pins and conduction areas minus one, summed (Java
    /// `maximumConnections`).
    pub fn maximum_connections(&self, board: &BasicBoard) -> usize {
        let mut state = self.lock();
        Self::ensure_built(&mut state, board);
        Self::flush_dirty(&mut state, board);
        state.maximum_connections
    }

    /// The nets with at least one incomplete connection, ascending (Java
    /// `incompleteNetNumbers`).
    pub fn incomplete_net_numbers(&self, board: &BasicBoard) -> BTreeSet<i32> {
        let mut state = self.lock();
        Self::ensure_built(&mut state, board);
        Self::flush_dirty(&mut state, board);
        state
            .incomplete_by_net
            .iter()
            .enumerate()
            .filter(|(_, count)| **count > 0)
            .map(|(i, _)| i as i32 + 1)
            .collect()
    }

    fn ensure_built(state: &mut LedgerState, board: &BasicBoard) {
        if state.built {
            return;
        }
        let lists = net_item_lists(board);
        state.max_by_net = lists
            .iter()
            .map(|items| max_connections(board, std::slice::from_ref(items)))
            .collect();
        state.maximum_connections = state.max_by_net.iter().sum();
        state.incomplete_by_net = lists
            .iter()
            .enumerate()
            .map(|(i, items)| NetIncompletes::new(board, i as i32 + 1, items).count())
            .collect();
        state.incomplete_total = state.incomplete_by_net.iter().sum();
        state.items_by_net = lists
            .into_iter()
            .map(|items| items.into_iter().collect())
            .collect();
        state.dirty_nets.clear();
        state.built = true;
    }

    fn flush_dirty(state: &mut LedgerState, board: &BasicBoard) {
        if state.dirty_nets.is_empty() {
            return;
        }
        let dirty = std::mem::take(&mut state.dirty_nets);
        for net in dirty {
            let index = net as usize - 1;
            let items: Vec<ItemId> = state.items_by_net[index].iter().copied().collect();
            let count = NetIncompletes::new(board, net, &items).count();
            state.incomplete_total =
                state.incomplete_total - state.incomplete_by_net[index] + count;
            state.incomplete_by_net[index] = count;
            // Java recounts the endpoints of every net here; only the dirty
            // nets can have changed.
            let max = max_connections(board, std::slice::from_ref(&items));
            state.maximum_connections = state.maximum_connections - state.max_by_net[index] + max;
            state.max_by_net[index] = max;
        }
    }
}

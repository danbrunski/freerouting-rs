//! The board change journal (B01.7).
//!
//! Replaces Java's board observers and graphics update box
//! (`BasicBoard.java:1255-1295`): a consumer such as the GUI turns it on with
//! [`BasicBoard::set_journaling`] and patches its view from
//! [`BasicBoard::take_changes`]. The journal records at the board's own choke
//! points (insert, remove, every in-place change, undo and redo, net,
//! clearance-class, fixed-state and conduction changes), so it can't miss a
//! change. It is off by default and costs one branch per change when off.
//!
//! [`BoardChanges::dirty`] is the union of the bounding boxes of the affected
//! items, before and after each change. Past [`JOURNAL_CAP`] records the log
//! is dropped and [`BoardChanges::overflow`] set: the consumer rebuilds.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{IntBox, PolylineShapeOps};

use crate::board::basic_board::BasicBoard;
use crate::board::items::ItemId;

/// The number of records after which the journal gives up and reports an
/// overflow.
pub const JOURNAL_CAP: usize = 100_000;

/// What changed since the last [`BasicBoard::take_changes`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct BoardChanges {
    /// Items that exist now and did not before, highest id first.
    pub inserted: Vec<ItemId>,
    /// Items that existed before and are gone now, highest id first.
    pub removed: Vec<ItemId>,
    /// Items that existed before and still exist, changed, highest id first.
    pub changed: Vec<ItemId>,
    /// The nets of the affected items, before and after.
    pub changed_nets: BTreeSet<i32>,
    /// The union of the bounding boxes of the affected items, before and
    /// after each change; `None` if nothing with an extent changed.
    pub dirty: Option<IntBox>,
    /// True if the log passed [`JOURNAL_CAP`] records and was dropped; the
    /// consumer must rebuild its view from the board.
    pub overflow: bool,
}

impl BoardChanges {
    /// True if nothing was recorded.
    pub fn is_empty(&self) -> bool {
        self.inserted.is_empty()
            && self.removed.is_empty()
            && self.changed.is_empty()
            && !self.overflow
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Record {
    Inserted,
    Removed,
    Changed,
}

/// The change log of a board while journaling.
#[derive(Clone, Debug, Default)]
pub(crate) struct Journal {
    records: Vec<(ItemId, Record)>,
    changed_nets: BTreeSet<i32>,
    dirty: Option<IntBox>,
    overflow: bool,
    cap: usize,
}

impl Journal {
    pub(crate) fn new(cap: usize) -> Self {
        Journal {
            cap,
            ..Journal::default()
        }
    }

    pub(crate) fn record(&mut self, id: ItemId, record: Record, nets: &[i32], bounds: IntBox) {
        if self.overflow {
            return;
        }
        if self.records.len() >= self.cap {
            self.records = Vec::new();
            self.changed_nets.clear();
            self.dirty = None;
            self.overflow = true;
            return;
        }
        self.records.push((id, record));
        self.changed_nets.extend(nets.iter().copied());
        if !bounds.is_empty() {
            self.dirty = Some(match self.dirty {
                Some(dirty) => dirty.union_box(&bounds),
                None => bounds,
            });
        }
    }

    /// The log reduced to the final state per id, and a fresh log.
    pub(crate) fn take(&mut self) -> BoardChanges {
        let journal = std::mem::replace(self, Journal::new(self.cap));
        // Per id: did it exist before the first record, does it exist now.
        let mut states: BTreeMap<ItemId, (bool, bool)> = BTreeMap::new();
        for (id, record) in journal.records {
            let exists_now = record != Record::Removed;
            states
                .entry(id)
                .and_modify(|state| state.1 = exists_now)
                .or_insert((record != Record::Inserted, exists_now));
        }
        let mut changes = BoardChanges {
            changed_nets: journal.changed_nets,
            dirty: journal.dirty,
            overflow: journal.overflow,
            ..BoardChanges::default()
        };
        for (id, state) in states {
            match state {
                (false, true) => changes.inserted.push(id),
                (true, false) => changes.removed.push(id),
                (true, true) => changes.changed.push(id),
                (false, false) => {}
            }
        }
        changes
    }
}

impl BasicBoard {
    /// Turns the change journal on or off. Turning it on starts an empty
    /// log; turning it off drops the log.
    pub fn set_journaling(&mut self, on: bool) {
        self.set_journaling_with_cap(on, JOURNAL_CAP);
    }

    /// [`set_journaling`](Self::set_journaling) with a record cap other
    /// than [`JOURNAL_CAP`].
    pub fn set_journaling_with_cap(&mut self, on: bool, cap: usize) {
        self.journal = on.then(|| Box::new(Journal::new(cap)));
    }

    /// True while the journal records.
    pub fn is_journaling(&self) -> bool {
        self.journal.is_some()
    }

    /// The changes since journaling started or since the last call; empty
    /// if journaling is off.
    pub fn take_changes(&mut self) -> BoardChanges {
        self.journal
            .as_mut()
            .map(|journal| journal.take())
            .unwrap_or_default()
    }
}

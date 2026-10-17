//! Insert, check, commit or revert (B09.1).
//!
//! A board transaction for algorithm stages (D9): a stage changes the board
//! in a closure, the transaction checks the result with the clearance check
//! and keeps it only if it is clean. It is Java's strict-DRC rule
//! (`AutorouteConnectionRouter.java:88-94`, `enforceStrictDrc`) as a
//! service: note `maxGeneratedId` before the change, then check every item
//! with a higher id.
//!
//! [`RoutingBoard::try_change`]:
//! 1. notes the id generator's `max_generated_id` and runs
//!    [`generate_snapshot`](BasicBoard::generate_snapshot);
//! 2. runs the closure; an `Err` reverts at once ([`TransactionResult::Aborted`]);
//! 3. checks the items with an id above the noted one that are on the board
//!    after the change, and with [`CheckPolicy::NewAndChangedItems`] also
//!    the items the change journal (B01.7) reports as changed or restored,
//!    with [`clearance_violations`](BasicBoard::clearance_violations);
//! 4. no violation: [`pop_snapshot`](BasicBoard::pop_snapshot), the change
//!    is merged into the enclosing undo level ([`TransactionResult::Committed`]);
//!    else [`undo`](BasicBoard::undo) ([`TransactionResult::Reverted`], with
//!    the violations).
//!
//! Semantics kept from Java's undo (B01.8):
//! - the id generator and the trace half-width bounds are not rolled back:
//!   ids taken inside a reverted attempt stay taken. A stage that needs
//!   untouched ids works on a [`deep_copy`](RoutingBoard::deep_copy);
//! - after a revert the undone change can be redone, as after any undo; the
//!   next `generate_snapshot` (or `pop_snapshot`) drops the redo data;
//! - an enclosing snapshot is untouched: a revert undoes only the
//!   transaction's own level.
//!
//! Determinism (D2): the checked ids are visited highest first, and the
//! violations are sorted by (first item, second item) in item order
//! (highest id first), then by layer; violations with equal keys keep the
//! order the clearance check found them in. The clearance queries count
//! work units (B02.7) like every tree query.
//!
//! With [`CheckPolicy::NewAndChangedItems`] the transaction journals its own
//! change. A journal the caller had running is set aside for the closure and
//! then gets the transaction's changes (reduced per item, which is what its
//! `take_changes` reports anyway) and the revert's own records.

use std::collections::BTreeSet;

use freerouting_geometry::IntBox;

use crate::board::BasicBoard;
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::journal::{Journal, Record};
use crate::datastructures::IdGenerator;
use crate::drc::ClearanceViolation;

/// Which items a transaction checks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckPolicy {
    /// The items with an id above the one noted before the change (Java's
    /// strict-DRC rule).
    NewItemsOnly,
    /// The new items and the older items the change journal reports as
    /// changed or restored (net, clearance class or fixed-state changes in
    /// place, undo inside the closure).
    NewAndChangedItems,
}

/// The outcome of [`RoutingBoard::try_change`].
#[derive(Debug)]
pub enum TransactionResult<T, E> {
    /// The change was clean and is kept.
    Committed(T),
    /// The change had clearance violations and was undone.
    Reverted {
        /// The closure's value.
        value: T,
        /// The violations of the checked items, sorted (module docs).
        violations: Vec<ClearanceViolation>,
    },
    /// The closure returned an error; its change was undone unchecked.
    Aborted(E),
}

impl<T, E> TransactionResult<T, E> {
    /// True if the change is kept.
    pub fn is_committed(&self) -> bool {
        matches!(self, TransactionResult::Committed(_))
    }
}

impl RoutingBoard {
    /// Runs `change` on the board and keeps its result only if no checked
    /// item has a clearance violation (module docs).
    pub fn try_change<T, E>(
        &mut self,
        change: impl FnOnce(&mut RoutingBoard) -> Result<T, E>,
        check: CheckPolicy,
    ) -> TransactionResult<T, E> {
        let max_id_before = self.communication.id_generator.max_generated_id();
        let outer_journal = match check {
            CheckPolicy::NewItemsOnly => None,
            CheckPolicy::NewAndChangedItems => Some(
                self.journal
                    .replace(Box::new(Journal::new(usize::MAX)))
                    .map(|journal| *journal),
            ),
        };
        self.generate_snapshot();
        let result = change(self);
        let changed = outer_journal.map(|outer| self.restore_journal(outer));
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                self.undo(None);
                return TransactionResult::Aborted(error);
            }
        };
        let violations = self.violations_of_changed(max_id_before, changed.as_ref());
        if violations.is_empty() {
            self.pop_snapshot();
            TransactionResult::Committed(value)
        } else {
            self.undo(None);
            TransactionResult::Reverted { value, violations }
        }
    }
}

impl BasicBoard {
    /// Ends the transaction's journal, puts `outer` back and replays the
    /// transaction's changes into it. Returns the ids the journal reports
    /// as changed or inserted.
    fn restore_journal(&mut self, outer: Option<Journal>) -> BTreeSet<ItemId> {
        let changes = self
            .journal
            .take()
            .map(|mut journal| journal.take())
            .unwrap_or_default();
        self.journal = outer.map(Box::new);
        if let Some(journal) = self.journal.as_mut() {
            let nets: Vec<i32> = changes.changed_nets.iter().copied().collect();
            let mut extra = Some((nets, changes.dirty.unwrap_or(IntBox::EMPTY)));
            let records = changes
                .inserted
                .iter()
                .map(|id| (*id, Record::Inserted))
                .chain(changes.removed.iter().map(|id| (*id, Record::Removed)))
                .chain(changes.changed.iter().map(|id| (*id, Record::Changed)));
            for (id, record) in records {
                match extra.take() {
                    Some((nets, dirty)) => journal.record(id, record, &nets, dirty),
                    None => journal.record(id, record, &[], IntBox::EMPTY),
                }
            }
        }
        changes
            .changed
            .into_iter()
            .chain(changes.inserted)
            .collect()
    }

    /// The sorted clearance violations of the items above `max_id_before`
    /// and of `changed` that are on the board.
    fn violations_of_changed(
        &self,
        max_id_before: i32,
        changed: Option<&BTreeSet<ItemId>>,
    ) -> Vec<ClearanceViolation> {
        let mut ids: BTreeSet<ItemId> = self
            .items
            .keys()
            .take_while(|id| id.0 > max_id_before)
            .copied()
            .collect();
        if let Some(changed) = changed {
            ids.extend(changed.iter().filter(|id| self.items.contains(id)));
        }
        let mut violations: Vec<ClearanceViolation> = ids
            .into_iter()
            .flat_map(|id| self.clearance_violations(id))
            .collect();
        violations.sort_by(|a, b| {
            (a.first_item, a.second_item, a.layer).cmp(&(b.first_item, b.second_item, b.layer))
        });
        violations
    }
}

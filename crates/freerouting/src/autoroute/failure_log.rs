//! Routing failures per item.
//!
//! Port of `autoroute/RoutingFailureLog.java` at `cda540f`. The pass runner
//! records every failed connection attempt of an item; from the second
//! failure on it rips the item's net (A11), and an item that failed
//! [`FAILURE_THRESHOLD`] times can be skipped.
//!
//! The log is a field of the [`RoutingBoard`] (Java
//! `RoutingBoard.failureLog`, `RoutingBoard.java:64`), so it belongs to the
//! board's serialized state and rolls back when a board snapshot is
//! restored. Java keys a `ConcurrentHashMap` by item id; the port uses a
//! `BTreeMap`, so [`FailureLog::unroutable_items`] lists items by ascending
//! id (Java's `getUnroutableItems` is unused at the pin, so its hash order
//! never reaches a result). Java's `ItemFailureInfo` keeps the `Item`
//! object; the port keeps its id.

use std::collections::BTreeMap;

use crate::autoroute::AttemptState;
use crate::board::items::ItemId;
use crate::board::routing_board::RoutingBoard;

/// The number of failures after which an item is given up (Java
/// `FAILURE_THRESHOLD`).
pub const FAILURE_THRESHOLD: u32 = 50;

/// The failures of one item (Java `RoutingFailureLog.ItemFailureInfo`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureInfo {
    /// The item.
    pub item: ItemId,
    /// The item's first net when its first failure was recorded, -1 if it
    /// had none.
    pub net_number: i32,
    /// The number of recorded failures.
    pub failure_count: u32,
    /// The state of the last failure (Java `null` before the first).
    pub last_failure_state: Option<AttemptState>,
    /// The reason given with the last failure, empty for none.
    pub last_failure_reason: String,
    /// The pass number of the last failure.
    pub last_attempt_pass: i64,
}

impl FailureInfo {
    fn new(item: ItemId, net_number: i32) -> Self {
        FailureInfo {
            item,
            net_number,
            failure_count: 0,
            last_failure_state: None,
            last_failure_reason: String::new(),
            last_attempt_pass: 0,
        }
    }

    /// True once the item failed [`FAILURE_THRESHOLD`] times (Java
    /// `shouldGiveUp`).
    pub fn should_give_up(&self) -> bool {
        self.failure_count >= FAILURE_THRESHOLD
    }
}

/// The routing failures of the items of a board (Java `RoutingFailureLog`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FailureLog {
    failures: BTreeMap<i32, FailureInfo>,
}

impl FailureLog {
    /// An empty log.
    pub fn new() -> Self {
        FailureLog::default()
    }

    /// Records a failure of `item` (Java `recordFailure`). The entry is made
    /// on the item's first failure with `first_net` (the item's first net,
    /// -1 for none); later failures keep that net. `reason` `None` is stored
    /// as empty, as Java stores `null`.
    pub fn record_failure(
        &mut self,
        item: ItemId,
        first_net: i32,
        pass_no: i32,
        state: AttemptState,
        reason: Option<&str>,
    ) {
        let info = self
            .failures
            .entry(item.0)
            .or_insert_with(|| FailureInfo::new(item, first_net));
        info.failure_count += 1;
        info.last_attempt_pass = i64::from(pass_no);
        info.last_failure_state = Some(state);
        info.last_failure_reason = reason.unwrap_or("").to_owned();
    }

    /// The number of recorded failures of `item`, 0 for none (Java
    /// `getFailureCount`).
    pub fn failure_count(&self, item: ItemId) -> u32 {
        self.failures
            .get(&item.0)
            .map_or(0, |info| info.failure_count)
    }

    /// The failures of `item`, if any were recorded.
    pub fn info(&self, item: ItemId) -> Option<&FailureInfo> {
        self.failures.get(&item.0)
    }

    /// True if `item` failed at least [`FAILURE_THRESHOLD`] times (Java
    /// `shouldSkip`).
    pub fn should_skip(&self, item: ItemId) -> bool {
        self.failures
            .get(&item.0)
            .is_some_and(FailureInfo::should_give_up)
    }

    /// The items that failed at least [`FAILURE_THRESHOLD`] times, by
    /// ascending id (Java `getUnroutableItems`, in hash order there).
    pub fn unroutable_items(&self) -> Vec<&FailureInfo> {
        self.failures
            .values()
            .filter(|info| info.should_give_up())
            .collect()
    }

    /// True if some item failed at least [`FAILURE_THRESHOLD`] times (Java
    /// `hasUnroutableItems`).
    pub fn has_unroutable_items(&self) -> bool {
        self.failures.values().any(FailureInfo::should_give_up)
    }

    /// The number of items with a recorded failure.
    pub fn len(&self) -> usize {
        self.failures.len()
    }

    /// True if no failure is recorded.
    pub fn is_empty(&self) -> bool {
        self.failures.is_empty()
    }

    /// Forgets every failure (Java `clear`).
    pub fn clear(&mut self) {
        self.failures.clear();
    }
}

impl RoutingBoard {
    /// Records a failed routing attempt of `item` in the board's failure log
    /// (Java `board.failureLog.recordFailure(item, passNo, state, reason)`).
    /// A new entry takes the item's first net, -1 if the item has no net or
    /// is not on the board.
    pub fn record_routing_failure(
        &mut self,
        item: ItemId,
        pass_no: i32,
        state: AttemptState,
        reason: Option<&str>,
    ) {
        let first_net = self
            .item(item)
            .and_then(|item| item.net_numbers().first().copied())
            .unwrap_or(-1);
        self.failure_log_mut()
            .record_failure(item, first_net, pass_no, state, reason);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAILED: AttemptState = AttemptState::Failed;

    #[test]
    fn first_failure_creates_the_entry_with_its_net() {
        let mut log = FailureLog::new();
        assert_eq!(log.failure_count(ItemId(7)), 0);
        assert!(log.info(ItemId(7)).is_none());
        log.record_failure(ItemId(7), 12, 3, FAILED, Some("no path"));
        log.record_failure(ItemId(7), 99, 4, AttemptState::InsertError, None);
        let info = log.info(ItemId(7)).unwrap();
        assert_eq!(info.item, ItemId(7));
        assert_eq!(info.net_number, 12, "later failures keep the first net");
        assert_eq!(info.failure_count, 2);
        assert_eq!(info.last_attempt_pass, 4);
        assert_eq!(info.last_failure_state, Some(AttemptState::InsertError));
        assert_eq!(
            info.last_failure_reason, "",
            "a null reason is stored empty"
        );
        assert_eq!(log.failure_count(ItemId(7)), 2);
        assert_eq!(log.len(), 1);
    }

    #[test]
    fn skip_starts_at_the_threshold() {
        let mut log = FailureLog::new();
        for pass in 0..FAILURE_THRESHOLD as i32 - 1 {
            log.record_failure(ItemId(1), -1, pass, FAILED, None);
        }
        assert!(!log.should_skip(ItemId(1)));
        assert!(!log.has_unroutable_items());
        assert!(log.unroutable_items().is_empty());
        log.record_failure(ItemId(1), -1, 49, FAILED, None);
        assert_eq!(log.failure_count(ItemId(1)), 50);
        assert!(log.should_skip(ItemId(1)));
        assert!(log.has_unroutable_items());
        assert!(!log.should_skip(ItemId(2)));
    }

    #[test]
    fn unroutable_items_are_sorted_by_ascending_id() {
        let mut log = FailureLog::new();
        for id in [30, 4, 17, 9] {
            let failures = if id == 9 { 3 } else { FAILURE_THRESHOLD };
            for pass in 0..failures as i32 {
                log.record_failure(ItemId(id), id, pass, FAILED, None);
            }
        }
        let ids: Vec<i32> = log.unroutable_items().iter().map(|i| i.item.0).collect();
        assert_eq!(ids, vec![4, 17, 30]);
    }

    #[test]
    fn board_records_with_the_first_net_of_the_item() {
        use crate::io::{BoardReadResult, read_board};
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/Issue026-J2_reference.dsn");
        let mut board = match read_board(&std::fs::read(path).unwrap()) {
            BoardReadResult::Success { board, .. } => *board,
            _ => panic!("J2 must load"),
        };
        let (pin, net) = board
            .items()
            .find(|item| item.is_pin() && !item.net_numbers().is_empty())
            .map(|item| (item.id(), item.net_numbers()[0]))
            .unwrap();
        assert!(board.failure_log().is_empty());
        board.record_routing_failure(pin, 1, FAILED, Some("no path"));
        board.record_routing_failure(pin, 2, FAILED, Some("no path"));
        let missing = ItemId(i32::MAX);
        board.record_routing_failure(missing, 2, FAILED, None);
        let log = board.failure_log();
        assert_eq!(log.info(pin).unwrap().net_number, net);
        assert_eq!(log.failure_count(pin), 2);
        assert_eq!(log.info(missing).unwrap().net_number, -1);
        board.failure_log_mut().clear();
        assert!(board.failure_log().is_empty());
    }

    #[test]
    fn clear_forgets_everything() {
        let mut log = FailureLog::new();
        log.record_failure(ItemId(1), 1, 1, FAILED, Some("x"));
        log.clear();
        assert!(log.is_empty());
        assert_eq!(log.failure_count(ItemId(1)), 0);
    }
}

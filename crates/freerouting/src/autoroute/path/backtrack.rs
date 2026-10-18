//! The backtrack chain of a found connection (A05.1).
//!
//! Port of `FoundConnectionLocator.backtrack` (`FoundConnectionLocator.java:225-327`
//! at `a773a70ec`): from the destination door, follow each section's backtrack
//! door and section to the start door, and collect the obstacle items the
//! search decided to rip on the way.
//!
//! Orders that decide results, reproduced as Java has them:
//! - the chain runs from the destination to the start;
//! - an item is added to the ripped set and its cost is put into the cost
//!   list at the moment Java does: for a drill destination entered with a
//!   ripup, the obstacle rooms of every layer of the drill first, then each
//!   element of the chain whose section was entered with a ripup;
//! - the cost list follows `LinkedHashMap.put` keyed by identity (Java's
//!   `Item` has no `equals`): an item keeps the position of its first
//!   insertion and takes the cost of its last.

use std::collections::BTreeSet;

use super::LocateError;
use crate::autoroute::session::search::SearchResult;
use crate::autoroute::session::{ExpandableRef, Room, RoomId, SectionState, Session};
use crate::board::items::ItemId;

/// One element of the backtrack chain (Java
/// `FoundConnectionLocator.BacktrackElement`): a door, its section, and the
/// room the chain enters through it (`nextRoom`, the room shared with the
/// next element of the chain).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BacktrackElement {
    pub door: ExpandableRef,
    pub section: usize,
    pub next_room: Option<RoomId>,
}

/// The ripup costs of the ripped items, in Java's `LinkedHashMap` order.
pub type RipupCosts = Vec<(ItemId, i32)>;

/// Java `ripupCosts.put(item, cost)` on a `LinkedHashMap<Item, Integer>`:
/// a new item is appended; an item already there keeps its place and
/// takes the new cost.
pub fn put_cost(costs: &mut RipupCosts, item: ItemId, cost: i32) {
    // PARITY: LinkedHashMap.put keeps the insertion order of a re-inserted
    // key and replaces its value (FoundConnectionLocator.java:261,320).
    match costs.iter_mut().find(|(id, _)| *id == item) {
        Some(entry) => entry.1 = cost,
        None => costs.push((item, cost)),
    }
}

/// Java `getMazeSearchElement(section)`: a target door has one element
/// whatever the index; the others index their section array.
fn element(
    session: &Session<'_>,
    object: ExpandableRef,
    section: usize,
) -> Result<SectionState, LocateError> {
    let section = match object {
        ExpandableRef::TargetDoor(_) => 0,
        _ => section,
    };
    session
        .section_state(object, section)
        .cloned()
        .ok_or(LocateError::IndexOutOfBounds {
            at: "FoundConnectionLocator.backtrack",
        })
}

/// The item of an obstacle room.
fn obstacle_item(session: &Session<'_>, room: Option<RoomId>) -> Option<ItemId> {
    match room.map(|r| session.room(r)) {
        Some(Room::Obstacle { item, .. }) => Some(*item),
        _ => None,
    }
}

/// `roomArr[index]` of a drill, Java's array bounds checked.
fn drill_room(
    session: &Session<'_>,
    drill: crate::autoroute::session::drills::DrillId,
    index: i64,
) -> Result<Option<RoomId>, LocateError> {
    let rooms = &session.drill(drill).rooms;
    usize::try_from(index)
        .ok()
        .and_then(|i| rooms.get(i).copied())
        .ok_or(LocateError::IndexOutOfBounds {
            at: "FoundConnectionLocator.backtrack",
        })
}

/// Java `FoundConnectionLocator.backtrack`: the chain from the destination
/// of `result` to the start door. Ripped items are added to `ripped` (Java's
/// `TreeSet<Item>`; `ItemId` orders by descending id as Java's `Item`
/// does) and their costs put into `costs` when it is given, in Java's order.
/// Both stay filled even when the caller later rejects the result.
pub fn backtrack(
    session: &Session<'_>,
    result: &SearchResult,
    ripped: &mut BTreeSet<ItemId>,
    mut costs: Option<&mut RipupCosts>,
) -> Result<Vec<BacktrackElement>, LocateError> {
    let (destination, destination_section) = result.destination;
    let mut chain = Vec::new();
    let mut current = element(session, destination, destination_section)?;
    let mut next_room = match destination {
        ExpandableRef::TargetDoor(door) => session.target_door(door).room,
        ExpandableRef::Drill(drill) => {
            let first_layer = session.drill(drill).first_layer;
            // PARITY(quirk, java_quirks.drill_destination_room_index):
            // FoundConnectionLocator.java:255 indexes the drill's rooms by
            // `firstLayer + section`, though `roomArr` starts at the first
            // layer. Every drill page's drills start at layer 0, so only a
            // via's drill (a blind or buried via) differs: Java takes the
            // room `first_layer` layers further, or throws past the end
            // (a failed locate). `false` indexes by the section.
            let offset = if session.control.quirks.drill_destination_room_index {
                i64::from(first_layer)
            } else {
                0
            };
            if first_layer != 0 {
                // Counted in either arm: the destinations the switch indexes
                // differently (N06.6).
                crate::run_stats::record_n06(
                    crate::run_stats::Counter::DrillDestinationRoomIndexFlips,
                );
            }
            let room = drill_room(session, drill, offset + destination_section as i64)?;
            if current.room_ripped {
                for tmp in session.drill(drill).rooms.clone() {
                    if let Some(item) = obstacle_item(session, tmp) {
                        ripped.insert(item);
                        if let Some(costs) = costs.as_deref_mut() {
                            put_cost(costs, item, current.ripup_cost);
                        }
                    }
                }
            }
            room
        }
        _ => None,
    };
    let mut element_now = BacktrackElement {
        door: destination,
        section: destination_section,
        next_room,
    };
    loop {
        chain.push(element_now);
        let Some((door, section)) = current.backtrack else {
            break;
        };
        let count = session.section_count(door);
        let section = if section >= count {
            // Java warns "currentSectionNo to big" and takes the last
            // section (FoundConnectionLocator.java:278-281); with no
            // section the index -1 throws.
            count.checked_sub(1).ok_or(LocateError::IndexOutOfBounds {
                at: "FoundConnectionLocator.backtrack",
            })?
        } else {
            section
        };
        next_room = match door {
            ExpandableRef::Drill(drill) => drill_room(session, drill, section as i64)?,
            _ => next_room.and_then(|room| session.expandable_other_room(door, room)),
        };
        current = element(session, door, section)?;
        element_now = BacktrackElement {
            door,
            section,
            next_room,
        };
        if current.room_ripped
            && let Some(item) = obstacle_item(session, next_room)
        {
            ripped.insert(item);
            if let Some(costs) = costs.as_deref_mut() {
                put_cost(costs, item, current.ripup_cost);
            }
        }
    }
    Ok(chain)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Java `LinkedHashMap.put` keyed by identity: an item met twice keeps
    /// the position of its first insertion and takes its last cost.
    #[test]
    fn an_item_met_twice_keeps_its_first_place_and_its_last_cost() {
        let mut costs = RipupCosts::new();
        put_cost(&mut costs, ItemId(7), 100);
        put_cost(&mut costs, ItemId(3), 200);
        put_cost(&mut costs, ItemId(7), 300);
        assert_eq!(costs, vec![(ItemId(7), 300), (ItemId(3), 200)]);
    }

    #[test]
    fn the_ripped_set_iterates_by_descending_id_like_javas_tree_set() {
        let ripped: BTreeSet<ItemId> = [ItemId(3), ItemId(12), ItemId(5)].into_iter().collect();
        let order: Vec<i32> = ripped.iter().map(|id| id.0).collect();
        assert_eq!(order, vec![12, 5, 3]);
    }
}

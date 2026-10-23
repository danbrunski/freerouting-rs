//! K15.47: a cache of the conduction areas' normal contacts (Java
//! `ConductionArea.getNormalContacts`, [`BasicBoard::normal_contacts`]).
//!
//! An area's contacts are a search over each of its tile shapes, and the
//! cycle test (`Trace.isCycle`, [`BasicBoard::trace_is_cycle`]) and the
//! connected sets ask for them again and again while nothing changes: on
//! kit-dev-coldfire-xilinx_5213's route without the pour pair 44 593
//! computations, 48.2 s of a 119 s route, 41 348 of them on trees and items
//! unchanged since the area's last one (72.3 s of 200 s with the pair,
//! where the pour contacts' islands ask too).
//!
//! The contacts are a function of the search trees and the items, so the
//! cache keeps each area's contacts with the stamp of the trees
//! ([`SearchTreeManager::stamp`](crate::board::search::SearchTreeManager),
//! new at every change of a tree) and the board's item stamp (new at every
//! insertion, removal, change and net change the board's bookkeeping hears,
//! [`BasicBoard`]'s `note_*`), and answers from it only while both are the
//! ones it was computed under.
//!
//! **The work meter.** Each tree query adds the nodes it visits to the
//! board's work meter, and the work limits (the maze search, the pull-tight
//! guard, the shover, the fanout's per-pin budget) cut off by that count
//! (B02.7). So the cache keeps what the computation added to the meter
//! ([`recorded`]) and a cached answer adds it again ([`WorkRecord::replay`]):
//! under the same stamps the queries would visit the same nodes. The meter,
//! the cut-offs and the routes are those of the computation without the
//! cache; debug builds compute every cached answer again, dry, and assert
//! that the contacts and the records are equal.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex, MutexGuard};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId};
use crate::board::search::work::{WorkRecord, recorded};

/// The board state an area's contacts were computed in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Stamps {
    trees: u64,
    items: u64,
}

#[derive(Debug)]
struct Entry {
    stamps: Stamps,
    contacts: BTreeSet<ItemId>,
    work: Vec<WorkRecord>,
}

/// The cached contacts of a board's conduction areas.
#[derive(Debug, Default)]
pub(crate) struct AreaContacts {
    entries: Mutex<BTreeMap<ItemId, Arc<Entry>>>,
}

impl Clone for AreaContacts {
    /// A copy with the same entries (a copied board has the same trees and
    /// items, and so the same stamps).
    fn clone(&self) -> Self {
        AreaContacts {
            entries: Mutex::new(self.lock().clone()),
        }
    }
}

impl AreaContacts {
    fn lock(&self) -> MutexGuard<'_, BTreeMap<ItemId, Arc<Entry>>> {
        self.entries.lock().unwrap_or_else(|e| e.into_inner())
    }
}

/// The next item stamp of a board ([`BasicBoard::touch_items`]): unique
/// across boards, so two boards' stamps are equal only where one is a copy
/// of the other with the same items.
static NEXT_ITEM_STAMP: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

pub(crate) fn fresh_item_stamp() -> u64 {
    NEXT_ITEM_STAMP.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

impl BasicBoard {
    /// K15.47: takes a new item stamp (an item was inserted, removed or
    /// changed, or its nets changed).
    pub(crate) fn touch_items(&mut self) {
        self.item_stamp = fresh_item_stamp();
    }

    fn contact_stamps(&self) -> Stamps {
        Stamps {
            trees: self.search_trees.stamp(),
            items: self.item_stamp,
        }
    }

    /// K15.47 (test support): [`BasicBoard::normal_contacts`] of conduction
    /// area `id` computed now, past the cache (empty for another item).
    #[doc(hidden)]
    pub fn area_normal_contacts_uncached(&self, id: ItemId) -> BTreeSet<ItemId> {
        self.item(id)
            .filter(|i| i.is_conduction_area())
            .map(|i| self.conduction_area_normal_contacts(i))
            .unwrap_or_default()
    }

    /// Java `ConductionArea.getNormalContacts` of conduction area `item`,
    /// from the cache while the board is as it was (module docs).
    pub(crate) fn cached_area_normal_contacts(&self, item: &Item) -> BTreeSet<ItemId> {
        let stamps = self.contact_stamps();
        let id = item.id();
        let known = self
            .area_contacts
            .lock()
            .get(&id)
            .filter(|e| e.stamps == stamps)
            .cloned();
        if let Some(entry) = known {
            let meter = self.search_trees.work_meter();
            for record in &entry.work {
                record.replay(meter);
            }
            #[cfg(debug_assertions)]
            {
                let (again, work) = recorded(true, || self.conduction_area_normal_contacts(item));
                debug_assert_eq!(again, entry.contacts, "cached contacts of area {id:?}");
                debug_assert_eq!(work, entry.work, "cached work of area {id:?}");
            }
            return entry.contacts.clone();
        }
        let (contacts, work) = recorded(false, || self.conduction_area_normal_contacts(item));
        let entry = Entry {
            stamps,
            contacts: contacts.clone(),
            work,
        };
        self.area_contacts.lock().insert(id, Arc::new(entry));
        contacts
    }
}

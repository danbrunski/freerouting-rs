//! Board operations the desktop app and the interactive tools call (B08.5,
//! B08.6): detaching an item for a move, unfilling the conduction areas, a
//! count of non-45-degree traces, and the item list of a net.
//!
//! Port of `BoardItemRepository.detachItemForMove` (`:176-184`, with the
//! `forMove` branch of `removeItem`, `:185-220`), `ConductionArea.setIsFilled`
//! (`:37`), `BasicBoard.unfillConductionAreas` (`:1546-1560`),
//! `BasicBoard.getNon45DegreeTraceCount` (`:1585-1595`) and `Net.getItems`
//! (`rules/Net.java:195`). `BasicBoard.insertItem` (`:1332`) is
//! [`BasicBoard::insert_item`], which is public now.
//!
//! Differences from Java:
//! - `ConductionArea.setIsFilled` and `setIsObstacle` change the shared item
//!   object without telling the undo list, so in Java a later undo keeps the
//!   change. The port saves the area for undo first, so
//!   [`BasicBoard::unfill_conduction_areas`] is undone by
//!   [`BasicBoard::undo`] (`unfill_is_undone_by_undo`). The rules flag
//!   `ignoreConduction` is not part of the undo list, as in Java.
//! - [`DetachError::NotOnBoard`]: Java's `removeItem(null)` returns silently.

use std::fmt;

use crate::board::AutorouteChange;
use crate::board::basic_board::BasicBoard;
use crate::board::items::{AreaKind, Item, ItemId, ItemKind};
use crate::board::structure::ComponentId;

/// Why [`BasicBoard::detach_item_for_move`] refused an item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetachError {
    /// The item is not on the board.
    NotOnBoard(ItemId),
    /// The item is user fixed or belongs to a component whose position is
    /// fixed (Java throws `IllegalArgumentException`).
    Fixed(ItemId),
}

impl fmt::Display for DetachError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DetachError::NotOnBoard(id) => write!(f, "item {id} is not on the board"),
            DetachError::Fixed(id) => write!(f, "Fixed items cannot be detached for a move: {id}"),
        }
    }
}

impl std::error::Error for DetachError {}

impl BasicBoard {
    /// Removes item `id` from the board so that a move can reinsert it (Java
    /// `BoardItemRepository.detachItemForMove`).
    ///
    /// An error for a user-fixed item and for an item of a component with
    /// `position_fixed`. Otherwise the item is removed, an item of a
    /// component too although its deletion is forbidden. Another item whose
    /// deletion is forbidden (a conduction area of a power plane) is kept and
    /// the call returns `Ok`, as Java's `removeItem` returns without an error.
    pub fn detach_item_for_move(&mut self, id: ItemId) -> Result<(), DetachError> {
        let item = self.item(id).ok_or(DetachError::NotOnBoard(id))?;
        let component_fixed = item.component_id() > 0
            && self
                .components
                .get(ComponentId(item.component_id()))
                .is_some_and(|c| c.position_fixed);
        if item.is_user_fixed() || component_fixed {
            return Err(DetachError::Fixed(id));
        }
        self.remove_item_checked(id, true);
        Ok(())
    }

    /// Java `ConductionArea.setIsFilled`: sets the filled flag of conduction
    /// area `id` and clears its derived data. False if `id` is not a
    /// conduction area. The area is saved for undo first (see the module
    /// docs).
    pub fn set_conduction_is_filled(&mut self, id: ItemId, value: bool) -> bool {
        let Some(AreaKind::Conduction { is_obstacle, .. }) = self
            .item(id)
            .and_then(Item::as_conduction_area)
            .map(|a| a.kind)
        else {
            return false;
        };
        self.items.save_for_undo(&id);
        let Some(ItemKind::Area(area)) = self.items.get_mut(&id).map(|i| &mut i.kind) else {
            return false;
        };
        area.set_kind(AreaKind::Conduction {
            is_obstacle,
            is_filled: value,
        });
        self.log_autoroute(AutorouteChange::InfoCleared(id));
        self.note_changed(id);
        true
    }

    /// Java `BasicBoard.unfillConductionAreas`: the router ignores conduction
    /// areas from now on, every conduction area becomes unfilled and no
    /// obstacle, and the search trees are rebuilt from the items.
    ///
    /// The areas are visited highest id first, as Java's item list iterates.
    pub fn unfill_conduction_areas(&mut self) {
        self.rules.set_ignore_conduction(true);
        for id in self.item_ids() {
            let is_conduction = self.item(id).is_some_and(Item::is_conduction_area);
            if !is_conduction {
                continue;
            }
            self.set_conduction_is_filled(id, false);
            self.set_conduction_is_obstacle_for_undo(id, false);
        }
        self.reinsert_tree_items();
    }

    /// Java `ConductionArea.setIsObstacle`, saving the area for undo first.
    fn set_conduction_is_obstacle_for_undo(&mut self, id: ItemId, value: bool) {
        self.items.save_for_undo(&id);
        self.set_conduction_is_obstacle(id, value);
    }

    /// Java `Net.getItems`: all items of net `net_number`, connectable or not
    /// (obstacle areas of a net too), highest id first.
    pub fn net_items(&self, net_number: i32) -> Vec<ItemId> {
        self.items()
            .filter(|item| item.contains_net(net_number))
            .map(Item::id)
            .collect()
    }

    /// Java `BasicBoard.getNon45DegreeTraceCount`: the traces whose polyline
    /// is not made of lines at multiples of 45 degrees.
    pub fn non_45_degree_trace_count(&self) -> usize {
        self.items()
            .filter_map(Item::as_trace)
            .filter(|trace| !trace.polyline.is_multiple_of_45_degree())
            .count()
    }
}

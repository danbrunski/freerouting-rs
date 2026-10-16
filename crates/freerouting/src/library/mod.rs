//! The board library: padstacks, packages (footprints) and logical parts.
//!
//! Port of `core.library`.

mod logical_part;
mod package;
mod padstack;

pub use logical_part::{LogicalPart, LogicalPartId, LogicalParts, PartPin};
pub use package::{Package, PackageId, PackageKeepout, PackagePin, Packages};
pub use padstack::{Padstack, PadstackId, Padstacks};

use crate::board::BasicBoard;

/// The padstacks, packages, logical parts and via padstacks of a board.
///
/// Port of `core.library.BoardLibrary`.
#[derive(Clone, Debug)]
pub struct BoardLibrary {
    pub padstacks: Padstacks,
    pub packages: Packages,
    pub logical_parts: LogicalParts,
    via_padstacks: Option<Vec<PadstackId>>,
}

impl BoardLibrary {
    pub fn new(padstacks: Padstacks, packages: Packages) -> Self {
        BoardLibrary {
            padstacks,
            packages,
            logical_parts: LogicalParts::default(),
            via_padstacks: None,
        }
    }

    /// Java `BoardLibrary.isUsed`: true if a pin or via on `board` has
    /// `padstack`, or a package pin of the library names it.
    pub fn is_used(&self, padstack: PadstackId, board: &BasicBoard) -> bool {
        board
            .items()
            .any(|item| item.drill().is_some_and(|drill| drill.padstack == padstack))
            || self
                .packages
                .iter()
                .any(|package| package.pins().iter().any(|pin| pin.padstack_id == padstack))
    }

    /// The number of via padstacks.
    pub fn via_padstack_count(&self) -> usize {
        self.via_padstacks.as_ref().map_or(0, Vec::len)
    }

    /// The `no`-th via padstack.
    pub fn get_via_padstack(&self, no: usize) -> Option<&Padstack> {
        let id = *self.via_padstacks.as_ref()?.get(no)?;
        self.padstacks.get(id)
    }

    /// The via padstack named `name` (exact match).
    pub fn get_via_padstack_by_name(&self, name: &str) -> Option<&Padstack> {
        self.via_padstacks
            .as_ref()?
            .iter()
            .filter_map(|id| self.padstacks.get(*id))
            .find(|p| p.name == name)
    }

    /// The ids of the via padstacks.
    pub fn via_padstack_ids(&self) -> &[PadstackId] {
        self.via_padstacks.as_deref().unwrap_or(&[])
    }

    /// Replaces the via padstacks.
    pub fn set_via_padstacks(&mut self, padstacks: Vec<PadstackId>) {
        self.via_padstacks = Some(padstacks);
    }

    /// Appends a via padstack unless one with the same name exists. Returns
    /// false in that case.
    pub fn add_via_padstack(&mut self, id: PadstackId) -> bool {
        let Some(name) = self.padstacks.get(id).map(|p| p.name.clone()) else {
            return false;
        };
        if self.get_via_padstack_by_name(&name).is_some() {
            return false;
        }
        self.via_padstacks.get_or_insert_with(Vec::new).push(id);
        true
    }

    /// Removes a via padstack. Returns false if it was not one.
    pub fn remove_via_padstack(&mut self, id: PadstackId) -> bool {
        let Some(list) = self.via_padstacks.as_mut() else {
            return false;
        };
        match list.iter().position(|p| *p == id) {
            Some(pos) => {
                list.remove(pos);
                true
            }
            None => false,
        }
    }

    /// The via padstack spanning the mirrored layer range of `via_padstack`:
    /// the padstack itself if it spans all layers.
    pub fn get_mirrored_via_padstack(&self, via_padstack: PadstackId) -> Option<PadstackId> {
        let layer_count = self.padstacks.board_layer_count() as i32;
        let padstack = self.padstacks.get(via_padstack)?;
        if padstack.from_layer() == 0 && padstack.to_layer() == layer_count - 1 {
            return Some(via_padstack);
        }
        let new_from = layer_count - padstack.to_layer() - 1;
        let new_to = layer_count - padstack.from_layer() - 1;
        self.via_padstack_ids().iter().copied().find(|id| {
            self.padstacks
                .get(*id)
                .is_some_and(|p| p.from_layer() == new_from && p.to_layer() == new_to)
        })
    }
}

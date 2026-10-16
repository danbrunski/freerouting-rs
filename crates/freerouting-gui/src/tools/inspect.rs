//! Inspect mode (G05.1, G05.3): Java's `InspectMenuState`,
//! `InspectedItemState` and `InspectItemsInRegionState`.
//!
//! The selection is the app's [`Selection`]; "no items selected" is Java's
//! `InspectMenuState` and "some selected" its `InspectedItemState`. What a
//! pick returns is the engine's: the GUI converts the cursor to a board
//! point and calls `pick_items_filtered` and `overlapping_items` with the
//! filter built from the user's [`SelectionSettings`], and extends a
//! selection with `connectable_items`, `connected_set`, `connection_items`
//! and `component_items` (B04, G05 decisions). It never tests an item's
//! class itself.
//!
//! Rules kept from Java:
//! - A click with nothing selected selects everything the pick returns
//!   (`MenuState.selectItems`); a click on nothing leaves things as they are.
//! - A click with something selected toggles (`toggleSelect`): a single
//!   picked item is added or removed, several do nothing, none (or removing
//!   the last item) ends the selection. Ctrl+click is the same toggle for
//!   the case of nothing selected too.
//! - A region adds to the selection (`state.getItemList().addAll`).
//! - Nets, connected sets and connections replace the selection and apply
//!   the filter; components add to it and are not filtered.

use std::collections::BTreeSet;

use eframe::egui::{InputState, Key};
use freerouting::board::items::{ItemId, StopConnectionOption};
use freerouting::board::{BasicBoard, ItemFilter};
use freerouting::drc::ClearanceViolation;
use freerouting::geometry::{Area, FloatPoint, IntBox, IntPoint, Point, Shape};

use crate::commands::{Command, ExtendKind};
use crate::panels::select_settings::{SelectionLayers, SelectionSettings};
use crate::scene::P;
use crate::selection::Selection;

/// A point of the design file's frame in millimetres, as a board point (the
/// inverse of `overlays::Frame::pt`).
pub fn board_point(board: &BasicBoard, p: P) -> FloatPoint {
    let comm = &board.communication;
    let mm_per_unit = comm.unit.micrometers() / 1000.0;
    comm.coordinate_transform
        .dsn_to_board_point(p.x / mm_per_unit, p.y / mm_per_unit)
}

/// The layers a pick may use: the current layer, and whether a layer is
/// visible (shown at a level above 0).
pub struct PickLayers<'a> {
    pub current: usize,
    pub visible: &'a [bool],
}

impl PickLayers<'_> {
    fn is_visible(&self, layer: usize) -> bool {
        self.visible.get(layer).copied().unwrap_or(false)
    }
}

/// Java `GuiBoardInteractionController.pickItems(location, filter)`: the
/// items at `at` on the current layer; if there are none and the settings
/// allow all visible layers, the items on every other visible layer.
pub fn pick(
    board: &BasicBoard,
    at: IntPoint,
    settings: &SelectionSettings,
    layers: &PickLayers<'_>,
) -> BTreeSet<ItemId> {
    let filter = settings.filter();
    let location = Point::from(at);
    let mut result = board.pick_items_filtered(&location, layers.current as i32, Some(&filter));
    if result.is_empty() && settings.layers == SelectionLayers::AllVisible {
        for layer in 0..board.layer_structure.layers.len() {
            if layer == layers.current || !layers.is_visible(layer) {
                continue;
            }
            result.extend(board.pick_items_filtered(&location, layer as i32, Some(&filter)));
        }
    }
    result
}

/// Java `InspectItemsInRegionState.selectAllInRegion`: the items that
/// overlap the box of the two corners, through the filter. With all visible
/// layers the search covers every layer and drops the items that are on no
/// visible layer.
pub fn region(
    board: &BasicBoard,
    from: IntPoint,
    to: IntPoint,
    settings: &SelectionSettings,
    layers: &PickLayers<'_>,
) -> BTreeSet<ItemId> {
    let area = Area::from(Shape::from(IntBox::new(
        from.x.min(to.x),
        from.y.min(to.y),
        from.x.max(to.x),
        from.y.max(to.y),
    )));
    let all_visible = settings.layers == SelectionLayers::AllVisible;
    let layer = if all_visible {
        -1
    } else {
        layers.current as i32
    };
    let found = settings
        .filter()
        .filter(board, &board.overlapping_items(&area, layer));
    if !all_visible {
        return found;
    }
    found
        .into_iter()
        .filter(|id| {
            board.item(*id).is_some_and(|item| {
                (item.first_layer()..=item.last_layer())
                    .any(|l| usize::try_from(l).is_ok_and(|l| layers.is_visible(l)))
            })
        })
        .collect()
}

/// What a click did to the selection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ClickOutcome {
    /// The selection is now these items (non-empty).
    Selected(BTreeSet<ItemId>),
    /// The selection ended (Java returns to the menu state).
    Ended,
    /// Nothing changed.
    Unchanged,
}

/// Java `MenuState.selectItems` (nothing selected) and
/// `InspectedItemState.toggleSelect` (something selected, or `toggle`).
pub fn click(current: &BTreeSet<ItemId>, picked: &BTreeSet<ItemId>, toggle: bool) -> ClickOutcome {
    if current.is_empty() && !toggle {
        return if picked.is_empty() {
            ClickOutcome::Unchanged
        } else {
            ClickOutcome::Selected(picked.clone())
        };
    }
    let mut items = current.clone();
    let mut ended = picked.is_empty();
    if picked.len() == 1 {
        let id = *picked.iter().next().expect("one picked item");
        if !items.remove(&id) {
            items.insert(id);
        }
        if items.is_empty() {
            ended = true;
        }
    } else if picked.len() > 1 && current.is_empty() {
        // Ctrl+click with nothing selected: take what is there.
        items = picked.clone();
    }
    if ended {
        ClickOutcome::Ended
    } else {
        ClickOutcome::Selected(items)
    }
}

/// The selection after extending it; empty when nothing is left (Java
/// leaves the inspect state then).
pub fn extend(
    board: &BasicBoard,
    current: &BTreeSet<ItemId>,
    kind: ExtendKind,
    filter: &ItemFilter,
) -> BTreeSet<ItemId> {
    let connectable = |id: &ItemId| board.item(*id).filter(|i| i.is_connectable_kind());
    match kind {
        ExtendKind::Nets => {
            let nets: BTreeSet<i32> = current
                .iter()
                .filter_map(connectable)
                .flat_map(|item| item.net_numbers().iter().copied())
                .collect();
            let items: Vec<ItemId> = nets
                .into_iter()
                .flat_map(|net| board.connectable_items(net))
                .collect();
            filter.filter(board, &items)
        }
        ExtendKind::ConnectedSets => {
            let items: BTreeSet<ItemId> = current
                .iter()
                .filter(|id| connectable(id).is_some())
                .flat_map(|id| board.connected_set(*id, -1, false))
                .collect();
            filter.filter(board, &items)
        }
        ExtendKind::Connections => {
            let items: BTreeSet<ItemId> = current
                .iter()
                .filter(|id| connectable(id).is_some())
                .flat_map(|id| board.connection_items(*id, StopConnectionOption::None))
                .collect();
            filter.filter(board, &items)
        }
        ExtendKind::Components => {
            let components: BTreeSet<i32> = current
                .iter()
                .filter_map(|id| board.item(*id))
                .map(|i| i.component_id())
                .filter(|c| *c > 0)
                .collect();
            let mut items = current.clone();
            for component in components {
                items.extend(board.component_items(component));
            }
            items
        }
    }
}

/// Java `ClearanceViolation.aggregateSortedBySeverity`: the clearance
/// violations of every item (each reported with that item first), the worst
/// first. A violation between two selected items is listed once for each.
pub fn selection_violations(
    board: &BasicBoard,
    items: &BTreeSet<ItemId>,
) -> Vec<ClearanceViolation> {
    let mut violations: Vec<ClearanceViolation> = items
        .iter()
        .flat_map(|id| board.clearance_violations(*id))
        .collect();
    // Java's `List.sort` is stable: the worst shortfall first.
    violations.sort_by(|a, b| {
        let da = a.expected_clearance - a.actual_clearance;
        let db = b.expected_clearance - b.actual_clearance;
        db.partial_cmp(&da).unwrap_or(std::cmp::Ordering::Equal)
    });
    violations
}

/// Java's keys of `InspectedItemState.keyTyped` while items are selected
/// (`N`, `S`, `E`, `I`, `R`, `V`) and `Delete` for the deletion added by
/// decision; and `I` without a selection (`MenuState`: select at the
/// cursor). `V` toggles the global markers without a selection, which
/// the key table already maps.
pub fn key_commands(
    input: &InputState,
    typing: bool,
    selection: &Selection,
    cursor: Option<P>,
) -> Vec<Command> {
    let mut out = Vec::new();
    if typing || input.modifiers.command || input.modifiers.alt {
        return out;
    }
    if selection.is_empty() {
        if input.key_pressed(Key::I)
            && let Some(at) = cursor
        {
            out.push(Command::SelectAt(at));
        }
        return out;
    }
    let table = [
        (Key::N, Command::ExtendSelection(ExtendKind::Nets)),
        (Key::S, Command::ExtendSelection(ExtendKind::ConnectedSets)),
        (Key::E, Command::ExtendSelection(ExtendKind::Connections)),
        (Key::I, Command::ShowItemInfo(selection.ids())),
        (Key::R, Command::ZoomToRectangle),
        (Key::Delete, Command::DeleteSelection),
    ];
    for (key, command) in table {
        if input.key_pressed(key) {
            out.push(command);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[i32]) -> BTreeSet<ItemId> {
        ids.iter().map(|i| ItemId(*i)).collect()
    }

    #[test]
    fn a_click_with_nothing_selected_selects_the_pick() {
        assert_eq!(
            click(&set(&[]), &set(&[3, 5]), false),
            ClickOutcome::Selected(set(&[3, 5]))
        );
        assert_eq!(click(&set(&[]), &set(&[]), false), ClickOutcome::Unchanged);
    }

    #[test]
    fn a_click_with_a_selection_toggles_one_item() {
        let current = set(&[1, 2]);
        // One item: added, then removed again.
        let ClickOutcome::Selected(added) = click(&current, &set(&[7]), false) else {
            panic!("selected");
        };
        assert_eq!(added, set(&[1, 2, 7]));
        assert_eq!(
            click(&added, &set(&[7]), false),
            ClickOutcome::Selected(current.clone())
        );
        // Several items under the cursor: nothing happens.
        assert_eq!(
            click(&current, &set(&[8, 9]), false),
            ClickOutcome::Selected(current.clone())
        );
        // Nothing under the cursor, or the last item removed: the selection ends.
        assert_eq!(click(&current, &set(&[]), false), ClickOutcome::Ended);
        assert_eq!(click(&set(&[4]), &set(&[4]), false), ClickOutcome::Ended);
    }

    #[test]
    fn ctrl_click_toggles_even_with_nothing_selected() {
        assert_eq!(
            click(&set(&[]), &set(&[6]), true),
            ClickOutcome::Selected(set(&[6]))
        );
        assert_eq!(
            click(&set(&[]), &set(&[6, 7]), true),
            ClickOutcome::Selected(set(&[6, 7]))
        );
        assert_eq!(click(&set(&[]), &set(&[]), true), ClickOutcome::Ended);
    }
}

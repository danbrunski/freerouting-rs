//! Selection (I03.2): the board calls of Java's `InspectedItemState`,
//! `InspectItemsInRegionState` and `GuiBoardInteractionController.pickItems`
//! (`gui/interactive/*.java` at `a773a70ec`).
//!
//! A selection is a [`BTreeSet<ItemId>`], highest id first like Java's
//! `TreeSet<Item>`. Each function returns the new selection or `None` where
//! Java leaves the inspect state for its return state.
//!
//! Orders and quirks kept from Java:
//! - [`pick_items`]: the current layer; if nothing is there and "select on
//!   all visible layers" is on, every other visible layer in index order.
//! - [`extend_to_nets`]: the nets of the connectable selected items
//!   (ascending), the connectable items of each, then the filter.
//! - [`extend_to_components`]: keeps the old selection, adds the items of
//!   the selected items' components and does **not** filter.
//! - [`extend_to_connected_sets`] (`getConnectedSet(-1)`) and
//!   [`extend_to_connections`] (`getConnectionItems()`): replace the
//!   selection, then filter.
//! - After the filter of an extension Java ignores the filter's result and
//!   stays in the inspect state, so the selection may be empty there
//!   (`extentToWholeNets` returns `this` after `filter()`). Only an empty set
//!   *before* the filter leaves the state.
//! - [`select_in_region`]: `overlapping_items` of the box on layer −1 with
//!   "select on all visible layers", else the current layer, through the
//!   filter; with all visible layers the items on no visible layer are
//!   dropped afterwards.

use std::collections::BTreeSet;

use freerouting_geometry::{Area, FloatPoint, IntBox, Point, Shape};

use super::settings::EditSettings;
use crate::board::items::{ItemId, StopConnectionOption};
use crate::board::{BasicBoard, ItemFilter};

/// How a selection is extended (Java's `extentToWhole*`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Extent {
    Nets,
    Components,
    ConnectedSets,
    Connections,
}

/// Java `GuiBoardInteractionController.pickItems(location, filter)`.
pub fn pick_items(
    board: &BasicBoard,
    settings: &EditSettings,
    at: FloatPoint,
    filter: &ItemFilter,
) -> BTreeSet<ItemId> {
    let location = Point::Int(at.round());
    let mut result = board.pick_items_filtered(&location, settings.layer, Some(filter));
    if result.is_empty() && settings.select_on_all_visible_layers {
        for layer in 0..board.layer_count() as i32 {
            if layer == settings.layer || !settings.visible_layers.is_visible(layer) {
                continue;
            }
            result.extend(board.pick_items_filtered(&location, layer, Some(filter)));
        }
    }
    result
}

/// Java `InspectedItemState.toggleSelect` with the items `picked` at the
/// pointer: one picked item is removed if selected, else added; several do
/// nothing. `None`: the state ends (nothing picked, or the last item
/// removed).
pub fn toggle(current: &BTreeSet<ItemId>, picked: &BTreeSet<ItemId>) -> Option<BTreeSet<ItemId>> {
    let mut items = current.clone();
    let mut ended = picked.is_empty();
    if picked.len() == 1 {
        let id = *picked.iter().next().expect("one picked item");
        if items.remove(&id) {
            ended |= items.is_empty();
        } else {
            items.insert(id);
        }
    }
    (!ended).then_some(items)
}

fn is_connectable(board: &BasicBoard, id: ItemId) -> bool {
    board.item(id).is_some_and(|i| i.is_connectable_kind())
}

/// Java `extentToWholeNets`.
pub fn extend_to_nets(
    board: &BasicBoard,
    current: &BTreeSet<ItemId>,
    filter: &ItemFilter,
) -> Option<BTreeSet<ItemId>> {
    let nets: BTreeSet<i32> = current
        .iter()
        .filter(|id| is_connectable(board, **id))
        .filter_map(|id| board.item(*id))
        .flat_map(|item| item.net_numbers().iter().copied())
        .collect();
    let items: BTreeSet<ItemId> = nets
        .into_iter()
        .flat_map(|net| board.connectable_items(net))
        .collect();
    (!items.is_empty()).then(|| filter.filter(board, &items))
}

/// Java `extentToWholeComponents` (no filter afterwards).
pub fn extend_to_components(
    board: &BasicBoard,
    current: &BTreeSet<ItemId>,
) -> Option<BTreeSet<ItemId>> {
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
    (!items.is_empty()).then_some(items)
}

/// Java `extentToWholeConnectedSets`.
pub fn extend_to_connected_sets(
    board: &BasicBoard,
    current: &BTreeSet<ItemId>,
    filter: &ItemFilter,
) -> Option<BTreeSet<ItemId>> {
    let items: BTreeSet<ItemId> = current
        .iter()
        .filter(|id| is_connectable(board, **id))
        .flat_map(|id| board.connected_set(*id, -1, false))
        .collect();
    (!items.is_empty()).then(|| filter.filter(board, &items))
}

/// Java `extentToWholeConnections`.
pub fn extend_to_connections(
    board: &BasicBoard,
    current: &BTreeSet<ItemId>,
    filter: &ItemFilter,
) -> Option<BTreeSet<ItemId>> {
    let items: BTreeSet<ItemId> = current
        .iter()
        .filter(|id| is_connectable(board, **id))
        .flat_map(|id| board.connection_items(*id, StopConnectionOption::None))
        .collect();
    (!items.is_empty()).then(|| filter.filter(board, &items))
}

/// One of the four extensions.
pub fn extend(
    board: &BasicBoard,
    current: &BTreeSet<ItemId>,
    extent: Extent,
    filter: &ItemFilter,
) -> Option<BTreeSet<ItemId>> {
    match extent {
        Extent::Nets => extend_to_nets(board, current, filter),
        Extent::Components => extend_to_components(board, current),
        Extent::ConnectedSets => extend_to_connected_sets(board, current, filter),
        Extent::Connections => extend_to_connections(board, current, filter),
    }
}

/// Java `InspectedItemState.filter`: `None` if nothing is left.
pub fn filter(
    board: &BasicBoard,
    current: &BTreeSet<ItemId>,
    filter: &ItemFilter,
) -> Option<BTreeSet<ItemId>> {
    let items = filter.filter(board, current);
    (!items.is_empty()).then_some(items)
}

/// Java `InspectItemsInRegionState.createSelectionBox`: the box of the two
/// rounded corners.
pub fn region_box(corner1: FloatPoint, corner2: FloatPoint) -> IntBox {
    let (a, b) = (corner1.round(), corner2.round());
    IntBox::new(a.x.min(b.x), a.y.min(b.y), a.x.max(b.x), a.y.max(b.y))
}

/// Java `InspectItemsInRegionState.selectAllInRegion`'s search: the items in
/// `region` through the filter (empty: Java's `nothing_selected`).
pub fn select_in_region(
    board: &BasicBoard,
    settings: &EditSettings,
    region: IntBox,
    filter: &ItemFilter,
) -> BTreeSet<ItemId> {
    let all_visible = settings.select_on_all_visible_layers;
    let layer = if all_visible { -1 } else { settings.layer };
    let area = Area::from(Shape::from(region));
    let found = filter.filter(board, &board.overlapping_items(&area, layer));
    if !all_visible {
        return found;
    }
    found
        .into_iter()
        .filter(|id| {
            board.item(*id).is_some_and(|item| {
                (item.first_layer()..=item.last_layer())
                    .any(|l| settings.visible_layers.is_visible(l))
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(ids: &[i32]) -> BTreeSet<ItemId> {
        ids.iter().map(|i| ItemId(*i)).collect()
    }

    #[test]
    fn toggle_adds_removes_and_ends() {
        let current = set(&[1, 2]);
        assert_eq!(toggle(&current, &set(&[7])), Some(set(&[1, 2, 7])));
        assert_eq!(toggle(&set(&[1, 2, 7]), &set(&[7])), Some(current.clone()));
        // several items under the pointer: nothing happens
        assert_eq!(toggle(&current, &set(&[8, 9])), Some(current.clone()));
        assert_eq!(toggle(&current, &set(&[])), None);
        assert_eq!(toggle(&set(&[4]), &set(&[4])), None);
    }

    #[test]
    fn region_box_rounds_and_orders_the_corners() {
        let b = region_box(FloatPoint::new(10.6, -3.2), FloatPoint::new(-4.4, 7.5));
        assert_eq!(b, IntBox::new(-4, -3, 11, 8));
    }
}

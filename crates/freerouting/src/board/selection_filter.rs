//! Item selection filters and filtered picks (B04.2).
//!
//! Port of `board.actions.ItemSelectionFilter` and the per-class
//! `Item.isSelectedByFilter` overrides, plus the filtered form of
//! `BasicBoard.pickItems`.
//!
//! [`ItemFilter`] is a bitset over Java's `SelectableChoices`, in Java's
//! order. An item is selected if its kind's choice is set and, for every kind,
//! `FIXED` (user-fixed items) or `UNFIXED` (all others) is set too. A
//! component outline is never selected. The constructors match Java's:
//! [`ItemFilter::default`] (everything but the keepouts, conduction areas and
//! the board outline), [`ItemFilter::only`] and [`ItemFilter::of`] (the given
//! choices plus `FIXED` and `UNFIXED`).
//!
//! [`BasicBoard::pick_items_filtered`] returns a `BTreeSet<ItemId>`, which
//! iterates highest id first like Java's `TreeSet<Item>`.

use std::collections::BTreeSet;

use freerouting_geometry::Point;

use crate::board::BasicBoard;
use crate::board::items::{AreaKind, Item, ItemId, ItemKind};

/// The item classes and fixed states a filter can select (Java
/// `ItemSelectionFilter.SelectableChoices`, in Java's order).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum SelectableChoice {
    Traces,
    Vias,
    Pins,
    Conduction,
    Keepout,
    ViaKeepout,
    ComponentKeepout,
    BoardOutline,
    Fixed,
    Unfixed,
}

impl SelectableChoice {
    /// Every choice in Java's ordinal order.
    pub const ALL: [SelectableChoice; 10] = [
        SelectableChoice::Traces,
        SelectableChoice::Vias,
        SelectableChoice::Pins,
        SelectableChoice::Conduction,
        SelectableChoice::Keepout,
        SelectableChoice::ViaKeepout,
        SelectableChoice::ComponentKeepout,
        SelectableChoice::BoardOutline,
        SelectableChoice::Fixed,
        SelectableChoice::Unfixed,
    ];

    /// Java's constant name (`TRACES`, `VIA_KEEPOUT`, ...).
    pub fn java_name(self) -> &'static str {
        match self {
            SelectableChoice::Traces => "TRACES",
            SelectableChoice::Vias => "VIAS",
            SelectableChoice::Pins => "PINS",
            SelectableChoice::Conduction => "CONDUCTION",
            SelectableChoice::Keepout => "KEEPOUT",
            SelectableChoice::ViaKeepout => "VIA_KEEPOUT",
            SelectableChoice::ComponentKeepout => "COMPONENT_KEEPOUT",
            SelectableChoice::BoardOutline => "BOARD_OUTLINE",
            SelectableChoice::Fixed => "FIXED",
            SelectableChoice::Unfixed => "UNFIXED",
        }
    }

    fn bit(self) -> u16 {
        1 << (self as u16)
    }
}

/// Which item classes and fixed states a pick selects (Java
/// `ItemSelectionFilter`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ItemFilter {
    bits: u16,
}

impl Default for ItemFilter {
    /// Java `new ItemSelectionFilter()`: everything but the keepouts, the
    /// conduction areas and the board outline.
    fn default() -> Self {
        ItemFilter::all()
            .with(SelectableChoice::Keepout, false)
            .with(SelectableChoice::ViaKeepout, false)
            .with(SelectableChoice::ComponentKeepout, false)
            .with(SelectableChoice::Conduction, false)
            .with(SelectableChoice::BoardOutline, false)
    }
}

impl ItemFilter {
    /// Every choice selected (Java `selectAll`).
    pub fn all() -> Self {
        SelectableChoice::ALL
            .iter()
            .fold(ItemFilter::none(), |f, c| f.with(*c, true))
    }

    /// No choice selected (Java `deselectAll`).
    pub fn none() -> Self {
        ItemFilter { bits: 0 }
    }

    /// Java `new ItemSelectionFilter(choice)`: `choice`, `FIXED` and
    /// `UNFIXED`.
    pub fn only(choice: SelectableChoice) -> Self {
        ItemFilter::of(&[choice])
    }

    /// Java `new ItemSelectionFilter(choices)`: `choices`, `FIXED` and
    /// `UNFIXED`.
    pub fn of(choices: &[SelectableChoice]) -> Self {
        choices
            .iter()
            .fold(ItemFilter::none(), |f, c| f.with(*c, true))
            .with(SelectableChoice::Fixed, true)
            .with(SelectableChoice::Unfixed, true)
    }

    /// This filter with `choice` set to `value` (Java `setSelected` on a
    /// copy).
    #[must_use]
    pub fn with(self, choice: SelectableChoice, value: bool) -> Self {
        let bits = if value {
            self.bits | choice.bit()
        } else {
            self.bits & !choice.bit()
        };
        ItemFilter { bits }
    }

    /// Java `isSelected`.
    pub fn is_selected(&self, choice: SelectableChoice) -> bool {
        self.bits & choice.bit() != 0
    }

    /// The items of `items` the filter selects (Java `filter`), highest id
    /// first.
    pub fn filter<'a>(
        &self,
        board: &BasicBoard,
        items: impl IntoIterator<Item = &'a ItemId>,
    ) -> BTreeSet<ItemId> {
        items
            .into_iter()
            .filter(|id| board.item(**id).is_some_and(|i| i.is_selected_by(self)))
            .copied()
            .collect()
    }
}

impl Item {
    /// The choice that selects this item's class, `None` for a component
    /// outline, which no filter selects.
    pub fn selectable_choice(&self) -> Option<SelectableChoice> {
        Some(match &self.kind {
            ItemKind::Pin(_) => SelectableChoice::Pins,
            ItemKind::Via(_) => SelectableChoice::Vias,
            ItemKind::Trace(_) => SelectableChoice::Traces,
            ItemKind::Area(area) => match area.kind {
                // A trace-only keepout (Q09) is a keepout to the filter.
                AreaKind::Obstacle | AreaKind::TraceObstacle => SelectableChoice::Keepout,
                AreaKind::ViaObstacle => SelectableChoice::ViaKeepout,
                AreaKind::ComponentObstacle => SelectableChoice::ComponentKeepout,
                AreaKind::Conduction { .. } => SelectableChoice::Conduction,
            },
            ItemKind::BoardOutline(_) => SelectableChoice::BoardOutline,
            ItemKind::ComponentOutline(_) => return None,
        })
    }

    /// Java `Item.isSelectedByFilter`: the fixed state's choice (`FIXED` for
    /// user-fixed items, else `UNFIXED`) and the class's choice are set.
    pub fn is_selected_by(&self, filter: &ItemFilter) -> bool {
        let Some(choice) = self.selectable_choice() else {
            return false;
        };
        let fixed_choice = if self.is_user_fixed() {
            SelectableChoice::Fixed
        } else {
            SelectableChoice::Unfixed
        };
        filter.is_selected(fixed_choice) && filter.is_selected(choice)
    }
}

impl BasicBoard {
    /// Java `BasicBoard.pickItems(Point, int, ItemSelectionFilter)`: the
    /// items whose shape on `layer` (any layer if negative) contains
    /// `location`, restricted to the items `filter` selects if given.
    pub fn pick_items_filtered(
        &self,
        location: &Point,
        layer: i32,
        filter: Option<&ItemFilter>,
    ) -> BTreeSet<ItemId> {
        let picked = self.pick_items(location, layer);
        match filter {
            Some(filter) => filter.filter(self, &picked),
            None => picked,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::board::items::{
        BoardOutline, ComponentOutline, DrillData, ObstacleArea, Pin, PolylineTrace, Via,
    };
    use crate::board::structure::FixedState;
    use crate::library::PadstackId;
    use freerouting_geometry::{Area, IntBox, IntPoint, IntVector, Polyline, Shape, Vector};

    fn drill() -> DrillData {
        DrillData {
            center: IntPoint { x: 0, y: 0 },
            padstack: PadstackId(0),
            first_layer: 0,
            last_layer: 0,
            shapes: Vec::new(),
            hole: None,
        }
    }

    fn area_kind(kind: AreaKind) -> ItemKind {
        let area = Area::from(Shape::from(IntBox::new(0, 0, 10, 10)));
        ItemKind::Area(ObstacleArea::new(
            kind,
            area,
            0,
            Vector::Int(IntVector::ZERO),
            0.0,
            false,
            None,
        ))
    }

    /// One item of every kind with the choice that selects it.
    fn kinds() -> Vec<(ItemKind, Option<SelectableChoice>)> {
        let area = Area::from(Shape::from(IntBox::new(0, 0, 10, 10)));
        let corners = [
            IntPoint { x: 0, y: 0 }.into(),
            IntPoint { x: 9, y: 0 }.into(),
        ];
        vec![
            (
                ItemKind::Pin(Pin {
                    pin_index: 0,
                    drill: drill(),
                    changed_to: ItemId(0),
                }),
                Some(SelectableChoice::Pins),
            ),
            (
                ItemKind::Via(Via {
                    attach_allowed: false,
                    is_escape_via: false,
                    escape_via_smd_layer: -1,
                    drill: drill(),
                }),
                Some(SelectableChoice::Vias),
            ),
            (
                ItemKind::Trace(PolylineTrace {
                    polyline: Polyline::from_corners(&corners[0], &corners[1]),
                    layer: 0,
                    half_width: 1,
                }),
                Some(SelectableChoice::Traces),
            ),
            (
                area_kind(AreaKind::Obstacle),
                Some(SelectableChoice::Keepout),
            ),
            (
                area_kind(AreaKind::ViaObstacle),
                Some(SelectableChoice::ViaKeepout),
            ),
            (
                area_kind(AreaKind::ComponentObstacle),
                Some(SelectableChoice::ComponentKeepout),
            ),
            (
                area_kind(AreaKind::Conduction {
                    is_obstacle: true,
                    is_filled: true,
                }),
                Some(SelectableChoice::Conduction),
            ),
            (
                ItemKind::ComponentOutline(ComponentOutline::new(
                    area,
                    true,
                    Vector::Int(IntVector::ZERO),
                    0.0,
                    false,
                    false,
                    true,
                    2,
                )),
                None,
            ),
            (
                ItemKind::BoardOutline(BoardOutline::new(Vec::new(), 2)),
                Some(SelectableChoice::BoardOutline),
            ),
        ]
    }

    /// Java `isSelectedByFilter` per class: the class's choice and the
    /// fixed state's choice (`FIXED` from `USER_FIXED` up); a component
    /// outline never.
    #[test]
    fn every_item_kind_and_fixed_state() {
        let states = [
            (FixedState::Unfixed, SelectableChoice::Unfixed),
            (FixedState::ShoveFixed, SelectableChoice::Unfixed),
            (FixedState::UserFixed, SelectableChoice::Fixed),
            (FixedState::SystemFixed, SelectableChoice::Fixed),
        ];
        for (kind, choice) in kinds() {
            assert_eq!(
                Item::new(ItemId(1), vec![1], 1, 0, FixedState::Unfixed, kind.clone())
                    .selectable_choice(),
                choice
            );
            for (state, fixed_choice) in states {
                let item = Item::new(ItemId(1), vec![1], 1, 0, state, kind.clone());
                for class in SelectableChoice::ALL {
                    for fixed in [SelectableChoice::Fixed, SelectableChoice::Unfixed] {
                        let filter = ItemFilter::none().with(class, true).with(fixed, true);
                        let expected = choice == Some(class) && fixed == fixed_choice;
                        assert_eq!(
                            item.is_selected_by(&filter),
                            expected,
                            "{choice:?} {state:?} filter {class:?}+{fixed:?}"
                        );
                    }
                }
                assert_eq!(
                    item.is_selected_by(&ItemFilter::all()),
                    choice.is_some(),
                    "{choice:?} {state:?}"
                );
                if let Some(c) = choice {
                    assert!(item.is_selected_by(&ItemFilter::only(c)));
                    assert!(!item.is_selected_by(&ItemFilter::only(c).with(fixed_choice, false)));
                }
            }
        }
    }

    #[test]
    fn default_filter_matches_java_constructor() {
        let f = ItemFilter::default();
        for c in SelectableChoice::ALL {
            let expected = !matches!(
                c,
                SelectableChoice::Keepout
                    | SelectableChoice::ViaKeepout
                    | SelectableChoice::ComponentKeepout
                    | SelectableChoice::Conduction
                    | SelectableChoice::BoardOutline
            );
            assert_eq!(f.is_selected(c), expected, "{c:?}");
        }
    }

    #[test]
    fn single_choice_filter_adds_fixed_states() {
        let f = ItemFilter::only(SelectableChoice::Vias);
        let selected: Vec<_> = SelectableChoice::ALL
            .into_iter()
            .filter(|c| f.is_selected(*c))
            .collect();
        assert_eq!(
            selected,
            [
                SelectableChoice::Vias,
                SelectableChoice::Fixed,
                SelectableChoice::Unfixed
            ]
        );
        assert_eq!(
            ItemFilter::of(&[SelectableChoice::Vias, SelectableChoice::Pins]),
            f.with(SelectableChoice::Pins, true)
        );
        assert_eq!(
            ItemFilter::none().with(SelectableChoice::Pins, true).bits,
            4
        );
        assert!(
            !ItemFilter::all()
                .with(SelectableChoice::Pins, false)
                .is_selected(SelectableChoice::Pins)
        );
    }
}

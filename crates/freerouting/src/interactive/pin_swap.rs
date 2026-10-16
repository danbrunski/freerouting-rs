//! Pin swap (I03.3): Java's `MenuState.swapPins` and `PinSwapState`
//! (`gui/interactive/{MenuState,PinSwapState}.java` at `a773a70ec`), with
//! `Pin.getSwappablePins` ([`BasicBoard::swappable_pins`]), `Pin.swap`
//! ([`BasicBoard::swap_pins`]) and `RoutingBoard.checkChangeNet`
//! ([`RoutingBoard::check_change_net`]).
//!
//! Order of [`PinSwap::complete`] (exact): pins with more than one net are
//! refused (no status, Java only logs); `check_change_net` of the first pin
//! to the second pin's net (−1 when it has none), then the reverse, each
//! refusal with its own status key; a snapshot; the swap; the nets of both
//! pins after the swap are the changed nets.

use std::collections::BTreeSet;

use freerouting_geometry::FloatPoint;

use super::Status;
use super::select::pick_items;
use super::settings::EditSettings;
use crate::board::items::ItemId;
use crate::board::{BasicBoard, ItemFilter, RoutingBoard, SelectableChoice};

/// A pin swap waiting for its second pin (Java `PinSwapState`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PinSwap {
    from: ItemId,
    swappable: BTreeSet<ItemId>,
}

/// What [`PinSwap::complete`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SwapOutcome {
    /// True if the nets were swapped.
    pub swapped: bool,
    /// Java's status message, if it sets one.
    pub status: Option<Status>,
    /// The nets of both pins after the swap (Java's `updateRatsnest` calls).
    pub changed_nets: BTreeSet<i32>,
}

impl SwapOutcome {
    fn refused(status: Option<Status>) -> Self {
        SwapOutcome {
            swapped: false,
            status,
            changed_nets: BTreeSet::new(),
        }
    }
}

fn pins_filter() -> ItemFilter {
    ItemFilter::only(SelectableChoice::Pins)
}

fn status(key: &'static str) -> Option<Status> {
    Some(Status::new(key, []))
}

impl PinSwap {
    /// Java `MenuState.swapPins` and `PinSwapState.getInstance`: picks a pin
    /// at `at` (PINS filter, all visible layers fallback). `Err` carries the
    /// status (`no_pin_selected`, `no_swappable_pin_found`; none when the
    /// first picked item is not a pin, which Java only logs).
    pub fn start(
        board: &BasicBoard,
        settings: &EditSettings,
        at: FloatPoint,
    ) -> Result<(PinSwap, Status), Option<Status>> {
        let picked = pick_items(board, settings, at, &pins_filter());
        let Some(&first) = picked.iter().next() else {
            return Err(status("no_pin_selected"));
        };
        if !board.item(first).is_some_and(|i| i.is_pin()) {
            return Err(None);
        }
        let swappable = board.swappable_pins(first);
        if swappable.is_empty() {
            return Err(status("no_swappable_pin_found"));
        }
        Ok((
            PinSwap {
                from: first,
                swappable,
            },
            Status::new("please_click_second_pin_with_the_left_mouse_button", []),
        ))
    }

    /// A swap of `from` without the pick, for scripts and probes; `None` if
    /// it has no swappable pin.
    pub fn for_pin(board: &BasicBoard, from: ItemId) -> Option<PinSwap> {
        let swappable = board.swappable_pins(from);
        (!swappable.is_empty()).then_some(PinSwap { from, swappable })
    }

    /// The first pin.
    pub fn from_pin(&self) -> ItemId {
        self.from
    }

    /// The pins the first can be swapped with, highest id first.
    pub fn swappable_pins(&self) -> &BTreeSet<ItemId> {
        &self.swappable
    }

    /// Java `PinSwapState.leftButtonClicked`: picks the second pin at `at`
    /// and completes the swap if it is one of the swappable pins.
    pub fn click(
        self,
        board: &mut RoutingBoard,
        settings: &EditSettings,
        at: FloatPoint,
    ) -> SwapOutcome {
        let picked = pick_items(board, settings, at, &pins_filter());
        let Some(&to) = picked.iter().next() else {
            return SwapOutcome::refused(status("no_pin_selected"));
        };
        if !board.item(to).is_some_and(|i| i.is_pin()) {
            return SwapOutcome::refused(status("picked_pin_expected"));
        }
        if !self.swappable.contains(&to) {
            return SwapOutcome::refused(None);
        }
        self.complete(board, Some(to))
    }

    /// Java `PinSwapState.complete` with the second pin `to` (`None`:
    /// `pin_to_swap_missing`).
    pub fn complete(self, board: &mut RoutingBoard, to: Option<ItemId>) -> SwapOutcome {
        let (Some(from_item), Some(to_item)) =
            (board.item(self.from), to.and_then(|id| board.item(id)))
        else {
            return SwapOutcome::refused(status("pin_to_swap_missing"));
        };
        let to = to_item.id();
        if from_item.net_count() > 1 || to_item.net_count() > 1 {
            board.log.push(
                "PinSwapState.complete: pin swap is not implemented for pins belonging to more \
                 than one net."
                    .into(),
            );
            return SwapOutcome::refused(None);
        }
        let from_net = from_item.net_numbers().first().copied().unwrap_or(-1);
        let to_net = to_item.net_numbers().first().copied().unwrap_or(-1);
        if !board.check_change_net(self.from, to_net) {
            return SwapOutcome::refused(status("pin_not_swapped_because_it_is_already_connected"));
        }
        if !board.check_change_net(to, from_net) {
            return SwapOutcome::refused(status(
                "pin_not_swapped_because_second_pin_is_already_connected",
            ));
        }
        board.generate_snapshot();
        let swapped = board.swap_pins(self.from, to);
        let changed_nets = [self.from, to]
            .iter()
            .filter_map(|id| board.item(*id))
            .flat_map(|item| item.net_numbers().iter().copied())
            .collect();
        SwapOutcome {
            swapped,
            status: status("pin_swap_completed"),
            changed_nets,
        }
    }
}

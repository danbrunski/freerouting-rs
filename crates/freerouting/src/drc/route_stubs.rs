//! The route stubs list (B08.3): the traces and vias that end in the air.
//!
//! Port of `WindowRouteStubs.fillList` (`gui/windows/routing/WindowRouteStubs.java`,
//! lines 36-100) and `RouteStubInfo.compareTo` (lines 156-169). Items come in
//! item-list order (highest id first) and only traces and vias with exactly one
//! net count:
//! - a via is a stub if it has no contacts, or all its `getAllContacts()` have
//!   the same first and last layer. Its layer is its own first layer without
//!   contacts; with contacts it is the first contact's first layer when the
//!   via's range passes Java's test at lines 72-77, else the contact's last
//!   layer. The test is copied as written: it compares *both* of the via's
//!   layer bounds with the contact's first layer
//!   (`via.firstLayer >= c.firstLayer && via.lastLayer <= c.firstLayer`).
//! - a trace is a stub at its first corner if it has no start contacts, else at
//!   its last corner if it has no end contacts.
//!
//! The list is a `TreeSet` ordered by net name (Java `String.compareTo`, UTF-16
//! code units), then the sign of the x difference, the y difference and the
//! layer difference, where x and y are the *user* coordinates of the location
//! (`CoordinateTransform.boardToUser`, the board's units scaled to DSN units
//! without the file's origin offset). An entry equal to an earlier one in this
//! order is dropped, as `TreeSet.add` does.
//!
//! The window needs a GUI, so the Java side of the comparison
//! (`tools/java-reference/probes/board/RouteStubsProbe.java`) is a stand-in with
//! a copy of the window's logic; `tests/drc.rs` compares this list with it.

use std::cmp::Ordering;

use freerouting_geometry::FloatPoint;

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};

/// What kind of item a stub is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteStubKind {
    Trace,
    Via,
}

/// One entry of the route stubs list.
#[derive(Clone, Debug, PartialEq)]
pub struct RouteStub {
    pub item: ItemId,
    pub kind: RouteStubKind,
    /// The net number of the item (its only net).
    pub net: i32,
    pub net_name: String,
    /// The stub's location in board units.
    pub location: FloatPoint,
    /// The location in user coordinates (Java `boardToUser`), which the list is
    /// ordered by.
    pub user_location: FloatPoint,
    pub layer: i32,
}

/// Java `CoordinateTransform.boardToUser(double)` as `GuiBoardManager` builds
/// the transform: the scale factor `boardToDsn(1)`, then
/// `Unit.scale(v * factor, unit, unit)` = `v * factor * um / um`, in Java's
/// operation order.
pub fn board_to_user(board: &BasicBoard, value: f64) -> f64 {
    let factor = board.communication.coordinate_transform.board_to_dsn(1.0);
    let um = board.communication.unit.micrometers();
    value * factor * um / um
}

/// The route stubs of `board` in the window's order. See the module docs.
pub fn route_stubs(board: &BasicBoard) -> Vec<RouteStub> {
    let mut stubs: Vec<RouteStub> = board
        .items()
        .filter_map(|item| candidate(board, item))
        .collect();
    // A stable sort keeps the first of equal entries first; `dedup_by` then keeps
    // it, as `TreeSet.add` keeps the entry that was there.
    stubs.sort_by(compare);
    stubs.dedup_by(|later, earlier| compare(earlier, later) == Ordering::Equal);
    stubs
}

fn candidate(board: &BasicBoard, item: &Item) -> Option<RouteStub> {
    if item.net_count() != 1 {
        return None;
    }
    let (kind, location, layer) = match &item.kind {
        ItemKind::Via(via) => (
            RouteStubKind::Via,
            via.drill.center.to_float(),
            via_stub_layer(board, item)?,
        ),
        ItemKind::Trace(trace) => {
            let location = if board.trace_start_contacts(item.id()).is_empty() {
                trace.first_corner().to_float()
            } else if board.trace_end_contacts(item.id()).is_empty() {
                trace.last_corner().to_float()
            } else {
                return None;
            };
            (RouteStubKind::Trace, location, trace.layer)
        }
        _ => return None,
    };
    let net = item.net_number(0);
    Some(RouteStub {
        item: item.id(),
        kind,
        net,
        net_name: board
            .rules
            .nets
            .get(net)
            .map(|n| n.name.clone())
            .unwrap_or_default(),
        location,
        user_location: FloatPoint::new(
            board_to_user(board, location.x),
            board_to_user(board, location.y),
        ),
        layer,
    })
}

/// The layer of a via stub; `None` if the via is no stub.
fn via_stub_layer(board: &BasicBoard, via: &Item) -> Option<i32> {
    let contacts = board.all_contacts(via.id(), None);
    let mut layers = contacts.iter().map(|id| {
        let contact = board.item(*id).expect("a contact is on the board");
        (contact.first_layer(), contact.last_layer())
    });
    let Some((first_contact_first, first_contact_last)) = layers.next() else {
        return Some(via.first_layer());
    };
    if layers.any(|l| l != (first_contact_first, first_contact_last)) {
        return None;
    }
    // PARITY(quirk, java_quirks.route_stubs_layer_test): WindowRouteStubs.java:72-77
    // compares both via bounds with the contact's first layer. Left as Java by N06.12
    // (which layer a via stub should name is unclear), so the switch is inert:
    // `false` changes nothing.
    if via.first_layer() >= first_contact_first && via.last_layer() <= first_contact_first {
        Some(first_contact_first)
    } else {
        Some(first_contact_last)
    }
}

/// `RouteStubInfo.compareTo`.
fn compare(a: &RouteStub, b: &RouteStub) -> Ordering {
    a.net_name
        .encode_utf16()
        .cmp(b.net_name.encode_utf16())
        .then_with(|| sign(a.user_location.x - b.user_location.x))
        .then_with(|| sign(a.user_location.y - b.user_location.y))
        .then_with(|| a.layer.cmp(&b.layer))
}

/// Java `Signum.asInt(double)`.
fn sign(difference: f64) -> Ordering {
    if difference > 0.0 {
        Ordering::Greater
    } else if difference < 0.0 {
        Ordering::Less
    } else {
        Ordering::Equal
    }
}

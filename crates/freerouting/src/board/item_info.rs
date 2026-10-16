//! The fields of the item info window (B08.2).
//!
//! Port of `Item.printInfo(ItemInfoPrinter, Locale)` of each class (`Pin.java:608`,
//! `Via.java:242`, `Trace.java:413`, `ConductionArea.java:425`,
//! `ObstacleArea.java:311`, `ViaObstacleArea.java:113`,
//! `ComponentObstacleArea.java:90`, `ComponentOutline.java:233`,
//! `BoardOutline.java:360`) with the helpers of `Item` (`printNetInfo`,
//! `printClearanceInfo`, `printFixedInfo`, `printContactInfo`,
//! `printClearanceViolationInfo`, `printConnectableItemInfo`, `printItemInfo`,
//! `Item.java:1169-1248`) and `ObstacleArea.printShapeInfo` (`:329`).
//!
//! Java prints text through an `ItemInfoPrinter`. Here the result is data:
//! [`ItemInfo::fields`] lists the values Java prints, in Java's order, and the
//! GUI turns them into localized text. The words Java puts between the values
//! ("at", "on layer", "net", ...) are the GUI's. Locations are in board units
//! (Java transforms them to user coordinates when it prints).
//!
//! What each kind lists, in order (`[...]` only when Java prints it):
//! - pin: component, pin name, padstack, location, then the connectable fields;
//! - via: location, padstack, connectable fields;
//! - trace: first corner, last corner, layer, width, length, connectable fields;
//! - conduction area: centre of gravity, [number of holes], layer, connectable
//!   fields;
//! - keepout: [component], centre of gravity, [holes], layer, then the item
//!   fields (clearance class, fixed state, violations);
//! - via keepout, component keepout: centre of gravity, [holes], layer, clearance
//!   class, violations;
//! - board outline: clearance class; component outline: nothing.
//!
//! The connectable fields are the clearance class, the fixed state, one net per
//! net of the item, the normal contacts and the clearance violations. The
//! clearance class is listed only for a class above 0 (`printClearanceInfo`),
//! the fixed state only if the item is not unfixed, the contacts and violations
//! only if there are some.
//!
//! Not in Java's text, so not fields: the item id and the area name; they are
//! plain members of [`ItemInfo`] for the GUI's use.

use freerouting_geometry::FloatPoint;

use crate::board::basic_board::BasicBoard;
use crate::board::items::{AreaKind, BoardItemType, Item, ItemId, ItemKind};
use crate::board::structure::{ComponentId, FixedState};
use crate::drc::ClearanceViolation;
use crate::library::PadstackId;

/// One value of the info text. See the module docs for the order.
#[derive(Clone, Debug)]
pub enum InfoField {
    /// The component of a pin, or of a keepout that belongs to one.
    Component {
        id: i32,
        name: String,
    },
    /// The name of a pin in its package.
    PinName(String),
    Padstack {
        id: PadstackId,
        name: String,
    },
    /// The pin or via centre, or the centre of gravity of an area.
    Location(FloatPoint),
    /// The first corner of a trace.
    From(FloatPoint),
    /// The last corner of a trace.
    To(FloatPoint),
    /// The name of the layer.
    Layer(String),
    /// The width of a trace, `2 * half width`, in board units.
    Width(f64),
    /// The length of a trace, in board units.
    Length(f64),
    /// The number of holes of an area with holes.
    HoleCount(usize),
    /// The clearance class name; only for a class above 0.
    ClearanceClass {
        index: i32,
        name: String,
    },
    /// The fixed state; only if the item is not unfixed.
    Fixed(FixedState),
    /// One of the nets of a connectable item.
    Net {
        number: i32,
        name: String,
    },
    /// The normal contacts, in Java's `TreeSet` order (highest id first).
    Contacts(Vec<ItemId>),
    /// The clearance violations of the item; only if there are some.
    ClearanceViolations(Vec<ClearanceViolation>),
}

/// The info of one item.
#[derive(Clone, Debug)]
pub struct ItemInfo {
    pub id: ItemId,
    pub kind: BoardItemType,
    /// The name of an area, if it has one (not printed by Java).
    pub name: Option<String>,
    /// The printed values, in Java's order.
    pub fields: Vec<InfoField>,
}

impl BasicBoard {
    /// The info of item `id` as Java's `printInfo` prints it; `None` if the
    /// item is not on the board.
    pub fn item_info(&self, id: ItemId) -> Option<ItemInfo> {
        let item = self.item(id)?;
        let mut fields = Vec::new();
        let mut name = None;
        match &item.kind {
            ItemKind::Pin(pin) => {
                self.push_component(item, &mut fields);
                if let Some(pin_name) = self.pin_name(item) {
                    fields.push(InfoField::PinName(pin_name.to_string()));
                }
                self.push_padstack(pin.drill.padstack, &mut fields);
                fields.push(InfoField::Location(pin.drill.center.to_float()));
                self.push_connectable(item, &mut fields);
            }
            ItemKind::Via(via) => {
                fields.push(InfoField::Location(via.drill.center.to_float()));
                self.push_padstack(via.drill.padstack, &mut fields);
                self.push_connectable(item, &mut fields);
            }
            ItemKind::Trace(trace) => {
                fields.push(InfoField::From(trace.first_corner().to_float()));
                fields.push(InfoField::To(trace.last_corner().to_float()));
                fields.push(InfoField::Layer(self.layer_name(trace.layer)));
                fields.push(InfoField::Width(f64::from(2 * trace.half_width)));
                fields.push(InfoField::Length(trace.length()));
                self.push_connectable(item, &mut fields);
            }
            ItemKind::Area(area) => {
                name = area.name.clone();
                match area.kind {
                    AreaKind::Conduction { .. } => {
                        self.push_shape_info(item, &mut fields);
                        self.push_connectable(item, &mut fields);
                    }
                    AreaKind::Obstacle => {
                        if item.component_id() > 0 {
                            self.push_component(item, &mut fields);
                        }
                        self.push_shape_info(item, &mut fields);
                        self.push_clearance_class(item, &mut fields);
                        self.push_fixed(item, &mut fields);
                        self.push_violations(item, &mut fields);
                    }
                    AreaKind::ViaObstacle | AreaKind::ComponentObstacle => {
                        self.push_shape_info(item, &mut fields);
                        self.push_clearance_class(item, &mut fields);
                        self.push_violations(item, &mut fields);
                    }
                }
            }
            ItemKind::ComponentOutline(_) => {}
            ItemKind::BoardOutline(_) => self.push_clearance_class(item, &mut fields),
        }
        Some(ItemInfo {
            id,
            kind: item.board_item_type(),
            name,
            fields,
        })
    }

    fn layer_name(&self, layer: i32) -> String {
        usize::try_from(layer)
            .ok()
            .and_then(|l| self.layer_structure.layers.get(l))
            .map(|l| l.name.clone())
            .unwrap_or_default()
    }

    fn push_component(&self, item: &Item, fields: &mut Vec<InfoField>) {
        let id = item.component_id();
        let name = self
            .components
            .get(ComponentId(id))
            .map(|c| c.name.clone())
            .unwrap_or_default();
        fields.push(InfoField::Component { id, name });
    }

    fn push_padstack(&self, id: PadstackId, fields: &mut Vec<InfoField>) {
        let name = self
            .library
            .padstacks
            .get(id)
            .map(|p| p.name.clone())
            .unwrap_or_default();
        fields.push(InfoField::Padstack { id, name });
    }

    /// `ObstacleArea.printShapeInfo`.
    fn push_shape_info(&self, item: &Item, fields: &mut Vec<InfoField>) {
        let ItemKind::Area(area) = &item.kind else {
            return;
        };
        fields.push(InfoField::Location(
            area.area().get_border().centre_of_gravity(),
        ));
        let holes = area.relative_area().get_holes().len();
        if holes > 0 {
            fields.push(InfoField::HoleCount(holes));
        }
        fields.push(InfoField::Layer(self.layer_name(area.layer())));
    }

    /// `printClearanceInfo`.
    fn push_clearance_class(&self, item: &Item, fields: &mut Vec<InfoField>) {
        let index = item.clearance_class();
        if index > 0 {
            let name = self
                .rules
                .clearance_matrix
                .get_name(index)
                .unwrap_or_default()
                .to_string();
            fields.push(InfoField::ClearanceClass { index, name });
        }
    }

    /// `printFixedInfo`.
    fn push_fixed(&self, item: &Item, fields: &mut Vec<InfoField>) {
        if item.fixed_state() != FixedState::Unfixed {
            fields.push(InfoField::Fixed(item.fixed_state()));
        }
    }

    /// `printClearanceViolationInfo`.
    fn push_violations(&self, item: &Item, fields: &mut Vec<InfoField>) {
        let violations = self.clearance_violations(item.id());
        if !violations.is_empty() {
            fields.push(InfoField::ClearanceViolations(violations));
        }
    }

    /// `printConnectableItemInfo`: clearance class, fixed state, nets,
    /// contacts, clearance violations.
    fn push_connectable(&self, item: &Item, fields: &mut Vec<InfoField>) {
        self.push_clearance_class(item, fields);
        self.push_fixed(item, fields);
        for &number in item.net_numbers() {
            let name = self
                .rules
                .nets
                .get(number)
                .map(|n| n.name.clone())
                .unwrap_or_default();
            fields.push(InfoField::Net { number, name });
        }
        let contacts: Vec<ItemId> = self.normal_contacts(item.id()).into_iter().collect();
        if !contacts.is_empty() {
            fields.push(InfoField::Contacts(contacts));
        }
        self.push_violations(item, fields);
    }
}

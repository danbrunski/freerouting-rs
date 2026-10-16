//! The placement scope: the components, grouped by package.
//!
//! Port of `Placement.writeScope`, `Package.writePlacementScope`, `Component.writeScope`,
//! `writePinInfo` and `writeKeepoutInfos`.

use std::collections::{HashMap, HashSet};

use freerouting_dsn::ast::{
    ComponentPlacement, ItemClearance, Place, PlaceLocation, Placement, Point as DsnPoint, Side,
};

use super::Cx;
use crate::board::items::{AreaKind, ItemKind};

/// The components of each package in component order that still own an item or are not placed,
/// for every package with at least one.
pub(super) fn placement(cx: &Cx) -> Placement {
    let board = cx.board;
    let owners: HashSet<i32> = board.items().map(|i| i.component_id()).collect();
    // The clearance class of every pin and the first keepout of each name, per component.
    let mut pin_classes: HashMap<(i32, usize), i32> = HashMap::new();
    let mut keepout_classes: HashMap<(i32, &str), i32> = HashMap::new();
    for item in cx.items.iter().copied() {
        if item.component_id() == 0 {
            continue;
        }
        match &item.kind {
            ItemKind::Pin(pin) => {
                pin_classes.insert((item.component_id(), pin.pin_index), item.clearance_class());
            }
            ItemKind::Area(area) => {
                if let Some(name) = area.name.as_deref() {
                    keepout_classes
                        .entry((item.component_id(), name))
                        .or_insert(item.clearance_class());
                }
            }
            _ => {}
        }
    }
    let mut by_package: Vec<Vec<usize>> = vec![Vec::new(); board.library.packages.count() + 1];
    let components: Vec<_> = board.components.iter().collect();
    for (index, component) in components.iter().enumerate() {
        let package = component.package().0;
        if package < by_package.len()
            && (owners.contains(&component.id.0) || !component.is_placed())
        {
            by_package[package].push(index);
        }
    }
    let mut result = Vec::new();
    for package in board.library.packages.iter() {
        let places: Vec<Place> = by_package[package.id.0]
            .iter()
            .map(|&index| {
                place(
                    cx,
                    components[index],
                    package,
                    &pin_classes,
                    &keepout_classes,
                )
            })
            .collect();
        if !places.is_empty() {
            result.push(ComponentPlacement {
                image: package.name.clone(),
                places,
            });
        }
    }
    Placement {
        resolution: None,
        flip_style_rotate_first: board.components.flip_style_rotate_first().then_some(true),
        components: result,
    }
}

fn place(
    cx: &Cx,
    component: &crate::board::structure::Component,
    package: &crate::library::Package,
    pin_classes: &HashMap<(i32, usize), i32>,
    keepout_classes: &HashMap<(i32, &str), i32>,
) -> Place {
    let location = component.exact_location().map(|at| {
        let [x, y] = cx.transform.board_to_dsn_point(at);
        PlaceLocation {
            at: DsnPoint { x, y },
            side: if component.placed_on_front() {
                Side::Front
            } else {
                Side::Back
            },
            rotation: component.rotation_in_degree(),
        }
    });
    let mut place = Place {
        name: component.name.clone(),
        location,
        position_locked: component.position_fixed,
        pins: Vec::new(),
        keepouts: Vec::new(),
        via_keepouts: Vec::new(),
        place_keepouts: Vec::new(),
        part_number: None,
    };
    if !component.is_placed() {
        return place;
    }
    let id = component.id.0;
    for (index, pin) in package.pins().iter().enumerate() {
        if let Some(class) = pin_classes.get(&(id, index)) {
            place.pins.push(ItemClearance {
                name: pin.name.clone(),
                clearance_class: cx.class_name(*class),
            });
        }
    }
    let kinds = [
        (&package.keepouts, AreaKind::Obstacle),
        (&package.via_keepouts, AreaKind::ViaObstacle),
        (&package.place_keepouts, AreaKind::ComponentObstacle),
    ];
    for (keepouts, kind) in kinds {
        for keepout in keepouts {
            let Some(class) = keepout_classes.get(&(id, keepout.name.as_str())) else {
                continue;
            };
            if *class == 0 {
                continue;
            }
            let info = ItemClearance {
                name: keepout.name.clone(),
                clearance_class: cx.class_name(*class),
            };
            match kind {
                AreaKind::Obstacle => place.keepouts.push(info),
                AreaKind::ViaObstacle => place.via_keepouts.push(info),
                _ => place.place_keepouts.push(info),
            }
        }
    }
    place
}

//! The library scope (images and padstacks) and the part library.
//!
//! Port of `Library.writeScope`, `Library.writePadstackScope`, `Package.writeScope`,
//! `Package.writePackageKeepout` and `PartLibrary.writeScope`.

use std::collections::HashMap;

use freerouting_dsn::ast::{
    Area as DsnArea, Image, ImagePin, Keepout, KeepoutKind, LayerRef, Library, LogicalPart,
    LogicalPartMapping, Padstack as DsnPadstack, PartLibrary, PartPin, Point as DsnPoint,
};
use freerouting_geometry::Shape;

use super::Cx;
use super::board_shape_to_dsn;
use crate::library::{Package, PackageKeepout, Padstack};

pub(super) fn library(cx: &Cx) -> Library {
    let board = cx.board;
    Library {
        images: board
            .library
            .packages
            .iter()
            .map(|package| image(cx, package))
            .collect(),
        padstacks: board
            .library
            .padstacks
            .iter()
            .filter_map(|padstack| padstack_scope(cx, padstack))
            .collect(),
    }
}

/// `Package.writeScope`.
fn image(cx: &Cx, package: &Package) -> Image {
    let board = cx.board;
    let pins = package
        .pins()
        .iter()
        .map(|pin| {
            let [x, y] = cx.transform.board_to_dsn_rel(pin.exact_relative_location());
            ImagePin {
                padstack: board
                    .library
                    .padstacks
                    .get(pin.padstack_id)
                    .map(|p| p.name.clone())
                    .unwrap_or_default(),
                name: pin.name.clone(),
                at: DsnPoint { x, y },
                rotation: (pin.rotation_in_degree != 0.0).then_some(pin.rotation_in_degree),
            }
        })
        .collect();
    let outlines = package
        .outline
        .iter()
        .flatten()
        .flatten()
        .filter_map(|shape| board_shape_to_dsn(shape, LayerRef::Signal, &cx.transform, true))
        .collect();
    let mut keepouts = package_keepouts(cx, &package.keepouts, KeepoutKind::Keepout, "keepout_");
    keepouts.extend(package_keepouts(
        cx,
        &package.via_keepouts,
        KeepoutKind::ViaKeepout,
        "via_keepout_",
    ));
    // Java leaves the place keepouts out.
    keepouts.extend(package_keepouts(
        cx,
        &package.place_keepouts,
        KeepoutKind::PlaceKeepout,
        "place_keepout_",
    ));
    Image {
        name: package.name.clone(),
        side: Some(if package.is_front {
            freerouting_dsn::ast::Side::Front
        } else {
            freerouting_dsn::ast::Side::Back
        }),
        pins,
        outlines,
        keepouts,
    }
}

/// `Package.writePackageKeepout` for each keepout of one kind. The reader names keepouts
/// `<prefix><n>` unless every keepout of the kind has a name, and the keepouts of a placed
/// component refer to those names, so the names are written when they are not the generated
/// ones.
fn package_keepouts(
    cx: &Cx,
    keepouts: &[PackageKeepout],
    kind: KeepoutKind,
    prefix: &str,
) -> Vec<Keepout> {
    let generated = keepouts
        .iter()
        .enumerate()
        .all(|(i, k)| k.name == format!("{prefix}{}", i + 1));
    keepouts
        .iter()
        .filter_map(|keepout| {
            let layer = if keepout.layer >= 0 {
                LayerRef::Named(cx.layer_name(keepout.layer))
            } else {
                LayerRef::Signal
            };
            let (shape, windows) = cx.area_shapes(&keepout.area, layer, false);
            Some(Keepout {
                kind,
                area: DsnArea {
                    name: (!generated).then(|| keepout.name.clone()),
                    shape: shape?,
                    windows,
                    clearance_class: None,
                },
            })
        })
        .collect()
}

/// `Library.writePadstackScope`: the shapes from the first to the last layer that has one,
/// each followed by its auxiliary shapes. `None` for a padstack without a shape.
fn padstack_scope(cx: &Cx, padstack: &Padstack) -> Option<DsnPadstack> {
    let layer_count = cx.board.layer_count() as i32;
    let first = (0..layer_count).find(|l| padstack.get_shape(*l).is_some())?;
    let last = (0..layer_count)
        .rev()
        .find(|l| padstack.get_shape(*l).is_some())?;
    let mut shapes = Vec::new();
    for layer in first..=last {
        let Some(shape) = padstack.get_shape(layer) else {
            continue;
        };
        let layer_ref = LayerRef::Named(cx.layer_name(layer));
        let mut convert = |shape: Shape| {
            shapes.extend(board_shape_to_dsn(
                &shape,
                layer_ref.clone(),
                &cx.transform,
                true,
            ));
        };
        convert(shape.to_shape());
        for aux in padstack.get_auxiliary_shapes(layer).unwrap_or_default() {
            convert(aux.to_shape());
        }
    }
    Some(DsnPadstack {
        name: padstack.name.clone(),
        shapes,
        attach: (!padstack.attach_allowed).then_some(false),
        absolute: padstack.placed_absolute.then_some(true),
    })
}

/// `PartLibrary.writeScope`; nothing without logical parts.
pub(super) fn part_library(cx: &Cx) -> Option<PartLibrary> {
    let board = cx.board;
    let parts = &board.library.logical_parts;
    if parts.count() == 0 {
        return None;
    }
    // The components of each part, in component order (Java scans all components per part).
    let mut components: HashMap<usize, Vec<String>> = HashMap::new();
    for component in board.components.iter() {
        if let Some(part) = component.logical_part() {
            components
                .entry(part.0)
                .or_default()
                .push(component.name.clone());
        }
    }
    let part_ids: Vec<_> = (1..=parts.count())
        .filter_map(|i| parts.get(crate::library::LogicalPartId(i)))
        .collect();
    Some(PartLibrary {
        mappings: part_ids
            .iter()
            .map(|part| LogicalPartMapping {
                name: part.name.clone(),
                components: components.remove(&part.id.0).unwrap_or_default(),
            })
            .collect(),
        parts: part_ids
            .iter()
            .map(|part| LogicalPart {
                name: part.name.clone(),
                pins: (0..part.pin_count())
                    .filter_map(|i| part.get_pin(i))
                    .map(|pin| PartPin {
                        name: pin.pin_name.clone(),
                        code: 0,
                        gate: pin.gate_name.clone(),
                        gate_swap_code: pin.gate_swap_code,
                        gate_pin: pin.gate_pin_name.clone(),
                        gate_pin_swap_code: pin.gate_pin_swap_code,
                    })
                    .collect(),
            })
            .collect(),
    })
}

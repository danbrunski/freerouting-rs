//! Copy of a selection (I04.3): the engine part of Java's
//! `gui.interactive.CopyItemState` at `a773a70ec`.
//!
//! Disabled in Java's GUI at the pin (nothing calls `CopyItemState`);
//! enabled here by decision (DIVERGENCES, "Interactive editor").
//!
//! A [`CopySession`] keeps a preview: a `copy(0)` of every drill item (pin
//! or via) and every obstacle, keepout or conduction area of the selection,
//! in selection order (one id burned each), except the `_aux` companions of a
//! component's pins, which the pin copy makes again. The source stays
//! untouched: the preview moves ([`CopySession::change_position`]) and the
//! board is changed only by [`CopySession::insert`], which can be repeated at
//! other positions.
//!
//! [`CopySession::insert`] follows `CopyItemState.insert`:
//! 1. If the layer was changed, every preview via gets a padstack whose
//!    first layer is the new one ([`change_padstack_layers`], one new
//!    padstack per old one).
//! 2. Every preview item of a component gets a copy of that component, moved
//!    by the position offset, once per component. With a changed layer the
//!    copy has a new package with such padstacks, else the old package.
//! 3. Every preview item without a clearance violation is copied again
//!    (`copy(0)`, another id) and inserted, after one snapshot before the
//!    first. A pin whose padstack has auxiliary shapes also gets its `_aux`
//!    obstacles. The others are not inserted.
//!
//! Java keeps the preview after an insert and starts the next offset from the
//! position of this one, so the next insert copies the already moved
//! component again; so does this port.

use std::collections::{BTreeMap, BTreeSet};

use freerouting_geometry::{Area, FloatPoint, IntPoint, PolylineArea, Shape, Vector};

use super::Status;
use super::move_items::floating_violations;
use crate::board::items::{AreaKind, Item, ItemId, ItemKind};
use crate::board::structure::ComponentId;
use crate::board::{BasicBoard, ItemGeometry, ItemTransform, RoutingBoard, geometry_of_item};
use crate::library::{PackagePin, PadstackId};

/// What an insert did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CopyOutcome {
    /// The inserted items, in preview order (the `_aux` obstacles of a pin
    /// follow it).
    pub inserted: Vec<ItemId>,
    /// `allItemsInserted`, else `some_items_not_inserted_because_of_obstacles`.
    pub status: Option<Status>,
}

/// The interactive copy of a selection.
#[derive(Clone, Debug)]
pub struct CopySession {
    items: Vec<Item>,
    start: IntPoint,
    current: IntPoint,
    previous: IntPoint,
    layer: i32,
    layer_changed: bool,
}

impl CopySession {
    /// Java `CopyItemState.getInstance` and its constructor: `None` for an
    /// empty selection. `layer` is the current layer, `at` the pointer.
    pub fn start(
        board: &mut RoutingBoard,
        selection: &BTreeSet<ItemId>,
        layer: i32,
        at: FloatPoint,
    ) -> Option<CopySession> {
        if selection.is_empty() {
            return None;
        }
        let mut items = Vec::new();
        for id in selection {
            let Some(item) = board.item(*id).cloned() else {
                continue;
            };
            let copy = match &item.kind {
                ItemKind::Pin(_) | ItemKind::Via(_) => board.copy_of(&item, None),
                ItemKind::Area(area) => {
                    // Skip companion obstacles belonging to a component to
                    // prevent duplicate companions on copy.
                    let companion = item.component_id() > 0
                        && area.name.as_deref().is_some_and(|n| n.ends_with("_aux"));
                    if companion {
                        continue;
                    }
                    board.copy_of(&item, None)
                }
                _ => continue,
            };
            // Java adds the null that `ConductionArea.copy` returns for an
            // area of several nets and fails later; the port leaves it out.
            items.extend(copy);
        }
        let start = at.round();
        Some(CopySession {
            items,
            start,
            current: start,
            previous: start,
            layer,
            layer_changed: false,
        })
    }

    /// The preview items.
    pub fn items(&self) -> &[Item] {
        &self.items
    }

    /// The current layer of the copy (the first layer of copied vias).
    pub fn layer(&self) -> i32 {
        self.layer
    }

    /// The drawable preview in board coordinates. Java draws the items of a
    /// component (pins, keepouts) at the offset of the copy, since their
    /// geometry follows their component until the insert gives the copy a
    /// component of its own; the other items were moved with the preview.
    pub fn preview_geometry(&self) -> Vec<ItemGeometry> {
        let (dx, dy) = self.offset();
        self.items
            .iter()
            .map(|item| {
                let geometry = geometry_of_item(item);
                if item.component_id() > 0 {
                    geometry.translated(dx, dy)
                } else {
                    geometry
                }
            })
            .collect()
    }

    /// The current position of the preview (the rounded pointer).
    pub fn position(&self) -> IntPoint {
        self.current
    }

    /// The offset of the preview from where the copy started.
    pub fn offset(&self) -> (i32, i32) {
        (self.current.x - self.start.x, self.current.y - self.start.y)
    }

    /// Java `changePosition`: moves the preview to `new_position` (rounded).
    pub fn change_position(&mut self, board: &BasicBoard, new_position: FloatPoint) {
        self.current = new_position.round();
        if self.current == self.previous {
            return;
        }
        let vector = Vector::Int(self.current.difference_by_int(self.previous));
        let transform = ItemTransform::Translate(vector);
        for item in &mut self.items {
            board.transform_detached(item, &transform);
        }
        self.previous = self.current;
    }

    /// Java `changeLayerAction`: the first layer of the copied vias.
    pub fn change_layer(&mut self, layer: i32) {
        self.layer = layer;
        self.layer_changed = true;
    }

    /// The clearance violations of the preview at the current position.
    pub fn violations(&self, board: &mut BasicBoard) -> usize {
        floating_violations(board, self.items.iter()).len()
    }

    /// Java `insert`: copies the preview into the board; see the module
    /// docs. `None`: Java gave up half way (a package padstack is missing)
    /// without a message.
    pub fn insert(&mut self, board: &mut RoutingBoard) -> Option<CopyOutcome> {
        let mut padstack_pairs: BTreeMap<PadstackId, PadstackId> = BTreeMap::new();
        if self.layer_changed {
            // create new via padstacks
            for item in &mut self.items {
                let Some(via) = item.as_via() else { continue };
                let new = change_padstack_layers(
                    board,
                    via.drill.padstack,
                    self.layer,
                    &mut padstack_pairs,
                );
                if let ItemKind::Via(via) = &mut item.kind {
                    via.drill.padstack = new;
                    via.drill = board.via_drill_data(new, via.drill.center);
                }
            }
        }
        // Copy the components of the old items and assign the new items to
        // the copied components.
        let mut component_pairs: BTreeMap<i32, i32> = BTreeMap::new();
        let translate = Vector::Int(self.current.difference_by_int(self.start));
        for index in 0..self.items.len() {
            let old = self.items[index].component_id();
            if old <= 0 {
                continue;
            }
            let new = match component_pairs.get(&old) {
                Some(new) => *new,
                None => {
                    let Some(old_component) = board.components.get(ComponentId(old)) else {
                        continue; // Java logs "component not found"
                    };
                    let package = old_component.package();
                    let new_package = if self.layer_changed {
                        // create a new package with changed layers of the padstacks.
                        let pins = board.library.packages.get(package)?.pins().to_vec();
                        let mut new_pins = Vec::with_capacity(pins.len());
                        for pin in &pins {
                            board.library.padstacks.get(pin.padstack_id)?;
                            let padstack = change_padstack_layers(
                                board,
                                pin.padstack_id,
                                self.layer,
                                &mut padstack_pairs,
                            );
                            new_pins.push(PackagePin::new(
                                pin.name.clone(),
                                padstack,
                                pin.exact_relative_location(),
                                pin.rotation_in_degree,
                            ));
                        }
                        board.library.packages.add_unnamed(new_pins)
                    } else {
                        package
                    };
                    let Some(copied) =
                        board
                            .components
                            .copy(ComponentId(old), &translate, new_package)
                    else {
                        continue;
                    };
                    component_pairs.insert(old, copied.0);
                    copied.0
                }
            };
            board.assign_component_id(&mut self.items[index], new);
        }
        let mut all_inserted = true;
        let mut first_time = true;
        let mut inserted = Vec::new();
        for index in 0..self.items.len() {
            // Java counts the violations just before it inserts the item, so
            // the copies inserted so far count as obstacles.
            let blocked =
                !floating_violations(board, std::iter::once(&self.items[index])).is_empty();
            if blocked {
                all_inserted = false;
                continue;
            }
            if first_time {
                // make the current situation restorable by undo
                board.generate_snapshot();
                first_time = false;
            }
            let Some(copied) = board.copy_of(&self.items[index], None) else {
                continue;
            };
            let id = board.insert_item(copied);
            inserted.push(id);
            insert_aux_obstacles(board, id, &mut inserted);
        }
        self.start = self.current;
        self.layer_changed = false;
        let key = if all_inserted {
            "allItemsInserted"
        } else {
            "some_items_not_inserted_because_of_obstacles"
        };
        Some(CopyOutcome {
            inserted,
            status: Some(Status::new(key, [])),
        })
    }
}

/// Java `changePadstackLayers`: a padstack with the shapes of `old` shifted
/// so that its first layer is `new_layer` (the same padstack if it is
/// already there; one new padstack per old one, kept in `pairs`). Java's
/// `Padstacks.add` leaves `attach` and `placedAbsolute` off.
pub fn change_padstack_layers(
    board: &mut BasicBoard,
    old: PadstackId,
    new_layer: i32,
    pairs: &mut BTreeMap<PadstackId, PadstackId>,
) -> PadstackId {
    let Some(old_padstack) = board.library.padstacks.get(old).cloned() else {
        return old;
    };
    let old_layer = old_padstack.from_layer();
    if old_layer == new_layer {
        return old;
    }
    if let Some(new) = pairs.get(&old) {
        return *new;
    }
    let count = board.layer_count();
    let has_aux = old_padstack.has_auxiliary_shapes();
    let mut shapes = vec![None; count];
    let mut aux = vec![None; if has_aux { count } else { 0 }];
    let layer_diff = old_layer - new_layer;
    for (i, shape) in shapes.iter_mut().enumerate() {
        let old_layer_no = i as i32 + layer_diff;
        if old_layer_no >= 0 && (old_layer_no as usize) < count {
            *shape = old_padstack.get_shape(old_layer_no).cloned();
            if has_aux {
                aux[i] = old_padstack
                    .get_auxiliary_shapes(old_layer_no)
                    .map(<[_]>::to_vec);
            }
        }
    }
    let new = if has_aux {
        let name = format!("padstack#{}", board.library.padstacks.count() + 1);
        let new = board.library.padstacks.add(name, shapes, aux, false, false);
        if old_padstack.has_non_convex_geometry()
            && let Some(padstack) = board.library.padstacks.get_mut(new)
        {
            padstack.has_non_convex_geometry = true;
        }
        new
    } else {
        board.library.padstacks.add_unnamed(shapes)
    };
    pairs.insert(old, new);
    new
}

/// The `_aux` obstacles of a copied pin whose padstack has auxiliary shapes
/// (the second half of `CopyItemState.insert`'s loop body).
fn insert_aux_obstacles(board: &mut RoutingBoard, pin_id: ItemId, inserted: &mut Vec<ItemId>) {
    let Some(item) = board.item(pin_id) else {
        return;
    };
    let Some(pin) = item.as_pin() else { return };
    let Some(padstack) = board.library.padstacks.get(pin.drill.padstack).cloned() else {
        return;
    };
    if !padstack.has_auxiliary_shapes() {
        return;
    }
    let (nets, class, component_id, fixed) = (
        item.net_numbers().to_vec(),
        item.clearance_class(),
        item.component_id(),
        item.fixed_state(),
    );
    let (first, last, pin_index) = (item.first_layer(), item.last_layer(), pin.pin_index);
    let component = board.components.get(ComponentId(component_id));
    let on_front = component.is_none_or(|c| c.placed_on_front());
    let layer_count = padstack.board_layer_count() as i32;
    let rotate_first = board.components.flip_style_rotate_first();
    let transform = component
        .filter(|c| c.is_placed())
        .map(|c| c.placement_transform(rotate_first));
    let package_pin = component
        .and_then(|c| board.library.packages.get(c.package()))
        .and_then(|p| p.get_pin(pin_index))
        .cloned();
    let (Some(transform), Some(package_pin)) = (transform, package_pin) else {
        return;
    };
    for board_layer in first..=last {
        let padstack_layer = if on_front || padstack.placed_absolute {
            board_layer
        } else {
            layer_count - board_layer - 1
        };
        let Some(aux_shapes) = padstack.get_auxiliary_shapes(padstack_layer) else {
            continue;
        };
        for aux_shape in aux_shapes {
            // PARITY(quirk, java_quirks.placed_shape_convex): `true` places the
            // auxiliary shape as Java does (K15.2).
            let shape = transform.shape_with(
                &aux_shape.to_shape(),
                package_pin.exact_relative_location(),
                package_pin.rotation_in_degree,
                board.java_quirks().placed_shape_convex,
            );
            let area = match shape {
                Shape::Polyline(p) => Area::PolylineArea(PolylineArea::new(p, Vec::new())),
                other => Area::Shape(other),
            };
            if !area.is_empty() {
                let id = board.insert_area(
                    AreaKind::Obstacle,
                    area,
                    board_layer,
                    Vector::ZERO,
                    0.0,
                    false,
                    &nets,
                    class,
                    component_id,
                    Some(format!("{}_aux", padstack.name)),
                    fixed,
                );
                inserted.push(id);
            }
        }
    }
}

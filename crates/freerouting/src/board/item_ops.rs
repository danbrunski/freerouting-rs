//! Item operations on the board (B05): moves and geometry transforms,
//! copies and `connect_to_trace`.
//!
//! Port of `Item.moveBy` (`Item.java:302-316`), `DrillItem.moveBy`
//! (`DrillItem.java:96-146`), the per-class `translateBy`, `turn90Degree`,
//! `rotateApprox` and `changePlacementSide` (`DrillItem.java:63-94`,
//! `PolylineTrace.java:144-170`, `Via.java:191`, `Pin.java:443-460`,
//! `ObstacleArea.java:235-280`, `ComponentOutline.java`,
//! `BoardOutline.java:265-302`), the per-class `copy(int id)` and
//! `Item.assignComponentId`.
//!
//! A board-level transform is Java's `moveBy` sequence: `saveForUndo`,
//! remove from the search trees, transform the item, insert it into the
//! trees, and invalidate the edge-pin cache for a pin or the outline. Java's
//! `turn90Degree`, `rotateApprox` and `changePlacementSide` only change the
//! item and leave the trees to the caller; the board-level methods here wrap
//! them in the same sequence. [`BasicBoard::transform_detached`] is the
//! item-level form, for items that are not on the board (copies).
//!
//! The data Java derives lazily (pin centres and shapes, absolute areas) is
//! recomputed right after the transform, as Java's next read would compute
//! it: a pin's shapes and layers from its component's placement, a via's from
//! its padstack and centre, an area's from its component or its own
//! placement. Each `clearDerivedData` Java runs is recorded in the autoroute
//! change log (B02.8) as [`AutorouteChange::InfoCleared`].
//!
//! Java behaviour kept on purpose:
//! - `PolylineTrace.rotateApprox` does not call `clearDerivedData`.
//! - `BoardOutline`'s transforms assign the moved shape to the loop variable
//!   only, so the outline shapes never move; only a generated keepout area
//!   does.
//! - A pin's `translateBy` moves its cached centre, but its shapes come from
//!   the component, which the caller moves first (`MoveComponent`). Java's
//!   centre is `null` until first read; the port always has it, so a
//!   translate always moves it.

use std::collections::BTreeMap;

use freerouting_geometry::{FloatPoint, IntPoint, Point, Vector};

use crate::board::autoroute_log::AutorouteChange;
use crate::board::basic_board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::board::routing_board::RoutingBoard;
use crate::board::structure::{AngleRestriction, ComponentId, FixedState};

/// A geometry transform of an item (the four `Item` transform methods).
#[derive(Clone, Debug)]
pub enum ItemTransform {
    /// `translateBy(vector)`.
    Translate(Vector),
    /// `turn90Degree(factor, pole)`.
    Turn90 { factor: i32, pole: IntPoint },
    /// `rotateApprox(angleInDegree, pole)`.
    Rotate {
        angle_in_degree: f64,
        pole: FloatPoint,
    },
    /// `changePlacementSide(pole)`.
    ChangeSide { pole: IntPoint },
}

/// What `DrillItem.moveBy` remembers of a trace contact (Java `TraceInfo`).
#[derive(Clone, Copy, Debug)]
struct TraceInfo {
    half_width: i32,
    clearance_class: i32,
}

impl BasicBoard {
    /// Moves item `id` by `vector` on the board (Java `Item.moveBy`). A pin
    /// or via also gets a trace from its old centre to its new one on every
    /// layer where a trace touched it (`DrillItem.moveBy`). False if the item
    /// is not on the board.
    pub fn move_item(&mut self, id: ItemId, vector: &Vector) -> bool {
        let Some(item) = self.items.get(&id) else {
            return false;
        };
        if item.drill().is_some() {
            return self.move_drill_item(id, vector);
        }
        self.transform_on_board(id, &ItemTransform::Translate(vector.clone()))
    }

    /// Turns item `id` by `factor` times 90 degrees around `pole`, keeping the
    /// board's trees in step.
    pub fn turn_90_degree(&mut self, id: ItemId, factor: i32, pole: IntPoint) -> bool {
        self.transform_on_board(id, &ItemTransform::Turn90 { factor, pole })
    }

    /// Rotates item `id` by `angle_in_degree` around `pole`, keeping the
    /// board's trees in step.
    pub fn rotate_approx(&mut self, id: ItemId, angle_in_degree: f64, pole: FloatPoint) -> bool {
        self.transform_on_board(
            id,
            &ItemTransform::Rotate {
                angle_in_degree,
                pole,
            },
        )
    }

    /// Mirrors item `id` at the vertical line through `pole` and moves it to
    /// the other board side, keeping the board's trees in step.
    pub fn change_placement_side(&mut self, id: ItemId, pole: IntPoint) -> bool {
        self.transform_on_board(id, &ItemTransform::ChangeSide { pole })
    }

    /// Java `DrillItem.moveBy`.
    fn move_drill_item(&mut self, id: ItemId, vector: &Vector) -> bool {
        let old_center = self.item_ref(id).drill().expect("drill item").center;
        // PARITY: Java fills a TreeSet<TraceInfo> ordered by layer descending
        // from the contacts in TreeSet<Item> order (highest id first); `add`
        // keeps the first trace per layer.
        let mut contact_traces: BTreeMap<std::cmp::Reverse<i32>, TraceInfo> = BTreeMap::new();
        for contact in self.normal_contacts(id) {
            let item = self.item_ref(contact);
            if let Some(trace) = item.as_trace() {
                contact_traces
                    .entry(std::cmp::Reverse(trace.layer))
                    .or_insert(TraceInfo {
                        half_width: trace.half_width,
                        clearance_class: item.clearance_class(),
                    });
            }
        }
        if !self.transform_on_board(id, &ItemTransform::Translate(vector.clone())) {
            return false;
        }
        let new_center = self.item_ref(id).drill().expect("drill item").center;
        let add_corner = match self.rules.trace_angle_restriction() {
            AngleRestriction::NinetyDegree => old_center.ninety_degree_corner(new_center, true),
            AngleRestriction::FortyfiveDegree => {
                old_center.fortyfive_degree_corner(new_center, true)
            }
            AngleRestriction::None => None,
        };
        let mut points = vec![Point::Int(old_center)];
        points.extend(add_corner.map(Point::Int));
        points.push(Point::Int(new_center));
        let net_numbers = self.item_ref(id).net_numbers().to_vec();
        for (std::cmp::Reverse(layer), info) in contact_traces {
            self.insert_trace_points(
                &points,
                layer,
                info.half_width,
                &net_numbers,
                info.clearance_class,
                FixedState::Unfixed,
            );
        }
        true
    }

    /// Java's `moveBy` sequence around an item transform: `saveForUndo`,
    /// remove from the trees, transform, insert, invalidate the edge-pin
    /// cache for a pin or the outline.
    fn transform_on_board(&mut self, id: ItemId, transform: &ItemTransform) -> bool {
        let Some(mut item) = self.items.get(&id).cloned() else {
            return false;
        };
        self.items.save_for_undo(&id);
        self.note_changed(id);
        self.remove_from_trees(id);
        let cleared = self.transform_detached(&mut item, transform);
        let invalidates = matches!(item.kind, ItemKind::Pin(_) | ItemKind::BoardOutline(_));
        if let Some(stored) = self.items.get_mut(&id) {
            // The tree removal cleared the on-board flag; the insertion sets
            // it again.
            item.on_the_board = stored.on_the_board;
            *stored = item;
        }
        for _ in 0..cleared {
            self.log_autoroute(AutorouteChange::InfoCleared(id));
        }
        self.insert_into_trees(id);
        if invalidates {
            self.invalidate_edge_pin_nets();
        }
        self.note_changed(id);
        true
    }

    /// Applies `transform` to `item`, which need not be on the board, and
    /// recomputes its derived data. Returns how many times Java calls
    /// `clearDerivedData` for it.
    pub fn transform_detached(&self, item: &mut Item, transform: &ItemTransform) -> usize {
        let layer_count = self.layer_count();
        let rotate_first = self.components.flip_style_rotate_first();
        let component_id = item.component_id;
        let mut refresh = false;
        let cleared = match &mut item.kind {
            ItemKind::Pin(pin) => {
                // Pin.turn90Degree, rotateApprox and changePlacementSide reset
                // the centre; translateBy is DrillItem's and moves it.
                let moved_center = match transform {
                    ItemTransform::Translate(vector) => {
                        Some(translate_point(pin.drill.center, vector))
                    }
                    _ => None,
                };
                if let Some(mut drill) = self.pin_drill_data(component_id, pin.pin_index) {
                    if let Some(center) = moved_center {
                        drill.center = center;
                    }
                    pin.drill = drill;
                } else if let Some(center) = moved_center {
                    pin.drill.center = center;
                }
                1
            }
            ItemKind::Via(via) => {
                let center = via.drill.center;
                let new_center = match transform {
                    ItemTransform::Translate(vector) => translate_point(center, vector),
                    ItemTransform::Turn90 { factor, pole } => center.turn_90_degree(*factor, *pole),
                    ItemTransform::Rotate {
                        angle_in_degree,
                        pole,
                    } => center
                        .to_float()
                        .rotate(angle_in_degree.to_radians(), *pole)
                        .round(),
                    ItemTransform::ChangeSide { pole } => {
                        let Some(mirrored) =
                            self.library.get_mirrored_via_padstack(via.drill.padstack)
                        else {
                            return 0;
                        };
                        via.drill.padstack = mirrored;
                        center.mirror_vertical(*pole)
                    }
                };
                via.drill = self.via_drill_data(via.drill.padstack, new_center);
                if matches!(transform, ItemTransform::ChangeSide { .. }) {
                    2
                } else {
                    1
                }
            }
            ItemKind::Trace(trace) => match transform {
                ItemTransform::Translate(vector) => {
                    trace.translate_by(vector);
                    1
                }
                ItemTransform::Turn90 { factor, pole } => {
                    trace.turn_90_degree(*factor, *pole);
                    1
                }
                ItemTransform::Rotate {
                    angle_in_degree,
                    pole,
                } => {
                    trace.rotate_approx(*angle_in_degree, *pole);
                    0
                }
                ItemTransform::ChangeSide { pole } => {
                    trace.change_placement_side(*pole, layer_count);
                    1
                }
            },
            ItemKind::Area(area) => {
                match transform {
                    ItemTransform::Translate(vector) => area.translate_by(vector),
                    ItemTransform::Turn90 { factor, pole } => area.turn_90_degree(*factor, *pole),
                    ItemTransform::Rotate {
                        angle_in_degree,
                        pole,
                    } => area.rotate_approx(*angle_in_degree, *pole, rotate_first),
                    ItemTransform::ChangeSide { pole } => {
                        area.change_placement_side(*pole, layer_count)
                    }
                }
                refresh = true;
                1
            }
            ItemKind::ComponentOutline(outline) => {
                match transform {
                    ItemTransform::Translate(vector) => outline.translate_by(vector),
                    ItemTransform::Turn90 { factor, pole } => {
                        outline.turn_90_degree(*factor, *pole)
                    }
                    ItemTransform::Rotate {
                        angle_in_degree,
                        pole,
                    } => outline.rotate_approx(*angle_in_degree, *pole, rotate_first),
                    ItemTransform::ChangeSide { pole } => outline.change_placement_side(*pole),
                }
                refresh = true;
                1
            }
            ItemKind::BoardOutline(outline) => {
                match transform {
                    ItemTransform::Translate(vector) => {
                        outline.transform_keepout(|a| a.translate_by(vector))
                    }
                    ItemTransform::Turn90 { factor, pole } => {
                        outline.transform_keepout(|a| a.turn_90_degree(*factor, *pole))
                    }
                    ItemTransform::Rotate {
                        angle_in_degree,
                        pole,
                    } => {
                        let angle = angle_in_degree.to_radians();
                        outline.transform_keepout(|a| a.rotate_approx(angle, *pole))
                    }
                    ItemTransform::ChangeSide { pole } => {
                        outline.transform_keepout(|a| a.mirror_vertical(*pole))
                    }
                }
                0
            }
        };
        if refresh {
            self.refresh_derived_data(item);
        }
        cleared
    }

    /// Recomputes the data Java derives lazily from an item's placement: a
    /// pin's centre and shapes, a via's shapes, an area's or component
    /// outline's absolute area.
    pub(crate) fn refresh_derived_data(&self, item: &mut Item) {
        let component_id = item.component_id;
        let rotate_first = self.components.flip_style_rotate_first();
        let placed = (component_id > 0)
            .then(|| self.components.get(ComponentId(component_id)))
            .flatten()
            .filter(|c| c.is_placed())
            .map(|c| c.placement_transform(rotate_first));
        match &mut item.kind {
            ItemKind::Pin(pin) => {
                if let Some(drill) = self.pin_drill_data(component_id, pin.pin_index) {
                    pin.drill = drill;
                }
            }
            ItemKind::Via(via) => {
                via.drill = self.via_drill_data(via.drill.padstack, via.drill.center);
            }
            ItemKind::Area(area) => match &placed {
                Some(transform) => {
                    let absolute = transform.area(area.relative_area());
                    area.set_area(absolute);
                }
                None => area.update_area(rotate_first),
            },
            ItemKind::ComponentOutline(outline) => match &placed {
                Some(transform) => {
                    let absolute = transform.area(outline.relative_area());
                    outline.set_area(absolute);
                }
                None => outline.update_area(rotate_first),
            },
            ItemKind::Trace(_) | ItemKind::BoardOutline(_) => {}
        }
    }

    /// Binds the detached item `item` to component `component_id` and
    /// recomputes its derived data (Java `Item.assignComponentId`, which
    /// clears it; `Pin.assignComponentId` also resets the centre).
    pub fn assign_component_id(&self, item: &mut Item, component_id: i32) {
        item.component_id = component_id;
        self.refresh_derived_data(item);
    }

    /// A copy of item `id` with id `new_id`, or with a new id from the
    /// board's generator for `None` (Java `copy(0)`, which takes an id even
    /// if the copy is dropped). The copy is not on the board; the caller
    /// inserts it. `None` if the item does not exist, and for a conduction
    /// area of more than one net, which Java's `ConductionArea.copy` refuses
    /// (before it takes an id).
    pub fn copy_item(&mut self, id: ItemId, new_id: Option<ItemId>) -> Option<Item> {
        let item = self.items.get(&id)?.clone();
        self.copy_of(&item, new_id)
    }

    /// [`copy_item`](Self::copy_item) for an item that need not be on the
    /// board, such as a detached copy (Java `copy(int)` on any item).
    pub fn copy_of(&mut self, item: &Item, new_id: Option<ItemId>) -> Option<Item> {
        if item.is_conduction_area() && item.net_count() != 1 {
            self.log.push(
                "ConductionArea.copy not yet implemented for areas with more than 1 net".into(),
            );
            return None;
        }
        let new_id = match new_id {
            Some(id) if id.0 > 0 => id,
            _ => self.burn_item_id(),
        };
        Some(copy_with_id(item, new_id))
    }
}

/// A copy of `item` with the id `new_id` that is not on the board (the part
/// of Java's per-class `copy(int)` after the id is chosen).
pub(crate) fn copy_with_id(item: &Item, new_id: ItemId) -> Item {
    let kind = match &item.kind {
        ItemKind::BoardOutline(outline) => ItemKind::BoardOutline(outline.copy_shapes()),
        other => other.clone(),
    };
    let (nets, class, component, fixed) = (
        item.net_numbers.clone(),
        item.clearance_class,
        item.component_id,
        item.fixed_state,
    );
    let mut kind = kind;
    if let ItemKind::Pin(pin) = &mut kind {
        // A new Pin's changedTo is the pin itself.
        pin.changed_to = new_id;
    }
    let fixed = if matches!(kind, ItemKind::BoardOutline(_)) {
        FixedState::SystemFixed
    } else {
        fixed
    };
    Item::new(new_id, nets, class, component, fixed, kind)
}

/// `Point.translateBy` for a drill centre. A translation by a rational
/// vector, which Java would keep as a rational point and no caller makes,
/// is rounded.
fn translate_point(center: IntPoint, vector: &Vector) -> IntPoint {
    let moved = Point::Int(center).translate_by(vector);
    moved.as_int().unwrap_or_else(|| moved.to_float().round())
}

impl RoutingBoard {
    /// Inserts a trace from `from` to the nearest point of trace `trace`
    /// with half width `half_width` and clearance class `clearance_class`.
    /// False if `trace` is not a polyline trace or the connection would
    /// violate a clearance (B05.6).
    ///
    /// Port of `RoutingBoard.connectToTrace` (`RoutingBoard.java:1150-1204`):
    /// true at once if the trace contains `from`; else the projection line
    /// from `from` onto the trace, which must be a 3-line polyline, is checked
    /// with `check_polyline_trace` (which burns an id), its corners join the
    /// changed area while marking, and it is inserted unfixed. Then the trace
    /// tails at the trace's first and last corner are removed, each unless
    /// `from` is that corner or the tail is user fixed.
    pub fn connect_to_trace(
        &mut self,
        from: IntPoint,
        trace: ItemId,
        half_width: i32,
        clearance_class: i32,
    ) -> bool {
        let Some(item) = self.item(trace) else {
            return false;
        };
        let Some(polyline_trace) = item.as_trace() else {
            return false;
        };
        let from_point = Point::Int(from);
        if polyline_trace.polyline.contains(&from_point) {
            return true;
        }
        let Some(projection) = polyline_trace.polyline.projection_line(&from_point) else {
            return false;
        };
        let connection = projection.to_polyline();
        if connection.lines().len() != 3 {
            return false;
        }
        let layer = polyline_trace.layer;
        // Java reads the corners after the insertion from the trace object,
        // whose geometry a split or combine of other items leaves alone.
        let (first_corner, last_corner) =
            (polyline_trace.first_corner(), polyline_trace.last_corner());
        let net_numbers = item.net_numbers().to_vec();
        if !self.check_polyline_trace(
            &connection,
            layer,
            half_width,
            &net_numbers,
            clearance_class,
        ) {
            return false;
        }
        if self.changed_area().is_some() {
            for i in 0..connection.corner_count() {
                self.join_changed_area(connection.corner_approx(i), layer);
            }
        }
        self.insert_trace(
            connection,
            layer,
            half_width,
            &net_numbers,
            clearance_class,
            FixedState::Unfixed,
        );
        for corner in [first_corner, last_corner] {
            if from_point == corner {
                continue;
            }
            if let Some(tail) = self.get_trace_tail(&corner, layer, &net_numbers)
                && !self.item_ref(tail).is_user_fixed()
            {
                self.remove_item(tail);
            }
        }
        true
    }
}

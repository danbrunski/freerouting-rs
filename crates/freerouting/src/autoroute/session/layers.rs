//! Layer changes of the maze search (A04.5).
//!
//! Port of `MazeExpansionEngine` (`MazeExpansionEngine.java`) at
//! `a773a70ec`: `expandToDrill` (`:31-116`), `expandToDrillPage`
//! (`:118-146`), `expandToDrillsOfPage` (`:148-242`), `expandToOtherLayers`
//! (`:244-382`) and `checkLayerWithAnyMatchingVia` (`:384-421`).
//!
//! A free room's element first reaches the drill pages that overlap the
//! room; popping a page computes its drills (A03) and queues those whose
//! room on the element's layer is the element's room. Popping a drill
//! reached from a room queues the drill on every layer a via of the rule
//! can reach, after `ForcedViaInserter.checkLayer` (O03) on each layer
//! between. A via obstacle room leads to the via's own drill, which only a
//! ripup may use.

// The hooks compiled out (`--cfg freerouting_no_search_hooks`, A04.8) leave
// their inputs unused.
#![cfg_attr(
    freerouting_no_search_hooks,
    allow(unused_imports, unused_variables, unused_mut)
)]

use freerouting_geometry::{FloatLine, TileShape, TileShapeOps};

use super::drills::{DrillId, DrillPageId};
use super::queue::MazeListElement;
use super::search::MazeSearch;
use super::sink::{BlockReason, ExtraCost, SearchSink, sink};
use super::{Adjustment, ExpandableRef, Room, RoomError};
use crate::shove::drill_check::CheckDrillResult;
use crate::shove::trace_check::TraceShover;
use crate::shove::via_check::ShoveDepths;

impl<S: SearchSink, C: ExtraCost> MazeSearch<'_, '_, S, C> {
    /// `expandToDrill` (`:31-116`): queues `drill` on the layer of `from`'s
    /// room at the point of the drill shape (shrunk by the via radius)
    /// nearest the entry, at `+ min_normal_via_cost` unless `from` is a
    /// drill page.
    pub(crate) fn expand_to_drill(
        &mut self,
        drill: DrillId,
        from: &MazeListElement,
        add_costs: i32,
    ) -> Result<(), RoomError> {
        let control = self.s.control;
        let room = from.next_room.expect("the element has a next room");
        let layer = self.s.room(room).layer();
        let layer_index = layer as usize;
        let trace_half_width = control.compensated_trace_half_width[layer_index];
        let room_is_thin = self.room_shape(room)?.min_width() < f64::from(2 * trace_half_width);
        if room_is_thin {
            let intersects_backtrack = match from.backtrack {
                Some((backtrack, _)) => {
                    let backtrack_shape = self.s.expandable_shape(backtrack)?;
                    self.s.drill(drill).shape.intersects_tile(&backtrack_shape)
                }
                None => false,
            };
            if !intersects_backtrack {
                return Ok(());
            }
        }
        let via_radius = control.via_radii[layer_index];
        let shrinked = self.s.drill(drill).shape.shrink(via_radius);
        let mut compare_corner = from.shape_entry.a.middle_point(from.shape_entry.b);
        let from_page = matches!(from.door, ExpandableRef::DrillPage(_));
        if from_page && let Some((ExpandableRef::TargetDoor(door), _)) = from.backtrack {
            let item = self.s.target_door(door).item;
            if self.s.board.item(item).is_some_and(|i| i.is_pin()) {
                let location = self.s.drill(drill).location.to_float();
                if let Some(corner) =
                    self.s
                        .board
                        .nearest_trace_exit_corner(item, location, trace_half_width, layer)
                {
                    compare_corner = corner;
                }
            }
        }
        let nearest =
            shrinked
                .nearest_point_approx(compare_corner)
                .ok_or(RoomError::NullPointer {
                    at: "MazeExpansionEngine.expandToDrill",
                })?;
        let section = (layer - self.s.drill(drill).first_layer) as usize;
        let costs = &control.trace_costs[layer_index];
        // PARITY: MazeExpansionEngine.java:70-86, summed left to right.
        let mut expansion_value = from.expansion_value
            + f64::from(add_costs)
            + nearest.weighted_distance(compare_corner, costs.horizontal, costs.vertical);
        let backtrack = if from_page {
            from.backtrack
        } else {
            expansion_value += control.min_normal_via_cost;
            Some((from.door, from.section))
        };
        let sorting_value = expansion_value + self.s.distance.calculate_point(&nearest, layer);
        let element = MazeListElement {
            door: ExpandableRef::Drill(drill),
            section,
            backtrack,
            expansion_value,
            sorting_value,
            next_room: None,
            shape_entry: FloatLine::new(nearest, nearest),
            room_ripped: from.room_ripped,
            adjustment: Adjustment::None,
            already_checked: false,
            ripup_cost: 0,
        };
        self.push(element)?;
        Ok(())
    }

    /// `expandToDrillPage` (`:118-146`): queues `page` on the layer of
    /// `from`'s room; the distance to the page goes into the sorting value
    /// only.
    pub(crate) fn expand_to_drill_page(&mut self, page: DrillPageId, from: &MazeListElement) {
        let control = self.s.control;
        let room = from.next_room.expect("the element has a next room");
        let layer = self.s.room(room).layer();
        let middle = from.shape_entry.a.middle_point(from.shape_entry.b);
        let page_box = self
            .s
            .built_drill_pages()
            .expect("the pages overlapping the room were built")
            .page(page)
            .shape;
        let nearest = page_box.nearest_point(middle);
        let costs = &control.trace_costs[layer as usize];
        let expansion_value = from.expansion_value + control.min_normal_via_cost;
        // PARITY: MazeExpansionEngine.java:125-131, summed left to right.
        let sorting_value = expansion_value
            + nearest.weighted_distance(middle, costs.horizontal, costs.vertical)
            + self.s.distance.calculate_point(&nearest, layer);
        let element = MazeListElement {
            door: ExpandableRef::DrillPage(page),
            section: layer as usize,
            backtrack: Some((from.door, from.section)),
            expansion_value,
            sorting_value,
            next_room: from.next_room,
            shape_entry: from.shape_entry,
            room_ripped: from.room_ripped,
            adjustment: Adjustment::None,
            already_checked: false,
            ripup_cost: 0,
        };
        // The page's id is a function of its shape and net; it cannot fail.
        let _ = self.push(element);
    }

    /// `expandToDrillsOfPage` (`:148-242`): the drills of the popped page
    /// whose room on its layer is the element's room and whose section is
    /// free.
    pub(crate) fn expand_to_drills_of_page(
        &mut self,
        from: &MazeListElement,
    ) -> Result<(), RoomError> {
        let ExpandableRef::DrillPage(page) = from.door else {
            return Ok(());
        };
        let from_layer = from.section as i32;
        let before = self.s.drill_count();
        let attach_smd = self.s.control.attach_smd_allowed;
        let drills = self.s.page_drills(page, attach_smd)?.to_vec();
        sink!({
            let computed = self.s.drill_count() - before;
            self.sink.on_drill_page(self.s, page, computed)
        });
        let _ = before;
        for drill in drills {
            let (index, room) = {
                let d = self.s.drill(drill);
                let index = from_layer - d.first_layer;
                if index < 0 || index as usize >= d.rooms.len() {
                    continue;
                }
                (index as usize, d.rooms[index as usize])
            };
            if room != from.next_room {
                continue;
            }
            if self.s.drill_sections(drill)[index].occupied {
                continue;
            }
            self.expand_to_drill(drill, from, 0)?;
        }
        Ok(())
    }

    /// `expandToOtherLayers` (`:244-382`): queues the popped drill on every
    /// layer a via of the rule reaches from the element's layer.
    pub(crate) fn expand_to_other_layers(
        &mut self,
        element: &MazeListElement,
    ) -> Result<(), RoomError> {
        let ExpandableRef::Drill(drill) = element.door else {
            return Ok(());
        };
        let control = self.s.control;
        let (first_layer, last_layer) = {
            let d = self.s.drill(drill);
            (d.first_layer, d.last_layer)
        };
        let from_layer = first_layer + element.section as i32;
        let mut smd_on_component_side = false;
        let mut smd_on_solder_side = false;
        let room_ripped;
        let (via_lower_bound, via_upper_bound);
        let from_room = self
            .s
            .drill(drill)
            .rooms
            .get(element.section)
            .copied()
            .flatten();
        let obstacle_item = from_room.and_then(|r| match self.s.room(r) {
            Room::Obstacle { item, .. } => Some(*item),
            _ => None,
        });
        if let Some(item) = obstacle_item {
            if !control.ripup_allowed {
                return Ok(());
            }
            let board = &*self.s.board;
            let Some(via) = board.item(item).and_then(|i| i.as_via().map(|v| (i, v))) else {
                return Ok(());
            };
            let padstack_id = via.1.drill.padstack;
            let in_rule = control
                .via_rule_vias(&board.rules)
                .iter()
                .any(|v| board.rules.via_infos.get(*v).padstack == padstack_id);
            if !in_rule || via.0.clearance_class() != control.via_clearance_class {
                return Ok(());
            }
            let padstack =
                board
                    .library
                    .padstacks
                    .get(padstack_id)
                    .ok_or(RoomError::NullPointer {
                        at: "Via.getPadstack",
                    })?;
            via_lower_bound = padstack.from_layer();
            via_upper_bound = padstack.to_layer();
            room_ripped = true;
        } else {
            room_ripped = false;
            let lower_limit = first_layer.max(control.via_lower_bound);
            let upper_limit = last_layer.min(control.via_upper_bound);
            let lower;
            let mut layer = from_layer;
            loop {
                let result = self.check_drill_layer(drill, layer)?;
                if result == CheckDrillResult::NotDrillable {
                    lower = layer + 1;
                    break;
                } else if result == CheckDrillResult::DrillableWithAttachSmd {
                    if layer == 0 {
                        smd_on_component_side = true;
                    } else if layer == control.layer_count as i32 - 1 {
                        smd_on_solder_side = true;
                    }
                }
                if layer <= lower_limit {
                    lower = lower_limit;
                    break;
                }
                layer -= 1;
            }
            if lower > first_layer {
                return Ok(());
            }
            let upper;
            layer = from_layer + 1;
            loop {
                if layer > upper_limit {
                    upper = upper_limit;
                    break;
                }
                let result = self.check_drill_layer(drill, layer)?;
                if result == CheckDrillResult::NotDrillable {
                    upper = layer - 1;
                    break;
                } else if result == CheckDrillResult::DrillableWithAttachSmd
                    && layer == control.layer_count as i32 - 1
                {
                    smd_on_solder_side = true;
                }
                layer += 1;
            }
            if upper < last_layer {
                return Ok(());
            }
            via_lower_bound = lower;
            via_upper_bound = upper;
        }
        let last_board_layer = control.layer_count as i32 - 1;
        for to_layer in via_lower_bound..=via_upper_bound {
            if to_layer == from_layer {
                continue;
            }
            let (first, last) = if to_layer < from_layer {
                (to_layer, from_layer)
            } else {
                (from_layer, to_layer)
            };
            let mask_found = control.via_infos.iter().any(|mask| {
                first >= mask.from_layer
                    && last <= mask.to_layer
                    && mask.from_layer >= via_lower_bound
                    && mask.to_layer <= via_upper_bound
                    && (!(mask.from_layer == 0 && smd_on_component_side
                        || mask.to_layer == last_board_layer && smd_on_solder_side)
                        || mask.attach_smd_allowed)
            });
            if !mask_found {
                continue;
            }
            let index = (to_layer - first_layer) as usize;
            if self.s.drill_sections(drill)[index].occupied {
                continue;
            }
            let add = control.add_via_costs[from_layer as usize].to_layer[to_layer as usize];
            let expansion_value = element.expansion_value + f64::from(add);
            let middle = element.shape_entry.a.middle_point(element.shape_entry.b);
            let sorting_value =
                expansion_value + self.s.distance.calculate_point(&middle, to_layer);
            let next_room = self.s.drill(drill).rooms[index];
            let new_element = MazeListElement {
                door: ExpandableRef::Drill(drill),
                section: index,
                backtrack: Some((ExpandableRef::Drill(drill), element.section)),
                expansion_value,
                sorting_value,
                next_room,
                shape_entry: element.shape_entry,
                room_ripped,
                adjustment: Adjustment::None,
                already_checked: false,
                ripup_cost: 0,
            };
            self.push(new_element)?;
        }
        Ok(())
    }

    /// `checkLayerWithAnyMatchingVia` on the drill's room of `layer`.
    fn check_drill_layer(
        &mut self,
        drill: DrillId,
        layer: i32,
    ) -> Result<CheckDrillResult, RoomError> {
        let d = self.s.drill(drill);
        let room = d.rooms[(layer - d.first_layer) as usize].ok_or(RoomError::NullPointer {
            at: "MazeExpansionEngine.expandToOtherLayers",
        })?;
        let shape = self.room_shape(room)?.clone();
        let result = self.check_layer_with_any_matching_via(drill, layer, &shape);
        if result == CheckDrillResult::NotDrillable
            && let Some(blocker) = self.s.board.shove_failing_obstacle()
        {
            self.blocked(blocker, layer, BlockReason::ViaCheckFailed);
        }
        Ok(result)
    }

    /// `checkLayerWithAnyMatchingVia` (`:384-421`): drillable if a via of
    /// the rule passes `ForcedViaInserter.checkLayer` on `layer`.
    fn check_layer_with_any_matching_via(
        &mut self,
        drill: DrillId,
        layer: i32,
        room_shape: &TileShape,
    ) -> CheckDrillResult {
        let control = self.s.control;
        let layer_index = layer as usize;
        let location = self.s.drill(drill).location.clone();
        let vias: Vec<(f64, i32, bool)> = {
            let board = &*self.s.board;
            control
                .via_rule_vias(&board.rules)
                .iter()
                .filter_map(|id| {
                    let info = board.rules.via_infos.get(*id);
                    let padstack = board.library.padstacks.get(info.padstack)?;
                    if layer < padstack.from_layer() || layer > padstack.to_layer() {
                        return None;
                    }
                    let radius = padstack
                        .get_shape(layer)
                        .map_or(0.0, |shape| 0.5 * shape.max_width());
                    let required = freerouting_geometry::jmath::max_f64(
                        radius,
                        f64::from(control.trace_half_width[layer_index]),
                    );
                    Some((required, info.clearance_class, info.attach_smd_allowed))
                })
                .collect()
        };
        let mut drillable_with_attach_smd = false;
        let net = [control.net_number];
        for (radius, clearance_class, attach_smd) in vias {
            let result = TraceShover::new(self.s.board).check_via_layer(
                radius,
                clearance_class,
                attach_smd,
                room_shape,
                &location,
                layer,
                &net,
                ShoveDepths {
                    recursion: control.max_shove_trace_recursion_depth,
                    via_recursion: 0,
                },
                control.trace_half_width[layer_index],
                control.trace_clearance_class_index,
            );
            if result == CheckDrillResult::Drillable {
                return result;
            }
            if result == CheckDrillResult::DrillableWithAttachSmd {
                drillable_with_attach_smd = true;
            }
        }
        if drillable_with_attach_smd {
            CheckDrillResult::DrillableWithAttachSmd
        } else {
            CheckDrillResult::NotDrillable
        }
    }
}

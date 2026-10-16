//! Spring-over: wrapping a trace around a fixed obstacle (O03.5).
//!
//! Port of `TraceShover.springOver` and `springOverObstacles` (Java at
//! `a773a70ec`). Where a substitute trace runs into an obstacle that cannot
//! be shoved, the trace is wrapped around the obstacle's shape, enlarged by
//! the half width and clearance, and the result is checked again, up to the
//! recursion depth.
//!
//! Java tells "no obstacle" by returning the input polyline object and
//! "impossible" by `null`; the port returns [`SpringOver::NoObstacle`],
//! [`SpringOver::Wrapped`] and [`SpringOver::Blocked`] (O00 "'Unchanged' is
//! an explicit outcome"). A recursive call that finds no obstacle returns
//! the wrapped polyline it was given, which is new to the caller.

use std::collections::BTreeSet;

use freerouting_geometry::{
    FloatPoint, IntBox, Line, Polyline, PolylineShapeOps, TileShape, TileShapeOps,
};

use crate::board::items::{AreaKind, Item, ItemId, ItemKind};
use crate::board::structure::AngleRestriction;

use super::trace_check::TraceShover;

/// The outcome of a spring-over.
#[derive(Clone, Debug)]
pub enum SpringOver {
    /// No obstacle in the way: Java returned the polyline itself.
    NoObstacle,
    /// The polyline wrapped around the obstacles.
    Wrapped(Polyline),
    /// Not possible (Java `null`); the shove-failing obstacle is set where
    /// Java sets it.
    Blocked,
}

/// Java `springOverObstacles`' own recursion depth.
const MAX_SPRING_OVER_RECURSION_DEPTH: i32 = 20;

impl TraceShover<'_> {
    /// Java `springOverObstacles(polyline, halfWidth, layer, netNumbers,
    /// clearanceClassIndex, contactPins)`: wraps `polyline` around the
    /// obstacles in its way, trying both senses, and keeps the shorter
    /// result (the reversed sense on a tie). With `contact_pins`, every pin
    /// not in it is an obstacle, even of the own net.
    pub fn spring_over_obstacles(
        &mut self,
        polyline: &Polyline,
        half_width: i32,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        contact_pins: Option<&BTreeSet<ItemId>>,
    ) -> SpringOver {
        let counter_clockwise = self.spring_over(
            polyline,
            half_width,
            layer,
            net_numbers,
            clearance_class,
            true,
            MAX_SPRING_OVER_RECURSION_DEPTH,
            contact_pins,
        );
        if matches!(counter_clockwise, SpringOver::NoObstacle) {
            return SpringOver::NoObstacle;
        }
        let reversed = polyline.reverse();
        let clockwise = match self.spring_over(
            &reversed,
            half_width,
            layer,
            net_numbers,
            clearance_class,
            true,
            MAX_SPRING_OVER_RECURSION_DEPTH,
            contact_pins,
        ) {
            // Java compares the result of the reversed polyline with the
            // reversed polyline, which it then reverses again.
            SpringOver::NoObstacle => Some(reversed),
            SpringOver::Wrapped(p) => Some(p),
            SpringOver::Blocked => None,
        };
        let counter_clockwise = match counter_clockwise {
            SpringOver::Wrapped(p) => Some(p),
            _ => None,
        };
        match (clockwise, counter_clockwise) {
            (Some(cw), Some(ccw)) => {
                if cw.length_approx() <= ccw.length_approx() {
                    SpringOver::Wrapped(cw.reverse())
                } else {
                    SpringOver::Wrapped(ccw)
                }
            }
            (Some(cw), None) => SpringOver::Wrapped(cw.reverse()),
            (None, Some(ccw)) => SpringOver::Wrapped(ccw),
            (None, None) => SpringOver::Blocked,
        }
    }

    /// Java `springOver`: wraps `polyline` counterclockwise around the first
    /// obstacle in its way and recurses on the result.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn spring_over(
        &mut self,
        polyline: &Polyline,
        half_width: i32,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        over_connected_pins: bool,
        recursion_depth: i32,
        contact_pins: Option<&BTreeSet<ItemId>>,
    ) -> SpringOver {
        let mut current = polyline.clone();
        let mut depth = recursion_depth;
        let mut wrapped = false;
        loop {
            match self.spring_over_step(
                &current,
                half_width,
                layer,
                net_numbers,
                clearance_class,
                over_connected_pins,
                depth,
                contact_pins,
            ) {
                Step::NoObstacle if wrapped => return SpringOver::Wrapped(current),
                Step::NoObstacle => return SpringOver::NoObstacle,
                Step::Blocked => return SpringOver::Blocked,
                Step::Wrapped(next) => {
                    current = next;
                    depth -= 1;
                    wrapped = true;
                }
            }
        }
    }

    /// One level of Java's `springOver`, the recursion unrolled into
    /// [`spring_over`](Self::spring_over)'s loop.
    #[allow(clippy::too_many_arguments)]
    fn spring_over_step(
        &mut self,
        polyline: &Polyline,
        half_width: i32,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        over_connected_pins: bool,
        recursion_depth: i32,
        contact_pins: Option<&BTreeSet<ItemId>>,
    ) -> Step {
        let found = match self.first_spring_obstacle(
            polyline,
            half_width,
            layer,
            net_numbers,
            clearance_class,
            contact_pins,
        ) {
            Err(()) => return Step::Blocked,
            Ok(None) => return Step::NoObstacle,
            Ok(Some(found)) => found,
        };
        let board = &*self.board;
        let item = board.item(found).expect("obstacle on the board");
        let blocking_outline = matches!(item.kind, ItemKind::BoardOutline(_))
            && board.outline_blocks_nets(net_numbers);
        if recursion_depth <= 0
            || blocking_outline
            || (item.is_trace() && !board.is_shove_fixed(item))
        {
            return self.fail(found);
        }
        let Some(obstacle_shape) = self.spring_obstacle_shape(item, layer, over_connected_pins)
        else {
            return self.fail(found);
        };
        let offset_shape = self.spring_offset_shape(
            &obstacle_shape,
            item.clearance_class(),
            clearance_class,
            half_width,
            layer,
        );
        if offset_shape.contains_inside(&polyline.first_corner())
            || offset_shape.contains_inside(&polyline.last_corner())
        {
            // Can happen with clearance compensation off because of the
            // asymmetry of the offset shape calculations.
            return self.fail(found);
        }
        let entries = offset_shape.entrance_points(polyline);
        if entries.is_empty() {
            return Step::NoObstacle;
        }
        if entries.len() < 2 {
            return self.fail(found);
        }
        Step::Wrapped(wrap_around(polyline, &offset_shape, &entries))
    }

    fn fail(&mut self, obstacle: ItemId) -> Step {
        self.board.set_shove_failing_obstacle(Some(obstacle));
        Step::Blocked
    }

    /// The first obstacle of Java's `springOver` scan: `Ok(None)` if there
    /// is none, `Err` where Java returns `null` (two overlapping obstacles,
    /// neither containing the other).
    fn first_spring_obstacle(
        &self,
        polyline: &Polyline,
        half_width: i32,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        contact_pins: Option<&BTreeSet<ItemId>>,
    ) -> Result<Option<ItemId>, ()> {
        let board = &*self.board;
        let check_nets: &[i32] = if contact_pins.is_none() {
            net_numbers
        } else {
            &[]
        };
        let mut found: Option<(ItemId, IntBox)> = None;
        for i in 0..polyline.lines().len().saturating_sub(2) {
            let Some(shape) = polyline.offset_shape(half_width, i) else {
                continue;
            };
            let obstacles =
                board.overlapping_items_with_clearance(&shape, layer, check_nets, clearance_class);
            for id in obstacles {
                let Some(item) = board.item(id) else {
                    continue;
                };
                if !self.is_spring_obstacle(item, net_numbers, contact_pins) {
                    continue;
                }
                match found {
                    None => found = Some((id, item.bounding_box())),
                    Some((found_id, found_box)) if found_id != id => {
                        // One obstacle may contain the other (fixed vias in
                        // pins): take the bigger one.
                        let current_box = item.bounding_box();
                        if found_box.intersects_box(&current_box) {
                            if found_box.is_contained_in(&current_box) {
                                found = Some((id, current_box));
                            } else if !current_box.is_contained_in(&found_box) {
                                return Err(());
                            }
                        }
                    }
                    Some(_) => {}
                }
            }
            if found.is_some() {
                break;
            }
        }
        Ok(found.map(|(id, _)| id))
    }

    /// Java `springOver`'s `isObstacle`.
    fn is_spring_obstacle(
        &self,
        item: &Item,
        net_numbers: &[i32],
        contact_pins: Option<&BTreeSet<ItemId>>,
    ) -> bool {
        let board = &*self.board;
        if item.shares_net_no(net_numbers) {
            // Avoids acid traps.
            return item.is_pin() && contact_pins.is_some_and(|pins| !pins.contains(&item.id()));
        }
        match &item.kind {
            ItemKind::Area(area) => match area.kind {
                AreaKind::Conduction { is_obstacle, .. } => is_obstacle,
                AreaKind::ViaObstacle | AreaKind::ComponentObstacle => false,
                AreaKind::Obstacle => !item.is_routable(),
            },
            ItemKind::BoardOutline(_) => board.outline_blocks_nets(net_numbers),
            ItemKind::Trace(_) => {
                // A shove-fixed trace is an obstacle, unless it is an exit
                // stub at a tie pin of the own net; an unfixed one may be
                // pushed aside.
                board.is_shove_fixed(item)
                    && !board
                        .normal_contacts(item.id())
                        .into_iter()
                        .any(|c| board.item(c).is_some_and(|c| c.shares_net_no(net_numbers)))
            }
            // An unfixed via can be pushed aside eventually.
            _ => !item.is_routable(),
        }
    }

    /// The shape to spring over: the obstacle's single tree shape, the whole
    /// convex area of a partitioned obstacle area, or a drill item's tree
    /// shape on `layer`; `None` where Java gives up (`trySpringOver` false).
    fn spring_obstacle_shape(
        &self,
        item: &Item,
        layer: i32,
        over_connected_pins: bool,
    ) -> Option<TileShape> {
        let board = &*self.board;
        if !over_connected_pins {
            // The obstacle has a trace contact on the layer.
            let contacts = board.all_contacts(item.id(), Some(layer));
            if contacts
                .iter()
                .any(|c| board.item(*c).is_some_and(Item::is_trace))
            {
                return None;
            }
        }
        let tree = board.search_trees().default_tree();
        let ctx = board.shape_context();
        match &item.kind {
            ItemKind::Area(_) | ItemKind::Trace(_) => {
                if tree.tree_shape_count(item, &ctx) == 1 {
                    return tree.tree_shape(item, 0, &ctx);
                }
                let ItemKind::Area(area) = &item.kind else {
                    return None;
                };
                // Search-tree sections are an indexing detail: recover the
                // whole convex area, enlarged by the compensation the tree
                // applies before partitioning. Holes and concave areas are
                // not bridged.
                let convex = area.area().split_to_convex()?;
                if convex.len() != 1 {
                    return None;
                }
                let compensation = tree.clearance_compensation_value(
                    &board.rules.clearance_matrix,
                    item.clearance_class(),
                    layer,
                );
                Some(convex[0].enlarge(f64::from(compensation)))
            }
            ItemKind::Pin(_) | ItemKind::Via(_) => {
                let drill = item.drill()?;
                if layer < drill.first_layer || layer > drill.last_layer {
                    return None;
                }
                tree.tree_shape(item, (layer - drill.first_layer) as usize, &ctx)
            }
            _ => None,
        }
    }

    /// Java `springOver`'s enlarged obstacle shape, regularized for the
    /// board's angle restriction.
    fn spring_offset_shape(
        &self,
        obstacle_shape: &TileShape,
        obstacle_class: i32,
        clearance_class: i32,
        half_width: i32,
        layer: i32,
    ) -> TileShape {
        let board = &*self.board;
        let offset = f64::from(half_width + 1);
        let mut shape = if board.search_trees().is_clearance_compensation_used() {
            obstacle_shape.enlarge(offset)
        } else {
            let half_cl_offset =
                0.5 * f64::from(board.clearance_value(obstacle_class, clearance_class, layer));
            if obstacle_shape.is_int_octagon() {
                // Java preserves the established offsets for octagons.
                obstacle_shape
                    .enlarge(offset + half_cl_offset)
                    .enlarge(half_cl_offset)
            } else {
                // Java matches the checker's separately rounded
                // half-clearance and width expansions.
                obstacle_shape
                    .enlarge(half_cl_offset)
                    .enlarge(half_cl_offset)
                    .enlarge(offset)
            }
        };
        match board.rules.trace_angle_restriction() {
            AngleRestriction::NinetyDegree => shape = TileShape::IntBox(shape.bounding_box()),
            AngleRestriction::FortyfiveDegree => {
                if let Some(octagon) = shape.bounding_octagon() {
                    shape = TileShape::IntOctagon(octagon);
                }
            }
            AngleRestriction::None => {}
        }
        shape
    }
}

/// One level of the spring-over.
enum Step {
    NoObstacle,
    Wrapped(Polyline),
    Blocked,
}

/// The polyline wrapped counterclockwise around `offset_shape` from its
/// first to its last entrance point, joined to the parts outside.
fn wrap_around(
    polyline: &Polyline,
    offset_shape: &TileShape,
    entries: &[(usize, usize)],
) -> Polyline {
    let border_count = offset_shape.border_line_count();
    let (first_line_no, first_side_no) = entries[0];
    let (last_line_no, last_side_no) = entries[entries.len() - 1];
    let lines = polyline.lines();
    let mut side_diff = last_side_no as i64 - first_side_no as i64;
    if side_diff < 0 {
        side_diff += border_count as i64;
    } else if side_diff == 0 {
        let compare_corner: FloatPoint = offset_shape.corner_approx(first_side_no);
        let first_intersection =
            lines[first_line_no].intersection_approx(&offset_shape.border_line(first_side_no));
        let second_intersection =
            lines[last_line_no].intersection_approx(&offset_shape.border_line(last_side_no));
        if compare_corner.distance(second_intersection)
            < compare_corner.distance(first_intersection)
        {
            side_diff += border_count as i64;
        }
    }
    let side_diff = side_diff as usize;
    let mut substitute: Vec<Line> = Vec::with_capacity(side_diff + 3);
    substitute.push(lines[first_line_no]);
    let mut edge_no = first_side_no;
    for _ in 0..=side_diff {
        substitute.push(offset_shape.border_line(edge_no));
        edge_no = if edge_no == border_count - 1 {
            0
        } else {
            edge_no + 1
        };
    }
    substitute.push(lines[last_line_no]);
    let substitute = Polyline::new(&substitute);
    // A circuit around the offset shape counterclockwise from the first to
    // the last intersection.
    let pieces = offset_shape.cutout_polyline(polyline);
    let mut result = substitute;
    if let Some(first) = pieces.first() {
        result = first.combine(&result);
    }
    if let Some(second) = pieces.get(1) {
        result = result.combine(second);
    }
    result
}

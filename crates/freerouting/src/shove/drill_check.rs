//! Drill and pad checks (O03.6).
//!
//! Port of `DrillItemMover.check` and `tryShoveViaPoints`, and of
//! `ForcedPadRouter.checkForcedPad`, `calcFromSide`,
//! `calcCheckShapeForFromSide`, `inFrontOfPad` and `CheckDrillResult`
//! (Java at `a773a70ec`): can a via (or pad) be moved, or a pad shape be
//! placed, if the traces and vias in the way are shoved aside?
//!
//! Java bug kept on purpose: `inFrontOfPad` for from side 0 compares
//! `Math.min(lineA.x + lineA.y, lineB.x + lineB.x)` (`ForcedPadRouter.java:78`)
//! where `lineB.x + lineB.y` is meant; [`in_front_of_pad`] keeps the typo
//! (`in_front_of_pad_keeps_java_typo`). [`in_front_of_pad_with`] fixes it under
//! `router.java_quirks.in_front_of_pad_typo = false` (N06.7).
//!
//! Java quirk kept (a judgement call, N06's (c) row `in_front_of_pad_side0_flank`):
//! side 0's second flank clause tests `Math.min(lineA.x + lineA.y, lineB.x + lineB.y)`
//! against the upper right diagonal (`ForcedPadRouter.java:86`), where sides 2, 4 and
//! 6 test the extreme that reaches past their diagonal (`max` for a `>=` bound).
//! `router.java_quirks.in_front_of_pad_side0_flank = false` takes `max`
//! (`tests/n06_shove.rs::side0_flank_switch_both_behaviours`).
//!
//! With statistics on, the counters `shove_in_front_of_pad_typo_flips` and
//! `shove_in_front_of_pad_flank_flips` count the side-0 tests whose result the other
//! value of the switch would flip, in either arm (N06.7's reachability measurement).

use freerouting_geometry::{
    Direction, IntPoint, IntVector, Line, Point, Polyline, PolylineShapeOps, TileShape,
    TileShapeOps, Vector,
};

use crate::board::RoutingBoard;
use crate::board::items::{Item, ItemId, ItemKind};
use crate::board::search::WorkLimit;
use crate::board::structure::AngleRestriction;
use crate::settings::JavaQuirks;

use super::entry_side::{ShapeAndEntrySide, ShapeEntrySide};
use super::trace_check::TraceShover;
use super::trace_entries::ShapeTraceEntries;

/// Java `ForcedPadRouter.CheckDrillResult`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckDrillResult {
    NotDrillable,
    Drillable,
    /// Drillable, and an obstacle is a pin of the own net (copper sharing
    /// allowed); the caller decides whether attaching to it is allowed.
    DrillableWithAttachSmd,
}

/// True if `limit` is set and exceeded; logs Java's cut-off line once.
pub(crate) fn limit_exceeded(board: &mut RoutingBoard, limit: Option<&mut WorkLimit>) -> bool {
    let Some(limit) = limit else {
        return false;
    };
    // Not a clone of the meter: a clone is another meter (another board's).
    let mut log = Vec::new();
    let exceeded = limit.exceeded_logging(board.search_trees.work_meter(), String::new, &mut log);
    board.log.extend(log);
    exceeded
}

impl TraceShover<'_> {
    /// Java `DrillItemMover.check(drillItem, vector, maxRecursionDepth,
    /// maxViaRecursionDepth, ignoreItems, board, workLimit)`: can drill item
    /// `drill` be moved by `vector`? `ignore_items` gets the drill item
    /// added, as in Java.
    #[allow(clippy::too_many_arguments)]
    pub fn check_drill_item_move(
        &mut self,
        drill: ItemId,
        vector: &Vector,
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        ignore_items: &mut Vec<ItemId>,
        mut work_limit: Option<&mut WorkLimit>,
    ) -> bool {
        if limit_exceeded(self.board, work_limit.as_deref_mut()) {
            return false;
        }
        let board = &*self.board;
        let Some(item) = board.item(drill) else {
            return false;
        };
        if board.is_shove_fixed(item) {
            return false;
        }
        // The drill item may only be connected to traces.
        let only_traces = board.normal_contacts(drill).into_iter().all(|c| {
            board
                .item(c)
                .is_some_and(|c| c.is_trace() || c.is_conduction_area())
        });
        if !only_traces {
            return false;
        }
        ignore_items.push(drill);
        let attach_allowed = matches!(&item.kind, ItemKind::Via(v) if v.attach_allowed);
        let Some(data) = item.drill() else {
            return false;
        };
        let (first_layer, last_layer, center) = (data.first_layer, data.last_layer, data.center);
        let nets = item.net_numbers().to_vec();
        let class = item.clearance_class();
        let item = item.clone();
        for layer in first_layer..=last_layer {
            let board = &*self.board;
            let tree = board.search_trees().default_tree();
            let index = (layer - first_layer) as usize;
            let Some(current_shape) = tree.tree_shape(&item, index, &board.shape_context()) else {
                continue;
            };
            let new_shape = current_shape.translate_by(vector);
            let tile = if board.rules.trace_angle_restriction() == AngleRestriction::NinetyDegree {
                TileShape::IntBox(new_shape.bounding_box())
            } else {
                match new_shape.bounding_octagon() {
                    Some(o) => TileShape::IntOctagon(o),
                    None => continue,
                }
            };
            let from_side = ShapeEntrySide::from_point(&Point::Int(center), &tile);
            let result = self.check_forced_pad(
                &tile,
                Some(from_side),
                layer,
                &nets,
                class,
                attach_allowed,
                ignore_items,
                max_recursion_depth,
                max_via_recursion_depth,
                true,
                work_limit.as_deref_mut(),
            );
            if result == CheckDrillResult::NotDrillable {
                return false;
            }
        }
        true
    }

    /// Java `DrillItemMover.tryShoveViaPoints(obstacleShape, layer, via,
    /// clearanceClassIndex, extendedCheck, board)`: the centres to try for
    /// via `via` pushed out of `obstacle_shape`, best first.
    pub fn try_shove_via_points(
        &self,
        obstacle_shape: &TileShape,
        layer: i32,
        via: ItemId,
        clearance_class: i32,
        extended_check: bool,
    ) -> Vec<IntPoint> {
        let board = &*self.board;
        let Some(item) = board.item(via) else {
            return Vec::new();
        };
        let Some(mut via_shape) = drill_tree_shape_on_layer(board, item, layer) else {
            return Vec::new();
        };
        let center = item.drill().expect("via").center;
        let compensated = board.search_trees().is_clearance_compensation_used();
        let is_octagon = obstacle_shape.is_int_octagon();
        let ninety = board.rules.trace_angle_restriction() == AngleRestriction::NinetyDegree;
        let clearance =
            f64::from(board.clearance_value(clearance_class, item.clearance_class(), layer));
        let mut shove_distance = if ninety || is_octagon {
            let mut d = 0.5 * via_shape.bounding_box().max_width();
            if !compensated {
                d += clearance;
            }
            d
        } else {
            // A different algorithm calculates the new centres; half the
            // clearance synchronizes with the clearance check of the tree.
            let mut d = 0.0;
            if !compensated {
                d += 0.5 * clearance;
            }
            d
        };
        // Java: an empirical tolerance for diagonal shoving.
        shove_distance += 2.0;
        if ninety {
            let offset_box = obstacle_shape.bounding_box().offset(shove_distance);
            let count = if extended_check { 2 } else { 1 };
            offset_box.nearest_border_projections(center, count)
        } else if is_octagon {
            let Some(octagon) = obstacle_shape.bounding_octagon() else {
                return Vec::new();
            };
            let count = if extended_check { 4 } else { 1 };
            octagon
                .enlarge(shove_distance)
                .nearest_border_projections(center, count)
        } else {
            let offset_shape = obstacle_shape.enlarge(shove_distance);
            if !compensated {
                via_shape = via_shape.enlarge(0.5 * clearance);
            }
            let count = if extended_check { 4 } else { 1 };
            offset_shape
                .nearest_relative_outside_locations(&via_shape, count)
                .into_iter()
                .map(|delta| {
                    let d = delta.round();
                    center.translate_by_int(IntVector { x: d.x, y: d.y })
                })
                .collect()
        }
    }

    /// Java `ForcedPadRouter.checkForcedPad(padShape, fromSide, layer,
    /// netNumbers, clearanceClassIndex, copperSharingAllowed, ignoreItems,
    /// maxRecursionDepth, maxViaRecursionDepth, checkOnlyFront,
    /// workLimit)`: can a pad of shape `pad_shape` go on `layer` if the
    /// traces and vias in the way are shoved aside?
    #[allow(clippy::too_many_arguments)]
    pub fn check_forced_pad(
        &mut self,
        pad_shape: &TileShape,
        from_side: Option<ShapeEntrySide>,
        layer: i32,
        net_numbers: &[i32],
        clearance_class: i32,
        copper_sharing_allowed: bool,
        ignore_items: &[ItemId],
        max_recursion_depth: i32,
        max_via_recursion_depth: i32,
        check_only_front: bool,
        mut work_limit: Option<&mut WorkLimit>,
    ) -> CheckDrillResult {
        if !pad_shape.is_contained_in(&self.board.bounding_box()) {
            let outline = self.board.outline().map(Item::id);
            self.board.set_shove_failing_obstacle(outline);
            return CheckDrillResult::NotDrillable;
        }
        let obstacles: Vec<ItemId> = self
            .board
            .overlapping_items_with_clearance(pad_shape, layer, &[], clearance_class)
            .into_iter()
            .filter(|id| !ignore_items.contains(id))
            .collect();
        let mut entries = ShapeTraceEntries::new(
            pad_shape.clone(),
            layer,
            net_numbers,
            clearance_class,
            from_side,
        );
        if !entries.store_items(
            self.board,
            obstacles.iter().copied(),
            true,
            copper_sharing_allowed,
        ) {
            self.board
                .set_shove_failing_obstacle(entries.found_obstacle());
            return CheckDrillResult::NotDrillable;
        }
        // Can the obstacle vias be shoved?
        for via in entries.shove_via_list.clone() {
            if max_via_recursion_depth <= 0 {
                self.board.set_shove_failing_obstacle(Some(via));
                return CheckDrillResult::NotDrillable;
            }
            let centers = self.try_shove_via_points(pad_shape, layer, via, clearance_class, false);
            let Some(first) = centers.first() else {
                self.board.set_shove_failing_obstacle(Some(via));
                return CheckDrillResult::NotDrillable;
            };
            let delta = via_delta(self.board, via, *first);
            let mut check_ignore = Vec::new();
            if !self.check_drill_item_move(
                via,
                &delta,
                max_recursion_depth,
                max_via_recursion_depth - 1,
                &mut check_ignore,
                work_limit.as_deref_mut(),
            ) {
                return CheckDrillResult::NotDrillable;
            }
        }
        let mut result = CheckDrillResult::Drillable;
        if copper_sharing_allowed
            && obstacles
                .iter()
                .any(|id| self.board.item(*id).is_some_and(Item::is_pin))
        {
            result = CheckDrillResult::DrillableWithAttachSmd;
        }
        if entries.substitute_trace_count() == 0 {
            return result;
        }
        if max_recursion_depth <= 0 || entries.stack_depth() > 1 {
            self.board
                .set_shove_failing_obstacle(entries.found_obstacle());
            return CheckDrillResult::NotDrillable;
        }
        let orthogonal = matches!(pad_shape, TileShape::IntBox(_));
        let from_no = from_side.and_then(|s| s.no).map_or(-1, |n| n as i32);
        while let Some(piece) = entries.next_substitute_trace_piece(self.board) {
            let trace = piece.as_trace().expect("substitute trace");
            for i in 0..trace.tile_shape_count() {
                let current_line = trace.polyline.lines()[i + 1];
                let direction: Direction = current_line.direction();
                let in_front = !check_only_front
                    || in_front_of_pad_with(
                        &current_line,
                        pad_shape,
                        from_no,
                        trace.half_width,
                        true,
                        &self.board.java_quirks(),
                    );
                if !in_front {
                    continue;
                }
                let Some(current) = ShapeAndEntrySide::new(self.board, &piece, i, orthogonal, true)
                else {
                    return CheckDrillResult::NotDrillable;
                };
                if !self.check(
                    &current.shape,
                    current.from_side,
                    Some(direction),
                    layer,
                    piece.net_numbers(),
                    piece.clearance_class(),
                    max_recursion_depth - 1,
                    max_via_recursion_depth,
                    0,
                    work_limit.as_deref_mut(),
                ) {
                    return CheckDrillResult::NotDrillable;
                }
            }
        }
        result
    }
}

impl TraceShover<'_> {
    /// Java `ForcedPadRouter.calcFromSide(shape, shapeCenter, layer, offset,
    /// clearanceClassIndex)`: the first side of `shape` (enlarged by
    /// `offset`) such that a trace line from `shape_center` to the nearest
    /// point of that side fits on `layer`, first with the clearance class,
    /// then without clearance; [`ShapeEntrySide::NOT_CALCULATED`] if none.
    pub fn calc_from_side(
        &self,
        shape: &TileShape,
        shape_center: &Point,
        layer: i32,
        offset: i32,
        clearance_class: i32,
    ) -> ShapeEntrySide {
        let board = &*self.board;
        let offset_shape = shape.offset(f64::from(offset));
        for class in [clearance_class, 0] {
            for i in 0..offset_shape.border_line_count() {
                let fits =
                    calc_check_shape_for_from_side(shape_center, &offset_shape.border_line(i))
                        .is_some_and(|check| {
                            board.check_trace_shape(&check, layer, &[], class, None)
                        });
                if fits {
                    return ShapeEntrySide::from_values(Some(i), None);
                }
            }
        }
        ShapeEntrySide::NOT_CALCULATED
    }
}

/// Java `ForcedPadRouter.calcCheckShapeForFromSide(shape, shapeCenter,
/// borderLine)`: the shape of a trace of half width 1 from `shape_center`
/// straight to `border_line`, keeping the direction restrictions. `None`
/// where Java's `offsetShape` logs "no out of range" and returns `null`
/// (only for a degenerate polyline; Java would then fail on the `null`), and
/// for a rational centre, for which Java's `Line(Point, Direction)` is not
/// implemented either (it logs a warning).
pub fn calc_check_shape_for_from_side(
    shape_center: &Point,
    border_line: &Line,
) -> Option<TileShape> {
    let &Point::Int(center) = shape_center else {
        return None;
    };
    let offset_projection = shape_center.to_float().projection_approx(border_line);
    let Direction::Int(direction) = border_line.direction() else {
        // Border lines of tile shapes have integer directions.
        return None;
    };
    let lines = [
        Line::from_point_and_direction(center, direction),
        Line::from_point_and_direction(center, direction.turn_45_degree(2)),
        Line::from_point_and_direction(offset_projection.round(), direction),
    ];
    Polyline::new(&lines).offset_shape(1, 0)
}

/// `target - via centre` (Java `newViaCenter[0].differenceBy(via.getCenter())`).
pub(crate) fn via_delta(board: &RoutingBoard, via: ItemId, target: IntPoint) -> Vector {
    let center = board
        .item(via)
        .and_then(Item::drill)
        .map_or(target, |d| d.center);
    Point::Int(target).difference_by(&Point::Int(center))
}

/// Java `DrillItem.getTreeShapeOnLayer(defaultTree, layer)`.
pub(crate) fn drill_tree_shape_on_layer(
    board: &RoutingBoard,
    item: &Item,
    layer: i32,
) -> Option<TileShape> {
    let drill = item.drill()?;
    if layer < drill.first_layer || layer > drill.last_layer {
        return None;
    }
    board.search_trees().default_tree().tree_shape(
        item,
        (layer - drill.first_layer) as usize,
        &board.shape_context(),
    )
}

/// Java `ForcedPadRouter.inFrontOfPad(line, padShape, fromSide, width,
/// withSides)`: is `line` in front of side `from_side` of the octagon
/// `pad_shape`? Only implemented for octagons and integer lines (true
/// otherwise). Keeps Java's typo for side 0 (see the module docs).
pub fn in_front_of_pad(
    line: &Line,
    pad_shape: &TileShape,
    from_side: i32,
    width: i32,
    with_sides: bool,
) -> bool {
    in_front_of_pad_with(
        line,
        pad_shape,
        from_side,
        width,
        with_sides,
        &JavaQuirks::JAVA,
    )
}

/// [`in_front_of_pad`] with the switches `router.java_quirks.in_front_of_pad_typo`
/// (`false` takes `min(a.x + a.y, b.x + b.y)` for side 0) and
/// `in_front_of_pad_side0_flank` (`false` takes `max(a.x + a.y, b.x + b.y)` in side 0's
/// second flank clause, as sides 2, 4 and 6 do; N06.7 registers it as a reported arm,
/// N06.12 records its outcome).
///
/// With statistics on, a side-0 test also evaluates each switch's other value and
/// counts the tests whose result it flips (`shove_in_front_of_pad_typo_flips`,
/// `shove_in_front_of_pad_flank_flips`, N06.7's reachability counters); the returned
/// value is always the configured one.
pub fn in_front_of_pad_with(
    line: &Line,
    pad_shape: &TileShape,
    from_side: i32,
    width: i32,
    with_sides: bool,
    quirks: &JavaQuirks,
) -> bool {
    if !pad_shape.is_int_octagon() {
        return true;
    }
    let Some(pad) = pad_shape.bounding_octagon() else {
        return true;
    };
    let typo = quirks.in_front_of_pad_typo;
    let flank = quirks.in_front_of_pad_side0_flank;
    let eval = |typo: bool, flank: bool| {
        in_front_of_octagon(line, &pad, from_side, width, with_sides, typo, flank)
    };
    let result = eval(typo, flank);
    if from_side == 0 && crate::run_stats::enabled() {
        use crate::run_stats::{Counter, record_n06};
        if eval(!typo, flank) != result {
            record_n06(Counter::ShoveInFrontOfPadTypoFlips);
        }
        if eval(typo, !flank) != result {
            record_n06(Counter::ShoveInFrontOfPadFlankFlips);
        }
    }
    result
}

/// The octagon half of [`in_front_of_pad_with`], with the two side-0 switches as
/// plain flags (`true` = Java's formula).
fn in_front_of_octagon(
    line: &Line,
    pad: &freerouting_geometry::IntOctagon,
    from_side: i32,
    width: i32,
    with_sides: bool,
    typo: bool,
    flank: bool,
) -> bool {
    let (a, b) = (line.a(), line.b());
    let (ax, ay, bx, by) = (
        i64::from(a.x),
        i64::from(a.y),
        i64::from(b.x),
        i64::from(b.y),
    );
    let w = i64::from(width);
    let diag = f64::from(width) * 2f64.sqrt();
    let (left, bottom, right, top) = (
        i64::from(pad.left_x),
        i64::from(pad.bottom_y),
        i64::from(pad.right_x),
        i64::from(pad.top_y),
    );
    let ul = f64::from(pad.upper_left_diagonal_x);
    let lr = f64::from(pad.lower_right_diagonal_x);
    let ll = f64::from(pad.lower_left_diagonal_x);
    let ur = f64::from(pad.upper_right_diagonal_x);
    let f = |v: i64| v as f64;
    let min_y = ay.min(by);
    let max_y = ay.max(by);
    let min_x = ax.min(bx);
    let max_x = ax.max(bx);
    let min_diff = f((ax - ay).min(bx - by));
    let max_diff = f((ax - ay).max(bx - by));
    let min_sum = f((ax + ay).min(bx + by));
    let max_sum = f((ax + ay).max(bx + by));
    match from_side {
        0 => {
            // PARITY(quirk, java_quirks.in_front_of_pad_typo): Java's typo
            // `lineB.x + lineB.x` (ForcedPadRouter.java:78).
            let side0_min_sum = if typo {
                f((ax + ay).min(bx + bx))
            } else {
                min_sum
            };
            let mut result =
                min_y >= top + w || max_diff <= ul - diag || side0_min_sum >= ur + diag;
            // PARITY(quirk, java_quirks.in_front_of_pad_side0_flank): the second
            // flank clause takes `min(x + y)` where sides 2, 4 and 6 test the
            // extreme past their diagonal (ForcedPadRouter.java:86); `false` takes
            // `max`.
            let flank_sum = if flank { min_sum } else { max_sum };
            if with_sides && !result {
                result = max_x <= left - w && min_diff <= ul - diag
                    || min_x >= right + w && flank_sum >= ur + diag;
            }
            result
        }
        1 => {
            let mut result = min_y >= top + w || max_diff <= ul - diag || max_x <= left - w;
            if with_sides && !result {
                result = min_x <= left - w && max_sum <= ll - diag
                    || max_y >= top + w && min_sum >= ur + diag;
            }
            result
        }
        2 => {
            let mut result = max_x <= left - w || max_diff <= ul - diag || max_sum <= ll - diag;
            if with_sides && !result {
                result = max_y <= bottom - w && min_sum <= ll - diag
                    || min_y >= top + w && min_diff <= ul - diag;
            }
            result
        }
        3 => {
            let mut result = max_x <= left - w || max_y <= bottom - w || max_sum <= ll - diag;
            if with_sides && !result {
                result = min_y <= bottom - w && min_diff >= lr + diag
                    || min_x <= left - w && max_diff <= ul - diag;
            }
            result
        }
        4 => {
            let mut result = max_y <= bottom - w || max_sum <= ll - diag || min_diff >= lr + diag;
            if with_sides && !result {
                result = min_x >= right + w && max_diff >= lr + diag
                    || max_x <= left - w && min_sum <= ll - diag;
            }
            result
        }
        5 => {
            let mut result = max_y <= bottom - w || min_x >= right + w || min_diff >= lr + diag;
            if with_sides && !result {
                result = max_x >= right + w && min_sum >= ur + diag
                    || min_y <= bottom - w && max_sum <= ll - diag;
            }
            result
        }
        6 => {
            let mut result = min_x >= right + w || min_sum >= ur + diag || min_diff >= lr + diag;
            if with_sides && !result {
                result = max_y <= bottom - w && max_diff >= lr + diag
                    || min_y >= top + w && max_sum >= ur + diag;
            }
            result
        }
        7 => {
            let mut result = min_y >= top + w || min_sum >= ur + diag || min_x >= right + w;
            if with_sides && !result {
                result = max_y >= top + w && max_diff <= ul - diag
                    || max_x >= right + w && min_diff >= lr + diag;
            }
            result
        }
        // Java logs "fromSide out of range".
        _ => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use freerouting_geometry::{IntOctagon, RationalPoint};

    /// An integer centre gives a check shape (half width 1 from the centre
    /// to the border line); a rational one gives none, where Java warns and
    /// goes on with a rational line (DIVERGENCES, "Shove checks").
    #[test]
    fn calc_check_shape_skips_rational_centre() {
        let border = Line::new(IntPoint { x: 100, y: -100 }, IntPoint { x: 100, y: 100 });
        let shape = calc_check_shape_for_from_side(&Point::Int(IntPoint { x: 0, y: 0 }), &border)
            .expect("check shape");
        let b = shape.bounding_box();
        assert_eq!((b.ll.x, b.ur.x, b.ll.y, b.ur.y), (-1, 101, -1, 1));
        let rational = Point::Rational(std::sync::Arc::new(RationalPoint::new(
            1.into(),
            1.into(),
            2.into(),
        )));
        assert!(calc_check_shape_for_from_side(&rational, &border).is_none());
    }

    /// The typo for side 0 (`lineB.x + lineB.x`): a line whose second end has
    /// `x + x` beyond the upper right diagonal but `x + y` not counts as in
    /// front. Fails if the typo is "fixed".
    #[test]
    fn in_front_of_pad_keeps_java_typo() {
        // The octagon of the box (-100, -100)-(100, 100): ur = 200.
        let pad =
            TileShape::IntOctagon(IntOctagon::new(-100, -100, 100, 100, -200, 200, -200, 200));
        // a = (300, -100): x + y = 200; b = (150, -140): x + y = 10,
        // x + x = 300. Below the top, left of nothing: only the third test
        // can hold. With the typo min(200, 300) = 200 >= 200 + 0.
        let line = Line::new(IntPoint { x: 300, y: -100 }, IntPoint { x: 150, y: -140 });
        assert!(in_front_of_pad(&line, &pad, 0, 0, false));
        // The intended formula, min(x + y), is symmetric in the two ends and
        // gives min(200, 10) = 10: no. The typo is not: with the ends
        // swapped it takes min(150 - 140, 300 + 300) = 10, no.
        let reversed = Line::new(IntPoint { x: 150, y: -140 }, IntPoint { x: 300, y: -100 });
        assert!(!in_front_of_pad(&reversed, &pad, 0, 0, false));
        // N06 `in_front_of_pad_typo`: `true` is the typo above, `false` the
        // intended, symmetric formula.
        let java = JavaQuirks::JAVA;
        let fixed = JavaQuirks::JAVA
            .with("in_front_of_pad_typo", false)
            .unwrap();
        assert!(in_front_of_pad_with(&line, &pad, 0, 0, false, &java));
        assert!(!in_front_of_pad_with(&line, &pad, 0, 0, false, &fixed));
        assert!(!in_front_of_pad_with(&reversed, &pad, 0, 0, false, &fixed));
    }
}

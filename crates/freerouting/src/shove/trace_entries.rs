//! The traces and vias a shove has to push out of a shape (O03.2, O03.3).
//!
//! Port of `board.searchtree.ShapeTraceEntries` (Java at `a773a70ec`)
//! without its cutouts, which are in [`cutout`](super::cutout).
//! [`ShapeTraceEntries::store_items`] sorts the obstacles of a shape:
//! vias go to the shove list, traces leave their entry points into the
//! shape enlarged by their half width and clearance, everything else that
//! cannot be shoved stops the store with that obstacle recorded. The entry
//! points are kept sorted around the border, starting in the middle of the
//! side the shove comes from; their stack levels say how deep the shove has
//! to recurse. [`ShapeTraceEntries::next_substitute_trace_piece`] then gives
//! the substitute traces around the shape, innermost level first.
//!
//! Java keeps the entry points in a singly linked list with an anchor. The
//! port keeps them in a `Vec` with `next` indices (O03 "Entry lists are
//! indexed `Vec`s"); removing a node only relinks, as in Java. Every float
//! comparison keeps Java's operation order.
//!
//! Java quirk kept: in `storeTrace` the contact-trace test compares a
//! contact's clearance class with itself (`contactItem.clearanceClassIndex()
//! != contactTrace.clearanceClassIndex()`, the same object), so only the
//! shove-fixed state and the half width decide there. Under
//! `router.java_quirks.store_trace_class_self_compare = false` (N06) the
//! contact's class is compared with the stored trace's.

use freerouting_geometry::{FloatPoint, Line, Polyline, PolylineShapeOps, TileShape, TileShapeOps};

use crate::board::BasicBoard;
use crate::board::items::{AreaKind, Item, ItemId, ItemKind, PolylineTrace};
use crate::board::structure::FixedState;

use super::entry_side::{ShapeEntrySide, compensated_half_width_of};

/// Java `ShapeTraceEntries.c_offset_add`.
pub(crate) const C_OFFSET_ADD: f64 = 1.0;

/// An entry point of a trace into the shape (Java `EntryPoint`).
#[derive(Clone, Debug)]
struct EntryPoint {
    trace: ItemId,
    trace_line_no: usize,
    edge_index: usize,
    entry_approx: FloatPoint,
    /// -1 while not calculated.
    stack_level: i32,
    next: Option<usize>,
}

/// Java `ShapeTraceEntries`.
#[derive(Clone, Debug)]
pub struct ShapeTraceEntries {
    shape: TileShape,
    layer: i32,
    own_net_nos: Vec<i32>,
    clearance_class: i32,
    from_side: Option<ShapeEntrySide>,
    entries: Vec<EntryPoint>,
    list_anchor: Option<usize>,
    trace_piece_count: i32,
    max_stack_level: i32,
    shape_contains_trace_tails: bool,
    found_obstacle: Option<ItemId>,
    /// The vias to shove (Java `shoveViaList`), in store order.
    pub shove_via_list: Vec<ItemId>,
}

/// The shape `shape` enlarged for trace `trace` as Java's `storeTrace`,
/// `nextSubstituteTracePiece` and `cutoutTrace` do: with clearance
/// compensation by the compensated half width + [`C_OFFSET_ADD`]; without,
/// first by the half width and then by the clearance + [`C_OFFSET_ADD`]
/// ("for symmetry").
pub(crate) fn trace_offset_shape(
    board: &BasicBoard,
    shape: &TileShape,
    trace: &Item,
    clearance_class: i32,
    layer: i32,
) -> TileShape {
    let tree = board.search_trees().default_tree();
    if tree.is_clearance_compensation_used() {
        let offset = f64::from(compensated_half_width_of(board, tree, trace)) + C_OFFSET_ADD;
        shape.offset(offset)
    } else {
        let half_width = trace.as_trace().map_or(0, |t| t.half_width);
        let cl_offset =
            f64::from(board.clearance_value(trace.clearance_class(), clearance_class, layer))
                + C_OFFSET_ADD;
        shape.offset(f64::from(half_width)).offset(cl_offset)
    }
}

/// Java `netNosEqual`: same length and every net of `a` in `b`.
fn net_nos_equal(a: &[i32], b: &[i32]) -> bool {
    a.len() == b.len() && a.iter().all(|n| b.contains(n))
}

impl ShapeTraceEntries {
    /// Java `ShapeTraceEntries(shape, layer, ownNetNos, clearanceClassIndex,
    /// fromSide, board)`. `from_side` is the side the shove comes from;
    /// `None` or a side without number makes [`store_items`](Self::store_items)
    /// calculate it.
    pub fn new(
        shape: TileShape,
        layer: i32,
        own_net_nos: &[i32],
        clearance_class: i32,
        from_side: Option<ShapeEntrySide>,
    ) -> Self {
        ShapeTraceEntries {
            shape,
            layer,
            own_net_nos: own_net_nos.to_vec(),
            clearance_class,
            from_side,
            entries: Vec::new(),
            list_anchor: None,
            trace_piece_count: 0,
            max_stack_level: 0,
            shape_contains_trace_tails: false,
            found_obstacle: None,
            shove_via_list: Vec::new(),
        }
    }

    /// The shape the obstacles are shoved out of.
    pub fn shape(&self) -> &TileShape {
        &self.shape
    }

    /// The nets of the shoving item (Java `ownNetNos`).
    pub fn own_net_nos(&self) -> &[i32] {
        &self.own_net_nos
    }

    /// The clearance class of the shoving item.
    pub fn clearance_class(&self) -> i32 {
        self.clearance_class
    }

    /// The side the shove comes from, as calculated by `store_items`.
    pub fn from_side(&self) -> Option<ShapeEntrySide> {
        self.from_side
    }

    /// Java `stackDepth`: the maximum recursion depth for shoving the
    /// obstacle traces.
    pub fn stack_depth(&self) -> i32 {
        self.max_stack_level
    }

    /// Java `substituteTraceCount`: the number of substitute trace pieces.
    pub fn substitute_trace_count(&self) -> i32 {
        self.trace_piece_count
    }

    /// Java `traceTailsInShape`: an unconnected end of a foreign trace lies
    /// in the interior of the shape.
    pub fn trace_tails_in_shape(&self) -> bool {
        self.shape_contains_trace_tails
    }

    /// Java `getFoundObstacle`: the item responsible for a failed store.
    ///
    /// Java's `storeTrace` also sets it to each trace it stores, so after a
    /// successful store it is the last stored trace (or the trace whose
    /// stack level rose above 1).
    pub fn found_obstacle(&self) -> Option<ItemId> {
        self.found_obstacle
    }

    /// Java `storeItems(itemList, isPadCheck, copperSharingAllowed)`: stores
    /// the traces and vias of `items`, walked in the given order. False if
    /// `items` holds an obstacle that cannot be shoved aside; the walk stops
    /// there. With `is_pad_check` the check is for a via, else for a trace;
    /// with `copper_sharing_allowed` overlaps with items of the own net are
    /// allowed.
    pub fn store_items(
        &mut self,
        board: &BasicBoard,
        items: impl IntoIterator<Item = ItemId>,
        is_pad_check: bool,
        copper_sharing_allowed: bool,
    ) -> bool {
        for id in items {
            let Some(item) = board.item(id) else {
                continue;
            };
            if let ItemKind::Area(area) = &item.kind {
                match area.kind {
                    AreaKind::ViaObstacle if !is_pad_check => continue,
                    // A trace-only keepout (Q09) lets the via pad through.
                    AreaKind::TraceObstacle if is_pad_check => continue,
                    AreaKind::ComponentObstacle => continue,
                    _ => {}
                }
            }
            let contains_own_net = item.shares_net_no(&self.own_net_nos);
            // Java's flag, or the area policy (Q01) for one of the own nets.
            if let Some(area) = item.as_conduction_area()
                && (contains_own_net
                    || !(area.conduction_is_obstacle()
                        || if is_pad_check {
                            board.area_policy_blocks_any_net_vias(item, &self.own_net_nos)
                        } else {
                            board.area_policy_blocks_any_net(item, &self.own_net_nos)
                        }))
            {
                continue;
            }
            if matches!(item.kind, ItemKind::BoardOutline(_))
                && !board.outline_blocks_nets(&self.own_net_nos)
            {
                continue;
            }
            if board.is_shove_fixed(item) && !contains_own_net {
                self.found_obstacle = Some(id);
                return false;
            }
            match &item.kind {
                ItemKind::Via(_) => {
                    if is_pad_check || !contains_own_net {
                        self.shove_via_list.push(id);
                    }
                }
                ItemKind::Trace(_) => {
                    if !self.store_trace(board, item) {
                        return false;
                    }
                }
                _ => {
                    let allowed = contains_own_net
                        && copper_sharing_allowed
                        && (!is_pad_check || item.pin_drill_allowed());
                    if !allowed {
                        self.found_obstacle = Some(id);
                        return false;
                    }
                }
            }
        }
        self.search_from_side(board);
        self.resort(board);
        self.calculate_stack_levels(board)
    }

    /// Java `storeTrace`: stores the intersections of `trace` with the
    /// border of the shape enlarged by its half width and clearance, and its
    /// end corners inside the shape.
    fn store_trace(&mut self, board: &BasicBoard, trace_item: &Item) -> bool {
        let Some(trace) = trace_item.as_trace() else {
            return true;
        };
        let id = trace_item.id();
        let offset_shape = trace_offset_shape(
            board,
            &self.shape,
            trace_item,
            self.clearance_class,
            trace.layer,
        );
        // Java uses offset, not enlarge, here because of a comparison in the
        // constructor of EntryPoint.
        for (line_no, edge_no) in offset_shape.entrance_points(&trace.polyline) {
            let entry_approx = trace.polyline.lines()[line_no]
                .intersection_approx(&offset_shape.border_line(edge_no));
            self.insert_entry_point(id, line_no, edge_no, entry_approx);
        }
        // An end point of the trace in the interior of the shape (for
        // example where a via touches the shape).
        if !trace_item.shares_net_no(&self.own_net_nos) {
            if !trace_item.nets_normal() {
                return false;
            }
            for i in 0..2 {
                let end_corner = if i == 0 {
                    trace.first_corner()
                } else {
                    trace.last_corner()
                };
                if offset_shape.contains(&end_corner)
                    && !self.store_end_corner(
                        board,
                        trace_item,
                        trace,
                        &offset_shape,
                        i,
                        &end_corner,
                    )
                {
                    return false;
                }
            }
        }
        self.found_obstacle = Some(id);
        true
    }

    /// The end-corner half of Java's `storeTrace` for end `end` (0 start, 1
    /// end) of a foreign trace, whose corner `end_corner` is in
    /// `offset_shape`. False where Java returns false.
    fn store_end_corner(
        &mut self,
        board: &BasicBoard,
        trace_item: &Item,
        trace: &PolylineTrace,
        offset_shape: &TileShape,
        end: usize,
        end_corner: &freerouting_geometry::Point,
    ) -> bool {
        let tree = board.search_trees().default_tree();
        let contacts = if end == 0 {
            board.trace_start_contacts(trace_item.id())
        } else {
            board.trace_end_contacts(trace_item.id())
        };
        let mut contact_count = 0;
        let mut store_end_corner = true;
        // Check for a contact object that cannot be shoved.
        for contact_id in contacts {
            let Some(contact) = board.item(contact_id) else {
                continue;
            };
            if !contact.is_routable() {
                self.found_obstacle = Some(contact_id);
                return false;
            }
            match &contact.kind {
                ItemKind::Trace(contact_trace) => {
                    // PARITY(quirk, java_quirks.store_trace_class_self_compare):
                    // Java compares the contact's class with itself here
                    // (ShapeTraceEntries.java:385); `false` compares it with the
                    // stored trace's.
                    let class_differs = !board.java_quirks().store_trace_class_self_compare
                        && contact.clearance_class() != trace_item.clearance_class();
                    if (board.is_shove_fixed(contact)
                        || contact_trace.half_width != trace.half_width
                        || class_differs)
                        && offset_shape.contains_inside(end_corner)
                    {
                        self.found_obstacle = Some(contact_id);
                        return false;
                    }
                }
                ItemKind::Via(_) => {
                    let Some(via_shape) = board.tile_shape_on_layer(contact_id, self.layer) else {
                        // Java throws on a via without a shape on the layer.
                        self.found_obstacle = Some(contact_id);
                        return false;
                    };
                    let mut via_trace_diff = via_shape.smallest_radius()
                        - f64::from(compensated_half_width_of(board, tree, trace_item));
                    if !tree.is_clearance_compensation_used() {
                        let via_clearance = board.clearance_value(
                            contact.clearance_class(),
                            self.clearance_class,
                            self.layer,
                        );
                        let trace_clearance = board.clearance_value(
                            trace_item.clearance_class(),
                            self.clearance_class,
                            self.layer,
                        );
                        if trace_clearance > via_clearance {
                            via_trace_diff += f64::from(via_clearance - trace_clearance);
                        }
                    }
                    if via_trace_diff < 0.0 {
                        // The via is smaller than the trace.
                        self.found_obstacle = Some(contact_id);
                        return false;
                    }
                    if via_trace_diff == 0.0 && !offset_shape.contains_inside(end_corner) {
                        // The via need not be shoved.
                        store_end_corner = false;
                    }
                }
                _ => {}
            }
            contact_count += 1;
        }
        if contact_count == 1 && store_end_corner {
            // PARITY(quirk, java_quirks.rational_perp_projection): a rational end
            // corner is projected with Java's formula unless the switch is `false`.
            if let Some(projection) = offset_shape
                .nearest_border_point_with(end_corner, board.java_quirks().rational_perp_projection)
                && let Some(projection_side) = offset_shape.contains_on_border_line_no(&projection)
            {
                // May not be correct: the trace may have no line suitable as
                // the end line of the substitute trace.
                let trace_line_no = if end == 0 {
                    0
                } else {
                    trace.polyline.lines().len() - 1
                };
                self.insert_entry_point(
                    trace_item.id(),
                    trace_line_no,
                    projection_side,
                    projection.to_float(),
                );
            }
        } else if contact_count == 0 && offset_shape.contains_inside(end_corner) {
            self.shape_contains_trace_tails = true;
        }
        true
    }

    fn nets_of(board: &BasicBoard, id: ItemId) -> &[i32] {
        board.item(id).map_or(&[], Item::net_numbers)
    }

    /// Java `searchFromSide`: without a legal side, the side of the first
    /// entry of an own-net trace (0 and no point without one).
    fn search_from_side(&mut self, board: &BasicBoard) {
        if self.from_side.is_some_and(|s| s.no.is_some()) {
            return;
        }
        let mut current = self.list_anchor;
        let mut no = 0;
        let mut entry_approx = None;
        while let Some(i) = current {
            let entry = &self.entries[i];
            if board
                .item(entry.trace)
                .is_some_and(|t| t.shares_net_no(&self.own_net_nos))
            {
                no = entry.edge_index;
                entry_approx = Some(entry.entry_approx);
                break;
            }
            current = entry.next;
        }
        self.from_side = Some(ShapeEntrySide::from_values(Some(no), entry_approx));
    }

    /// Java `resort`: rotates the entry list to start in the middle of the
    /// from side and removes the entries between the first and last entry
    /// of a connected set.
    fn resort(&mut self, board: &BasicBoard) {
        let edge_count = self.shape.border_line_count();
        let Some(from_side) = self.from_side else {
            return;
        };
        let Some(from_no) = from_side.no.filter(|no| *no < edge_count) else {
            // Java logs "from side not calculated".
            return;
        };
        let compare_corner1 = self.shape.corner_approx(from_no);
        let compare_corner2 = if from_no == edge_count - 1 {
            self.shape.corner_approx(0)
        } else {
            self.shape.corner_approx(from_no + 1)
        };
        let from_line = self.shape.border_line(from_no);
        let mut from_point_dist = 0.0;
        let mut from_point_projection = None;
        let mut border_intersection = from_side.border_intersection;
        if let Some(intersection) = border_intersection {
            let projection = intersection.projection_approx(&from_line);
            from_point_dist = projection.distance_square(compare_corner1);
            from_point_projection = Some(projection);
            if from_point_dist >= compare_corner1.distance_square(compare_corner2) {
                border_intersection = None;
                self.from_side = Some(ShapeEntrySide::from_values(Some(from_no), None));
            }
        }
        // The first entry between the side middle and compare_corner2.
        let mut current = self.list_anchor;
        while let Some(i) = current {
            let entry = &self.entries[i];
            if entry.edge_index > from_no {
                break;
            }
            if entry.edge_index == from_no {
                if border_intersection.is_some() {
                    let from_projection = from_point_projection.expect("projection");
                    let current_projection = entry.entry_approx.projection_approx(&from_line);
                    if current_projection.distance_square(compare_corner1) >= from_point_dist
                        && current_projection.distance_square(from_projection)
                            <= current_projection.distance_square(compare_corner1)
                    {
                        break;
                    }
                } else if entry.entry_approx.distance_square(compare_corner2)
                    <= entry.entry_approx.distance_square(compare_corner1)
                {
                    break;
                }
            }
            current = entry.next;
        }
        if let Some(i) = current
            && Some(i) != self.list_anchor
        {
            self.rotate_entry_list_around_anchor(i, edge_count);
        }
        self.remove_inner_entries(board);
    }

    /// The second half of Java's `resort`: keeps only the first and last
    /// entry of each run of one connected set, then drops own-net entries at
    /// the start and the end.
    fn remove_inner_entries(&mut self, board: &BasicBoard) {
        let Some(anchor) = self.list_anchor else {
            return;
        };
        let mut prev = anchor;
        let mut prev_nets = Self::nets_of(board, self.entries[prev].trace);
        let mut current = self.entries[anchor].next;
        let mut current_nets: &[i32];
        let mut next;
        if let Some(c) = current {
            current_nets = Self::nets_of(board, self.entries[c].trace);
            next = self.entries[c].next;
        } else {
            next = None;
            current_nets = &[];
        }
        let mut before_prev = None;
        while let Some(n) = next {
            let next_nets = Self::nets_of(board, self.entries[n].trace);
            if net_nos_equal(prev_nets, current_nets) && net_nos_equal(current_nets, next_nets) {
                self.entries[prev].next = Some(n);
            } else {
                before_prev = Some(prev);
                prev = current.expect("current entry");
                prev_nets = current_nets;
            }
            current_nets = next_nets;
            current = Some(n);
            next = self.entries[n].next;
        }
        // Remove entries of the own net at the start and the end.
        if current.is_some() && net_nos_equal(current_nets, &self.own_net_nos) {
            self.entries[prev].next = None;
            if net_nos_equal(prev_nets, &self.own_net_nos) {
                match before_prev {
                    Some(b) => self.entries[b].next = None,
                    None => self.list_anchor = None,
                }
            }
        }
        for _ in 0..2 {
            let Some(a) = self.list_anchor else {
                break;
            };
            if !board
                .item(self.entries[a].trace)
                .is_some_and(|t| t.nets_equal_no(&self.own_net_nos))
            {
                break;
            }
            self.list_anchor = self.entries[a].next;
        }
    }

    /// Java `calculateStackLevels`. False if the entries do not form a
    /// stack (the shove would have to cross a trace).
    fn calculate_stack_levels(&mut self, board: &BasicBoard) -> bool {
        let Some(anchor) = self.list_anchor else {
            return true;
        };
        let mut current_entry = Some(anchor);
        let mut current_nets = Self::nets_of(board, self.entries[anchor].trace);
        // The own net is ignored when calculating the stack level.
        let mut current_level = if net_nos_equal(current_nets, &self.own_net_nos) {
            0
        } else {
            1
        };
        while let Some(ce) = current_entry {
            if self.entries[ce].stack_level < 0 {
                self.trace_piece_count += 1;
                self.entries[ce].stack_level = current_level;
                if current_level > self.max_stack_level {
                    if self.max_stack_level > 1 {
                        self.found_obstacle = Some(self.entries[ce].trace);
                    }
                    self.max_stack_level = current_level;
                }
            }
            // Set the stack level of all entries of the current net.
            let mut check_entry = self.entries[ce].next;
            let mut index_of_next_foreign_set = 0;
            let mut index_of_last_occurrence_of_set = 0;
            let mut next_index = 0;
            let mut last_own_entry = None;
            let mut first_foreign_entry = None;
            while let Some(ch) = check_entry {
                next_index += 1;
                let check_nets = Self::nets_of(board, self.entries[ch].trace);
                if net_nos_equal(check_nets, current_nets) {
                    index_of_last_occurrence_of_set = next_index;
                    last_own_entry = Some(ch);
                    self.entries[ch].stack_level = self.entries[ce].stack_level;
                } else if index_of_next_foreign_set == 0 {
                    // The first entry of a foreign connected set.
                    index_of_next_foreign_set = next_index;
                    first_foreign_entry = Some(ch);
                }
                check_entry = self.entries[ch].next;
            }
            if next_index == 0 {
                current_entry = None;
                continue;
            }
            let next_entry;
            if index_of_next_foreign_set != 0
                && index_of_next_foreign_set < index_of_last_occurrence_of_set
            {
                // Raise the level.
                let n = first_foreign_entry.expect("foreign entry");
                if self.entries[n].stack_level >= 0 {
                    // Already calculated: the stack property fails.
                    return false;
                }
                next_entry = n;
                current_level += 1;
            } else if index_of_last_occurrence_of_set != 0 {
                next_entry = last_own_entry.expect("own entry");
            } else {
                let n = first_foreign_entry.expect("foreign entry");
                if self.entries[n].stack_level >= 0 {
                    current_level -= 1;
                    if self.entries[n].stack_level != current_level {
                        return false;
                    }
                }
                next_entry = n;
            }
            current_nets = Self::nets_of(board, self.entries[next_entry].trace);
            // The entries between current and next are irrelevant.
            self.entries[ce].next = Some(next_entry);
            current_entry = Some(next_entry);
        }
        // Java logs "currentLevel inconsistent".
        current_level == 1
    }

    /// Java `insertEntryPoint`: inserts the entry into the list sorted by
    /// edge, and along an edge by the projection from its start corner.
    fn insert_entry_point(
        &mut self,
        trace: ItemId,
        trace_line_no: usize,
        edge_index: usize,
        entry_approx: FloatPoint,
    ) {
        let new_index = self.entries.len();
        let mut current_prev = None;
        let mut current_next = self.list_anchor;
        while let Some(n) = current_next {
            let next = &self.entries[n];
            if next.edge_index > edge_index {
                break;
            }
            if next.edge_index == edge_index {
                let prev_corner = self.shape.corner_approx(edge_index);
                let next_corner = if edge_index == self.shape.border_line_count() - 1 {
                    self.shape.corner_approx(0)
                } else {
                    self.shape.corner_approx(edge_index + 1)
                };
                if prev_corner.scalar_product(entry_approx, next_corner)
                    <= prev_corner.scalar_product(next.entry_approx, next_corner)
                {
                    break;
                }
            }
            current_prev = Some(n);
            current_next = next.next;
        }
        self.entries.push(EntryPoint {
            trace,
            trace_line_no,
            edge_index,
            entry_approx,
            stack_level: -1,
            next: current_next,
        });
        match current_prev {
            Some(p) => self.entries[p].next = Some(new_index),
            None => self.list_anchor = Some(new_index),
        }
    }

    /// Java `rotateEntryListAroundAnchor`: `new_anchor` becomes the list
    /// head; the entries moved behind the old tail get `edge_count` added to
    /// their edge index.
    fn rotate_entry_list_around_anchor(&mut self, new_anchor: usize, edge_count: usize) {
        let mut prev = new_anchor;
        while let Some(n) = self.entries[prev].next {
            prev = n;
        }
        self.entries[prev].next = self.list_anchor;
        let mut current = self.list_anchor;
        while let Some(c) = current {
            if c == new_anchor {
                break;
            }
            // Distinguishes the entries before and after the middle of the
            // from side.
            self.entries[c].edge_index += edge_count;
            prev = c;
            current = self.entries[c].next;
        }
        self.entries[prev].next = None;
        self.list_anchor = Some(new_anchor);
    }

    /// Java `popPiece`: removes the next run of entries of the maximal stack
    /// level and one net from the list and returns its first and last
    /// entry; own-net runs are skipped.
    fn pop_piece(&mut self, board: &BasicBoard) -> Option<(usize, usize)> {
        // Java logs when the piece count is inconsistent at the end.
        let anchor = self.list_anchor?;
        let mut first = Some(anchor);
        let mut prev_first = None;
        while let Some(f) = first {
            if self.entries[f].stack_level == self.max_stack_level {
                break;
            }
            prev_first = Some(f);
            first = self.entries[f].next;
        }
        // Java logs "maxStackLevel not found".
        let first = first?;
        let first_trace = board.item(self.entries[first].trace);
        let mut last = first;
        let mut after_last = self.entries[first].next;
        while let Some(a) = after_last {
            let entry = &self.entries[a];
            let same_nets = match (board.item(entry.trace), first_trace) {
                (Some(t), Some(f)) => t.nets_equal(f),
                _ => false,
            };
            if entry.stack_level != self.max_stack_level || !same_nets {
                break;
            }
            last = a;
            after_last = entry.next;
        }
        match prev_first {
            Some(p) => self.entries[p].next = after_last,
            None => self.list_anchor = after_last,
        }
        self.max_stack_level = 0;
        let mut current = self.list_anchor;
        while let Some(c) = current {
            self.max_stack_level = self.max_stack_level.max(self.entries[c].stack_level);
            current = self.entries[c].next;
        }
        self.trace_piece_count -= 1;
        if first_trace.is_some_and(|t| t.nets_equal_no(&self.own_net_nos)) {
            // The own net is ignored and may occur only at the lowest level.
            return self.pop_piece(board);
        }
        Some((first, last))
    }

    /// Java `nextSubstituteTracePiece`: the next substitute trace around the
    /// shape, as a temporary trace item that is not on the board; `None` at
    /// the end. Each returned piece takes an item id, as Java's
    /// `PolylineTrace` constructor does (O00 "Ids"); empty pieces are skipped
    /// without one.
    pub fn next_substitute_trace_piece(&mut self, board: &mut BasicBoard) -> Option<Item> {
        loop {
            let (first, last) = self.pop_piece(board)?;
            let (start, end) = (&self.entries[first], &self.entries[last]);
            let current = board.item(start.trace)?;
            let current_trace = current.as_trace()?;
            let offset_shape = trace_offset_shape(
                board,
                &self.shape,
                current,
                self.clearance_class,
                self.layer,
            );
            let edge_count = self.shape.border_line_count();
            let Some(edge_diff) = end.edge_index.checked_sub(start.edge_index) else {
                // Java builds fewer than 3 lines here (or throws below -2):
                // an empty piece.
                continue;
            };
            let end_trace = board.item(end.trace)?.as_trace()?;
            // Start with the trace line at the start entry, end with the one
            // at the end entry, and fill in the edges of the offset shape.
            let mut piece_lines: Vec<Line> = Vec::with_capacity(edge_diff + 3);
            piece_lines.push(current_trace.polyline.lines()[start.trace_line_no]);
            let mut current_edge_no = start.edge_index % edge_count;
            for _ in 0..=edge_diff {
                piece_lines.push(offset_shape.border_line(current_edge_no));
                current_edge_no = if current_edge_no == edge_count - 1 {
                    0
                } else {
                    current_edge_no + 1
                };
            }
            piece_lines.push(end_trace.polyline.lines()[end.trace_line_no]);
            let piece_polyline = Polyline::new(&piece_lines);
            if piece_polyline.is_empty() {
                // No valid trace piece: the next one.
                continue;
            }
            let half_width = current_trace.half_width;
            let nets = current.net_numbers().to_vec();
            let class = current.clearance_class();
            let id = board.burn_item_id();
            return Some(Item::new(
                id,
                nets,
                class,
                0,
                FixedState::Unfixed,
                ItemKind::Trace(PolylineTrace {
                    polyline: piece_polyline,
                    layer: self.layer,
                    half_width,
                }),
            ));
        }
    }
}

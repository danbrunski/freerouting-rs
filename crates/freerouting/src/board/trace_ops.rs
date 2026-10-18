//! Splitting, combining and normalizing traces.
//!
//! Port of the board-changing methods of `board.trace.PolylineTrace` and
//! `PolylineTraceNormalization`, and of the trace clean-up methods of
//! `BasicBoard` and `RoutingBoard`.

use std::collections::BTreeSet;

use crate::util::HashMap;

use freerouting_geometry::{
    IntOctagon, Line, LineSegment, Point, Polyline, PolylineShapeOps, Stoppable, TileShape,
};

use crate::board::autoroute_log::AutorouteChange;
use crate::board::basic_board::{BasicBoard, MAX_NORMALIZE_ITERATIONS};
use crate::board::items::{Item, ItemId, ItemKind, StopConnectionOption, point_tile};
use crate::board::search::{TraceChange, TraceMerge, TreeEntry};
use crate::util::java_hash_map_order;

/// The recursion limit of [`BasicBoard::normalize_trace`].
const MAX_NORMALIZATION_DEPTH: usize = 16;

/// Which lines of a trace's new polyline are the old polyline's own lines,
/// for [`BasicBoard::change_trace`].
///
/// Java's `PolylineTrace.change` finds the unchanged start and end of the
/// trace by comparing the `Line` objects by *reference*
/// (`PolylineTrace.java:956-982`), and keeps the tree leaves of the shapes
/// around them. The port has no object identity, so the caller says it:
/// `prefix` is the number of leading lines of the new polyline that are the
/// same Java objects as the old polyline's at the same index, `suffix` the
/// number of trailing ones (counted from each polyline's end). Callers compute
/// it from how they built the new polyline (O00: "unchanged is an explicit
/// outcome"). Java's `new Polyline(Line[])` creates a new object wherever
/// it turns a line around, and `Line.translate` always does.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LineReuse {
    pub prefix: usize,
    pub suffix: usize,
}

impl LineReuse {
    /// No line is reused: every line of the new polyline is a new object.
    pub const NONE: LineReuse = LineReuse {
        prefix: 0,
        suffix: 0,
    };

    /// The leading and trailing lines that are equal by value. Only for
    /// callers whose Java code is shown to create new line objects exactly
    /// where the values change; each says why in a `// PARITY:` comment.
    pub fn by_value(old: &Polyline, new: &Polyline) -> LineReuse {
        let (old, new) = (old.lines(), new.lines());
        let same = |a: &Line, b: &Line| a.a() == b.a() && a.b() == b.b();
        let last_index = old.len().min(new.len());
        let prefix = (0..last_index)
            .take_while(|&i| same(&old[i], &new[i]))
            .count();
        let suffix = (1..=last_index)
            .take_while(|&i| same(&old[old.len() - i], &new[new.len() - i]))
            .count();
        LineReuse { prefix, suffix }
    }
}

/// What [`BasicBoard::change_trace`] did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeOutcome {
    /// The new lines are the old ones (Java's early return); only the undo
    /// snapshot was taken.
    Unchanged,
    Changed,
}

/// The data of an item found by an overlap query in
/// [`BasicBoard::split_trace`]. Java keeps references to the found items,
/// whose data stays readable after they are removed from the board; the
/// port keeps a copy of what the split needs.
#[derive(Clone, Debug)]
struct FoundItem {
    nets: Vec<i32>,
    kind: FoundKind,
}

#[derive(Clone, Debug)]
enum FoundKind {
    Trace(Polyline),
    Drill(Point),
    ConductionArea,
    Other,
}

impl FoundItem {
    fn of(item: &Item) -> Self {
        let kind = match &item.kind {
            ItemKind::Trace(trace) => FoundKind::Trace(trace.polyline.clone()),
            ItemKind::Pin(_) | ItemKind::Via(_) => {
                FoundKind::Drill(Point::Int(item.drill().expect("drill").center))
            }
            ItemKind::Area(area) if area.is_conduction_area() => FoundKind::ConductionArea,
            _ => FoundKind::Other,
        };
        FoundItem {
            nets: item.net_numbers().to_vec(),
            kind,
        }
    }

    fn shares_net(&self, item: &Item) -> bool {
        self.nets.iter().any(|n| item.net_numbers().contains(n))
    }
}

impl BasicBoard {
    fn is_on_board(&self, id: ItemId) -> bool {
        self.items.get(&id).is_some_and(Item::is_on_the_board)
    }

    fn trace_data(&self, id: ItemId) -> Option<(&Item, &crate::board::items::PolylineTrace)> {
        let item = self.items.get(&id)?;
        Some((item, item.as_trace()?))
    }

    /// Splits the traces of net `net_number` on `layer` that contain
    /// `location`. Returns true if a trace was split.
    pub fn split_traces(&mut self, location: &Point, layer: i32, net_number: i32) -> bool {
        let picked: Vec<Item> = self
            .pick_items(location, layer)
            .into_iter()
            .map(|id| self.item_ref(id).clone())
            .filter(Item::is_trace)
            .collect();
        let location_shape = point_tile(location)
            .bounding_octagon()
            .expect("bounded point shape");
        let mut trace_split = false;
        for trace in picked {
            if trace.contains_net(net_number) {
                let pieces = self.split_trace_item(trace, Some(&location_shape));
                if pieces.len() != 1 {
                    trace_split = true;
                }
            }
        }
        trace_split
    }

    /// Splits trace `id` and the traces of its nets at their intersections;
    /// cycles created by the splits are removed. Returns the pieces of
    /// trace `id` (just `id` if it was not split). If `clip_shape` is given
    /// only segments touching it are considered.
    ///
    /// Port of `PolylineTrace.split(IntOctagon)`.
    pub fn split_trace(&mut self, id: ItemId, clip_shape: Option<&IntOctagon>) -> Vec<ItemId> {
        match self.items.get(&id) {
            Some(item) => self.split_trace_item(item.clone(), clip_shape),
            None => vec![id],
        }
    }

    fn split_trace_item(&mut self, this: Item, clip_shape: Option<&IntOctagon>) -> Vec<ItemId> {
        let id = this.id();
        let mut result = Vec::new();
        let Some(this_trace) = this.as_trace() else {
            return vec![id];
        };
        if !this.nets_normal() {
            result.push(id);
            return result;
        }
        let polyline = &this_trace.polyline;
        let layer = this_trace.layer;
        let mut own_trace_split = false;
        let mut found_items: HashMap<ItemId, FoundItem> = HashMap::default();
        for i in 0..polyline.lines().len().saturating_sub(2) {
            if let Some(clip) = clip_shape {
                let segment = LineSegment::from_polyline(polyline, i + 1);
                if !clip.intersects_octagon(&segment.bounding_box().to_int_octagon()) {
                    continue;
                }
            }
            let ctx = self.shape_context();
            let current_shape = self.search_trees.default_tree().tree_shape(&this, i, &ctx);
            let current_segment = LineSegment::from_polyline(polyline, i + 1);
            let mut entries: Vec<TreeEntry> = Vec::new();
            if let Some(shape) = &current_shape {
                self.collect_entries(shape, layer, &mut entries, &mut found_items);
            }
            let mut index = 0;
            while index < entries.len() {
                if !self.is_on_board(id) {
                    // this trace was deleted in a clean-up operation
                    return result;
                }
                let entry = entries[index];
                index += 1;
                let found = found_items[&entry.item].clone();
                if entry.item == id {
                    let found_index = entry.shape_index;
                    if found_index + 1 >= i && found_index <= i + 1 {
                        // don't split the own trace at this line or its
                        // neighbours
                        continue;
                    }
                    // intermediate segments of length 0
                    if i < found_index {
                        if polyline.corner(i + 1) == polyline.corner(found_index) {
                            continue;
                        }
                    } else if polyline.corner(found_index + 1) == polyline.corner(i) {
                        continue;
                    }
                }
                if !found.shares_net(&this) {
                    continue;
                }
                match &found.kind {
                    FoundKind::Trace(found_polyline) => {
                        let found_segment =
                            LineSegment::from_polyline(found_polyline, entry.shape_index + 1);
                        let intersecting = found_segment.intersection(&current_segment);
                        let mut split_pieces: Vec<ItemId> = Vec::new();
                        let mut found_trace_split = false;
                        if entry.item != id {
                            for line in &intersecting {
                                let pieces = self.split_trace_at_line(
                                    entry.item,
                                    entry.shape_index + 1,
                                    line,
                                );
                                if let Some(pieces) = pieces {
                                    for piece in pieces.into_iter().flatten() {
                                        found_trace_split = true;
                                        split_pieces.push(piece);
                                    }
                                    if found_trace_split {
                                        // The board has changed: append the
                                        // entries found now and start over.
                                        if let Some(shape) = &current_shape {
                                            self.collect_entries(
                                                shape,
                                                layer,
                                                &mut entries,
                                                &mut found_items,
                                            );
                                        }
                                        index = 0;
                                        break;
                                    }
                                }
                            }
                            if !found_trace_split {
                                split_pieces.push(entry.item);
                            }
                        }
                        // now try splitting the own trace
                        let intersecting = current_segment.intersection(&found_segment);
                        for line in &intersecting {
                            if let Some(pieces) = self.split_trace_at_line(id, i + 1, line) {
                                own_trace_split = true;
                                let snapshots: Vec<Option<Item>> = pieces
                                    .iter()
                                    .map(|p| p.map(|p| self.item_ref(p).clone()))
                                    .collect();
                                for piece in snapshots.into_iter().flatten() {
                                    let piece_result = self.split_trace_item(piece, clip_shape);
                                    result.extend(piece_result);
                                }
                                break;
                            }
                        }
                        if found_trace_split || own_trace_split {
                            // remove cycles containing a split piece, the
                            // own pieces last to preserve them if possible
                            for piece in split_pieces {
                                self.remove_if_cycle(piece);
                            }
                            for piece in result.clone() {
                                self.remove_if_cycle(piece);
                            }
                        }
                        if own_trace_split {
                            break;
                        }
                    }
                    FoundKind::Drill(center) => {
                        if current_segment.contains(center) {
                            let direction =
                                current_segment.get_line().int_direction().turn_45_degree(2);
                            let split_line =
                                Line::from_point_and_direction(center.expect_int(), direction);
                            // Java ignores the result here.
                            self.split_trace_at_line(id, i + 1, &split_line);
                        }
                    }
                    FoundKind::ConductionArea => {
                        if !this.is_user_fixed() {
                            let ignore_areas = self.ignore_cycles_with_areas(&this);
                            if !ignore_areas
                                && self.trace_start_contacts(id).contains(&entry.item)
                                && self.trace_end_contacts(id).contains(&entry.item)
                            {
                                // a cycle with the conduction area
                                self.remove_item(id);
                                return result;
                            }
                        }
                    }
                    FoundKind::Other => {}
                }
            }
            if own_trace_split {
                break;
            }
        }
        if !own_trace_split {
            result.push(id);
        }
        if result.len() > 1 && self.autoroute_log.is_some() {
            // Java `additionalUpdateAfterChange` on each piece.
            for piece in &result {
                self.log_autoroute(AutorouteChange::SplitPiece(*piece));
            }
        }
        result
    }

    /// Appends the default tree entries overlapping `shape` on `layer` to
    /// `entries` and records the data of the found items.
    fn collect_entries(
        &self,
        shape: &TileShape,
        layer: i32,
        entries: &mut Vec<TreeEntry>,
        found_items: &mut HashMap<ItemId, FoundItem>,
    ) {
        let start = entries.len();
        self.search_trees.default_tree().overlapping_tree_entries(
            shape,
            layer,
            &[],
            &self.items,
            entries,
        );
        for entry in &entries[start..] {
            found_items
                .entry(entry.item)
                .or_insert_with(|| FoundItem::of(self.item_ref(entry.item)));
        }
    }

    /// Splits trace `id` at its line `line_index` by inserting `end_line`
    /// as last line of the first piece and first line of the second.
    /// Returns the pieces (each `None` if it was too short to insert), or
    /// `None` if nothing was split.
    ///
    /// Port of `PolylineTrace.split(int, Line)`.
    fn split_trace_at_line(
        &mut self,
        id: ItemId,
        line_index: usize,
        end_line: &Line,
    ) -> Option<[Option<ItemId>; 2]> {
        let (item, trace) = self.trace_data(id)?;
        if !item.is_on_the_board() || self.is_deletion_forbidden(item) {
            return None;
        }
        let [first, second] = trace.polyline.split(line_index, end_line)?;
        if self.split_inside_drill_pad_prohibited(id, line_index, end_line) {
            return None;
        }
        let (layer, half_width) = (trace.layer, trace.half_width);
        let nets = item.net_numbers().to_vec();
        let (class, fixed_state) = (item.clearance_class(), item.fixed_state());
        self.remove_item(id);
        let a =
            self.insert_trace_without_cleaning(first, layer, half_width, &nets, class, fixed_state);
        let b = self.insert_trace_without_cleaning(
            second,
            layer,
            half_width,
            &nets,
            class,
            fixed_state,
        );
        Some([a, b])
    }

    /// Java `PolylineTrace.splitInsideDrillPadProhibited`: a trace is split
    /// inside a pin pad only at the pin center.
    fn split_inside_drill_pad_prohibited(
        &self,
        id: ItemId,
        line_index: usize,
        line: &Line,
    ) -> bool {
        let Some((item, trace)) = self.trace_data(id) else {
            return false;
        };
        let intersection = trace.polyline.lines()[line_index].intersection(line);
        let mut pad_found = false;
        for other_id in self.pick_items(&intersection, trace.layer) {
            let other = self.item_ref(other_id);
            if !other.shares_net(item) {
                continue;
            }
            match &other.kind {
                ItemKind::Pin(pin) => {
                    if Point::Int(pin.drill.center) == intersection {
                        return false;
                    }
                    pad_found = true;
                }
                // Java: `t != this && t.first == p || t.last == p`
                ItemKind::Trace(t)
                    if (other_id != id && t.first_corner() == intersection)
                        || t.last_corner() == intersection =>
                {
                    return false;
                }
                _ => {}
            }
        }
        pad_found
    }

    /// Splits trace `id` at `point`, if it is on a segment. Returns the
    /// pieces.
    ///
    /// Port of `PolylineTrace.split(Point)`.
    pub fn split_trace_at_point(
        &mut self,
        id: ItemId,
        point: &Point,
    ) -> Option<[Option<ItemId>; 2]> {
        let (_, trace) = self.trace_data(id)?;
        let polyline = trace.polyline.clone();
        for i in 0..polyline.lines().len().saturating_sub(2) {
            let segment = LineSegment::from_polyline(&polyline, i + 1);
            if segment.contains(point) {
                let direction = segment.get_line().int_direction().turn_45_degree(2);
                let split_line = Line::from_point_and_direction(point.expect_int(), direction);
                if let Some(result) = self.split_trace_at_line(id, i + 1, &split_line) {
                    return Some(result);
                }
            }
        }
        None
    }

    /// Combines trace `id` with traces that continue it without fork.
    /// Returns true if something was combined; trace `id` remains.
    ///
    /// Port of `PolylineTrace.combine`.
    pub fn combine_trace(&mut self, id: ItemId) -> bool {
        let mut something_changed = false;
        while self.is_on_board(id)
            && (self.combine_trace_at(id, true) || self.combine_trace_at(id, false))
        {
            something_changed = true;
            // Java `board.additionalUpdateAfterChange(this)` after each combine.
            self.log_autoroute(AutorouteChange::Combined(id));
        }
        something_changed
    }

    /// Java `combineAtStart(true)` (`at_start`) and `combineAtEnd(true)`.
    fn combine_trace_at(&mut self, id: ItemId, at_start: bool) -> bool {
        let Some((this, this_trace)) = self.trace_data(id) else {
            return false;
        };
        let corner = if at_start {
            this_trace.first_corner()
        } else {
            this_trace.last_corner()
        };
        let contacts: Vec<ItemId> = self
            .trace_normal_contacts(id, &corner, false)
            .into_iter()
            .filter(|c| !self.item_ref(*c).is_conduction_area())
            .collect();
        if contacts.len() != 1 {
            return false;
        }
        let other_id = contacts[0];
        let other = self.item_ref(other_id);
        let Some(other_trace) = other.as_trace() else {
            return false;
        };
        if !(other_trace.layer == this_trace.layer
            && other.nets_equal(this)
            && other_trace.half_width == this_trace.half_width
            && other.fixed_state() == this.fixed_state()
            && !self.is_deletion_forbidden(other)
            && !self.is_deletion_forbidden(this))
        {
            return false;
        }
        let reverse_order = if at_start {
            if corner == other_trace.last_corner() {
                false
            } else if corner == other_trace.first_corner() {
                true
            } else {
                return false;
            }
        } else if corner == other_trace.first_corner() {
            false
        } else if corner == other_trace.last_corner() {
            true
        } else {
            return false;
        };
        let this_lines = this_trace.polyline.lines().to_vec();
        let other_lines: Vec<Line> = if reverse_order {
            other_trace
                .polyline
                .lines()
                .iter()
                .rev()
                .map(Line::opposite)
                .collect()
        } else {
            other_trace.polyline.lines().to_vec()
        };
        let layer = this_trace.layer;
        let half_width = this_trace.half_width;
        let clearance_class = this.clearance_class();
        // Java `changeOrder` of the merge: the other trace runs the other way.
        let change_order = if at_start {
            other_trace.first_corner() == this_trace.first_corner()
        } else {
            other_trace.last_corner() == this_trace.last_corner()
        };
        let (front, back) = if at_start {
            (&other_lines, &this_lines)
        } else {
            (&this_lines, &other_lines)
        };
        let skip_line = front[front.len() - 2].is_equal_or_opposite(&back[1]);
        let keep = front.len() - 1 - usize::from(skip_line);
        let mut new_lines: Vec<Line> = front[..keep].to_vec();
        new_lines.extend_from_slice(&back[1..]);
        let joined = Polyline::new(&new_lines);
        // Java's entry numbers: the other trace's lines in front, this
        // trace's at the end.
        let joint_lines = if at_start {
            other_lines.len()
        } else {
            this_lines.len()
        };

        self.items.save_for_undo(&id);
        self.note_changed(id);
        let has_tree_entries = {
            let tree = self.search_trees.default_tree();
            tree.contains(id) && tree.contains(other_id)
        };
        if joined.lines().len() != new_lines.len() || !has_tree_entries {
            // Java `replaceGeometry`: parallel lines were skipped at the
            // join, or a trace lacks default-tree entries.
            self.remove_from_trees(id);
            self.set_trace_polyline(id, joined);
            self.insert_into_trees(id);
        } else {
            let merge = TraceMerge {
                from: other_id,
                to: id,
                change_order,
                joined: &joined,
                to_half_width: half_width,
                to_clearance_class: clearance_class,
                to_layer: layer,
                from_entry_no: joint_lines - 3,
                to_entry_no: joint_lines - usize::from(skip_line),
            };
            let missing =
                self.search_trees
                    .merge_entries(&merge, at_start, &self.rules.clearance_matrix);
            // Java `otherTrace.clearSearchTreeEntries()`: the merge took the
            // other trace's entries, so its removal below removes no leaves.
            self.set_trace_polyline(id, joined);
            self.reinsert_in_trees(id, &missing);
        }
        self.note_changed(id);
        let too_short = self
            .trace_data(id)
            .is_some_and(|(_, t)| t.polyline.lines().len() < 3);
        if too_short {
            self.remove_item(id);
        }
        self.remove_item(other_id);
        self.join_changed_area(corner.to_float(), layer);
        true
    }

    /// Sets the polyline of trace `id` without touching the trees.
    fn set_trace_polyline(&mut self, id: ItemId, polyline: Polyline) {
        if let Some(ItemKind::Trace(trace)) = self.items.get_mut(&id).map(|i| &mut i.kind) {
            trace.polyline = polyline;
        }
    }

    /// Splits trace `id` at intersections and combines the pieces. Returns
    /// true if something changed.
    ///
    /// Port of `PolylineTrace.normalize`.
    pub fn normalize_trace(&mut self, id: ItemId, clip_shape: Option<&IntOctagon>) -> bool {
        self.normalize_trace_recu(id, clip_shape, 0)
    }

    fn normalize_trace_recu(
        &mut self,
        id: ItemId,
        clip_shape: Option<&IntOctagon>,
        depth: usize,
    ) -> bool {
        if depth > MAX_NORMALIZATION_DEPTH {
            return false;
        }
        if !self.items.contains(&id) {
            // Only a trace that a combine removed for being too short gets
            // here; Java's split of it changes nothing.
            return false;
        }
        let split_pieces = self.split_trace(id, clip_shape);
        let mut result = split_pieces.len() != 1;
        for piece in split_pieces {
            if !self.is_on_board(piece) {
                continue;
            }
            let combined = self.combine_trace(piece);
            let degenerate = self
                .trace_data(piece)
                .is_some_and(|(_, t)| t.corner_count() == 2 && t.first_corner() == t.last_corner());
            if degenerate {
                // remove a trace with only one corner, if allowed
                if !self.is_deletion_forbidden(self.item_ref(piece)) {
                    self.remove_item(piece);
                    result = true;
                }
            } else if combined {
                self.normalize_trace_recu(piece, clip_shape, depth + 1);
                result = true;
            }
        }
        result
    }

    /// Changes the geometry of trace `id` to `polyline` and normalizes it
    /// with the changed area's clip. `reuse` says how many leading and
    /// trailing lines of `polyline` are the old polyline's own lines (see
    /// [`LineReuse`]); their tree leaves are kept.
    ///
    /// Port of `PolylineTrace.change`, with its order: `saveForUndo` first,
    /// then the early return when nothing changed, then `changeEntries` on
    /// every tree, the new lines and the normalization. A trace that is not
    /// on the board only gets the new lines.
    pub fn change_trace(
        &mut self,
        id: ItemId,
        polyline: Polyline,
        reuse: LineReuse,
    ) -> ChangeOutcome {
        let Some((item, trace)) = self.trace_data(id) else {
            return ChangeOutcome::Unchanged;
        };
        if !item.is_on_the_board() {
            self.set_trace_polyline(id, polyline);
            return ChangeOutcome::Changed;
        }
        let (layer, half_width, clearance_class) =
            (trace.layer, trace.half_width, item.clearance_class());
        let old_lines = trace.polyline.lines().len();
        let new_lines = polyline.lines().len();
        // Java's additionalUpdateAfterChange does nothing without retained
        // autorouter rooms; the change log records it (B02.8).
        self.log_autoroute(AutorouteChange::Changed(id));
        self.items.save_for_undo(&id);
        let last_index = old_lines.min(new_lines);
        // PARITY: Java scans for the first and the last line that is not the
        // same object; both scans finding none means no change.
        let first_different = reuse.prefix.min(last_index);
        if first_different == last_index || reuse.suffix >= last_index {
            return ChangeOutcome::Unchanged;
        }
        let keep_at_start = first_different.saturating_sub(2);
        let keep_at_end = reuse.suffix.saturating_sub(2);
        self.note_changed(id);
        let change = TraceChange {
            id,
            new_polyline: &polyline,
            half_width,
            clearance_class,
            layer,
            keep_at_start,
            keep_at_end,
        };
        let missing = self
            .search_trees
            .change_entries(&change, &self.rules.clearance_matrix);
        self.set_trace_polyline(id, polyline);
        self.reinsert_in_trees(id, &missing);
        self.note_changed(id);
        let clip = self.changed_area_on_layer(layer);
        // Java logs and swallows a normalization exception; the port's
        // normalization has no failure path.
        self.normalize_trace(id, clip.as_ref());
        ChangeOutcome::Changed
    }

    /// Combines the traces of net `net_number` (all nets if negative) that
    /// continue each other without fork.
    pub fn combine_traces(&mut self, net_number: i32) -> bool {
        let mut result = false;
        loop {
            let mut something_changed = false;
            for id in self.item_ids() {
                let Some(item) = self.items.get(&id) else {
                    continue;
                };
                if (net_number < 0 || item.contains_net(net_number))
                    && item.is_trace()
                    && item.is_on_the_board()
                    && self.combine_trace(id)
                {
                    something_changed = true;
                    result = true;
                    break;
                }
            }
            if !something_changed {
                return result;
            }
        }
    }

    fn net_traces(&self, net_number: i32) -> Vec<ItemId> {
        self.items()
            .filter(|i| i.contains_net(net_number) && i.is_trace() && i.is_on_the_board())
            .map(Item::id)
            .collect()
    }

    fn net_name(&self, net_number: i32) -> String {
        self.rules
            .nets
            .get(net_number)
            .map_or_else(|| net_number.to_string(), |net| net.name.clone())
    }

    /// One normalization pass over `traces`; true if something changed.
    ///
    /// The normalization can remove the trace and still report no change:
    /// when the last tree entry its split looks at is a drill on the trace,
    /// the split cuts the trace there (replacing it by two pieces, which the
    /// drill case does not collect) and returns the removed trace as its only
    /// piece, as Java's `PolylineTrace.split` does. Java goes on with the
    /// stale `PolylineTrace`: `isUserFixed` reads its field and
    /// `removeIfCycle` returns false for a trace off the board
    /// (`BasicBoard.normalizeTraces` and `normalizeAllTraces`). The port asks
    /// the board instead of reading the removed item.
    ///
    /// `stop` ends the pass at the next trace once it asks (see
    /// [`normalize_traces_until`](Self::normalize_traces_until)); the pass
    /// then reports what it changed so far.
    fn normalize_pass(&mut self, traces: &[ItemId], stop: Option<&dyn Stoppable>) -> bool {
        let mut something_changed = false;
        for &id in traces {
            if stop.is_some_and(|s| s.is_stop_requested()) {
                break;
            }
            if !self.is_on_board(id) {
                continue;
            }
            if self.normalize_trace(id, None) || self.remove_if_unfixed_cycle(id) {
                something_changed = true;
            }
        }
        something_changed
    }

    /// `!trace.isUserFixed() && removeIfCycle(trace)` of
    /// `BasicBoard.normalizeTraces`. A trace no longer on the board is no
    /// cycle to remove, whatever its fixed state was.
    fn remove_if_unfixed_cycle(&mut self, id: ItemId) -> bool {
        self.is_on_board(id) && !self.item_ref(id).is_user_fixed() && self.remove_if_cycle(id)
    }

    /// Normalizes the traces of net `net_number`. Gives up after
    /// [`MAX_NORMALIZE_ITERATIONS`] passes and skips the net from then on.
    pub fn normalize_traces(&mut self, net_number: i32) -> bool {
        self.normalize_traces_until(net_number, None)
    }

    /// [`normalize_traces`](Self::normalize_traces) that also ends when
    /// `stop` asks. The first pass always runs to its end, so a new trace is
    /// normalized once; a stop ends a later pass at its next trace and
    /// leaves the traces as they are, as the iteration limit does, without
    /// suppressing the net. Without a stop request the result is
    /// `normalize_traces`'s.
    ///
    /// Java's `normalizeTraces` has no stop check: a net whose split and
    /// combine oscillate runs all [`MAX_NORMALIZE_ITERATIONS`] passes, which
    /// on a large net is minutes (DIVERGENCES.md, "Stopping").
    pub fn normalize_traces_until(
        &mut self,
        net_number: i32,
        stop: Option<&dyn Stoppable>,
    ) -> bool {
        if self.normalize_suppressed().contains(&net_number) {
            return false;
        }
        let mut result = false;
        let mut iteration_count = 0;
        loop {
            iteration_count += 1;
            if iteration_count > MAX_NORMALIZE_ITERATIONS {
                let net_name = self.net_name(net_number);
                self.log.push(format!(
                    "BasicBoard.normalizeTraces: reached {MAX_NORMALIZE_ITERATIONS} iterations for net '{net_name}' — stopping to prevent hang. The board geometry for this net may be oscillating (split/combine cycle); traces are kept as-is and further normalization for this net is suppressed on this board candidate."
                ));
                self.normalize_suppressed().insert(net_number);
                break;
            }
            let later_pass_stop = stop.filter(|_| iteration_count > 1);
            if later_pass_stop.is_some_and(|s| s.is_stop_requested()) {
                break;
            }
            let traces = self.net_traces(net_number);
            if !self.normalize_pass(&traces, later_pass_stop) {
                break;
            }
            result = true;
        }
        result
    }

    /// Normalizes the traces of all nets, net by net in the iteration order
    /// of Java's `HashMap<Integer, List>`.
    pub fn normalize_all_traces(&mut self) -> bool {
        let mut result = false;
        let mut traces_by_net: HashMap<i32, Vec<ItemId>> = HashMap::default();
        let mut net_order: Vec<i32> = Vec::new();
        for item in self.items() {
            if item.is_trace() && item.is_on_the_board() {
                for net in item.net_numbers() {
                    traces_by_net
                        .entry(*net)
                        .or_insert_with(|| {
                            net_order.push(*net);
                            Vec::new()
                        })
                        .push(item.id());
                }
            }
        }
        for net_number in java_hash_map_order(&net_order) {
            let mut net_traces = traces_by_net.remove(&net_number).unwrap_or_default();
            let mut iteration_count = 0;
            loop {
                iteration_count += 1;
                if iteration_count > MAX_NORMALIZE_ITERATIONS {
                    let net_name = self.net_name(net_number);
                    self.log.push(format!(
                        "BasicBoard.normalize_all_traces: reached {MAX_NORMALIZE_ITERATIONS} iterations for net '{net_name}' — stopping to prevent hang."
                    ));
                    break;
                }
                if !self.normalize_pass(&net_traces, None) {
                    break;
                }
                result = true;
                net_traces = self.net_traces(net_number);
            }
        }
        result
    }

    /// The trace of exactly the nets `net_numbers` ending at `location` on
    /// `layer` without contact there.
    pub fn get_trace_tail(
        &self,
        location: &Point,
        layer: i32,
        net_numbers: &[i32],
    ) -> Option<ItemId> {
        for id in self.overlapping_objects(&point_tile(location), layer) {
            let item = self.item_ref(id);
            let Some(trace) = item.as_trace() else {
                continue;
            };
            if !item.nets_equal_no(net_numbers) {
                continue;
            }
            if trace.first_corner() == *location && self.trace_start_contacts(id).is_empty() {
                return Some(id);
            }
            if trace.last_corner() == *location && self.trace_end_contacts(id).is_empty() {
                return Some(id);
            }
        }
        None
    }

    /// Removes trace `id` with its connection if it is part of a cycle, and
    /// the tails the removal leaves.
    pub fn remove_if_cycle(&mut self, id: ItemId) -> bool {
        if !self.is_on_board(id) {
            return false;
        }
        if !self.trace_is_cycle(id) {
            return false;
        }
        let (item, trace) = self.trace_data(id).expect("trace");
        let layer = trace.layer;
        let nets = item.net_numbers().to_vec();
        let end_corners = [trace.first_corner(), trace.last_corner()];
        let tail_before: Vec<bool> = end_corners
            .iter()
            .map(|corner| self.get_trace_tail(corner, layer, &nets).is_some())
            .collect();
        let connection = self.connection_items(id, StopConnectionOption::None);
        self.remove_items(connection);
        for (corner, had_tail) in end_corners.iter().zip(tail_before) {
            if !had_tail && let Some(tail) = self.get_trace_tail(corner, layer, &nets) {
                let tail_connection = self.connection_items(tail, StopConnectionOption::None);
                self.remove_items(tail_connection);
            }
        }
        true
    }

    /// True if `items` contain a trace with an open end, ignoring traces of
    /// exactly the nets `except_nets`.
    pub fn contains_trace_tails(&self, items: &BTreeSet<ItemId>, except_nets: &[i32]) -> bool {
        items.iter().any(|id| {
            let item = self.item_ref(*id);
            item.is_trace() && !item.nets_equal_no(except_nets) && self.is_tail(*id)
        })
    }

    /// Removes the dead-end traces and vias of net `net_number` (all nets
    /// if not positive). Returns true if something was removed.
    pub fn remove_trace_tails(
        &mut self,
        net_number: i32,
        stop_option: StopConnectionOption,
    ) -> bool {
        let mut stubs: BTreeSet<ItemId> = BTreeSet::new();
        for item in self.items() {
            if !item.is_routable() || item.net_count() != 1 {
                continue;
            }
            if net_number > 0 && item.net_number(0) != net_number {
                continue;
            }
            let id = item.id();
            if !self.is_tail(id) {
                continue;
            }
            if item.is_via() {
                if stop_option == StopConnectionOption::Via {
                    continue;
                }
                if stop_option == StopConnectionOption::FanoutVia && self.is_fanout_via(id, None) {
                    continue;
                }
            }
            stubs.insert(id);
        }
        let mut stub_connections: BTreeSet<ItemId> = BTreeSet::new();
        for id in stubs {
            if self.normal_contacts(id).len() == 1 {
                stub_connections.extend(self.connection_items(id, stop_option));
            } else {
                stub_connections.insert(id);
            }
        }
        if stub_connections.is_empty() {
            return false;
        }
        self.remove_items(stub_connections);
        self.combine_traces(net_number);
        true
    }
}

//! Java's leaf-reuse operations on the tree entries of a trace (decision D5).
//!
//! When traces are combined or a trace changes, Java keeps the leaves of the
//! unchanged shapes in the tree and only relabels them (`Leaf.object`,
//! `Leaf.shapeIndexInObject`); it removes and inserts leaves only for the
//! shapes that change. The tree's shape after the operation, and so the
//! later room completion and work counts, depend on that. These are the
//! ports of `ShapeSearchTree.mergeEntriesInFront`, `mergeEntriesAtEnd`
//! (`ShapeSearchTree.java:184-360`) and `changeEntries` (`:123-182`).
//!
//! The stored shape of a kept leaf moves with the leaf to its new index; it
//! is never recomputed (a recomputed shape of a reversed trace could
//! differ).

use freerouting_geometry::{Polyline, TileShape};

use super::{ItemEntries, ShapeContext, ShapeSearchTree, TreeObject};
use crate::board::items::{Item, ItemId};
use crate::datastructures::LeafId;
use crate::rules::ClearanceMatrix;

/// The traces of a combine, as the merge needs them.
#[derive(Clone, Copy, Debug)]
pub struct TraceMerge<'a> {
    /// The trace that is absorbed (Java `fromTrace`, the other trace).
    pub from: ItemId,
    /// The trace that remains (Java `toTrace`, `this`).
    pub to: ItemId,
    /// Java `changeOrder`: `from`'s lines run the other way.
    pub change_order: bool,
    /// The lines of the combined trace.
    pub joined: &'a Polyline,
    pub to_half_width: i32,
    pub to_clearance_class: i32,
    pub to_layer: i32,
    /// The link shapes are `offset_shapes(joined, .., from_entry_no, to_entry_no)`.
    pub from_entry_no: usize,
    pub to_entry_no: usize,
}

/// A trace change, as `changeEntries` needs it.
#[derive(Clone, Copy, Debug)]
pub struct TraceChange<'a> {
    pub id: ItemId,
    pub new_polyline: &'a Polyline,
    pub half_width: i32,
    pub clearance_class: i32,
    pub layer: i32,
    pub keep_at_start: usize,
    pub keep_at_end: usize,
}

impl ShapeSearchTree {
    /// The trace shape `index` of `polyline` with `half_width` for this
    /// tree (Java `ShapeSearchTree.offsetShape`; the 90 degree tree takes
    /// the bounding box of the segment, `ShapeSearchTree90Degree.java:507-511`).
    pub fn offset_shape(
        &self,
        polyline: &Polyline,
        half_width: i32,
        index: usize,
    ) -> Option<TileShape> {
        self.trace_offset_shape(polyline, half_width, index)
    }

    /// The trace shapes `from..to - 1` of `polyline` with `half_width` for
    /// this tree (Java `ShapeSearchTree.offsetShapes(polyline, halfWidth,
    /// fromNo, toNo)`, with the 90 degree override,
    /// `ShapeSearchTree90Degree.java:513-520`): shape `j` lies around line
    /// `j + 1`, and `to` is cut to the last line.
    pub fn offset_shapes(
        &self,
        polyline: &Polyline,
        half_width: i32,
        from: usize,
        to: usize,
    ) -> Vec<TileShape> {
        match self.kind {
            super::TreeKind::NinetyDegree => {
                let to = to.min(polyline.lines().len().saturating_sub(1));
                (from..to.saturating_sub(1))
                    .map(|j| TileShape::IntBox(polyline.offset_box(half_width, j)))
                    .collect()
            }
            _ => polyline.offset_shapes_range(half_width, from, to),
        }
    }

    /// Inserts a leaf for shape `index` of item `id`, whose stored shape is
    /// `shape` (Java `ShapeTree.insert(object, index)`).
    fn insert_item_leaf(
        &mut self,
        id: ItemId,
        index: usize,
        shape: Option<&TileShape>,
    ) -> Option<LeafId> {
        let bounds = shape?.bounding_shape(self.tree.bounding_directions())?;
        Some(self.tree.insert(TreeObject::Item(id), index, bounds))
    }

    fn remove_leaf_opt(&mut self, leaf: Option<LeafId>) {
        if let Some(leaf) = leaf {
            self.tree.remove_leaf(leaf);
        }
    }

    fn relabel(&mut self, leaf: Option<LeafId>, id: ItemId, index: usize) {
        if let Some(leaf) = leaf {
            self.tree.set_leaf(leaf, TreeObject::Item(id), index);
        }
    }

    /// Port of `ShapeSearchTree.mergeEntriesInFront`: `from` is joined in
    /// front of `to`, and the tree entries of both become `to`'s. Returns
    /// false, changing nothing, if one of the traces has no entries in this
    /// tree. `from` loses its entries without tree removals (Java's
    /// `clearSearchTreeEntries` after the merge).
    pub fn merge_entries_in_front(
        &mut self,
        merge: &TraceMerge<'_>,
        matrix: &ClearanceMatrix,
    ) -> bool {
        if !self.entries.contains_key(&merge.to) {
            return false;
        }
        let Some(from) = self.entries.remove(&merge.from) else {
            return false;
        };
        let to = self.entries.remove(&merge.to).expect("checked above");
        let from_minus1 = from.leaves.len().saturating_sub(1);
        // PARITY: from's joint leaf first (index 0 if the order changes,
        // else the last), then to's first leaf.
        let remove_no = if merge.change_order { 0 } else { from_minus1 };
        self.remove_leaf_opt(from.leaves.get(remove_no).copied().flatten());
        self.remove_leaf_opt(to.leaves.first().copied().flatten());
        let link = self.link_shapes(merge, matrix);
        let link_count = link.len();
        let new_count = from.leaves.len() + link_count + to.leaves.len() - 2;
        let mut result = ItemEntries {
            shapes: vec![None; new_count],
            leaves: vec![None; new_count],
        };
        // PARITY: the kept leaves are relabelled in place; from's follow
        // in reverse when the order changes.
        for i in 0..from_minus1 {
            let from_no = if merge.change_order {
                from_minus1 - i
            } else {
                i
            };
            result.shapes[i] = from.shapes[from_no].clone();
            result.leaves[i] = from.leaves[from_no];
            self.relabel(result.leaves[i], merge.to, i);
        }
        for i in 1..to.leaves.len() {
            let current = from_minus1 + link_count + i - 1;
            result.shapes[current] = to.shapes[i].clone();
            result.leaves[current] = to.leaves[i];
            self.relabel(result.leaves[current], merge.to, current);
        }
        // PARITY: the link leaves are inserted after every relabel, in
        // ascending index.
        for (i, shape) in link.into_iter().enumerate() {
            result.shapes[from_minus1 + i] = Some(shape);
        }
        for current in from_minus1..from_minus1 + link_count {
            result.leaves[current] =
                self.insert_item_leaf(merge.to, current, result.shapes[current].as_ref());
        }
        self.entries.insert(merge.to, result);
        true
    }

    /// Port of `ShapeSearchTree.mergeEntriesAtEnd`: `from` is joined at
    /// the end of `to`. Returns false, changing nothing, if one of the
    /// traces has no entries in this tree.
    pub fn merge_entries_at_end(
        &mut self,
        merge: &TraceMerge<'_>,
        matrix: &ClearanceMatrix,
    ) -> bool {
        if !self.entries.contains_key(&merge.to) {
            return false;
        }
        let Some(from) = self.entries.remove(&merge.from) else {
            return false;
        };
        let to = self.entries.remove(&merge.to).expect("checked above");
        let to_minus1 = to.leaves.len().saturating_sub(1);
        // PARITY: to's last leaf first, then from's joint leaf (the last if
        // the order changes, else index 0).
        self.remove_leaf_opt(to.leaves.get(to_minus1).copied().flatten());
        let remove_no = if merge.change_order {
            from.leaves.len().saturating_sub(1)
        } else {
            0
        };
        self.remove_leaf_opt(from.leaves.get(remove_no).copied().flatten());
        let link = self.link_shapes(merge, matrix);
        let link_count = link.len();
        let new_count = from.leaves.len() + link_count + to.leaves.len() - 2;
        let mut result = ItemEntries {
            shapes: vec![None; new_count],
            leaves: vec![None; new_count],
        };
        for i in 0..to_minus1 {
            result.shapes[i] = to.shapes[i].clone();
            result.leaves[i] = to.leaves[i];
        }
        for i in 1..from.leaves.len() {
            let current = to_minus1 + link_count + i - 1;
            let from_no = if merge.change_order {
                from.leaves.len() - i - 1
            } else {
                i
            };
            result.shapes[current] = from.shapes[from_no].clone();
            result.leaves[current] = from.leaves[from_no];
            self.relabel(result.leaves[current], merge.to, current);
        }
        for (i, shape) in link.into_iter().enumerate() {
            result.shapes[to_minus1 + i] = Some(shape);
        }
        for current in to_minus1..to_minus1 + link_count {
            result.leaves[current] =
                self.insert_item_leaf(merge.to, current, result.shapes[current].as_ref());
        }
        self.entries.insert(merge.to, result);
        true
    }

    fn link_shapes(&self, merge: &TraceMerge<'_>, matrix: &ClearanceMatrix) -> Vec<TileShape> {
        let half_width = merge.to_half_width
            + self.clearance_compensation_value(matrix, merge.to_clearance_class, merge.to_layer);
        self.offset_shapes(
            merge.joined,
            half_width,
            merge.from_entry_no,
            merge.to_entry_no,
        )
    }

    /// Port of `ShapeSearchTree.changeEntries`: keeps the first
    /// `keep_at_start` leaves, removes the old middle leaves in ascending
    /// index, relabels the last `keep_at_end` leaves to their new indexes,
    /// stores the new middle shapes and inserts their leaves in ascending
    /// index. Returns false, changing nothing, if the trace has no entries
    /// in this tree.
    pub fn change_entries(&mut self, change: &TraceChange<'_>, matrix: &ClearanceMatrix) -> bool {
        let Some(old) = self.entries.remove(&change.id) else {
            return false;
        };
        let half_width = change.half_width
            + self.clearance_compensation_value(matrix, change.clearance_class, change.layer);
        let new_lines = change.new_polyline.lines().len();
        let changed = self.offset_shapes(
            change.new_polyline,
            half_width,
            change.keep_at_start,
            (new_lines - 1).saturating_sub(change.keep_at_end),
        );
        let old_count = old.leaves.len();
        let (keep_start, keep_end) = (change.keep_at_start, change.keep_at_end);
        let new_count = changed.len() + keep_start + keep_end;
        let mut result = ItemEntries {
            shapes: vec![None; new_count],
            leaves: vec![None; new_count],
        };
        for i in 0..keep_start {
            result.shapes[i] = old.shapes[i].clone();
            result.leaves[i] = old.leaves[i];
        }
        for i in keep_start..old_count.saturating_sub(keep_end) {
            self.remove_leaf_opt(old.leaves[i]);
        }
        for i in 0..keep_end {
            let new_index = new_count - keep_end + i;
            let old_index = old_count - keep_end + i;
            result.shapes[new_index] = old.shapes[old_index].clone();
            result.leaves[new_index] = old.leaves[old_index];
            self.relabel(result.leaves[new_index], change.id, new_index);
        }
        for (i, shape) in changed.into_iter().enumerate() {
            result.shapes[keep_start + i] = Some(shape);
        }
        for i in keep_start..new_count - keep_end {
            result.leaves[i] = self.insert_item_leaf(change.id, i, result.shapes[i].as_ref());
        }
        self.entries.insert(change.id, result);
        true
    }

    /// Port of `ShapeSearchTree.reuseEntriesAfterCutout` (`:362-405`): a
    /// cutout split trace `from` into `start` and `end`. The start piece
    /// takes `from`'s first `start_lines - 3` leaves (relabelled) and gets
    /// one new leaf for its last shape; then the end piece gets one new leaf
    /// for its first shape and takes `from`'s last `end_lines - 3` leaves.
    /// The taken leaves leave `from`'s entries, so removing `from` afterwards
    /// (Java's caller `ShapeTraceEntries.fastCutoutTrace`) removes only the
    /// middle leaves, in index order. The pieces' stored shapes are
    /// calculated from their own polylines, as Java's `getTreeShape` does
    /// for a new item. Returns false, changing nothing, if `from` has no
    /// entries in this tree or a piece is not a trace.
    pub fn reuse_entries_after_cutout(
        &mut self,
        from: ItemId,
        start: &Item,
        end: &Item,
        ctx: &ShapeContext<'_>,
    ) -> bool {
        let (Some(start_trace), Some(end_trace)) = (start.as_trace(), end.as_trace()) else {
            return false;
        };
        let Some(mut old) = self.entries.remove(&from) else {
            return false;
        };
        let start_count = start_trace.polyline.lines().len().saturating_sub(2);
        let end_count = end_trace.polyline.lines().len().saturating_sub(2);
        let start_shapes = self.calculate_tree_shapes(start, ctx);
        let mut start_leaves = vec![None; start_count];
        // PARITY: relabel the start leaves, insert the start piece's last
        // leaf, insert the end piece's first leaf, relabel the end leaves.
        for (i, slot) in start_leaves
            .iter_mut()
            .enumerate()
            .take(start_count.saturating_sub(1))
        {
            let leaf = old.leaves.get_mut(i).and_then(Option::take);
            self.relabel(leaf, start.id(), i);
            *slot = leaf;
        }
        if start_count > 0 {
            let last = start_count - 1;
            start_leaves[last] = self.insert_item_leaf(
                start.id(),
                last,
                start_shapes.get(last).and_then(Option::as_ref),
            );
        }
        let end_shapes = self.calculate_tree_shapes(end, ctx);
        let mut end_leaves = vec![None; end_count];
        if end_count > 0 {
            end_leaves[0] =
                self.insert_item_leaf(end.id(), 0, end_shapes.first().and_then(Option::as_ref));
        }
        let old_count = old.leaves.len();
        for (i, slot) in end_leaves.iter_mut().enumerate().skip(1) {
            let from_index = (old_count + i).checked_sub(end_count);
            let leaf = from_index
                .and_then(|f| old.leaves.get_mut(f))
                .and_then(Option::take);
            self.relabel(leaf, end.id(), i);
            *slot = leaf;
        }
        self.entries.insert(
            start.id(),
            ItemEntries {
                shapes: start_shapes,
                leaves: start_leaves,
            },
        );
        self.entries.insert(
            end.id(),
            ItemEntries {
                shapes: end_shapes,
                leaves: end_leaves,
            },
        );
        self.entries.insert(from, old);
        true
    }

    /// Port of `ShapeSearchTree.changeItemShape` (`:967-994`): removes the
    /// leaf of shape `index` of item `id`, stores `shape` as that shape and
    /// inserts the leaf again; the other leaves stay. Returns false if the
    /// item has no such entry in this tree.
    pub fn change_item_shape(&mut self, id: ItemId, index: usize, shape: TileShape) -> bool {
        let Some(old_leaf) = self
            .entries
            .get(&id)
            .and_then(|e| e.leaves.get(index).copied())
        else {
            return false;
        };
        self.remove_leaf_opt(old_leaf);
        let leaf = self.insert_item_leaf(id, index, Some(&shape));
        let entries = self.entries.get_mut(&id).expect("checked above");
        entries.shapes[index] = Some(shape);
        entries.leaves[index] = leaf;
        true
    }
}

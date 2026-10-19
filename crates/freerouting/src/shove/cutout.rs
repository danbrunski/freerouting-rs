//! Cutting traces out of a shape (O03.3).
//!
//! Port of `ShapeTraceEntries.cutoutTrace`, `fastCutoutTrace` and
//! `cutoutTraces` (Java at `a773a70ec`). Unlike the checks this changes
//! items: the parts of a trace inside the shape (enlarged by the trace's
//! half width and clearance) are removed. Its first caller is the shove
//! insert (O04).
//!
//! The common case, one piece before and one after the shape, takes Java's
//! fast path: the two pieces go straight into the item list (no
//! `insertItem`, so no revision bump and no update of the trace half-width
//! bounds), the old trace's change hook fires first, and the pieces take
//! over the old trace's tree leaves at their ends (B02.6's
//! `reuse_entries_after_cutout`). Ids: start piece, then end piece. Java's
//! routing ledger never hears of the pieces; the port's ledger counts
//! exactly (B01.6), so the fast path notes both pieces as changed (B00
//! records that divergence). Every other cut removes the trace and inserts
//! each piece with `insert_trace_without_cleaning`.

use freerouting_geometry::{Polyline, TileShape, TileShapeOps};

use crate::board::BasicBoard;
use crate::board::items::{Item, ItemId, ItemKind, PolylineTrace};
use crate::board::search::ShapeContext;
use crate::board::structure::FixedState;

use super::trace_entries::{ShapeTraceEntries, trace_offset_shape};

/// What [`BasicBoard::cutout_trace`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CutoutResult {
    /// The trace is not on the board (Java logs and returns).
    NotOnBoard,
    /// Nothing was cut off (Java's `pieces[0] == traceLines`).
    Untouched,
    /// The fast path replaced the trace by these two pieces.
    Fast { start: ItemId, end: ItemId },
    /// The trace was removed and these pieces inserted (the ids of the
    /// pieces that were inserted, in piece order).
    Slow(Vec<ItemId>),
}

impl BasicBoard {
    /// Java `ShapeTraceEntries.cutoutTrace(trace, shape,
    /// clearanceClassIndex)`: cuts the part of trace `id` that comes closer
    /// to `shape` than its half width plus the clearance to class
    /// `clearance_class` out of the board.
    pub fn cutout_trace(
        &mut self,
        id: ItemId,
        shape: &TileShape,
        clearance_class: i32,
    ) -> CutoutResult {
        let Some(item) = self.item(id).filter(|i| i.is_on_the_board()) else {
            // Java: "ShapeTraceEntries.cutout_trace : trace is deleted".
            return CutoutResult::NotOnBoard;
        };
        let Some(trace) = item.as_trace() else {
            return CutoutResult::NotOnBoard;
        };
        let offset_shape = trace_offset_shape(self, shape, item, clearance_class, trace.layer);
        let Some(pieces) = offset_shape.cutout_polyline_outcome(&trace.polyline) else {
            return CutoutResult::Untouched;
        };
        let (layer, half_width) = (trace.layer, trace.half_width);
        let nets = item.net_numbers().to_vec();
        let class = item.clearance_class();
        if pieces.len() == 2
            && offset_shape.is_outside(&pieces[0].first_corner())
            && offset_shape.is_outside(&pieces[1].last_corner())
        {
            let [start, end]: [Polyline; 2] = pieces.try_into().expect("two pieces");
            let (start, end) =
                self.fast_cutout_trace(id, start, end, layer, half_width, &nets, class);
            return CutoutResult::Fast { start, end };
        }
        self.remove_item(id);
        let inserted = pieces
            .into_iter()
            .filter_map(|piece| {
                self.insert_trace_without_cleaning(
                    piece,
                    layer,
                    half_width,
                    &nets,
                    class,
                    FixedState::Unfixed,
                )
            })
            .collect();
        CutoutResult::Slow(inserted)
    }

    /// Java `fastCutoutTrace`: the standard cutout with one piece before and
    /// one after the shape.
    #[allow(clippy::too_many_arguments)]
    fn fast_cutout_trace(
        &mut self,
        id: ItemId,
        start_piece: Polyline,
        end_piece: Polyline,
        layer: i32,
        half_width: i32,
        nets: &[i32],
        class: i32,
    ) -> (ItemId, ItemId) {
        // Java's additionalUpdateAfterChange(trace), then saveForUndo.
        self.note_autoroute_cutout(id);
        self.items.save_for_undo(&id);
        let piece_item = |board: &mut BasicBoard, polyline: Polyline| {
            let piece_id = board.burn_item_id();
            let mut piece = Item::new(
                piece_id,
                nets.to_vec(),
                class,
                0,
                FixedState::Unfixed,
                ItemKind::Trace(PolylineTrace {
                    polyline,
                    layer,
                    half_width,
                }),
            );
            piece.on_the_board = true;
            board.items.insert(piece_id, piece.clone());
            piece
        };
        let start = piece_item(self, start_piece);
        let end = piece_item(self, end_piece);
        let ctx = ShapeContext {
            rules: &self.rules,
            padstacks: &self.library.padstacks,
            communication: &self.communication,
            layer_count: self.layer_structure.count(),
            split_quirks: self.java_quirks.split(),
        };
        let missing = self
            .search_trees
            .reuse_entries_after_cutout(id, &start, &end, &ctx);
        // Java has no case where the old trace lacks entries in a tree (it
        // would throw); the port inserts the pieces there instead.
        self.reinsert_in_trees(start.id(), &missing);
        self.reinsert_in_trees(end.id(), &missing);
        self.remove_item(id);
        // PARITY: Java's ledger never hears of the pieces; the port's counts
        // exactly (B01.6).
        self.note_changed(start.id());
        self.note_changed(end.id());
        (start.id(), end.id())
    }
}

impl ShapeTraceEntries {
    /// Java `cutoutTraces(itemList)`: cuts every trace of `items` without
    /// an own net out of the stored shape, in the given order.
    pub fn cutout_traces(
        &self,
        board: &mut BasicBoard,
        items: impl IntoIterator<Item = ItemId>,
    ) -> Vec<CutoutResult> {
        let mut results = Vec::new();
        for id in items {
            let is_foreign_trace = board
                .item(id)
                .is_some_and(|item| item.is_trace() && !item.shares_net_no(self.own_net_nos()));
            if is_foreign_trace {
                results.push(board.cutout_trace(id, self.shape(), self.clearance_class()));
            }
        }
        results
    }
}

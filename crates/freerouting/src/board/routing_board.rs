//! The board the router works on.

use std::ops::{Deref, DerefMut};

use freerouting_geometry::{FloatPoint, IntBox, PolylineShape};

use crate::board::basic_board::BasicBoard;
use crate::board::items::ItemKind;
use crate::board::state::{ChangedArea, Communication};
use crate::board::structure::LayerStructure;
use crate::rules::BoardRules;

/// A [`BasicBoard`] with the state the router needs.
///
/// Port of `board.facade.RoutingBoard` without the routing algorithms
/// (autorouting, shoving, pull-tight optimization, forced traces and vias
/// and the interactive operations), which belong to the router stage.
/// It dereferences to the [`BasicBoard`].
#[derive(Clone, Debug)]
pub struct RoutingBoard {
    board: BasicBoard,
}

impl RoutingBoard {
    pub fn new(
        bounding_box: IntBox,
        layer_structure: LayerStructure,
        outline_shapes: Vec<PolylineShape>,
        outline_clearance_class: i32,
        rules: BoardRules,
        communication: Communication,
    ) -> Self {
        RoutingBoard {
            board: BasicBoard::new(
                bounding_box,
                layer_structure,
                outline_shapes,
                outline_clearance_class,
                rules,
                communication,
            ),
        }
    }

    /// The underlying board.
    pub fn basic(&self) -> &BasicBoard {
        &self.board
    }

    /// Starts recording the area changed by the following edits.
    pub fn start_marking_changed_area(&mut self) {
        if self.board.changed_area.is_none() {
            self.board.changed_area = Some(ChangedArea::new(self.board.layer_count()));
        }
    }

    /// Enlarges the changed area on `layer` to contain `point`, while
    /// marking.
    pub fn join_changed_area(&mut self, point: FloatPoint, layer: i32) {
        self.board.join_changed_area(point, layer);
    }

    /// Marks the whole board as changed.
    pub fn mark_all_changed_area(&mut self) {
        self.start_marking_changed_area();
        let b = self.board.bounding_box();
        let corners = [
            FloatPoint::new(f64::from(b.ll.x), f64::from(b.ll.y)),
            FloatPoint::new(f64::from(b.ur.x), f64::from(b.ll.y)),
            FloatPoint::new(f64::from(b.ur.x), f64::from(b.ur.y)),
            FloatPoint::new(f64::from(b.ll.x), f64::from(b.ur.y)),
        ];
        for layer in 0..self.board.layer_count() as i32 {
            for corner in corners {
                self.join_changed_area(corner, layer);
            }
        }
    }

    /// The changed area, while marking.
    pub fn changed_area(&self) -> Option<&ChangedArea> {
        self.board.changed_area.as_ref()
    }

    /// Stops marking the changed area.
    // TODO(router): Java `optChangedArea` pull-tightens the traces in the
    // changed area before dropping it.
    pub fn clear_changed_area(&mut self) {
        self.board.changed_area = None;
    }

    /// Sets whether the conduction areas on signal layers are obstacles for
    /// other nets, and whether the router ignores conduction areas.
    pub fn change_plane_as_obstacle(&mut self, value: bool) {
        let target_ignore = !value;
        let mut something_changed = false;
        for id in self.board.item_ids() {
            let item = self.board.item_ref(id);
            let Some(area) = item.as_conduction_area() else {
                continue;
            };
            let on_signal_layer = self.board.layer_structure.layers[area.layer() as usize].is_signal;
            if on_signal_layer && area.conduction_is_obstacle() != value {
                self.board.set_conduction_is_obstacle(id, value);
                something_changed = true;
            }
        }
        if self.board.rules.ignore_conduction() != target_ignore {
            self.board.rules.set_ignore_conduction(target_ignore);
            something_changed = true;
        }
        if something_changed {
            self.board.reinsert_tree_items();
        }
    }

    /// True if any item is a trace (used by tests and callers that only
    /// need to know whether the board is routed at all).
    pub fn has_traces(&self) -> bool {
        self.board
            .items()
            .any(|item| matches!(item.kind, ItemKind::Trace(_)))
    }
}

impl Deref for RoutingBoard {
    type Target = BasicBoard;

    fn deref(&self) -> &BasicBoard {
        &self.board
    }
}

impl DerefMut for RoutingBoard {
    fn deref_mut(&mut self) -> &mut BasicBoard {
        &mut self.board
    }
}

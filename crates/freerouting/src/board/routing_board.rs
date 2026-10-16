//! The board the router works on.

use std::ops::{Deref, DerefMut};

use freerouting_geometry::{FloatPoint, IntBox, PolylineShape};

use crate::autoroute::failure_log::FailureLog;
use crate::board::basic_board::BasicBoard;
use crate::board::items::{ItemId, ItemKind};
use crate::board::state::{ChangedArea, Communication};
use crate::board::structure::{FixedState, LayerStructure};
use crate::rules::BoardRules;

/// A [`BasicBoard`] with the state the router needs.
///
/// Port of `board.facade.RoutingBoard` without the routing algorithms
/// (autorouting, shoving, pull-tight optimization, forced traces and vias
/// and the interactive operations), which belong to the router stage.
/// It dereferences to the [`BasicBoard`].
///
/// `Clone` is not a Java copy (it keeps the search trees and the transient
/// state); routing, history and optimizer code uses
/// [`deep_copy`](Self::deep_copy) or a
/// [`BoardSnapshot`](crate::board::BoardSnapshot).
#[derive(Clone, Debug)]
pub struct RoutingBoard {
    board: BasicBoard,
    /// The obstacle responsible for the last shove to fail (Java
    /// `shoveFailingObstacle`, transient).
    shove_failing_obstacle: Option<ItemId>,
    /// The layer of the last failed shove, -1 for none (Java
    /// `shoveFailingLayer`, transient).
    shove_failing_layer: i32,
    /// The routing failures per item (Java `failureLog`, serialized with
    /// the board, A08.2).
    failure_log: FailureLog,
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
        RoutingBoard::from_basic(BasicBoard::new(
            bounding_box,
            layer_structure,
            outline_shapes,
            outline_clearance_class,
            rules,
            communication,
        ))
    }

    /// A routing board around `board`, with no transient routing state.
    pub(crate) fn from_basic(board: BasicBoard) -> Self {
        RoutingBoard {
            board,
            shove_failing_obstacle: None,
            shove_failing_layer: -1,
            failure_log: FailureLog::new(),
        }
    }

    /// The obstacle responsible for the last shove to fail (Java
    /// `getShoveFailingObstacle`).
    pub fn shove_failing_obstacle(&self) -> Option<ItemId> {
        self.shove_failing_obstacle
    }

    /// Java `setShoveFailingObstacle`.
    pub fn set_shove_failing_obstacle(&mut self, item: Option<ItemId>) {
        self.shove_failing_obstacle = item;
    }

    /// The layer of the last failed shove, -1 for none (Java
    /// `getShoveFailingLayer`).
    pub fn shove_failing_layer(&self) -> i32 {
        self.shove_failing_layer
    }

    /// Java `setShoveFailingLayer`.
    pub fn set_shove_failing_layer(&mut self, layer: i32) {
        self.shove_failing_layer = layer;
    }

    /// Forgets the failed shove (Java `clearShoveFailingObstacle`).
    pub fn clear_shove_failing_obstacle(&mut self) {
        self.shove_failing_obstacle = None;
        self.shove_failing_layer = -1;
    }

    /// Drops the routing state that undo does not restore (Java
    /// `clearTransientAutorouteState`): the changed area and the
    /// shove-failing state. Java also finishes the autoroute engine and
    /// clears the items' autoroute info; in the port both live in the
    /// autoroute session, outside the board.
    pub fn clear_transient_autoroute_state(&mut self) {
        self.board.changed_area = None;
        self.clear_shove_failing_obstacle();
    }

    /// The underlying board.
    pub fn basic(&self) -> &BasicBoard {
        &self.board
    }

    /// The routing failures per item (Java `failureLog`).
    pub fn failure_log(&self) -> &FailureLog {
        &self.failure_log
    }

    /// The routing failures per item, for recording (Java `failureLog`).
    pub fn failure_log_mut(&mut self) -> &mut FailureLog {
        &mut self.failure_log
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

    /// Stops marking the changed area and drops it without optimizing.
    /// Java's path, which pull-tightens the traces in the area first, is
    /// [`opt_changed_area`](Self::opt_changed_area) (`crate::optimize`).
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
            let on_signal_layer =
                self.board.layer_structure.layers[area.layer() as usize].is_signal;
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

    /// Reduces the nets of traces and vias with more than one net to the
    /// nets of their contacts (traces at tie pins). Returns true if a net
    /// was removed.
    ///
    /// Port of `RoutingBoard.reduceNetsOfRouteItems`.
    pub fn reduce_nets_of_route_items(&mut self) -> bool {
        let mut result = false;
        loop {
            let mut something_changed = false;
            for id in self.board.item_ids() {
                let item = self.board.item_ref(id);
                if item.net_numbers().len() <= 1 || item.fixed_state() == FixedState::SystemFixed {
                    continue;
                }
                let nets = item.net_numbers().to_vec();
                match &item.kind {
                    ItemKind::Via(_) => {
                        let contacts = self.board.normal_contacts(id);
                        for net in nets {
                            if contacts
                                .iter()
                                .any(|c| !self.board.item_ref(*c).contains_net(net))
                            {
                                self.board.remove_from_net(id, net);
                                something_changed = true;
                                break;
                            }
                        }
                    }
                    ItemKind::Trace(_) => {
                        for contacts in [
                            self.board.trace_start_contacts(id),
                            self.board.trace_end_contacts(id),
                        ] {
                            // Java iterates over the nets the trace had when
                            // this end was started.
                            let nets = self.board.item_ref(id).net_numbers().to_vec();
                            for net in nets {
                                let mut pin_found = false;
                                let mut remove = false;
                                for c in &contacts {
                                    let contact = self.board.item_ref(*c);
                                    if contact.is_pin() {
                                        pin_found = true;
                                        if !contact.contains_net(net) {
                                            remove = true;
                                            break;
                                        }
                                    }
                                }
                                // At tie pins traces may have different nets.
                                if !pin_found {
                                    remove = contacts.iter().any(|c| {
                                        let contact = self.board.item_ref(*c);
                                        !contact.is_pin() && !contact.contains_net(net)
                                    });
                                }
                                if remove {
                                    self.board.remove_from_net(id, net);
                                    something_changed = true;
                                }
                            }
                            if something_changed {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
                if something_changed {
                    break;
                }
            }
            if !something_changed {
                break;
            }
            result = true;
        }
        result
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

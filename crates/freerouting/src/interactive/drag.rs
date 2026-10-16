//! Dragging items and making space (I02.2, I02.3).
//!
//! Port of the board logic of Java's drag mode at `a773a70ec`:
//! `DragState.getInstance`, `findItemToMove` and `selectItemToMove` pick
//! what a press starts ([`DragSession::start`]); `DragItemState.moveTo` and
//! `DragState.mouseDragged` drag a drill item or a component with
//! [`MoveComponent`] ([`DragSession::drag_to`]); `MakeSpaceState` pushes
//! copper away with a thin trace of the hidden net, which the release
//! removes again ([`DragSession::release`]).
//!
//! The pick tries the current layer, then (with "select on all visible
//! layers") layers 0 … n−1 except the current and the invisible ones, with
//! the drag settings' item filter. In each pick traces are skipped, component
//! items only when component dragging is on, and the first drill item wins
//! (else the last other item). Nothing found at all starts make-space; only
//! traces (or skipped items) found starts nothing.
//!
//! An item drag tries `MoveComponent(item, delta, 99, 5)` twice; before the
//! second try it shortens the move to twice the board's smallest trace half
//! width (or `max(1, min_drill_width − 1)` when that is smaller and
//! positive). The first successful step takes a snapshot. A failed insert
//! ends the drag and undoes to that snapshot, reporting the changed nets.
//!
//! What stays in the GUI: the ratsnest, the repaint boxes, the observers,
//! the status texts and moving the pointer (a make-space step returns the
//! point in [`DragStep::MadeSpace`]).

use std::collections::BTreeSet;

use freerouting_geometry::{FloatPoint, IntPoint, Point, Vector};

use super::Status;
use super::move_component::{MoveBudget, MoveComponent};
use super::route::{RouteBudget, RouteSession, RouteStep};
use super::settings::EditSettings;
use crate::board::RoutingBoard;
use crate::board::items::ItemId;
use crate::board::selection_filter::ItemFilter;
use crate::board::structure::AngleRestriction;
use crate::rules::Nets;

/// Java `DragItemState`'s shove depths for `MoveComponent`.
pub const DRAG_MAX_RECURSION_DEPTH: i32 = 99;
/// Java `DragItemState`'s via shove depth for `MoveComponent`.
pub const DRAG_MAX_VIA_RECURSION_DEPTH: i32 = 5;

/// The drag settings beside [`EditSettings`] (Java `WorkspaceSettings`
/// `dragComponentsEnabled` and `itemSelectionFilter`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DragSettings {
    /// Drag whole components by their items (Java default on).
    pub drag_components_enabled: bool,
    /// The items a press may pick (Java default: all but keepouts,
    /// conduction areas and the board outline).
    pub item_filter: ItemFilter,
}

impl Default for DragSettings {
    fn default() -> Self {
        DragSettings {
            drag_components_enabled: true,
            item_filter: ItemFilter::default(),
        }
    }
}

/// The work limits of a drag: the component moves' and the make-space
/// route's.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct DragBudget {
    pub moves: MoveBudget,
    pub route: RouteBudget,
}

impl DragBudget {
    /// Java's limits at the pin.
    pub const JAVA: DragBudget = DragBudget {
        moves: MoveBudget::JAVA,
        route: RouteBudget::JAVA,
    };
    /// No limits.
    pub const UNLIMITED: DragBudget = DragBudget {
        moves: MoveBudget::UNLIMITED,
        route: RouteBudget::UNLIMITED,
    };
}

/// An item drag in progress (Java `DragItemState`).
#[derive(Clone, Debug)]
pub struct ItemDrag {
    item: ItemId,
    previous_location: FloatPoint,
    something_dragged: bool,
}

impl ItemDrag {
    /// The dragged item.
    pub fn item(&self) -> ItemId {
        self.item
    }

    /// Where the drag has got to (Java `previousLocation`).
    pub fn previous_location(&self) -> FloatPoint {
        self.previous_location
    }
}

/// A make-space drag in progress (Java `MakeSpaceState`).
#[derive(Clone, Debug)]
pub struct MakeSpaceDrag {
    route: Box<RouteSession>,
    something_dragged: bool,
}

impl MakeSpaceDrag {
    /// The route of the hidden net.
    pub fn route(&self) -> &RouteSession {
        &self.route
    }
}

/// A drag in progress.
#[derive(Clone, Debug)]
pub enum DragSession {
    Item(ItemDrag),
    MakeSpace(MakeSpaceDrag),
}

/// What one [`DragSession::drag_to`] did.
#[derive(Clone, Debug, PartialEq)]
pub enum DragStep {
    /// The rounded (and angle-projected) pointer is where the drag is.
    Unchanged,
    /// The item is user fixed (Java's "Please unfix item before
    /// dragging"); nothing moved.
    Fixed,
    /// Both checks failed; nothing moved.
    Blocked,
    /// The group moved by `vector` (shortened if the first check failed).
    Moved { vector: Vector },
    /// The insert of a move by `vector` failed: the board was restored to
    /// the drag's snapshot and the drag is over. `changed_nets` are the nets
    /// of the undone items.
    Failed {
        vector: Vector,
        changed_nets: BTreeSet<i32>,
    },
    /// A make-space step; `warp_pointer` is the route end when the board has
    /// no angle restriction and the end differs from the rounded pointer.
    MadeSpace {
        step: RouteStep,
        warp_pointer: Option<FloatPoint>,
    },
}

impl DragStep {
    /// True if the drag is over after this step.
    pub fn ends_drag(&self) -> bool {
        matches!(self, DragStep::Failed { .. })
    }
}

/// What [`DragSession::release`] did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DragRelease {
    /// An item drag ended; `changed_nets` are the nets whose incompletes
    /// need an update (empty if nothing moved: the whole ratsnest is shown
    /// again).
    Item { changed_nets: BTreeSet<i32> },
    /// Make-space ended; `removed` items of the hidden net were removed.
    MakeSpace { removed: usize },
}

/// Java `DragState.findItemToMove`: the item a press at `at` drags, and
/// whether anything at all was found.
pub fn find_item_to_move(
    board: &RoutingBoard,
    settings: &EditSettings,
    drag: &DragSettings,
    at: FloatPoint,
) -> (Option<ItemId>, bool) {
    let mut try_count = 1;
    if settings.select_on_all_visible_layers {
        try_count += board.layer_count() as i32;
    }
    let current_layer = settings.layer;
    let mut pick_layer = current_layer;
    let mut item_found = false;
    let location = Point::Int(at.round());
    for i in 0..try_count {
        if i == 0 || pick_layer != current_layer && settings.visible_layers.is_visible(pick_layer) {
            let found = board.pick_items_filtered(&location, pick_layer, Some(&drag.item_filter));
            let (item, found_any) = select_item_to_move(board, drag, &found);
            item_found |= found_any;
            if item.is_some() {
                return (item, item_found);
            }
        }
        // nothing found on the settings layer; try all visible layers
        pick_layer = i;
    }
    (None, item_found)
}

/// Java `DragState.selectItemToMove`.
fn select_item_to_move(
    board: &RoutingBoard,
    drag: &DragSettings,
    found: &BTreeSet<ItemId>,
) -> (Option<ItemId>, bool) {
    let mut item_to_move = None;
    let mut item_found = false;
    for &id in found {
        let Some(item) = board.item(id) else {
            continue;
        };
        item_found = true;
        if item.is_trace() {
            continue; // traces are not moved
        }
        if !drag.drag_components_enabled && item.component_id() != 0 {
            continue;
        }
        item_to_move = Some(id);
        if item.is_drill_item() {
            break; // drill items are preferred
        }
    }
    (item_to_move, item_found)
}

impl DragSession {
    /// Java `DragState.getInstance`: an item drag if a press at `at` finds
    /// an item to move, make-space if it finds nothing, `None` if it finds
    /// only items it may not drag.
    pub fn start(
        board: &RoutingBoard,
        settings: &EditSettings,
        drag: &DragSettings,
        at: FloatPoint,
    ) -> Option<DragSession> {
        match find_item_to_move(board, settings, drag, at) {
            (Some(item), _) => Some(DragSession::Item(ItemDrag {
                item,
                previous_location: at,
                something_dragged: false,
            })),
            (None, false) => Some(DragSession::MakeSpace(MakeSpaceDrag {
                route: Box::new(RouteSession::make_space(board, settings, at)),
                something_dragged: false,
            })),
            (None, true) => None,
        }
    }

    /// True for make-space.
    pub fn is_make_space(&self) -> bool {
        matches!(self, DragSession::MakeSpace(_))
    }

    /// True once a step changed the board (and took the snapshot).
    pub fn something_dragged(&self) -> bool {
        match self {
            DragSession::Item(d) => d.something_dragged,
            DragSession::MakeSpace(d) => d.something_dragged,
        }
    }

    /// Java's default status of the state: `dragging_item` for an item drag
    /// (make-space has none).
    pub fn status(&self) -> Option<Status> {
        match self {
            DragSession::Item(_) => Some(Status::new("dragging_item", [])),
            DragSession::MakeSpace(_) => None,
        }
    }

    /// Java `DragState.mouseDragged(to)`: drags on to `to`.
    pub fn drag_to(
        &mut self,
        board: &mut RoutingBoard,
        settings: &EditSettings,
        to: FloatPoint,
        budget: DragBudget,
    ) -> DragStep {
        match self {
            DragSession::Item(drag) => drag.move_to(board, settings, to, budget.moves),
            DragSession::MakeSpace(drag) => drag.move_to(board, to, budget.route),
        }
    }

    /// Java `buttonReleased`: ends the drag.
    pub fn release(self, board: &mut RoutingBoard) -> DragRelease {
        match self {
            DragSession::Item(drag) => DragRelease::Item {
                changed_nets: drag.changed_nets(board),
            },
            DragSession::MakeSpace(_) => {
                let hidden = board.connectable_items(Nets::HIDDEN_NET_NUMBER);
                let removed = hidden.len();
                board.remove_items(hidden);
                DragRelease::MakeSpace { removed }
            }
        }
    }
}

impl ItemDrag {
    /// Java `DragItemState.moveTo` with `mouseDragged`'s undo on failure.
    fn move_to(
        &mut self,
        board: &mut RoutingBoard,
        settings: &EditSettings,
        to: FloatPoint,
        budget: MoveBudget,
    ) -> DragStep {
        let mut rounded = to.round();
        let from = self.previous_location.round();
        match board.rules.trace_angle_restriction() {
            AngleRestriction::NinetyDegree => rounded = rounded.orthogonal_projection(from),
            AngleRestriction::FortyfiveDegree => {
                rounded = rounded.fortyfive_degree_projection(from);
            }
            AngleRestriction::None => {}
        }
        if rounded == from {
            return DragStep::Unchanged;
        }
        if board.item(self.item).is_some_and(|i| i.is_user_fixed()) {
            return DragStep::Fixed;
        }
        let Some(moving) = self.checked_move(board, rounded, from, budget) else {
            return DragStep::Blocked;
        };
        if !self.something_dragged {
            // make the situation restorable by undo
            board.generate_snapshot();
            self.something_dragged = true;
        }
        let vector = moving.vector().clone();
        if !moving.insert(
            board,
            settings.trace_tidy_width,
            settings.pull_tight_accuracy,
            budget,
        ) {
            // an insert error occurred: end the drag and undo
            let mut changed_nets = BTreeSet::new();
            board.undo(Some(&mut changed_nets));
            return DragStep::Failed {
                vector,
                changed_nets,
            };
        }
        self.previous_location = self.previous_location.add(vector.to_float());
        DragStep::Moved { vector }
    }

    /// The two tries of `moveTo`: the first `MoveComponent` whose check
    /// passes, or `None`.
    fn checked_move(
        &self,
        board: &mut RoutingBoard,
        rounded: IntPoint,
        from: IntPoint,
        budget: MoveBudget,
    ) -> Option<MoveComponent> {
        let mut rel_coor = Vector::Int(rounded.difference_by_int(from));
        let length = rel_coor.length_approx();
        for i in 0..2 {
            let mut moving = MoveComponent::new(
                board,
                self.item,
                rel_coor.clone(),
                DRAG_MAX_RECURSION_DEPTH,
                DRAG_MAX_VIA_RECURSION_DEPTH,
            );
            if moving.check(board, budget) {
                return Some(moving);
            }
            if i == 0 {
                // reduce the shove distance to make the check shove work
                // properly if more than one trace has to be shoved
                let mut sample_width = 2.0 * f64::from(board.min_trace_half_width());
                let min_drill_width = moving.min_drill_item_width_with_traces(board);
                if min_drill_width > 0.0 && sample_width >= min_drill_width {
                    sample_width = (min_drill_width - 1.0).max(1.0);
                }
                if length > sample_width {
                    rel_coor = rel_coor.change_length_approx(sample_width);
                }
            }
        }
        None
    }

    /// The nets `buttonReleased` updates the incompletes of.
    fn changed_nets(&self, board: &RoutingBoard) -> BTreeSet<i32> {
        let mut nets = BTreeSet::new();
        if !self.something_dragged {
            return nets;
        }
        let Some(item) = board.item(self.item) else {
            return nets;
        };
        if item.component_id() == 0 {
            nets.extend(item.net_numbers().iter().copied());
        } else {
            for id in board.component_items(item.component_id()) {
                nets.extend(board.item_ref(id).net_numbers().iter().copied());
            }
        }
        nets
    }
}

impl MakeSpaceDrag {
    /// Java `MakeSpaceState.moveTo`.
    fn move_to(
        &mut self,
        board: &mut RoutingBoard,
        to: FloatPoint,
        budget: RouteBudget,
    ) -> DragStep {
        if !self.something_dragged {
            // make the situation restorable by undo
            board.generate_snapshot();
            self.something_dragged = true;
        }
        let step = self.route.next_corner(board, to, budget);
        let route_end = self.route.last_corner();
        let warp_pointer = (board.rules.trace_angle_restriction() == AngleRestriction::None
            && route_end != to.round())
        .then(|| route_end.to_float());
        DragStep::MadeSpace { step, warp_pointer }
    }
}

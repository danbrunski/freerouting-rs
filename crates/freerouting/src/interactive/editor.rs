//! The editor (I03.1): one entry point, [`Editor::apply`], for every
//! interactive edit. Port of the dispatch of Java's
//! `InteractiveStateController` and the engine semantics of `MenuState`,
//! `RouteMenuState`, `DragMenuState`, `InspectMenuState`,
//! `InspectedItemState`, `InspectItemsInRegionState`, the route, drag, pin
//! swap and construction states and `GuiBoardHistoryController`
//! (`gui/interactive/*.java` at `a773a70ec`).
//!
//! Java's state classes collapse into a mode ([`ModeKind`]) and match arms
//! over [`Command`]. The rules (Java's, unless noted):
//! - In a menu mode, `Click` starts the tool's session: a route
//!   (`RouteMenuState`), a selection (`InspectMenuState.selectItems`);
//!   `Press` starts a drag or make-space (`DragMenuState`). `DragTo` in the
//!   select menu or with a selection starts a region selection whose first
//!   corner is that point (`mouseDragged`); `Release` or `Complete` ends it
//!   at the last pointer position.
//! - `Complete` and `Cancel` go to the active session; in a menu mode they
//!   do nothing. A route's `Cancel` removes its tail (`RouteState.cancel`);
//!   a drag's `Complete` releases it (`DragState.complete`), its `Cancel`
//!   only ends it (Java's `InteractiveState.cancel`, so a make-space route
//!   stays on the hidden net as in Java).
//! - `SetLayer` and the layer keys (`NextSignalLayer`, `PreviousSignalLayer`,
//!   `SignalLayer(digit)`) change the route's layer with a via while
//!   routing (`RouteState.changeLayerAction`), else the current layer.
//! - `Undo` and `Redo` act only in a menu mode (`canChangeHistory`); with a
//!   selection or a session they are ignored without a message.
//! - A route, drag or pin swap starts only from a menu mode (Java's menu
//!   keys and popups); a region selection or a construction returns to the
//!   mode it started from, so a region adds to a selection.
//! - `SetTool` replaces the mode as Java's `setRouteMenuState` and its
//!   siblings do: a session in progress is dropped as it is and the
//!   selection is cleared.
//!
//! After every command the board's change journal (B01.7) is drained into
//! [`Response::delta`], so the GUI redraws what changed and refreshes the
//! ratsnest of the changed nets; undo and redo report Java's changed nets
//! there too.

use std::collections::BTreeSet;

use freerouting_geometry::{FloatPoint, IntBox, IntPoint};

use super::Status;
use super::construct::{
    ConstructClick, Construction, KeepoutKind, KeepoutShape, add_hole, insert_keepout,
};
use super::drag::{DragBudget, DragSession, DragSettings, DragStep};
use super::edit_ops::{delete_selection, fix_selection};
use super::history;
use super::overlay::RouteOverlay;
use super::pin_swap::PinSwap;
use super::route::{RouteBudget, RouteInput, RouteSession};
use super::select::{self, Extent};
use super::settings::EditSettings;
use crate::board::items::ItemId;
use crate::board::{BoardChanges, ItemFilter, RoutingBoard};

/// The tool of a menu mode (Java's `RouteMenuState`, `DragMenuState`,
/// `InspectMenuState`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    Route,
    Drag,
    Select,
}

/// The editor's mode (Java's current state class).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModeKind {
    Menu(Tool),
    Routing {
        stitch: bool,
    },
    Dragging,
    MakingSpace,
    /// Items are selected (`InspectedItemState`).
    Selecting,
    /// A region is being dragged open (`InspectItemsInRegionState`).
    SelectingRegion,
    PinSwap,
    /// A keepout or hole is being drawn.
    Constructing,
}

/// Editor input, in board coordinates. The GUI maps keys and the mouse to
/// these.
#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    Press(FloatPoint),
    Move(FloatPoint),
    DragTo(FloatPoint),
    Release,
    Click(FloatPoint),
    /// Java's Enter and Space.
    Complete,
    /// Java's Esc.
    Cancel,
    SetTool(Tool),
    /// Java's `changeLayerAction`.
    SetLayer(i32),
    /// Java's `+`: the next signal layer.
    NextSignalLayer,
    /// Java's `-`: the previous signal layer.
    PreviousSignalLayer,
    /// Java's digit keys: the `digit`-th signal layer (1-based, 0 and
    /// out-of-range digits clamp).
    SignalLayer(u32),
    UpdateSettings(EditSettings),
    UpdateOptions(EditorOptions),
    /// Java's `p` in a menu mode.
    TogglePush,
    Undo,
    Redo,
    /// Java's `s` in dynamic routing.
    Snapshot,
    /// Java's `t`.
    StartRoute {
        at: FloatPoint,
    },
    /// Java's `w`.
    StartPinSwap {
        at: FloatPoint,
    },
    /// Java's `i` in a menu mode.
    SelectAt(FloatPoint),
    /// A region selection in one step.
    SelectRegion(IntBox),
    ToggleSelect(FloatPoint),
    ExtendSelection(Extent),
    FilterSelection,
    /// Java's `startTile`, `startPolygon`, `startCircle` (popup menu).
    StartKeepout {
        kind: KeepoutKind,
        at: FloatPoint,
    },
    /// Java's `startHole`.
    StartHole {
        at: FloatPoint,
    },
    /// A whole keepout construction in one step.
    InsertKeepout {
        shape: KeepoutShape,
        layer: i32,
    },
    /// A whole hole construction in one step: `corners` are the clicks
    /// (the first one starts it).
    AddHole {
        area: ItemId,
        corners: Vec<FloatPoint>,
    },
    /// By decision 1 (I03.6).
    DeleteSelection,
    /// By decision 1 (I03.6): fix (`true`) or unfix the selection.
    FixSelection(bool),
}

/// Editor input that Java keeps in the workspace settings and the GUI
/// beside [`EditSettings`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditorOptions {
    pub drag: DragSettings,
    /// Java's `WorkspaceSettings.getItemSelectionFilter` (default: Java's
    /// `new ItemSelectionFilter()`).
    pub selection_filter: ItemFilter,
    /// The GUI shows the length-matching area (route overlay ellipses).
    pub length_matching_visible: bool,
    pub route_budget: RouteBudget,
    pub drag_budget: DragBudget,
}

impl Default for EditorOptions {
    fn default() -> Self {
        EditorOptions {
            drag: DragSettings::default(),
            selection_filter: ItemFilter::default(),
            length_matching_visible: true,
            route_budget: RouteBudget::JAVA,
            drag_budget: DragBudget::JAVA,
        }
    }
}

/// What a construction shows.
#[derive(Clone, Debug, PartialEq)]
pub struct ConstructionOverlay {
    pub corners: Vec<IntPoint>,
    /// The snapped pointer position (the rubber band's end).
    pub pointer: Option<IntPoint>,
    /// A circle's centre and radius.
    pub circle: Option<(FloatPoint, f64)>,
}

/// Drawable editor state, without colours or pixels.
#[derive(Clone, Debug, PartialEq, Default)]
pub struct Overlay {
    /// The selected items, highest id first.
    pub selection: Vec<ItemId>,
    pub route: Option<RouteOverlay>,
    /// A pin swap's first pin and its swappable pins (highest id first).
    pub swap: Option<(ItemId, Vec<ItemId>)>,
    pub construction: Option<ConstructionOverlay>,
    /// A region selection's first corner and the pointer.
    pub region: Option<(FloatPoint, FloatPoint)>,
}

/// The result of one command.
#[derive(Clone, Debug, PartialEq)]
pub struct Response {
    pub mode: ModeKind,
    /// The board changes of the command (B01.7).
    pub delta: BoardChanges,
    /// Java's message key and arguments; the GUI localizes.
    pub status: Option<Status>,
    pub overlay: Overlay,
    /// The current layer.
    pub layer: i32,
    /// Where to move the pointer (make-space without an angle restriction).
    pub warp_pointer: Option<FloatPoint>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Base {
    Menu,
    Selected,
}

#[derive(Clone, Debug)]
enum Session {
    None,
    Route(Box<RouteSession>),
    Drag(Box<DragSession>),
    Region { corner1: Option<FloatPoint> },
    PinSwap(PinSwap),
    Construct(Box<Construction>),
}

/// The interactive editor: a board, the settings, the mode and the
/// selection.
pub struct Editor {
    board: RoutingBoard,
    settings: EditSettings,
    options: EditorOptions,
    tool: Tool,
    base: Base,
    selection: BTreeSet<ItemId>,
    session: Session,
    pointer: Option<FloatPoint>,
    snapped_pointer: Option<IntPoint>,
}

/// Per-command output the arms collect.
#[derive(Default)]
struct Out {
    status: Option<Status>,
    warp_pointer: Option<FloatPoint>,
}

impl Out {
    fn status(status: Option<Status>) -> Out {
        Out {
            status,
            warp_pointer: None,
        }
    }
}

impl Editor {
    /// An editor in the route menu (Java's initial state), with the board's
    /// change journal on.
    pub fn new(mut board: RoutingBoard, settings: EditSettings) -> Self {
        if !board.is_journaling() {
            board.set_journaling(true);
        }
        Editor {
            board,
            settings,
            options: EditorOptions::default(),
            tool: Tool::Route,
            base: Base::Menu,
            selection: BTreeSet::new(),
            session: Session::None,
            pointer: None,
            snapped_pointer: None,
        }
    }

    pub fn board(&self) -> &RoutingBoard {
        &self.board
    }

    pub fn into_board(self) -> RoutingBoard {
        self.board
    }

    pub fn settings(&self) -> &EditSettings {
        &self.settings
    }

    pub fn options(&self) -> &EditorOptions {
        &self.options
    }

    /// The selected items, highest id first.
    pub fn selection(&self) -> &BTreeSet<ItemId> {
        &self.selection
    }

    /// The active route session, if routing or making space.
    pub fn route_session(&self) -> Option<&RouteSession> {
        match &self.session {
            Session::Route(route) => Some(route),
            _ => None,
        }
    }

    pub fn mode(&self) -> ModeKind {
        match &self.session {
            Session::None => match self.base {
                Base::Menu => ModeKind::Menu(self.tool),
                Base::Selected => ModeKind::Selecting,
            },
            Session::Route(route) => ModeKind::Routing {
                stitch: route.is_stitch(),
            },
            Session::Drag(drag) if drag.is_make_space() => ModeKind::MakingSpace,
            Session::Drag(_) => ModeKind::Dragging,
            Session::Region { .. } => ModeKind::SelectingRegion,
            Session::PinSwap(_) => ModeKind::PinSwap,
            Session::Construct(_) => ModeKind::Constructing,
        }
    }

    fn in_menu(&self) -> bool {
        matches!(self.session, Session::None) && self.base == Base::Menu
    }

    /// Applies one command; runs to completion within the budgets.
    pub fn apply(&mut self, cmd: Command) -> Response {
        if let Command::Press(p) | Command::Move(p) | Command::DragTo(p) | Command::Click(p) = cmd {
            self.pointer = Some(p);
        }
        let out = self.dispatch(cmd);
        Response {
            mode: self.mode(),
            delta: self.board.take_changes(),
            status: out.status,
            overlay: self.overlay(),
            layer: self.settings.layer,
            warp_pointer: out.warp_pointer,
        }
    }

    fn dispatch(&mut self, cmd: Command) -> Out {
        match cmd {
            Command::SetTool(tool) => {
                self.tool = tool;
                self.base = Base::Menu;
                self.selection.clear();
                self.session = Session::None;
                Out::default()
            }
            Command::UpdateSettings(settings) => {
                self.settings = settings;
                Out::default()
            }
            Command::UpdateOptions(options) => {
                self.options = options;
                Out::default()
            }
            Command::SetLayer(layer) => self.change_layer(layer),
            Command::NextSignalLayer => self.step_signal_layer(1),
            Command::PreviousSignalLayer => self.step_signal_layer(-1),
            Command::SignalLayer(digit) => self.digit_layer(digit),
            Command::Undo | Command::Redo => self.history(&cmd),
            Command::TogglePush => {
                if self.in_menu() {
                    self.settings.push_enabled = !self.settings.push_enabled;
                }
                Out::default()
            }
            cmd => match std::mem::replace(&mut self.session, Session::None) {
                Session::None => self.idle_command(cmd),
                Session::Route(route) => self.route_command(*route, cmd),
                Session::Drag(drag) => self.drag_command(*drag, cmd),
                Session::Region { corner1 } => self.region_command(corner1, cmd),
                Session::PinSwap(swap) => self.pin_swap_command(swap, cmd),
                Session::Construct(construction) => self.construct_command(*construction, cmd),
            },
        }
    }

    // ------------------------------------------------------------------
    // Menu and selection
    // ------------------------------------------------------------------

    fn idle_command(&mut self, cmd: Command) -> Out {
        let menu = self.base == Base::Menu;
        match cmd {
            Command::Click(p) if menu => match self.tool {
                Tool::Route => self.start_route(p),
                Tool::Select => self.select_at(p),
                Tool::Drag => Out::default(),
            },
            Command::Click(p) | Command::ToggleSelect(p) if !menu => self.toggle_select(p),
            Command::Press(p) if menu && self.tool == Tool::Drag => self.start_drag(p),
            Command::DragTo(p) if !menu || self.tool == Tool::Select => {
                self.session = Session::Region { corner1: Some(p) };
                Out::status(Some(Status::new(
                    "drag_left_mouse_button_to_select_items_in_region",
                    [],
                )))
            }
            Command::StartRoute { at } if menu => self.start_route(at),
            Command::StartPinSwap { at } if menu => self.start_pin_swap(at),
            Command::SelectAt(p) if menu => self.select_at(p),
            Command::SelectRegion(region) => self.select_region(region),
            Command::ExtendSelection(extent) if !menu => {
                let result = select::extend(
                    &self.board,
                    &self.selection,
                    extent,
                    &self.options.selection_filter,
                );
                self.set_selection(result);
                Out::default()
            }
            Command::FilterSelection if !menu => {
                let result =
                    select::filter(&self.board, &self.selection, &self.options.selection_filter);
                self.set_selection(result);
                Out::default()
            }
            Command::Complete | Command::Cancel if !menu => {
                self.set_selection(None);
                Out::default()
            }
            Command::StartKeepout { kind, at } => {
                let construction = Construction::keepout(&self.board, kind, at);
                self.session = Session::Construct(Box::new(construction));
                Out::default()
            }
            Command::StartHole { at } => {
                match Construction::hole(&self.board, &self.settings, at) {
                    Ok(construction) => {
                        self.session = Session::Construct(Box::new(construction));
                        Out::status(Some(Status::new("adding_hole_to_obstacle_area", [])))
                    }
                    Err(status) => Out::status(Some(status)),
                }
            }
            Command::InsertKeepout { shape, layer } => {
                Out::status(Some(insert_keepout(&mut self.board, &shape, layer).status))
            }
            Command::AddHole { area, corners } => match add_hole(&mut self.board, area, &corners) {
                Ok(outcome) => Out::status(Some(outcome.status)),
                Err(status) => Out::status(Some(status)),
            },
            Command::DeleteSelection if !menu => {
                let outcome = delete_selection(&mut self.board, &self.settings, &self.selection);
                let left: BTreeSet<ItemId> = self
                    .selection
                    .iter()
                    .copied()
                    .filter(|id| self.board.item(*id).is_some())
                    .collect();
                self.set_selection((!left.is_empty()).then_some(left));
                Out::status(outcome.status)
            }
            Command::FixSelection(fixed) if !menu => {
                fix_selection(&mut self.board, &self.selection, fixed);
                Out::default()
            }
            _ => Out::default(),
        }
    }

    /// Enters (`Some`) or leaves (`None`) the selection state.
    fn set_selection(&mut self, items: Option<BTreeSet<ItemId>>) {
        match items {
            Some(items) => {
                self.selection = items;
                self.base = Base::Selected;
            }
            None => {
                self.selection.clear();
                self.base = Base::Menu;
            }
        }
    }

    /// Java `MenuState.selectItems`.
    fn select_at(&mut self, at: FloatPoint) -> Out {
        let picked = select::pick_items(
            &self.board,
            &self.settings,
            at,
            &self.options.selection_filter,
        );
        if picked.is_empty() {
            return Out::default();
        }
        self.set_selection(Some(picked));
        Out::status(Some(Status::new("in_inspect_mode", [])))
    }

    /// Java `InspectedItemState.toggleSelect`.
    fn toggle_select(&mut self, at: FloatPoint) -> Out {
        let picked = select::pick_items(
            &self.board,
            &self.settings,
            at,
            &self.options.selection_filter,
        );
        let result = select::toggle(&self.selection, &picked);
        self.set_selection(result);
        Out::default()
    }

    /// Java `InspectItemsInRegionState.selectAllInRegion`.
    fn select_region(&mut self, region: IntBox) -> Out {
        let found = select::select_in_region(
            &self.board,
            &self.settings,
            region,
            &self.options.selection_filter,
        );
        if found.is_empty() {
            return Out::status(Some(Status::new("nothing_selected", [])));
        }
        let mut items = std::mem::take(&mut self.selection);
        items.extend(found);
        self.set_selection(Some(items));
        Out::default()
    }

    fn region_command(&mut self, corner1: Option<FloatPoint>, cmd: Command) -> Out {
        match cmd {
            Command::DragTo(p) => {
                self.session = Session::Region {
                    corner1: corner1.or(Some(p)),
                };
                Out::default()
            }
            Command::Release | Command::Complete => match (corner1, self.pointer) {
                (Some(c1), Some(c2)) => self.select_region(select::region_box(c1, c2)),
                _ => Out::status(Some(Status::new("nothing_selected", []))),
            },
            Command::Cancel => Out::default(),
            _ => {
                self.session = Session::Region { corner1 };
                Out::default()
            }
        }
    }

    // ------------------------------------------------------------------
    // History and layers
    // ------------------------------------------------------------------

    fn history(&mut self, cmd: &Command) -> Out {
        if !self.in_menu() {
            return Out::default();
        }
        let step = if *cmd == Command::Undo {
            history::undo(&mut self.board)
        } else {
            history::redo(&mut self.board)
        };
        Out::status(Some(step.status))
    }

    /// Java `changeLayerAction`: a layer change with a via while routing,
    /// else the current layer.
    fn change_layer(&mut self, layer: i32) -> Out {
        if let Session::Route(route) = &mut self.session {
            let (change, status) =
                route.change_layer(&mut self.board, layer, self.options.route_budget);
            self.settings.layer = route.current_layer();
            if change.ends_route() {
                self.session = Session::None;
            }
            return Out::status(status);
        }
        if layer >= 0 && (layer as usize) < self.board.layer_count() {
            self.settings.layer = layer;
        }
        Out::default()
    }

    /// Java's `+` and `-`: the next signal layer in direction `step`.
    fn step_signal_layer(&mut self, step: i32) -> Out {
        let layers = &self.board.layer_structure.layers;
        let mut index = self.settings.layer;
        loop {
            index += step;
            if index < 0 || index as usize >= layers.len() || layers[index as usize].is_signal {
                break;
            }
        }
        if index < 0 || index as usize >= layers.len() {
            return Out::default();
        }
        self.change_layer(index)
    }

    /// Java's digit keys (`InteractiveState.keyTyped`, `RouteState.keyTyped`).
    fn digit_layer(&mut self, digit: u32) -> Out {
        let structure = &self.board.layer_structure;
        let d = (digit as usize).min(structure.signal_layer_count());
        let layer = structure.get_signal_layer(d.saturating_sub(1)) as i32;
        self.change_layer(layer)
    }

    // ------------------------------------------------------------------
    // Route
    // ------------------------------------------------------------------

    /// Java `RouteState.getInstance`.
    fn start_route(&mut self, at: FloatPoint) -> Out {
        match RouteSession::start(&mut self.board, &self.settings, at) {
            Ok((session, info)) => {
                self.settings.layer = info.layer;
                self.session = Session::Route(Box::new(session));
                Out::status(Some(info.status))
            }
            Err(_) => Out::default(),
        }
    }

    fn route_command(&mut self, mut route: RouteSession, cmd: Command) -> Out {
        let budget = self.options.route_budget;
        let input = match cmd {
            Command::Move(p) => RouteInput::Move(p),
            Command::Click(p) => RouteInput::Click(p),
            Command::Snapshot => RouteInput::Snapshot,
            Command::Complete => RouteInput::Complete,
            Command::Cancel => {
                route.cancel(&mut self.board, &self.settings);
                return Out::default();
            }
            _ => {
                self.session = Session::Route(Box::new(route));
                return Out::default();
            }
        };
        let event = route.handle(&mut self.board, input, budget);
        self.settings.layer = route.current_layer();
        if !event.ends_route() {
            self.session = Session::Route(Box::new(route));
        }
        Out::default()
    }

    // ------------------------------------------------------------------
    // Drag
    // ------------------------------------------------------------------

    /// Java `DragState.getInstance`.
    fn start_drag(&mut self, at: FloatPoint) -> Out {
        let Some(drag) = DragSession::start(&self.board, &self.settings, &self.options.drag, at)
        else {
            return Out::default();
        };
        let status = drag.status();
        self.session = Session::Drag(Box::new(drag));
        Out::status(status)
    }

    fn drag_command(&mut self, mut drag: DragSession, cmd: Command) -> Out {
        match cmd {
            Command::DragTo(p) => {
                let step =
                    drag.drag_to(&mut self.board, &self.settings, p, self.options.drag_budget);
                let warp_pointer = match &step {
                    DragStep::MadeSpace { warp_pointer, .. } => *warp_pointer,
                    _ => None,
                };
                if !step.ends_drag() {
                    self.session = Session::Drag(Box::new(drag));
                }
                Out {
                    status: None,
                    warp_pointer,
                }
            }
            Command::Release | Command::Complete => {
                drag.release(&mut self.board);
                Out::default()
            }
            Command::Cancel => Out::default(),
            _ => {
                self.session = Session::Drag(Box::new(drag));
                Out::default()
            }
        }
    }

    // ------------------------------------------------------------------
    // Pin swap
    // ------------------------------------------------------------------

    fn start_pin_swap(&mut self, at: FloatPoint) -> Out {
        match PinSwap::start(&self.board, &self.settings, at) {
            Ok((swap, status)) => {
                self.session = Session::PinSwap(swap);
                Out::status(Some(status))
            }
            Err(status) => Out::status(status),
        }
    }

    fn pin_swap_command(&mut self, swap: PinSwap, cmd: Command) -> Out {
        match cmd {
            Command::Click(p) => Out::status(swap.click(&mut self.board, &self.settings, p).status),
            Command::Complete => Out::status(swap.complete(&mut self.board, None).status),
            Command::Cancel => Out::default(),
            _ => {
                self.session = Session::PinSwap(swap);
                Out::default()
            }
        }
    }

    // ------------------------------------------------------------------
    // Construction
    // ------------------------------------------------------------------

    fn construct_command(&mut self, mut construction: Construction, cmd: Command) -> Out {
        match cmd {
            Command::Click(p) => match construction.click(&self.board, p) {
                ConstructClick::Complete => self.complete_construction(construction),
                ConstructClick::Continue => {
                    self.session = Session::Construct(Box::new(construction));
                    Out::default()
                }
            },
            Command::Move(p) => {
                self.snapped_pointer = Some(construction.pointer_moved(&self.board, p));
                self.session = Session::Construct(Box::new(construction));
                Out::default()
            }
            Command::Complete => self.complete_construction(construction),
            Command::Cancel => Out::default(),
            _ => {
                self.session = Session::Construct(Box::new(construction));
                Out::default()
            }
        }
    }

    fn complete_construction(&mut self, construction: Construction) -> Out {
        self.snapped_pointer = None;
        let layer = self.settings.layer;
        Out::status(Some(construction.complete(&mut self.board, layer).status))
    }

    // ------------------------------------------------------------------
    // Overlay
    // ------------------------------------------------------------------

    fn overlay(&self) -> Overlay {
        let mut overlay = Overlay {
            selection: self.selection.iter().copied().collect(),
            ..Overlay::default()
        };
        match &self.session {
            Session::Route(route) => {
                overlay.route = Some(RouteOverlay::new(
                    route,
                    &self.board,
                    self.options.length_matching_visible,
                ));
            }
            Session::Drag(drag) => {
                if let DragSession::MakeSpace(space) = drag.as_ref() {
                    overlay.route = Some(RouteOverlay::new(space.route(), &self.board, false));
                }
            }
            Session::PinSwap(swap) => {
                overlay.swap = Some((
                    swap.from_pin(),
                    swap.swappable_pins().iter().copied().collect(),
                ));
            }
            Session::Construct(construction) => {
                let circle = (construction.keepout_kind() == Some(KeepoutKind::Circle))
                    .then(|| construction.circle());
                overlay.construction = Some(ConstructionOverlay {
                    corners: construction.corners().to_vec(),
                    pointer: self.snapped_pointer,
                    circle,
                });
            }
            Session::Region { corner1 } => {
                if let (Some(c1), Some(p)) = (corner1, self.pointer) {
                    overlay.region = Some((*c1, p));
                }
            }
            Session::None => {}
        }
        overlay
    }
}

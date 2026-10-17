//! Every action of the app. Menus, the toolbar, keys and tests all go
//! through [`App::execute`](crate::App::execute), so a test drives the app
//! the way a user does without pixel coordinates.

use std::path::PathBuf;

use freerouting::board::SelectableChoice;
use freerouting::board::items::ItemId;
use freerouting::board::structure::Unit;

use crate::dialogs::{RulesDialog, RulesEdit};
use crate::look::{ItemColor, Object, OtherColor};
use crate::panels::select_settings::SelectionLayers;
use crate::scene::{Kind, P};
use crate::view::Mirror;

#[derive(Clone, Debug, PartialEq)]
pub enum Command {
    /// Reads a design or session file (on a background thread).
    Open(PathBuf),
    /// Shows the file browser.
    OpenDialog,
    /// Reads the open files again.
    Reload,
    Quit,
    /// Fits the board in the canvas (Java: zoom all).
    FitBoard,
    /// Zoom in by 2 at the cursor.
    ZoomIn,
    /// Zoom out by 2 at the cursor.
    ZoomOut,
    /// Moves the view so the cursor point is at the centre.
    CenterOnCursor,
    /// The next left-drag draws a rectangle to zoom to (Java: `F`).
    ZoomToRectangle,
    /// Leaves a pending zoom rectangle.
    Cancel,
    ToggleCrosshair,
    SetActiveLayer(usize),
    /// The next (`1`) or previous (`-1`) signal layer becomes active.
    StepActiveLayer(i32),
    ToggleLayer(usize),
    ShowAllLayers,
    ActiveLayerOnly,
    ToggleKind(Kind),
    ToggleLabels,
    ShowWarnings,
    // G03: selection, analysis, lists, delete all, DRC export.
    /// Replaces the selection (Java `selectItems`).
    Select(Vec<ItemId>),
    ClearSelection,
    /// Fits the view to the selection (Java `zoomSelection`, key `W`).
    ZoomSelection,
    /// Shows or hides the ratsnest (key `G`).
    ToggleRatsnest,
    /// Shows or hides the clearance violation markers (key `V`).
    ToggleViolations,
    /// Runs the design rules check again (Info > Run DRC).
    RunDrc,
    /// Opens an Info list window.
    ShowList(ListKind),
    /// Asks before deleting all traces and vias (Other menu, toolbar).
    DeleteAll,
    /// Deletes all traces and vias after the confirmation.
    ConfirmDeleteAll,
    /// Key `U` and `Ctrl+Z`.
    Undo,
    /// Key `B` and `Ctrl+Y`.
    Redo,
    /// Opens the Export DRC report window.
    ExportDrcDialog,
    /// Writes the DRC report as JSON to the path.
    ExportDrcReport(PathBuf),
    // G04: windows, display options, colours.
    /// Opens (or brings up) a window.
    ShowWindow(WindowKind),
    /// Closes a window.
    CloseWindow(WindowKind),
    /// Rebuilds the rows of a board list (Nets, Components, Packages,
    /// Padstacks) from the board.
    RecalculateList(ListKind),
    /// The unit of the status bar, the lists and the item info window.
    SetUnit(Unit),
    /// The crosshair: small (the system cursor) is off, large is on.
    SetCrosshair(bool),
    /// Board rotation in quarter turns, counterclockwise (0, 1, 2, 3).
    SetRotation(u8),
    SetMirror(Mirror),
    /// A copper layer's visibility level, 0 to 100 %.
    SetLayerLevel(usize, u8),
    /// A kind of object's visibility level, 0 to 100 %.
    SetObjectLevel(Object, u8),
    /// The slider of the automatic layer dimming (0 = no dimming).
    SetDim(u8),
    /// Visibility window > Reset to defaults.
    ResetLevels,
    /// Shows or hides one of the six virtual layers (Java's order).
    ToggleVirtualLayer(usize),
    /// Colour manager: one layer's colour of one kind of item.
    SetItemColor(usize, ItemColor, [u8; 3]),
    SetOtherColor(OtherColor, [u8; 3]),
    ResetColors,
    // G06: rules dialogs (the dialogs are in `dialogs/`).
    /// Opens a rules dialog.
    ShowRulesDialog(RulesDialog),
    CloseRulesDialog(RulesDialog),
    /// One user change to the board rules, run through B08's facade.
    EditRules(RulesEdit),
    /// A text for the status line (a dialog's refusal).
    Notice(String),
    // G05: inspect mode, selection settings, safe edits, context menus and
    // keepout construction. Points are in the millimetres of the design
    // file's frame, as the camera and the scene use them.
    /// A left click on the canvas (Java `leftButtonClicked`). `toggle` is
    /// the Ctrl modifier.
    CanvasClick {
        at: P,
        toggle: bool,
    },
    /// A left drag that ended: select the items in the box (Java
    /// `InspectItemsInRegionState`).
    SelectRegion {
        from: P,
        to: P,
    },
    /// Key `I` without a selection, and the popup's "Select item": picks
    /// at the point (Java `MenuState.selectItems(location)`).
    SelectAt(P),
    /// Extends the selection (Java `InspectedItemState.extentTo...`).
    ExtendSelection(ExtendKind),
    /// Shows or hides the clearance violations of the selected items (key
    /// `V` while items are selected).
    ToggleSelectionViolations,
    /// Opens the item info window for these items (Java
    /// `WindowObjectInfo.display`); key `I` and the Info buttons.
    ShowItemInfo(Vec<ItemId>),
    /// Opens the "General Settings" window of the selection.
    ShowSelectionSettings,
    /// Which layers a click selects on.
    SetSelectionLayers(SelectionLayers),
    /// Turns one selectable item class (or fixed state) on or off.
    SetSelectable(SelectableChoice, bool),
    /// Deletes the unfixed selected items (decision G00 question 6).
    DeleteSelection,
    FixSelection,
    UnfixSelection,
    /// Right click: the context menu of the mode.
    OpenPopup {
        at: P,
    },
    ClosePopup,
    /// The popup's display rows act on the click location.
    CenterAt(P),
    ZoomInAt(P),
    ZoomOutAt(P),
    /// Starts constructing a keepout at the point (the popup's "Create
    /// keepout" entries).
    StartKeepout {
        kind: KeepoutKind,
        at: P,
    },
    /// Completes the tool under way (Java `complete`): `Enter`.
    Complete,
    // G09: interactive routing and editing (`tools/{host,route,drag,move_component}.rs`).
    /// Java's `S`, `R` and `D`: the tool of the left button.
    SetTool(crate::tools::ToolMode),
    /// Java's `T` and the popup's "Start route": a route from the item at
    /// the point.
    StartRoute {
        at: P,
    },
    /// Java's `W` and the popup's "Swap pin".
    StartPinSwap {
        at: P,
    },
    /// Java's `P`: push-and-shove on or off.
    TogglePush,
    /// The pointer moved on the canvas while a route, a move or a copy
    /// follows it.
    PointerMoved(P),
    /// Drag mode: the left button went down, moved (with it down) and went
    /// up.
    DragStart(P),
    DragMove(P),
    DragEnd,
    /// The popup's "Change layer" while routing (a board layer index).
    ChangeRouteLayer(usize),
    /// Settings > Routing: the interactive fields changed.
    UpdateInteractive(crate::settings::InteractiveSettings),
    /// Settings > Routing: the trace width of one layer in manual rule
    /// selection, in board units.
    SetManualTraceWidth {
        layer: usize,
        half_width: i32,
    },
    SetManualClearanceClass(i32),
    SetManualViaRule(i32),
    /// G09.6: picks the selection up and moves it with the pointer (key `M`).
    MoveSelection {
        at: P,
    },
    /// Turns the floating selection by this many degrees (`+` 90, `*` 180,
    /// `-` 270).
    RotateSelection(i32),
    /// Changes the placement side of the floating selection (`/`).
    MirrorSelection,
    ResetRotation,
    /// Starts a copy of the selection that follows the pointer (`Ctrl+D`).
    CopySelection {
        at: P,
    },
    /// Component Movement Settings: the placement grid in board units.
    SetComponentGrid {
        horizontal: i32,
        vertical: i32,
    },
    // G07: the autorouter (`routing.rs`).
    /// Starts the autorouter on the open board (toolbar, Autoroute).
    StartRouting,
    /// Stops the running autorouter; it hands back the board it keeps
    /// (Cancel, Esc, a click on the canvas).
    StopRouting,
    /// Opens Settings > Autorouter.
    ShowAutorouteSettings,
    CloseAutorouteSettings,
    /// The dialog's Apply: these are the settings the next job starts with.
    ApplyAutorouteSettings(Box<freerouting::settings::SettingsPatch>),
    CloseRoutingSummary,
    /// The summary window's "show after auto-routing" checkbox.
    SetShowRoutingSummary(bool),
}

impl Command {
    /// True for a command that changes the board or replaces the open one.
    /// The app refuses these while the autorouter runs (the job owns the
    /// board, G07).
    pub fn edits_board(&self) -> bool {
        matches!(
            self,
            Command::Open(_)
                | Command::Reload
                | Command::DeleteAll
                | Command::ConfirmDeleteAll
                | Command::Undo
                | Command::Redo
                | Command::RunDrc
                | Command::EditRules(_)
                | Command::DeleteSelection
                | Command::FixSelection
                | Command::UnfixSelection
                | Command::StartKeepout { .. }
                | Command::Complete
                | Command::StartRoute { .. }
                | Command::StartPinSwap { .. }
                | Command::PointerMoved(_)
                | Command::DragStart(_)
                | Command::DragMove(_)
                | Command::DragEnd
                | Command::ChangeRouteLayer(_)
                | Command::MoveSelection { .. }
                | Command::RotateSelection(_)
                | Command::MirrorSelection
                | Command::CopySelection { .. }
        )
    }
}

/// What an "extend selection" action extends to (the toolbar buttons and
/// the keys `N`, `S`, `E`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ExtendKind {
    Nets,
    ConnectedSets,
    Connections,
    Components,
}

/// The shapes of the keepout popup (Java `PopupMenuMain`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeepoutKind {
    Tile,
    Circle,
    Polygon,
    Hole,
}

/// The windows of the Info, Display and Help menus that are plain flags.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum WindowKind {
    Visibility,
    DisplayMisc,
    Colors,
    Log,
    About,
}

/// The Info list windows (Java `BoardMenuInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ListKind {
    Incompletes,
    LengthViolations,
    ClearanceViolations,
    Unconnected,
    ZoneIslands,
    // G04: lists over the board's own data.
    Nets,
    Components,
    Packages,
    Padstacks,
}

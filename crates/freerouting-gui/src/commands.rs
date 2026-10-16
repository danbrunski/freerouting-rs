//! Every action of the app. Menus, the toolbar, keys and tests all go
//! through [`App::execute`](crate::App::execute), so a test drives the app
//! the way a user does without pixel coordinates.

use std::path::PathBuf;

use freerouting::board::items::ItemId;
use freerouting::board::structure::Unit;

use crate::look::{ItemColor, Object, OtherColor};
use crate::scene::Kind;
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

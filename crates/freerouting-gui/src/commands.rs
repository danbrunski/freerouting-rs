//! Every action of the app. Menus, the toolbar, keys and tests all go
//! through [`App::execute`](crate::App::execute), so a test drives the app
//! the way a user does without pixel coordinates.

use std::path::PathBuf;

use freerouting::board::items::ItemId;

use crate::scene::Kind;

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
}

/// The Info list windows (Java `BoardMenuInfo`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ListKind {
    Incompletes,
    LengthViolations,
    ClearanceViolations,
    Unconnected,
    ZoneIslands,
}

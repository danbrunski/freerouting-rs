//! Every action of the app. Menus, the toolbar, keys and tests all go
//! through [`App::execute`](crate::App::execute), so a test drives the app
//! the way a user does without pixel coordinates.

use std::path::PathBuf;

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
}

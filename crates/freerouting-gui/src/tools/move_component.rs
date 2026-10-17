//! Move, rotate, mirror and copy of the selection (G09.6), the GUI side of
//! I04: Java's `MoveItemState` and `CopyItemState`, which the Java GUI cannot
//! enter at the pin (decision G00 question 6 and I00 decision 1; DIVERGENCES,
//! "Interactive move and copy of a selection").
//!
//! `M` picks the selection up (the editor's `MoveSelection`), the pointer
//! carries it, a left click puts it down (refused while it violates a
//! clearance), Esc puts everything back. While it floats, `+` turns it 90
//! degrees, `*` 180 and `-` 270 (`MoveItemState.keyTyped`), `/` changes the
//! placement side and the popup resets the rotation. `Ctrl+D` starts a copy
//! whose preview follows the pointer; each click inserts a copy and Enter or
//! Esc ends it.
//!
//! Every turn is one undo step, since the engine takes a snapshot before it
//! changes the board; the whole move is undone by the first Esc or by Undo
//! after it.

use eframe::egui::{InputState, Key};
use freerouting::interactive::{Command as EditorCommand, ModeKind, Response};

use crate::app::App;
use crate::commands::Command;
use crate::scene::P;

impl App {
    /// Runs a move or copy command: the editor takes the GUI's selection, and
    /// the GUI's selection becomes the editor's afterwards (picked-up items
    /// leave it; a refused move adds the items that block it).
    fn selection_command(&mut self, command: EditorCommand) -> Option<Response> {
        self.tools.popup = None;
        if self.tools.keepout.is_some() {
            return None;
        }
        if !self.editor_busy() {
            self.sync_tool();
        }
        let response = self.editor_apply_selected(command)?;
        self.selection
            .set(response.overlay.selection.iter().copied());
        if self.selection.is_empty() {
            self.tools.violations = None;
        }
        Some(response)
    }

    /// Key `M`: picks the selection up with the pointer at `at`.
    pub(crate) fn move_selection(&mut self, at: P) {
        if self.selection.is_empty() && !self.is_moving() {
            return;
        }
        if let Some(to) = self.board_point_of(at) {
            self.selection_command(EditorCommand::MoveSelection { to });
        }
    }

    /// Turns the floating selection (starting the move if needed).
    pub(crate) fn rotate_selection(&mut self, degrees: i32) {
        if self.selection.is_empty() && !self.is_moving() {
            return;
        }
        self.selection_command(EditorCommand::RotateSelection {
            degrees: f64::from(degrees),
        });
    }

    pub(crate) fn mirror_selection(&mut self) {
        if self.selection.is_empty() && !self.is_moving() {
            return;
        }
        self.selection_command(EditorCommand::MirrorSelection);
    }

    pub(crate) fn reset_rotation(&mut self) {
        if self.selection.is_empty() && !self.is_moving() {
            return;
        }
        self.selection_command(EditorCommand::ResetRotation);
    }

    /// `Ctrl+D`: starts a copy of the selection with the pointer at `at`.
    pub(crate) fn copy_selection(&mut self, at: P) {
        if self.selection.is_empty() || self.editor_busy() {
            return;
        }
        if let Some(to) = self.board_point_of(at) {
            self.selection_command(EditorCommand::CopySelection { to });
        }
    }

    /// True while the selection floats (a move).
    pub fn is_moving(&self) -> bool {
        self.editor_mode() == Some(ModeKind::Moving)
    }

    /// True while a copy preview follows the pointer.
    pub fn is_copying(&self) -> bool {
        self.editor_mode() == Some(ModeKind::Copying)
    }

    /// The component placement grid in board units (Java's
    /// `WindowMoveParameter`).
    pub(crate) fn set_component_grid(&mut self, horizontal: i32, vertical: i32) {
        self.tools.component_grid = (horizontal.max(0), vertical.max(0));
    }
}

/// The keys while the selection floats (`MoveItemState.keyTyped`): `+`, `*`
/// and `-` turn it by 90, 180 and 270 degrees, `/` changes the side. Keys
/// while nothing floats are not handled here.
pub fn float_key_commands(input: &InputState, typing: bool) -> Vec<Command> {
    let mut out = Vec::new();
    if typing || input.modifiers.command || input.modifiers.alt {
        return out;
    }
    if input.key_pressed(Key::Plus) || input.key_pressed(Key::Equals) {
        out.push(Command::RotateSelection(90));
    }
    if input.key_pressed(Key::Minus) {
        out.push(Command::RotateSelection(270));
    }
    if input.modifiers.shift && input.key_pressed(Key::Num8) {
        out.push(Command::RotateSelection(180));
    }
    if input.key_pressed(Key::Slash) {
        out.push(Command::MirrorSelection);
    }
    out
}

/// `Ctrl+D` with a selection: a copy.
pub fn copy_key_command(input: &InputState, typing: bool, cursor: Option<P>) -> Option<Command> {
    if typing || !input.modifiers.command || input.modifiers.alt {
        return None;
    }
    let at = cursor?;
    input
        .key_pressed(Key::D)
        .then_some(Command::CopySelection { at })
}

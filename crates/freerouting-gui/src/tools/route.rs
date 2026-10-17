//! Route mode, pin swap and the interactive keys (G09.1, G09.2, G09.4).
//!
//! `RouteMenuState.leftButtonClicked` → `RouteState.getInstance`, the route
//! states' mouse and key handling, and `MenuState.swapPins` as GUI input: the
//! tool turns a click, a pointer move or a key into an editor command
//! (`tools::host`) and shows what comes back. Which net, which rule, whether
//! stitch mode is forced and what a layer change does are the engine's
//! (I01); this file decides only which command a gesture is.
//!
//! The keys follow `MenuState.keyTyped` and `RouteState.keyTyped`: `R`
//! route mode, `D` drag mode, `S` inspect mode, `T` a route from the item at
//! the cursor, `P` push-and-shove, `W` pin swap, `I` select, digits and
//! `+`/`-` the layer (mid-route a layer change with a via), Enter completes,
//! Esc cancels. Space completes in Java and pans here (G01), so Enter is the
//! only completion key.

use eframe::egui::{InputState, Key};
use freerouting::interactive::{Command as EditorCommand, ModeKind};

use crate::app::App;
use crate::commands::Command;
use crate::scene::P;
use crate::text;
use crate::tools::ToolMode;

impl App {
    /// True when a click, a press or a pointer move belongs to the editor
    /// rather than to the inspect tool: a session is under way, or the
    /// selection is empty and the tool is not Select.
    pub(crate) fn editor_owns_pointer(&self) -> bool {
        self.editor_busy() || (self.selection.is_empty() && self.tools.mode != ToolMode::Select)
    }

    /// What the left button does now (G09): the inspect tool unless the
    /// editor owns the pointer, and not while a keepout is drawn (those take
    /// clicks through the inspect path).
    pub(crate) fn left_button(&self) -> crate::viewport::LeftButton {
        use crate::viewport::LeftButton;
        if self.tools.keepout.is_some() || !self.editor_owns_pointer() {
            LeftButton::Inspect
        } else if self.drag_tool_active() {
            LeftButton::Drag
        } else {
            LeftButton::Click
        }
    }

    /// Java's `S`, `R` and `D` (`setInspectMenuState`, `setRouteMenuState`,
    /// `setDragMenuState`): the left button's tool. Selected items are
    /// dropped, as the engine's `SetTool` does. Ignored while a session is
    /// under way: Java's route and drag states do not handle these keys.
    pub(crate) fn set_tool(&mut self, tool: ToolMode) {
        if self.editor_busy() || self.tools.keepout.is_some() {
            return;
        }
        let was = self.tools.mode;
        self.tools.mode = tool;
        if !self.selection.is_empty() {
            self.selection.clear();
            self.tools.drop_selection_state();
        }
        if tool == ToolMode::Select && was != ToolMode::Select {
            self.notice = Some(text::IN_INSPECT_MENU.to_string());
        }
    }

    /// A left click on the canvas that the editor handles: it starts a route
    /// in route mode and goes on with the session otherwise.
    pub(crate) fn editor_click(&mut self, at: P) {
        let Some(point) = self.board_point_of(at) else {
            return;
        };
        self.sync_tool();
        self.editor_apply(EditorCommand::Click(point));
    }

    /// Makes an idle editor use the GUI's tool (Java's menu state).
    pub(crate) fn sync_tool(&mut self) {
        let wanted = ModeKind::Menu(self.tools.mode.engine());
        if self.editor_busy() {
            return;
        }
        if self.tools.host.as_ref().is_none_or(|h| h.mode != wanted) {
            self.editor_apply(EditorCommand::SetTool(self.tools.mode.engine()));
        }
    }

    /// The pointer moved on the canvas while a route, a move or a copy
    /// follows it.
    pub(crate) fn pointer_moved(&mut self, at: P) {
        if !self
            .tools
            .host
            .as_ref()
            .is_some_and(|h| h.follows_pointer())
        {
            return;
        }
        if let Some(point) = self.board_point_of(at) {
            self.editor_apply(EditorCommand::Move(point));
        }
    }

    /// Java's `T` and the popup's "Start route".
    pub(crate) fn start_route(&mut self, at: P) {
        self.tools.popup = None;
        if self.editor_busy() || !self.selection.is_empty() || self.tools.keepout.is_some() {
            return;
        }
        let Some(point) = self.board_point_of(at) else {
            return;
        };
        // Java's `T` works from any menu state: the route returns to it.
        self.sync_tool();
        self.editor_apply(EditorCommand::StartRoute { at: point });
    }

    /// Java's `W` (`MenuState.swapPins`) and the popup's "Swap pin".
    pub(crate) fn start_pin_swap(&mut self, at: P) {
        self.tools.popup = None;
        if self.editor_busy() || !self.selection.is_empty() || self.tools.keepout.is_some() {
            return;
        }
        let Some(point) = self.board_point_of(at) else {
            return;
        };
        self.sync_tool();
        self.editor_apply(EditorCommand::StartPinSwap { at: point });
    }

    /// Settings > Routing: the interactive fields.
    pub(crate) fn update_interactive(&mut self, interactive: crate::settings::InteractiveSettings) {
        self.settings.interactive = interactive;
    }

    /// Settings > Routing, manual rules: changes the board's manual rules.
    pub(crate) fn edit_manual(
        &mut self,
        edit: impl FnOnce(&mut freerouting::interactive::ManualRules),
    ) {
        let Some(b) = &self.loaded else { return };
        let layers = b.session.board.layer_count();
        let manual = self
            .tools
            .manual
            .get_or_insert_with(|| freerouting::interactive::ManualRules::new(layers));
        edit(manual);
    }

    /// Java's `P`: push-and-shove on or off, in the menu states only.
    pub(crate) fn toggle_push(&mut self) {
        if self.editor_busy() || !self.selection.is_empty() {
            return;
        }
        let interactive = &mut self.settings.interactive;
        interactive.push_enabled = !interactive.push_enabled;
    }

    /// Enter: completes the session under way (Java `complete`). False if
    /// there is none.
    pub(crate) fn complete_session(&mut self) -> bool {
        if !self.editor_busy() {
            return false;
        }
        self.editor_apply(EditorCommand::Complete);
        true
    }

    /// Esc: cancels the session under way (Java `cancel`). False if there
    /// is none.
    pub(crate) fn cancel_session(&mut self) -> bool {
        if !self.editor_busy() {
            return false;
        }
        self.editor_apply(EditorCommand::Cancel);
        true
    }

    /// A layer key or the layers panel while a route or a copy is under way:
    /// the editor changes the layer (with a via while routing, Java
    /// `changeLayerAction`) and says what happened. False if no session wants
    /// it.
    pub(crate) fn editor_layer(&mut self, command: &Command) -> bool {
        let wanted = match command {
            Command::SetActiveLayer(n) | Command::ChangeRouteLayer(n) => {
                EditorCommand::SetLayer(*n as i32)
            }
            Command::StepActiveLayer(step) if *step >= 0 => EditorCommand::NextSignalLayer,
            Command::StepActiveLayer(_) => EditorCommand::PreviousSignalLayer,
            _ => return false,
        };
        let layered = self
            .tools
            .host
            .as_ref()
            .is_some_and(|h| matches!(h.mode, ModeKind::Routing { .. } | ModeKind::Copying));
        if !layered {
            return false;
        }
        self.editor_apply(wanted);
        true
    }
}

/// The keys of the interactive tools for the frame: `S`, `R`, `D`, `T`, `W`,
/// `P` without a selection and a session, plus `M` (move) with a selection.
/// `cursor` is the point under the pointer in the scene's millimetres.
pub fn key_commands(
    input: &InputState,
    typing: bool,
    selection_empty: bool,
    busy: bool,
    cursor: Option<P>,
) -> Vec<Command> {
    let mut out = Vec::new();
    if typing || input.modifiers.command || input.modifiers.alt || busy {
        return out;
    }
    if selection_empty {
        let table = [
            (Key::S, Command::SetTool(ToolMode::Select)),
            (Key::R, Command::SetTool(ToolMode::Route)),
            (Key::D, Command::SetTool(ToolMode::Drag)),
            (Key::P, Command::TogglePush),
        ];
        for (key, command) in table {
            if input.key_pressed(key) {
                out.push(command);
            }
        }
        if let Some(at) = cursor {
            if input.key_pressed(Key::T) {
                out.push(Command::StartRoute { at });
            }
            if input.key_pressed(Key::W) {
                out.push(Command::StartPinSwap { at });
            }
        }
    } else if let Some(at) = cursor
        && input.key_pressed(Key::M)
    {
        out.push(Command::MoveSelection { at });
    }
    out
}

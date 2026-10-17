//! The interactive editor behind the left button (G09): the engine's
//! [`Editor`] driven with `Editor::apply_on`, which lends it the session
//! board for the length of one command.
//!
//! The GUI holds no routing logic (decision D7). A tool turns input into an
//! editor [`Command`] (`route.rs`, `drag.rs`, `move_component.rs`), this
//! module runs it and then does what every response needs:
//! - the scene is patched from the response's change journal
//!   (`Loaded::sync_scene_with`), so a dragged component or a growing trace
//!   rebuilds only the chunks it touches and `stats.full_rebuilds` stays 0;
//! - the status line shows the response's Java message;
//! - the active layer follows the editor's current layer (a route starts on
//!   the layer of its first item and changes layers with vias);
//! - the overlay (the route's rubber band, the obstacle, the floating
//!   selection) is kept for [`crate::overlays::route`].
//!
//! The selection stays the GUI's own (G05). The editor is told about it only
//! for a move or a copy, which work on it; any other command first makes
//! sure the editor holds none, which is also what an undo needs: the engine
//! refuses history while items are selected, as Java does, and the GUI
//! allows it (DIVERGENCES, "Undo and redo work while items are selected").
//!
//! While a session is under way (a route, a drag, a floating selection) the
//! background analysis is held, so that its `Arc` clone of the board does
//! not force a copy of the whole board on every pointer move.

use std::sync::Arc;

use freerouting::board::BoardChanges;
use freerouting::geometry::FloatPoint;
use freerouting::interactive::{
    Command as EditorCommand, DragSettings, EditSettings, Editor, EditorOptions, LayerMask,
    ManualRules, ModeKind, Overlay, Response, Tool,
};

use crate::app::App;
use crate::panels::select_settings::SelectionLayers;
use crate::scene::P;
use crate::settings::InteractiveSettings;
use crate::text;
use crate::tools::inspect::board_point;
use crate::viewport::ViewState;

/// The tool of the left button (Java's `RouteMenuState`, `DragMenuState`,
/// `InspectMenuState`), chosen with `R`, `D` and `S`. Java starts in route
/// mode.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ToolMode {
    /// `S`: the inspect tool of G05.
    Select,
    /// `R`: a click starts a route.
    #[default]
    Route,
    /// `D`: a press drags the item under the cursor, or makes space.
    Drag,
}

impl ToolMode {
    /// The engine's tool.
    pub fn engine(self) -> Tool {
        match self {
            ToolMode::Select => Tool::Select,
            ToolMode::Route => Tool::Route,
            ToolMode::Drag => Tool::Drag,
        }
    }
}

/// The editor and what its last response left for the overlays.
pub struct Host {
    editor: Editor,
    /// The editor's mode after the last command.
    pub mode: ModeKind,
    /// What the last response shows (the route in progress, the floating
    /// selection, ...).
    pub overlay: Overlay,
    /// Commands run since the board was loaded.
    pub applied: u64,
}

impl Host {
    fn new(settings: EditSettings) -> Self {
        Host {
            editor: Editor::detached(settings),
            mode: ModeKind::Menu(Tool::Route),
            overlay: Overlay::default(),
            applied: 0,
        }
    }

    /// True while a session needs the pointer: a route, a drag, a pin swap,
    /// a floating selection or a copy preview.
    pub fn busy(&self) -> bool {
        !matches!(
            self.mode,
            ModeKind::Menu(_) | ModeKind::Selecting | ModeKind::SelectingRegion
        )
    }

    /// True while the editor's session follows the pointer without a button
    /// (a route, a floating selection or a copy preview).
    pub fn follows_pointer(&self) -> bool {
        matches!(
            self.mode,
            ModeKind::Routing { .. } | ModeKind::Moving | ModeKind::Copying
        )
    }

    /// The editor, for tests.
    pub fn editor(&self) -> &Editor {
        &self.editor
    }
}

/// The engine settings the GUI's choices make: the dialog's interactive
/// fields, the manual rules, the active layer, the visible layers and
/// whether a pick looks at every visible layer (G05.2).
pub fn edit_settings(
    interactive: &InteractiveSettings,
    manual: &ManualRules,
    view: &ViewState,
    layers: SelectionLayers,
    layer_count: usize,
) -> EditSettings {
    let mut settings = EditSettings::new(layer_count);
    interactive.apply_to(&mut settings);
    settings.manual_rules = manual.clone();
    settings.layer = view.active_layer.min(layer_count.saturating_sub(1)) as i32;
    let levels = view.look.layer_levels();
    settings.visible_layers = LayerMask::from_visibility(
        (0..layer_count)
            .map(|i| {
                view.layer_visible.get(i).copied().unwrap_or(true)
                    && levels.get(i).copied().unwrap_or(100) > 0
            })
            .collect(),
    );
    settings.select_on_all_visible_layers = layers == SelectionLayers::AllVisible;
    settings
}

impl App {
    /// True while an editor session is under way.
    pub fn editor_busy(&self) -> bool {
        self.tools.host.as_ref().is_some_and(Host::busy)
    }

    /// The editor's mode, `None` before the first command.
    pub fn editor_mode(&self) -> Option<ModeKind> {
        self.tools.host.as_ref().map(|h| h.mode)
    }

    /// What the editor's last response shows.
    pub fn editor_overlay(&self) -> Option<&Overlay> {
        self.tools.host.as_ref().map(|h| &h.overlay)
    }

    /// The editor, for tests (`None` before the first command).
    pub fn editor(&self) -> Option<&Editor> {
        self.tools.host.as_ref().map(Host::editor)
    }

    /// The board point of a point of the scene (millimetres).
    pub(crate) fn board_point_of(&self, at: P) -> Option<FloatPoint> {
        let b = self.loaded.as_ref()?;
        Some(board_point(b.session.board.basic(), at))
    }

    /// The engine options the GUI's choices make.
    fn editor_options(&self) -> EditorOptions {
        EditorOptions {
            drag: DragSettings {
                drag_components_enabled: self.settings.interactive.drag_components_enabled,
                item_filter: self.tools.select.filter(),
            },
            selection_filter: self.tools.select.filter(),
            component_grid: self.tools.component_grid,
            ..EditorOptions::default()
        }
    }

    /// Runs `command` on the editor, which works on the session board
    /// (copied first if a background job still shares it), then brings the
    /// scene, the status line, the layer and the overlay up to date.
    /// `None` without a board.
    pub(crate) fn editor_apply(&mut self, command: EditorCommand) -> Option<Response> {
        self.editor_apply_with(command, false)
    }

    /// [`editor_apply`](Self::editor_apply) for a command that works on the
    /// GUI's selection (a move or a copy): the editor takes it over first.
    pub(crate) fn editor_apply_selected(&mut self, command: EditorCommand) -> Option<Response> {
        self.editor_apply_with(command, true)
    }

    fn editor_apply_with(&mut self, command: EditorCommand, selected: bool) -> Option<Response> {
        let options = self.editor_options();
        let b = self.loaded.as_mut()?;
        let layer_count = b.session.board.layer_count();
        let manual = self
            .tools
            .manual
            .get_or_insert_with(|| ManualRules::new(layer_count));
        let settings = edit_settings(
            &self.settings.interactive,
            manual,
            &self.view,
            self.tools.select.layers,
            layer_count,
        );
        let host = self
            .tools
            .host
            .get_or_insert_with(|| Host::new(settings.clone()));
        let board = Arc::make_mut(&mut b.session.board);
        if host.editor.settings() != &settings {
            host.editor
                .apply_on(board, EditorCommand::UpdateSettings(settings));
        }
        if host.editor.options() != &options {
            host.editor
                .apply_on(board, EditorCommand::UpdateOptions(options));
        }
        // The editor holds the selection only for a move or a copy; an idle
        // editor is told the GUI's selection then and nothing otherwise.
        if !host.busy() {
            let wanted = if selected {
                self.selection.items.clone()
            } else {
                Default::default()
            };
            if *host.editor.selection() != wanted {
                host.editor
                    .apply_on(board, EditorCommand::SetSelection(wanted));
            }
        }
        let mut response = host.editor.apply_on(board, command);
        host.mode = response.mode;
        host.applied += 1;
        host.overlay = response.overlay.clone();
        // The editor's layer is the current layer of the route or the copy.
        if let Ok(layer) = usize::try_from(response.layer)
            && layer < self.view.layer_visible.len()
            && layer != self.view.active_layer
        {
            self.view.active_layer = layer;
        }
        if let Some(status) = &response.status {
            self.notice = Some(text::interactive_status(status));
        }
        let delta: BoardChanges = std::mem::take(&mut response.delta);
        if !delta.is_empty() {
            b.session.edit_generation += 1;
            self.tools.changed_nets = delta.changed_nets.clone();
            self.after_board_changes(Some(delta));
        }
        Some(response)
    }

    /// Ends whatever session the editor has (as Esc does) before a command
    /// that changes the board in another way: the session refers to items
    /// by id.
    pub(crate) fn end_editor_session(&mut self) {
        let mut guard = 0;
        while self.editor_busy() && guard < 4 {
            self.editor_apply(EditorCommand::Cancel);
            guard += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_is_the_default_tool_as_in_java() {
        assert_eq!(ToolMode::default(), ToolMode::Route);
        assert_eq!(ToolMode::Select.engine(), Tool::Select);
        assert_eq!(ToolMode::Drag.engine(), Tool::Drag);
    }
}
